# Recon Compliance Audit Chain Slice — Design

**Date:** 2026-05-26
**Status:** Approved (brainstorming) → ready for implementation plan
**Depends on:** UI slice (`2026-05-23-recon-ui-slice-design.md`), Backend slice (`2026-05-24-recon-backend-slice-design.md`), Auth & RBAC slice (`2026-05-24-recon-auth-rbac-design.md`), Bank-format ingestion slice (`2026-05-25-recon-ingestion-design.md`)

## Goal

Provide a hash-chained, tamper-evident audit log spanning every material action in
the platform (auth, admin, data, cases), per-tenant verifiable, with a global
anchor chain for wholesale-deletion protection. A controls registry maps ISO 27001
/ SOC 2 / FCA controls to the audit-event kinds that demonstrate them, surfaced via
an API endpoint and an admin UI panel.

## Decisions (locked during brainstorming)

1. **Audit scope:** ALL material actions — auth (login success/failure/lockout,
   logout, password change/reset, refresh-reuse detection, tenant switch); admin
   user management (create/update/disable/delete/role change); data plane (source
   create, file ingest, run create); plus every case event mirrored. ~20 event
   kinds across an enumerated taxonomy.
2. **Chain shape:** per-tenant chains for normal verification + a global anchor
   chain that periodically hashes the current tenant heads. Per-tenant isolation
   matches the existing data model; the anchor adds wholesale-deletion detection.
3. **Controls scaffolding:** `controls.md` doc + a `GET /api/audit/controls`
   endpoint returning the mapping as JSON + an admin **Controls** screen that
   click-through-filters the audit log to each control's event kinds.
4. **Emission discipline:** audit emission runs in the SAME DB transaction as the
   audited action. A failed audit insert rolls the action back. No silent gaps.

---

## Section 1 — Architecture & code organization

A new **`recon-audit`** crate holds the pure chain primitives (hashing,
verification, the controls registry), kept IO-light and exhaustively unit-testable
in isolation — the same pattern `recon-auth` and `recon-ingest` use. Layering
stays inward-pointing.

**Crate layout:**

- **`recon-audit`** (new) — depends only on `recon-domain`, `sha2`, `serde`,
  `thiserror`:
  - `chain` — deterministic canonical serialization + SHA-256 hashing; pure
    `verify(entries) -> Result<(), VerifyError { seq, reason }>`.
  - `events` — `AuditKind` enum (the closed set of event kinds) and an
    `AuditPayload` enum with a variant per kind (the non-sensitive fields).
  - `controls` — a static `CONTROLS: &[Control { id, framework, description,
    event_kinds }]` registry covering the ISO 27001 / SOC 2 / FCA items the
    platform claims to demonstrate.
- **`recon-store`** — new tables (Section 2), new methods `append_audit`,
  `list_audit`, `verify_audit`, `anchor_now`. **Append is a same-transaction
  concern** — `append_audit` takes the caller's `&mut sqlx::PgConnection`, so any
  failure to audit aborts the outer transaction. No silent gaps.
- **`recon-api`** — a thin `audit::emit(...)` helper wired into every existing
  handler that performs a material action (the emission sites are enumerated in
  Section 4). New routes `GET /api/audit`, `POST /api/audit/verify`,
  `POST /api/audit/anchor`, `GET /api/audit/anchors`, `GET /api/audit/controls`,
  all gated by a new `Permission::ViewAudit` (admin only, except `/controls` which
  is open to any authenticated member). A small `scheduler` module runs a
  `tokio::time::interval` that calls `anchor_now()` (default 3600s,
  env-configurable via `AUDIT_ANCHOR_INTERVAL_SECS`); the manual anchor endpoint
  reuses the same code path.
- **`web`** — a new admin-only **Audit Log** screen (`/audit`) listing chained
  events with filters (kind, actor, date range), Verify and Anchor-now actions,
  and an anchor history panel; and a **Controls** screen (`/controls`) listing
  each framework control with click-through to the audit log filtered to its
  supporting event kinds. Both gated through `useAuth()` like the existing
  **Users** screen.

**Dependency direction:** `recon-domain` → `recon-audit` → `recon-store` →
`recon-api`. The API is the only place that knows about both `recon-audit` (for
emission helpers) and `recon-store` (for persistence).

---

## Section 2 — Data model

Two new tables. Existing schema is untouched. **Migration `0004_compliance.sql`**:

