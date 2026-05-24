# Reconciliation Backend Slice — Design Spec

**Date:** 2026-05-24
**Status:** Approved (design); ready for implementation planning
**Predecessor:** `docs/superpowers/specs/2026-05-23-recon-ui-slice-design.md` (the UI vertical slice this backend serves)

---

## 1. Goal

Make the existing frontend `ApiClient` contract real over HTTP, backed by a Rust service, a real
matching engine, and PostgreSQL — so the already-deployed UI runs against real Rust instead of the
in-memory mock.

This is a **vertical slice**, the backend counterpart to the UI slice. It is deliberately scoped: it
implements every method the frontend already calls, end-to-end, and nothing the frontend does not
yet need.

## 2. Scope

### In scope
- A Rust HTTP service (Axum) implementing all nine `ApiClient` methods, returning the exact domain
  shapes the frontend expects.
- A deterministic, replayable **matching engine** as a standalone, property-tested crate.
- PostgreSQL persistence via `sqlx`, with shared-schema multi-tenancy and append-only/immutable
  tables for source records, match decisions, and case events.
- Server-side enforcement of tenant isolation and the four-eyes (maker ≠ checker) approval rule.
- An `AuthContext` extractor establishing tenant + user from request headers — the auth seam.
- A seed that loads raw transactions and runs the engine to materialize the same data the UI shows
  today, including the `case-pending` four-eyes scenario.
- Frontend wiring: `HttpApiClient implements ApiClient`, with the runtime provider switched to it.

### Out of scope (deferred to later phases)
- Bank-format parsers (MT940, BAI2, CAMT.053, CSV, PDF) and ingestion connectors (SFTP, webhook,
  polling). Seed data stands in for ingestion.
- Real authentication: login, password hashing, JWT/session issuance, RBAC beyond the four-eyes
  check, SSO. The header-based `AuthContext` is the seam these will plug into.
- The full ISO 27001 / SOC 2 / FCA compliance program (control mapping, hash-chained tamper-evident
  audit, separate audit pipeline).
- Multi-region / high-throughput Scale-tier concerns; schema-per-tenant or db-per-tenant isolation.
- OpenTelemetry tracing and RED/USE metrics (structured logging + request spans only this phase).

## 3. Architecture

### Stack and justification (rejected alternatives noted)
- **Web framework: Axum.** Tokio + Tower ecosystem; type-safe extractors fit the `AuthContext`
  seam; first-class `sqlx` integration; minimal macro magic. *Rejected:* Actix-web (own runtime
  model, heavier API), Poem (smaller ecosystem).
- **Async runtime: Tokio.** Required by Axum and `sqlx`.
- **Persistence: PostgreSQL via `sqlx`.** Production-grade target engine; native arrays and JSONB;
  `sqlx::migrate!` for migrations; **runtime-checked queries** (`query_as`) so the build does not
  require a live database or `sqlx-cli` offline cache. *Rejected:* SQLite (not the production
  engine), in-memory (does not demonstrate durable tenant isolation / immutability the brief
  requires).
- **API style: REST/JSON over HTTP.** The contract is a fixed, known surface. *Rejected:* GraphQL
  (overkill), gRPC (browser friction).
- **Core crates:** `axum`, `tokio`, `tower-http` (trace/CORS/timeout), `sqlx` (postgres), `serde` /
  `serde_json`, `thiserror`, `tracing` (+ `tracing-subscriber`), `uuid`, `time`, `proptest` (dev).

### Workspace layout
A Cargo workspace enforces the matching-engine boundary at the dependency-graph level (the brief
requires the engine to be a separately testable library, not part of the HTTP layer).

```
backend/
  Cargo.toml                 # workspace
  crates/
    recon-domain/    # canonical types + serde wire models = the contract. Pure, no IO.
    recon-matching/  # the deterministic engine. Pure fns + property tests. dep: recon-domain only
    recon-store/     # sqlx Postgres: migrations, repository traits + impls, tenant-scoped queries
    recon-api/       # Axum binary: routes, AuthContext extractor, DTOs, error mapping, seed cmd
  docker-compose.yml         # Postgres for local dev
  migrations/                # SQL, applied via sqlx::migrate! at startup
```

Layering (clean architecture): `recon-domain` (entities + pure rules) ← `recon-matching` (pure
application logic) and `recon-store` (infrastructure) ← `recon-api` (interface). Dependencies point
inward; the domain depends on nothing.

### Independence
The frontend and backend remain independently deployable and versioned. The contract source is the
backend; the frontend's `ApiClient` interface is the agreed shape. No build-time coupling, no
codegen.

