# Phase 7 — Phase 5/6 Polish Bundle (Design)

**Status:** Approved by user 2026-05-27.
**Predecessor phases:** Phase 5 (compliance audit chain, PR #2) and Phase 6 (MT940 + BAI v2 ingestion, PR #3) are both merged on master.

## 1. Motivation

Phases 5 and 6 each left a small set of deferred polish items. They are individually too small to be their own phase but related enough to ship together. This phase clears the five items in a single PR while keeping per-item commits so any one can be reverted in isolation.

The five items, in dependency order:

1. **Migration `0006`** adds `counterparty_bic` and `counterparty_account` to `transactions`.
2. **CanonicalTransaction and parsers** learn to emit those fields where the source format provides them (CSV via mapping, CAMT.053 from `<RltdPties>`, MT940 subfielded from `?32`/`?33`; MT940 generic, MT942 generic, and BAI v2 leave them `None`).
3. **MT942 intra-day parser** added, sharing helpers with MT940 (both Generic and Subfielded dialects).
4. **PATCH `/sources/:id`** lets admins rename a source and change its `format_dialect` after creation; audited.
5. **Concurrent-appender stress test** for the audit chain (Phase 5 follow-up).
6. **`audit/page.tsx` split** — 853 LOC shell decomposed into five focused components (Phase 5 follow-up).

The matching engine is **deliberately not** changed; counterparty fields are surfaced but not scored.

## 2. Scope

### In scope

- Schema migration adding two nullable text columns and one CHECK constraint.
- Canonical-transaction type and store-layer plumbing for the two new fields.
- Parser updates for CSV, CAMT.053, MT940 (subfielded), MT942 (both dialects).
- New MT942 ingest format wired through the API and the upload dialog.
- PATCH `/sources/:id` endpoint with admin-only RBAC and an audited diff.
- Frontend edit dialog for sources and an "Edit" row action on the sources page.
- Backend concurrent-appender stress test for the audit chain.
- Frontend refactor of `audit/page.tsx` into a thin shell and five focused subcomponents, with existing tests as the regression gate.

### Out of scope

- Matching engine consuming the new counterparty fields (score function unchanged).
- Bulk source-edit / merge / split / soft-delete UI.
- MT942 `:34F:` floor-limit awareness (parsed and discarded for state-machine cleanliness).
- Backfilling counterparty on historical transactions (no source data to derive from).
- Replacing the description blob's counterparty mentions when the structured field is set.
- Per-bank MT940/MT942 `?nn` profiles beyond Generic and Subfielded.
- Editing source `kind` or `default_currency` after creation (semantically dangerous; not covered by PATCH this phase).

## 3. Architecture

The phase touches three layers and one frontend page:

- **Store layer** — one additive migration; two new columns on `transactions`; one new optional `update_source` function on the sources store.
- **Domain + ingest layer** — `CanonicalTransaction` gains two `Option<String>` fields; parsers populate them where the format supports it; helpers shared between MT940 and MT942 move into a new `mt94x_shared` module.
- **API layer** — `PATCH /sources/:id` is added (admin-only, audited); the ingest dispatcher learns the `"mt942"` format.
- **Frontend** — upload dialog gains an MT942 option; a new edit-source dialog is wired into the sources page; the audit page is decomposed.

No new crates. No new external dependencies. No infrastructure changes.

## 4. Data model

### Migration `0006_transactions_counterparty.sql`

```sql
ALTER TABLE transactions
  ADD COLUMN counterparty_bic     TEXT NULL,
  ADD COLUMN counterparty_account TEXT NULL;

ALTER TABLE transactions
  ADD CONSTRAINT chk_counterparty_bic_shape
  CHECK (
    counterparty_bic IS NULL
    OR counterparty_bic ~ '^[A-Z0-9]{8}([A-Z0-9]{3})?$'
  );
```

- ISO 9362 BIC is 8 or 11 uppercase alphanumeric characters. We enforce shape only — country-code validity is the bank's problem, not ours.
- Account is free-form text; can be IBAN, BBAN, US ABA/account number, anything the source happens to supply. No CHECK.
- Both columns are nullable; existing rows satisfy the constraints trivially. No backfill.
- No index — counterparty is not a query key this phase.

### CanonicalTransaction

```rust
pub struct CanonicalTransaction {
    // ... existing fields ...
    pub counterparty_bic: Option<String>,
    pub counterparty_account: Option<String>,
}
```

The store-layer `TransactionRow` mirrors this; the `From<TransactionRow>` impl trivially threads the two `Option<String>` columns.

## 5. API surface

### `PATCH /sources/:id` (NEW)

**Request body** (any subset of fields, or `{}` for no-op):

```jsonc
{
  "name": "Acme USD Operating (renamed)",
  "format_dialect": "subfielded"
}
```

The `format_dialect` field uses double-`Option` semantics on the Rust side:
- Field absent in JSON → don't change.
- Field present with `null` → clear the dialect.
- Field present with `"generic"` or `"subfielded"` → set it.

Implementation uses a hand-rolled `deserialize_with` function (`deserialize_double_option`) — eight lines, no new crate dependency. Serialization uses `#[serde(skip_serializing_if = "Option::is_none")]` on the outer `Option` so unchanged fields never appear in the round-tripped response.

**Validation:**

- `name` — 1–80 chars after trim, no surrounding whitespace stored.
- `format_dialect` — `None`, `"generic"`, or `"subfielded"`; any other value → 400.
- Empty body (`{}`) is valid and results in a 200 with the unchanged source.

**Authorization:** admin role only, same as `create_source`. Non-admin → 403.

**Tenant scoping:** the source must belong to the caller's active tenant; otherwise 404 (not 403, to avoid leaking existence).

**Success:** 200 with the updated `Source` body (matches `create_source`'s response shape).

**Audit:** emits `AuditKind::SourceUpdated` with payload:

```json
{
  "source_id": "src-...",
  "before": { "name": "...", "format_dialect": "..." },
  "after":  { "name": "...", "format_dialect": "..." }
}
```

The emission happens **inside the same transaction** as the UPDATE, the same as every other material action — if the audit insert fails (chain race), the UPDATE rolls back. New `AuditKind::SourceUpdated` variant is added; its kebab-case wire form is `"source.updated"`.

### Ingest dispatcher

`POST /sources/:id/ingest` (existing endpoint) gains an `"mt942"` branch alongside the existing `"csv" | "camt053" | "mt940" | "bai2"`:

```rust
"mt942" => {
    let dialect = match source.format_dialect.as_deref() {
        Some("subfielded") => recon_ingest::mt94x_shared::Mt94xDialect::Subfielded,
        _ => recon_ingest::mt94x_shared::Mt94xDialect::Generic,
    };
    recon_ingest::mt942::Mt942Parser { dialect }.parse(&bytes)
}
```

The `Mt940Dialect` enum is renamed to `Mt94xDialect` and moved into `mt94x_shared`; MT940 and MT942 both import it. The existing `format_dialect` column on `sources` and its CHECK constraint stay unchanged — the same allowed values (`generic`, `subfielded`) apply to both MT940 and MT942.

The existing `data.ingest.completed` audit payload extends naturally to `format=mt942` — no new audit kind.

## 6. Parser changes

### CSV (`recon-ingest/src/csv.rs`)

The CSV mapping config gains two optional fields:

```rust
pub struct CsvMapping {
    // ... existing fields ...
    pub counterparty_bic_col: Option<usize>,
    pub counterparty_account_col: Option<usize>,
}
```

When set, the parser reads the corresponding column verbatim. BIC is `.trim().to_uppercase()` before being stored (so a column with `"deutdeff"` survives the CHECK constraint). Account is trimmed only. When unset, the field is `None`. The frontend mapping form gains two optional column-index inputs.

### CAMT.053 (`recon-ingest/src/camt053.rs`)

Per entry, after parsing the existing fields, the parser walks into `<NtryDtls><TxDtls><RltdPties>`:

- For a credit entry (CRDT): read `<Cdtr>` (counterparty's name is the counterparty), `<CdtrAcct>/<Id>/<IBAN>` (or `<CdtrAcct>/<Id>/<Othr>/<Id>` if non-IBAN), and `<CdtrAgt>/<FinInstnId>/<BIC>` (or `<BICFI>` in newer dialects).
- For a debit entry (DBIT): mirror with `<Dbtr>`, `<DbtrAcct>`, `<DbtrAgt>`.

All elements are optional. Missing → `None`. BIC is uppercased + trimmed; account is trimmed only. The existing XXE-safe `quick-xml` reader stays — no schema-validation change.

### MT940 subfielded (`recon-ingest/src/mt940.rs`)

`parse_subfielded_86` already extracts every `?nn` subfield; the change is purely in mapping the existing extracted values:

- `?32` → `counterparty_account` (the German/Dutch convention; "Auftraggeberkonto" or "tegenpartij rekening")
- `?33` → `counterparty_bic`

In the Generic dialect we don't have structured subfields, so both fields stay `None`. The existing `description` field continues to receive the full `:86:` blob, unchanged.

### MT942 (`recon-ingest/src/mt942.rs`) — NEW

A new parser that shares helpers with MT940 via a new `mt94x_shared` module. MT942 tag set:

- `:20:` transaction reference (required, first tag).
- `:25:` account identification (required).
- `:28C:` statement/sequence number (required).
- `:34F:` floor-limit indicator (one or two lines — parsed and discarded; their presence does not affect downstream rows).
- `:13D:` date/time of the statement (parsed; provides the `value_date` fallback when a `:61:` line omits it).
- `:61:` statement line — same grammar as MT940 (`parse_61_line` shared helper).
- `:86:` info on the preceding `:61:` — same as MT940; in subfielded dialect uses the shared `parse_subfielded_86`.
- `:90D:` / `:90C:` totals of debits and credits, e.g. `:90D:3EUR1500,00`. Parsed and used for a sanity check: the count and minor-amount sums of all parsed `:61:` debits must equal `:90D:` (and credits must equal `:90C:`). Mismatch → parse error with a descriptive message.

There are **no balance lines** (`:60F:`, `:62F:`) in MT942 — that's the point of intra-day. If a balance tag appears, the parser returns a parse error.

The parser supports the same dialects as MT940 (`Generic` and `Subfielded`), driven by the source's `format_dialect`. The shared `Mt94xDialect` enum lives in `mt94x_shared`.

**File layout:**

```
recon-ingest/src/
  mt94x_shared.rs    NEW
  mt940.rs           refactored to import shared helpers
  mt942.rs           NEW
  lib.rs             + pub mod mt942; + pub mod mt94x_shared;
```

### BAI v2

No change. Counterparty fields stay `None` (BAI's `16` record doesn't carry structured counterparty).

## 7. Frontend changes

### Upload dialog (`web/components/app/upload-dialog.tsx`)

- The format dropdown gains a fifth option `MT942 (intra-day)`. Internal value `mt942`.
- The CSV mapping section already hides for non-CSV formats. Now also hidden for MT942, naturally.
- The MT940 amber "dialect not set on the source" notice is reused for MT942 when the chosen source has `formatDialect === null`. The notice text gains "(applies to MT940 and MT942)".

### Edit source dialog (`web/components/app/edit-source-dialog.tsx`, NEW)

- Mirrors the New Source dialog structure but only exposes `name` and `formatDialect`.
- Pre-fills from the source row's current values.
- On submit: calls `api.updateSource(source.id, patch)` where `patch` contains **only the changed fields** (we compare against the initial values; unchanged fields are omitted). This matches the backend's "absent field = no change" contract.
- Closes and re-fetches the sources list on success.
- "Cancel" + dialog backdrop click reset the form.

### Sources page (`web/app/(app)/sources/page.tsx`)

- Adds an "Edit" button to each row, visible only to admins (same gating pattern as the existing source-creation button).
- Click opens `EditSourceDialog` with that row's source pre-loaded.

### Audit page split (`web/app/(app)/audit/page.tsx`)

Current state: 853 LOC. Target: ~100 LOC shell + five focused components.

```
audit/
  page.tsx                          composition root, URL→state, layout shell
  _components/
    audit-filter-bar.tsx            kind/actor/date filter form, URL-synced via props
    audit-table.tsx                 table itself + row-click handler
    verify-chain-dialog.tsx         "Verify chain" button + dialog + result panel
    anchor-now-button.tsx           "Anchor now" button + sonner toast
    event-detail-drawer.tsx         right-side drawer for the selected event's payload
```

**Constraint:** pure functional-equivalence refactor. No behavioural changes, no copy changes, no a11y changes. The success gate is that **existing tests** — both the existing unit/component tests for the audit page and the existing `compliance.spec.ts` e2e — pass unchanged.

### Mock + fixtures + types (`web/lib/...`)

- `IngestFormat` union extends with `"mt942"`.
- `Transaction` (Zod schema) extends with `counterparty_bic: z.string().nullable()` and `counterparty_account: z.string().nullable()`.
- `api.updateSource(id, patch)` added to the client + mock.
- `mockSources` fixtures: no field change needed (the existing `formatDialect: null` is fine).
- `mockTransactions` fixtures: keep existing transactions on `null` counterparty so existing snapshot/test assertions don't drift; add one fixture-level transaction with a populated BIC/account if a sources-table assertion needs to demonstrate non-null values.

## 8. Frontend display of counterparty

The new fields are surfaced **read-only** on:

- The run-detail page's transactions table — two new columns `Cpty BIC` and `Cpty account`, hidden when both are null for every row in the table; otherwise shown.
- The exceptions page's transactions table — same conditional show.

We do not add filtering by BIC/account this phase (YAGNI; surfaces unused at current data volumes). When a customer asks, we add it as a small follow-up.

## 9. Audit

The audit kind enum gains one variant:

```rust
pub enum AuditKind {
    // ... existing ...
    SourceUpdated,
}
```

Wire form: `source.updated` (kebab-case, matches the existing pattern handled by the custom Serialize/Deserialize impls from Phase 5).

The audit payload for `source.updated` is:

```json
{
  "source_id": "...",
  "before": { "name": "...", "format_dialect": "generic" | "subfielded" | null },
  "after":  { "name": "...", "format_dialect": "generic" | "subfielded" | null }
}
```

The new kind is referenced by **no** ISO/SOC2/FCA control item this phase — it's an operational event, not a control-demonstrating one. (Phase 7 follow-up if a customer's auditor demands mapping it.)

## 10. Testing strategy

Strict TDD per item, matching the Phase 6 cadence.

### Backend test inventory

| Item | Test file | Cases |
|---|---|---|
| Migration 0006 | `recon-store/tests/counterparty_schema.rs` (NEW) | Valid 8-char BIC, valid 11-char BIC, invalid BIC (lowercase) rejected, invalid BIC (wrong length) rejected, NULL allowed, round-trip insert+select |
| CSV counterparty | `recon-ingest/tests/csv.rs` (extend) | Mapping with both cols populated, mapping with only BIC col, mapping with neither (both fields `None`), BIC uppercased on extract |
| CAMT.053 counterparty | `recon-ingest/tests/camt053.rs` (extend) | Credit entry: `Cdtr` BIC + IBAN extracted; Debit entry: `Dbtr` BIC + IBAN extracted; missing `RltdPties` → both `None`; non-IBAN account via `<Othr>/<Id>` |
| MT940 subfielded counterparty | `recon-ingest/src/mt940.rs` (extend existing tests) | `?32` populates `counterparty_account`; `?33` populates `counterparty_bic`; absent subfields → both `None`; Generic dialect always `None` |
| MT942 generic | `recon-ingest/src/mt942.rs` (8 tests, NEW file) | Single message, multi-message, `:34F:` floor-limit lines skipped, `:13D:` date/time fallback, `:90D:/:90C:` sanity check passes when sums match, `:90D:/:90C:` sanity check fails when sums diverge (parse error), tag-order error rejected, Latin-1 byte fallback |
| MT942 subfielded | `recon-ingest/src/mt942.rs` (1 test, NEW) | Subfielded `:86:` parses `?00`/`?20`–`?29`/`?32`/`?33` exactly as MT940 does |
| MT942 via API | `recon-api/tests/ingest_api.rs` (extend) | MT942 fixture round-trips through `/sources/:id/ingest` and lands canonical transactions |
| PATCH `/sources/:id` | `recon-api/tests/api.rs` (extend) + `recon-store/tests/patch_source.rs` (NEW) | 200 rename only; 200 set dialect to subfielded; 200 clear dialect (`null`); 200 both changes; 200 empty body no-op; 400 invalid dialect; 400 empty / oversized name; 404 cross-tenant; 403 non-admin; audit row emitted with diff payload; rolled back UPDATE on audit failure |
| Concurrent appender | `recon-store/tests/audit_concurrent_appender.rs` (NEW) | 50 same-tenant parallel appends → all unique sequences `1..=50`, chain verifies clean; 25+25 interleaved across two tenants → both chains independently valid |

### Frontend test inventory

| Item | Test file | Cases |
|---|---|---|
| MT942 in upload dialog | `tests/upload-dialog.test.tsx` (extend) | Renders as 5th option; selection sets `format=mt942` on submit; CSV mapping fields hidden when MT942 picked; amber dialect-missing notice shown for null-dialect sources |
| Edit source dialog | `tests/edit-source-dialog.test.tsx` (NEW) | Renders pre-filled values; submits patch with only changed fields; closes on success and refetches; "Cancel" resets the form |
| Sources page edit | `tests/sources-page.test.tsx` (extend) | "Edit" button appears for admins; not visible for operators; click opens the dialog |
| Audit page split | (no new tests — existing tests are the regression gate) | All existing audit-page unit/component tests pass unchanged; `tests/e2e/compliance.spec.ts` passes unchanged |
| Counterparty columns | `tests/run-detail.test.tsx` (extend) and `tests/exceptions-page.test.tsx` (extend) | Columns hidden when every row's both fields are null; columns shown otherwise; values render verbatim |

### E2E

No new e2e scenarios. The existing `compliance.spec.ts` covers the audit page refactor (its acceptance gate). MT942 ingest and PATCH are well-covered by API + component tests; an e2e for them would be cargo-cult coverage.

## 11. Migration safety

`0006_transactions_counterparty.sql` is **purely additive**:

- Two nullable columns; Postgres adds these without a table rewrite — effectively instant on any table size.
- One CHECK constraint that's trivially satisfied by all existing rows (every NULL passes the constraint's `IS NULL` branch).
- No backfill, no data movement, no index creation.
- **Rollback:** drop the CHECK constraint, then drop both columns. Safe in isolation.
- **Forward/backward compat:** the parsers always emit `None` for counterparty fields when the source format doesn't supply them. If the new migration runs against the **old** binary, the old binary continues to function (it doesn't know about the new columns; sqlx ignores them on existing INSERTs). If the new binary runs against a database that hasn't migrated yet, sqlx's compile-time-checked queries fail loudly — that's deliberate and matches the project's existing migration discipline.