```sql
-- Per-tenant hash-chained audit log.
CREATE TABLE audit_events (
  tenant_id  TEXT     NOT NULL REFERENCES tenants(id),
  seq        BIGINT   NOT NULL,            -- per-tenant 1-based sequence
  at         TIMESTAMPTZ NOT NULL,
  actor_id   TEXT     NOT NULL,            -- user-id, or "system" for scheduler
  kind       TEXT     NOT NULL,            -- closed enum (Section 4)
  payload    JSONB    NOT NULL,            -- non-sensitive metadata only
  prev_hash  BYTEA    NOT NULL,            -- 32 bytes (zeroes for genesis)
  hash       BYTEA    NOT NULL,            -- 32 bytes
  PRIMARY KEY (tenant_id, seq)
);
CREATE INDEX idx_audit_tenant_at   ON audit_events(tenant_id, at);
CREATE INDEX idx_audit_tenant_kind ON audit_events(tenant_id, kind);

-- Global anchor chain: each anchor hashes the current heads of every
-- tenant's chain into a single hash, plus the previous anchor's hash.
CREATE TABLE audit_anchors (
  anchor_seq    BIGINT      NOT NULL PRIMARY KEY,   -- global 1-based
  at            TIMESTAMPTZ NOT NULL,
  tenant_heads  JSONB       NOT NULL,               -- { tenantId: { seq, hash } }
  prev_hash     BYTEA       NOT NULL,
  hash          BYTEA       NOT NULL
);
```

The per-tenant PK `(tenant_id, seq)` lets concurrent writers race safely: each
serializes on a `SELECT … FOR UPDATE` of the tail row, and the unique PK rejects
any double-insert from a stale read with `23505` → outer transaction retries.