## 4. Multi-tenancy

**Shared schema + `tenant_id` column** on every tenant-scoped table. Isolation is enforced in two
places:
1. The `AuthContext` extractor establishes the caller's tenant from the `X-Tenant-Id` header.
2. Every `recon-store` repository method takes `tenant_id` and filters by it. There is no unscoped
   read path.

*Rejected for this tier:* schema-per-tenant / db-per-tenant (operational overhead unjustified at
Launch; revisit at Scale). Isolation is a **tested invariant**: tenant A must not be able to read
tenant B's rows (returns `NotFound`, never another tenant's data).

## 5. Data model (PostgreSQL)

Migrations live in `migrations/` and are applied via `sqlx::migrate!` at service startup.

```
tenants(id PK, name, slug, created_at)
users(id PK, tenant_id FK, name, role, created_at)            -- wire User omits tenant_id
sources(id PK, tenant_id, kind, name, currency, created_at)

canonical_transactions(                                        -- IMMUTABLE (insert-only)
  id PK, tenant_id, source_id, external_ref,
  value_date, posted_at, amount_minor BIGINT, currency,
  direction, counterparty NULL, description, created_at)

reconciliation_runs(
  id PK, tenant_id, name, source_a_id, source_b_id, status,
  started_at, completed_at NULL, config_version, stats JSONB)  -- stats materialized at run

match_decisions(                                               -- IMMUTABLE (insert-only)
  id PK, tenant_id, run_id, type, txn_ids TEXT[],
  score DOUBLE PRECISION, config_version, created_at)

breaks(
  id PK, tenant_id, run_id, case_id, type, status,
  value_minor BIGINT, currency, assignee_id NULL,
  txn_ids TEXT[], opened_at)                                   -- status/assignee mutable

cases(id PK, tenant_id, break_id, assignee_id NULL, status)    -- status/assignee mutable

case_events(                                                   -- APPEND-ONLY (insert-only)
  id PK, tenant_id, case_id, seq INT, kind, actor_id, at, payload JSONB)
```

Decisions baked in:
- **Immutability = insert-only tables**: `canonical_transactions`, `match_decisions`, `case_events`.
  Corrections are modelled as new events, never edits.
- **Audit trail = `case_events`**, append-only with a monotonic `seq` per case. (Hash-chain
  tamper-evidence deferred to the compliance phase.)
- `breaks.status`/`assignee_id` and `cases.status` are the **current-state read model**; their full
  history lives in `case_events`. `breaks.case_id` ↔ `cases.break_id` is 1:1.
- **`ageing_days` / `ageing_bucket` are computed at read time** from `opened_at` vs now. The seed
  sets `opened_at = now − N days` so buckets stay deterministic.
- Money is `BIGINT` minor units throughout — no floating point for amounts.
- `match_decisions` and `case_events` carry `tenant_id` internally for defense-in-depth, but it is
  omitted from the wire DTO (the wire shapes match the frontend `types.ts` exactly).

## 6. HTTP contract

Tenant is taken from the `X-Tenant-Id` header (the `AuthContext`), not the URL path. The acting user
is taken from `X-User-Id`. JSON is **camelCase** (`#[serde(rename_all = "camelCase")]`) to match the
frontend domain types byte-for-byte.

| `ApiClient` method | Endpoint |
|---|---|
| `listTenants()` | `GET /api/tenants` — the only un-scoped endpoint; feeds the tenant switcher |
| `listUsers(t)` | `GET /api/users` |
| `getDashboard(t)` | `GET /api/dashboard` |
| `listRuns(t, q)` | `GET /api/runs?status=&sourceId=&from=&to=` |
| `getRun(t, id)` | `GET /api/runs/:runId` |
| `listBreaks(t, q)` | `GET /api/breaks?status=&type=&ageingBucket=&assigneeId=` |
| `getCase(t, id)` | `GET /api/cases/:caseId` → `{ case, brk, suggestions, transactionsById }` |
| `assignBreak(t, b, u)` | `POST /api/breaks/:breakId/assign` `{ userId }` → `Break` |
| `appendCaseEvent(t, c, e)` | `POST /api/cases/:caseId/events` `{ actorId, kind, payload }` → `Case` |

`GET /healthz` is added for liveness (not part of the `ApiClient`).