## 12. Sequencing, commits, and PR

Branch `feat/phase7-polish-bundle` off master. Commit at each TDD boundary, matching the Phase 6 cadence. Expected commits (~12):

```
chore(store): migration 0006 — counterparty_bic + counterparty_account on transactions
feat(domain): CanonicalTransaction gains counterparty_bic + counterparty_account
feat(ingest/csv): optional counterparty_bic_col + counterparty_account_col in mapping
feat(ingest/camt053): extract counterparty BIC + account from RltdPties
feat(ingest/mt940): map ?32/?33 subfields into counterparty_account / counterparty_bic
refactor(ingest): extract mt94x_shared helpers from mt940 (decode_with_fallback, parse_61_line, parse_subfielded_86, Mt94xDialect)
feat(ingest): MT942 parser (generic + subfielded) reusing mt94x_shared
feat(api): PATCH /sources/:id (name, format_dialect); audited as source.updated
feat(web): edit-source-dialog + "Edit" row action on sources page
feat(web): MT942 in upload dialog (4 formats → 5)
test(audit): concurrent-appender stress test — 50 parallel appends, chain integrity holds
refactor(web): split audit/page.tsx (853 → ~100 LOC shell + 5 focused components)
```

One PR for the whole bundle, titled **"Phase 7 — Phase 5/6 polish: MT942 + counterparty fields + PATCH source + concurrent-appender test + audit page split"**. Single PR because all five items are thematically "follow-ups on prior phases" and reviewers benefit from seeing them together. If any single item is contested during review, the per-item commits make peeling it off trivial.