`audit_anchors.tenant_heads` snapshots every tenant's `(seq, hash)` at anchor time;
wholesale deletion of a tenant's entries between anchors is detected by re-anchor
(the recorded head no longer matches what's in `audit_events`).

---

## Section 3 — Chain mechanics

**Canonical serialization for hashing.** Each entry hashes a deterministic byte
string built from `prev_hash || u64-BE(seq) || tenant_id || at(RFC3339) ||
actor_id || kind || canonical_json(payload)`, joined with explicit length prefixes
so concatenated fields can't collide. The canonical JSON is keys-sorted, no
whitespace, UTF-8 — implemented in `recon-audit::chain` and locked with golden
vector tests. SHA-256, 32-byte output. The genesis entry uses
`prev_hash = [0u8; 32]`.

**Append flow** (`store.append_audit(tx, tenant_id, at, actor_id, kind, payload)`
runs inside the caller's transaction):

1. `SELECT seq, hash FROM audit_events WHERE tenant_id=$1 ORDER BY seq DESC
   LIMIT 1 FOR UPDATE` — locks the per-tenant tail. Genesis if absent.
2. Compute `next_seq = prev_seq + 1`,
   `next_hash = sha256(canonical(prev_hash, next_seq, tenant_id, at, actor_id,
   kind, payload))`.
3. `INSERT INTO audit_events (...) VALUES (...)`. The composite PK
   `(tenant_id, seq)` rejects any colliding concurrent insert with `23505`; the
   outer transaction aborts and the action is retried.

**Verification** (`chain::verify(entries: &[AuditEntry]) -> Result<(),
VerifyError>` — pure):

- Confirms `entries[0].prev_hash == [0; 32]` if `entries[0].seq == 1`; otherwise
  accepts a caller-supplied `expected_prev_hash` (so partial ranges remain
  checkable).
- Walks the slice: `entries[i].seq == entries[i-1].seq + 1`,
  `entries[i].prev_hash == entries[i-1].hash`,
  `recompute(entries[i]) == entries[i].hash`.
- Any mismatch → `Err(VerifyError { seq: <first broken>, reason: <missing |
  reordered | tampered | wrong_prev> })`.
- `store.verify_audit(tenant_id, from, to, expected_prev_hash?)` loads entries
  in `seq` order and runs `chain::verify`.

**Anchor mechanics** (`store.anchor_now()` — runs in its own transaction):

1. `SELECT tenant_id, seq, hash FROM audit_events ae WHERE seq = (SELECT
   max(seq) FROM audit_events WHERE tenant_id = ae.tenant_id) ORDER BY
   tenant_id` — the current head row per tenant.
2. Build `tenant_heads = { tenant_id: { seq, hash } }` (sorted by tenant_id for
   deterministic JSON).
3. Read the prior `audit_anchors` row (or genesis). Compute `next_anchor_seq` and
   `hash = sha256(prev_hash || u64-BE(anchor_seq) || at(RFC3339) ||
   canonical_json(tenant_heads))`.
4. Insert into `audit_anchors`. Emit a `system.anchor.created` event (a per-tenant
   audit row in each affected tenant is overkill; emit ONE entry per tenant only
   when invoked manually with a single-tenant context, otherwise it's a
   system-level row tracked by the anchors table itself — see Section 4).

The internal scheduler is a `tokio::time::interval` started in `main.rs`
(configurable via `AUDIT_ANCHOR_INTERVAL_SECS`, default `3600`). The same
`anchor_now` is exposed as `POST /api/audit/anchor` for on-demand triggering
during testing and demos. **Anchor failures are logged but do not block other
operations** — anchoring is asynchronous to audit emission.

**Wholesale-deletion detection:** an attacker who drops a tenant's `audit_events`
rows between anchors leaves the most recent `audit_anchors` row referencing a
`(seq, hash)` that no longer exists in `audit_events`. `verify_audit` for that
tenant cross-checks the latest anchor (when one exists) and surfaces the mismatch
as `VerifyError { reason: "missing", … }`.

---

## Section 4 — Audit event taxonomy + emission sites + payload safety

**`AuditKind` is a closed enum** in `recon-audit::events`. Each kind has a typed
`AuditPayload` variant; the payload struct serializes to the `payload` JSONB.
There is **no `serde_json::Value` escape hatch** — the compiler rejects any
attempt to stuff sensitive material into an audit row.

| Domain | Kind | Payload (non-sensitive) | Emitted from |
|---|---|---|---|
| Auth | `auth.login.success` | `userId`, `email`, `ip` | `routes_auth::login` |
| Auth | `auth.login.failure` | `email`, `ip`, `reason` (`bad_credentials` \| `locked` \| `rate_limited`) | `routes_auth::login` |
| Auth | `auth.lockout` | `userId`, `email`, `lockedUntil` | `routes_auth::login` |
| Auth | `auth.logout` | `userId`, `ip` | `routes_auth::logout` |
| Auth | `auth.password.changed` | `userId`, `ip` | `routes_auth::change_password` |
| Auth | `auth.password.reset_requested` | `email`, `ip` | `routes_auth::forgot` |
| Auth | `auth.password.reset_completed` | `userId`, `ip` | `routes_auth::reset` |
| Auth | `auth.refresh.reused` | `userId`, `tokenId`, `ip` | `routes_auth::refresh` |
| Auth | `auth.tenant.switched` | `userId`, `fromTenant`, `toTenant` | `routes_auth::switch_tenant` |
| Admin | `admin.user.created` | `userId`, `email`, `role` | `routes_users::create_user` |
| Admin | `admin.user.role_changed` | `userId`, `from`, `to` | `routes_users::patch_user` |
| Admin | `admin.user.disabled` / `admin.user.enabled` | `userId` | `routes_users::patch_user` |
| Admin | `admin.user.removed` | `userId` | `routes_users::delete_user` |
| Data | `data.source.created` | `sourceId`, `kind`, `currency`, `name` | `routes::create_source` |
| Data | `data.ingest.completed` | `sourceId`, `format`, `fileSha256`, `bytes`, `ingested` | `routes::ingest_source` |
| Data | `data.run.created` | `runId`, `sourceAId`, `sourceBId`, `from`, `to`, `stats` | `routes::create_run` |
| Cases | `case.assigned` | `caseId`, `breakId`, `assigneeId` | `store::assign_break` |
| Cases | `case.event_appended` | `caseId`, `breakId`, `eventKind` (mirrors `CaseEventBody` tag) | `store::append_case_event` |
| System | `system.anchor.created` | `anchorSeq`, `tenantCount` | scheduler / manual anchor (one row per affected tenant) |

**Payload-safety hygiene** (enforced at the type level by typed `AuditPayload`
variants):

- **Never:** passwords, refresh-token plaintext, password-reset-token plaintext,
  JWT contents, file bytes, transaction descriptions.
- **Hashes / IDs only:** `fileSha256` (computed at ingest time over the raw bytes
  the user uploaded); `tokenId`/`jti` for refresh-reuse, never the plaintext.
- **PII minimization:** email appears on auth and admin user-management events
  (necessary for the audit story); transaction-level descriptions and
  counterparty fields stay in `canonical_transactions` (already tenant-isolated)
  and are NOT mirrored into the audit log.
- **IP capture:** `Option<String>`, pulled from `X-Forwarded-For` or the peer
  address, `null` in tests. Stays in the audit log; not duplicated to application
  logs.

**Case-event mirroring:** every call to `store::append_case_event` and
`store::assign_break` emits a parallel `audit_events` row inside the same DB
transaction. `case_events` remains the source for the UI case timeline; the audit
row is a cryptographic receipt of it.

**`system.anchor.created` semantics:** when an anchor fires, the scheduler emits
one `system.anchor.created` audit row PER TENANT (each tenant's chain gains an
entry recording that an anchor referenced its head), keeping every tenant chain
self-describing. The anchor row itself lives in `audit_anchors`.

---

## Section 5 — API surface & RBAC

A new **`Permission::ViewAudit`** is added to `recon-auth::rbac`, granted to
**`admin` only**. The matrix currently does not extend audit read access to other
roles; auditors are expected to receive a tenant-scoped admin membership, or a
future read-only "auditor" role (out of scope here).

All audit reads are tenant-scoped through `AuthContext.tenant_id`. The wire
contract uses camelCase and the existing error envelope `{ error: { code,
message, …extras } }`.

| Method · Path | Body / Query → Result | Notes |
|---|---|---|
| `GET /api/audit` | query: `from`, `to`, `kind` (repeatable), `actorId`, `limit` (default 100, max 500), `before` (cursor = seq) → `{ items: AuditEvent[], nextCursor }` | Paginated descending by `seq` for the active tenant |
| `POST /api/audit/verify` | `{ from, to, expectedPrevHash? }` → `{ status: "valid" \| "invalid", checked, firstBrokenSeq?, reason? }` | Loads the range in `seq` order, runs `chain::verify` |
| `POST /api/audit/anchor` | (no body) → `{ anchorSeq, hash }` | Triggers `store.anchor_now()`; same code path as the scheduler |
| `GET /api/audit/anchors` | query: `limit` (default 50) → `Anchor[]` | Most recent first |
| `GET /api/audit/controls` | (any authenticated member) → `Control[]` | Static `recon-audit::controls::CONTROLS` as JSON |

`AuditEvent` wire shape (hashes hex-encoded for JSON transport):

```json
{
  "tenantId": "tenant-acme",
  "seq": 42,
  "at": "2026-05-26T10:30:00Z",
  "actorId": "user-mia",
  "kind": "data.ingest.completed",
  "payload": { "sourceId": "src-...", "format": "csv", "fileSha256": "…", "bytes": 1234, "ingested": 20 },
  "prevHash": "0a1b…",
  "hash": "9f3c…"
}
```

`GET /api/audit/controls` is auth-gated but NOT `ViewAudit`-gated — the controls
registry itself is non-sensitive metadata, and exposing it to any authenticated
member lets a future auditor role read it without elevation.

**Verification failure is NOT an API error.** A tampered range returns 200 with
`status: "invalid"` and the diagnostics (`firstBrokenSeq`, `reason`). 4xx/5xx are
reserved for actual route failures (unauthorized, bad input, server error).

---

## Section 6 — Frontend

Two new admin screens. Both reuse `DataTable` / `Dialog` / `Select` / `Card` and
gate through `useAuth()`.

**`/audit` (admin only)** — paginated table of audit events for the active tenant.

- Columns: time, actor (resolved to name via `useMembers()`), kind, payload
  summary (e.g. `data.ingest.completed → "csv · 20 rows · 1.2 KB"`), short-form
  `prevHash` / `hash` (first 8 hex chars, copy-on-click).
- Filters: `kind` (multi-select from the closed `AuditKind` enum), `actor`, date
  range. URL-persisted via `nuqs` like the Runs page.
- Toolbar buttons: **Verify chain** (opens a dialog with `from`/`to` defaulted to
  the current filter; calls `POST /api/audit/verify`; renders the result inline
  — green "chain valid" or red "tampered at seq N, reason: …"); **Anchor now**
  (calls `POST /api/audit/anchor`, toasts the new `anchorSeq`).
- Collapsible anchor history panel listing recent anchors and their tenant-head
  snapshot.

**`/controls` (admin only)** — table of controls fetched from
`GET /api/audit/controls`. Columns: framework, control id (e.g. `A.9.2.1`),
description, supporting event kinds (chip list). Clicking a row navigates to
`/audit?kind=auth.login.success&kind=auth.lockout&…` with the kinds joined into
the URL filter.

**`ApiClient` additions** (and `MockApiClient` test double): `listAudit(tenantId,
q)`, `verifyAudit(tenantId, body)`, `anchorAudit(tenantId)`, `listAnchors
(tenantId, limit?)`, `listControls()`. `HttpApiClient` implements them through
the existing `req` helper (JSON, no FormData).

**Navigation:** two new entries in `app-sidebar.tsx` — **Audit** (icon:
`ShieldCheck`) and **Controls** (icon: `ClipboardCheck`), both `adminOnly` like
the existing **Users** item.

---

## Section 7 — Security & testing

**Security:**

- **RBAC + tenant isolation:** every audit route requires `ViewAudit`
  (admin-only) except `GET /api/audit/controls`. Every store read filters by
  `tenant_id` from the token's `tid`. Cross-tenant verification is impossible by
  construction.
- **Same-transaction emission:** `audit::emit` takes the caller's
  `&mut sqlx::PgConnection`. A failed audit insert propagates `?` → action
  rollback. There is no API surface for direct audit emission — events arise only
  from server-side handlers, never client requests.
- **Closed payload schema at the type level:** `AuditPayload` is an enum with a
  variant per `AuditKind` and no `serde_json::Value` fallback. Sensitive material
  is impossible to add by accident.
- **No PII bleed into application logs:** the application's `tracing` logs
  continue to redact tokens/passwords; the audit log carries IPs/emails by design
  but is itself access-gated.
- **Hash-chain semantics, not auth-grade signatures:** SHA-256 chains prove
  *no in-place tampering of historic rows*; they do NOT prevent an attacker with
  DB write access from continuing the chain with forged entries from a point
  onward. The anchor chain narrows the window for wholesale-deletion; external
  chain-head signing is out of scope and listed below.

**Testing (TDD throughout):**

- **`recon-audit`** (unit + property): canonical serialization is byte-exact,
  locked with a golden vector; `chain::verify` accepts a valid 100-entry
  generated chain (property test); detects single-byte payload tampering, swapped
  entries, removed entries, replaced `prev_hash`, wrong genesis; controls
  registry round-trips through serde and lists each declared framework / control
  id exactly once.
- **`recon-store`** (`#[sqlx::test]`): `append_audit` happy path; concurrent
  appenders on the same tenant — one wins via 23505 (spawn two transactions);
  cross-tenant isolation (one tenant's append never affects another's `seq`);
  `verify_audit` on a real 50-row chain (valid); `verify_audit` after a manual
  `UPDATE` on the payload (invalid, `firstBrokenSeq` correct);
  `anchor_now` with two tenants writes one row with both heads; anchor after
  wholesale-deletion of a tenant's events detects the gap on next verify.
- **`recon-api`** (integration): every audited handler — login
  success/failure/lockout, logout, password change/reset, switch tenant, all four
  user-management routes, source create, ingest, run create, case assign and
  append — produces exactly one audit row of the expected kind (verified by
  reading `GET /api/audit` immediately after). RBAC 403 for non-admins on
  `/api/audit/*` except `/controls`. Verify endpoint returns `status:"valid"` on
  a clean chain and `status:"invalid"` with the right seq after a hand-edited
  row. The anchor endpoint creates a row; the controls endpoint returns the
  static registry.
- **Frontend** (vitest): Audit screen renders, filters, paginates; Verify dialog
  shows valid vs invalid outcomes; Anchor button calls the API and toasts the
  new `anchorSeq`; Controls screen renders and a click-through builds the right
  filtered URL.
- **E2E** (Playwright, live stack): admin logs in → `/audit` → sees recent events
  including the just-occurred `auth.login.success` → clicks **Verify chain** →
  sees "chain valid" → clicks **Anchor now** → toast shows `anchorSeq` →
  `/controls` → clicks a control row → lands on `/audit` filtered to the
  expected kinds.

---

## Out of scope (candidate later slices)

- External chain-head publishing / signing (off-system replication for
  wholesale-tampering protection beyond the in-band anchor).
- A read-only **Auditor** role (this slice limits `ViewAudit` to `admin` to avoid
  expanding the RBAC matrix prematurely).
- Audit retention / archival policies (export to cold storage, age-based deletion
  under regulatory retention rules).
- Automated periodic compliance reports / SOC 2 evidence collection workflows.
- Real-time audit anomaly detection (unusual-actor alerts, lockout-spike
  detection, etc.).