### CaseEvent serialization
The discriminated union serializes via serde's adjacently-tagged enum representation
(`#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]`) flattened onto the base
`{ id, actorId, at }`, producing exactly `{ id, actorId, at, kind, payload }`. `NewCaseEvent` (the
POST body) omits `id` and `at`; the server assigns both (`at` = now, `id` = uuid). Event kinds and
payloads match `types.ts`: `comment{text}`, `assignment{assigneeId}`,
`manual_match_proposed{txnIds}`, `write_off_proposed{reason}`,
`approval_requested{resolution: "write_off"|"manual_match"}`, `approved{}`, `rejected{reason}`.

### Dashboard aggregation
`getDashboard` computes, over the tenant's runs and breaks: `matchRatePct`, `openBreaks`,
`valueAtRiskMinor` (sum of open breaks), `slaAdherencePct`, `breaksByType`, `breaksByAgeing`, and
`recentRuns`. These are SQL aggregates in `recon-store`, not stored values (except per-run `stats`,
which are materialized at run execution).

## 7. Matching engine (`recon-matching`)

Pure, no IO. Signature:

```rust
pub struct MatchConfig {
    pub version: String,              // pinned, e.g. "v1.2" — stamped on every output
    pub amount_tolerance_minor: i64,  // partial-match amount window
    pub date_tolerance_days: i64,     // ± value-date window
    pub fuzzy_threshold: f64,         // accept partial above this score
}

pub fn reconcile(
    a: &[CanonicalTransaction],
    b: &[CanonicalTransaction],
    cfg: &MatchConfig,
) -> RunResult;   // { decisions: Vec<MatchDecision>, breaks: Vec<BreakDraft>, stats: RunStats }
```

Layered rules, applied in order, with one-to-one greedy assignment:
1. **Exact** — equal `externalRef` (or amount + currency + direction + valueDate) → `matched`,
   score 1.0.
2. **Tolerant** — amount within `amount_tolerance_minor` **and** date within `date_tolerance_days`,
   same direction/currency → `partial`, score by closeness.
3. **Duplicate** — same `(amount, externalRef, valueDate)` appearing more than once within a source
   → `duplicate`.
4. **Fuzzy** — remaining scored by weighted similarity (amount proximity + date proximity +
   counterparty/description similarity); `>= fuzzy_threshold` → `partial`, otherwise `unmatched` →
   becomes a `break`.

**Determinism guarantees** (enforced as property tests):
- Inputs are sorted by `id` before processing; no RNG; no dependence on hashmap iteration order.
- Same inputs + same `config.version` ⇒ identical `RunResult` (replayability).
- Each transaction appears in **at most one** decision (no double-matching).
- Each transaction is classified **exactly once** (conservation: matched/partial/duplicate/break
  partition the input).
- Scores are finite and clamped to `0.0..=1.0` (never NaN).

`MatchSuggestion`s on the case screen are the engine's near-miss candidates for an unmatched break,
produced by the same scorer, sorted by descending score.

## 8. Four-eyes enforcement (server-side)

The `canApprove` logic from the frontend `lib/case/approval.ts` is ported into Rust in
`recon-domain` (pure, unit-tested). In `appendCaseEvent`, a `kind: "approved"` event is accepted
**only if all hold**:
- the case status is `pending_approval`,
- the actor's role is `approver` or `admin`,
- the actor is **not** the actor of the last `approval_requested` event.

It **fails closed**: if there is no `approval_requested` event, approval is denied. Violations return
**403 Forbidden**. The actor identity comes from the `AuthContext` (header), not trusted blindly
from the request body; the body `actorId` is validated against it. Illegal state transitions (e.g.
approving a case that is not `pending_approval`) return **409 Conflict**.

Applying an event also updates the current-state read model: `assignment` sets `cases.assignee_id`
and `breaks.assignee_id`; `approved` moves status to `resolved`; `rejected` moves it back to
`investigating`; `approval_requested` moves it to `pending_approval`.

## 9. Error model

A single `ApiError` enum (`thiserror`) implements `IntoResponse`, mapping to an HTTP status and a
JSON body `{ "error": { "code", "message" } }`:

| Variant | Status | When |
|---|---|---|
| `Unauthorized` | 401 | Missing or unknown tenant/user context |
| `Forbidden` | 403 | Four-eyes violation, cross-tenant write attempt |
| `NotFound` | 404 | Entity absent in the caller's tenant |
| `Validation` | 400 | Bad query params or event payload |
| `Conflict` | 409 | Illegal state transition |
| `Internal` | 500 | Database or unexpected error |