## 13. Risks and mitigations

| Risk | Mitigation |
|---|---|
| `0006` migration runs in production against a busy `transactions` table | Migration is metadata-only (nullable column + non-validated CHECK); Postgres takes a brief ACCESS EXCLUSIVE lock but does no table rewrite. |
| MT942 parser emits malformed transactions because of an undocumented bank quirk | Same risk class as Phase 6 MT940; mitigated by the same Latin-1 fallback + per-line error reporting + per-file rejection model. |
| `parse_subfielded_86` repurposed for MT942 introduces a subtle regression for MT940 | The refactor moves the function unchanged into `mt94x_shared`; existing MT940 tests gate the move. |
| audit/page.tsx split silently changes behaviour | All existing tests gate the refactor; if any single test needs updating, that's evidence of a behavioural drift — block and investigate. |
| `serde` double-`Option` for the PATCH endpoint trips up callers | We document the contract in the spec + add explicit tests covering each of the four states (absent / `null` / `"generic"` / `"subfielded"`). |
| Concurrent-appender test is flaky on slow CI | Use `tokio::join_all` rather than ad-hoc parking; assert on sequence-set equality (deterministic) rather than ordering (non-deterministic). |

## 14. Definition of done

- All five items merged on master via a single PR.
- All backend tests pass: existing 184 + new ~25 ≈ 209 tests.
- All frontend tests pass: existing 194 + new ~10 ≈ 204 tests.
- Existing 16 Playwright e2e tests pass unchanged.
- Local stack continues to boot and serve `/healthz` after migration `0006`.
- `audit/page.tsx` is under 150 LOC.
- Memory file `recon-ui-slice-status.md` updated to reflect Phase 7 completion.