`recon-store` and `recon-matching` define their own `thiserror` error types that map upward into
`ApiError`. Cross-tenant reads return `NotFound` (never another tenant's data).

## 10. Observability

`tracing` + `tracing-subscriber` emit structured JSON logs. `tower-http`'s `TraceLayer` creates a
span per request; a request-id middleware attaches a correlation id; each request span carries
`tenant_id` and `user_id`. The audit trail is the append-only `case_events` table. *Deferred to
Growth tier:* OpenTelemetry distributed tracing and RED/USE metrics.

## 11. Testing strategy

- **`recon-matching`** — per-rule unit tests plus `proptest` properties: determinism, replay,
  at-most-one-match, conservation, score bounds.
- **`recon-store`** — `#[sqlx::test]` (isolated database per test): tenant-isolation tests (A cannot
  read B), append-only/immutability checks, repository CRUD round-trips.
- **`recon-api`** — HTTP integration tests via `tower::ServiceExt` oneshot: JSON shape/casing
  matches the contract, four-eyes 403, cross-tenant 404, validation 400, happy-path reads/writes.
- **Frontend** — the existing 140 vitest tests keep using `MockApiClient` as a **test double**
  (unchanged; they inject the client through `renderWithProviders`, not the runtime provider). The
  Playwright E2E (`web/tests/e2e/operator-loop.spec.ts`) is re-pointed at the live backend with a
  deterministic **reseed** before the run.

## 12. Frontend wiring and local run

- **`web/lib/api/http.ts`** — `HttpApiClient implements ApiClient`, using `fetch`, base URL from
  `NEXT_PUBLIC_API_BASE_URL`. Sends `X-Tenant-Id` (from the method's `tenantId` argument) and
  `X-User-Id` (the current user). Maps non-2xx responses to thrown errors.
- **`web/lib/api/provider.tsx`** — runtime default becomes `HttpApiClient`; `MockApiClient` remains
  only as the test double. `ApiProvider` moves **inside** the tenant/user providers so the client
  can read the current user. Tests inject the client directly, so they are unaffected.
- **CORS** — `tower-http` `CorsLayer` permits `http://localhost:3000`.
- **Seed** — a `recon-api seed` subcommand loads the same tenants, users, and sources as today's
  fixtures plus **raw transactions**, then runs the engine to materialize runs, decisions, breaks,
  and cases — *including the `case-pending` four-eyes scenario* (status `pending_approval`, an
  `approval_requested` event by `user-mia`) so that screen still demonstrates correctly. IDs match
  the existing fixtures (`tenant-acme`, `tenant-globex`, `user-mia`/`sam`/`theo`/`ada`, etc.).
- **Run recipe:**
  ```
  docker compose -f backend/docker-compose.yml up -d postgres
  cargo run -p recon-api -- seed     # migrate + seed
  cargo run -p recon-api             # serve :8080
  NEXT_PUBLIC_API_BASE_URL=http://localhost:8080 pnpm -C web dev
  ```
- **Prerequisite:** the Rust toolchain is not currently installed on this machine — installing
  `rustup`/`cargo` is the first setup task. Docker + Compose are present.

## 13. Definition of Done

1. `cargo build` and `cargo clippy` are clean across the workspace.
2. `cargo test` passes: matching-engine property tests, store isolation/immutability tests, API
   integration tests.
3. All nine `ApiClient` endpoints return JSON matching the frontend domain types (camelCase, exact
   field names).
4. Tenant isolation is proven by tests (A cannot read B).
5. The four-eyes rule is enforced server-side (maker blocked with 403; a different approver
   succeeds) and covered by tests.
6. The matching engine is deterministic and replayable, proven by property tests.
7. `recon-api seed` produces data equivalent to today's fixtures, including the `case-pending`
   scenario.
8. The frontend `HttpApiClient` is wired in; with the backend running and seeded, the deployed UI
   renders the dashboard and the four-eyes case screen against real Rust, and the Playwright E2E
   passes against the live backend.

## 14. Risks

- **Highest-risk assumption:** the frontend `ApiClient` shapes in `web/lib/domain/types.ts` are a
  complete and accurate contract; any drift surfaces as serialization mismatches. Mitigation: API
  integration tests assert exact field names/casing against the contract.
- **`sqlx` compile-time checking** is intentionally avoided (runtime-checked queries) to keep the
  build independent of a live database; the trade-off is that query/column mistakes surface at test
  time rather than compile time. Mitigation: thorough store integration tests.
- **Determinism of fuzzy scoring** with floating point: mitigated by clamping, stable sort, and
  property tests; amounts never use floats.
