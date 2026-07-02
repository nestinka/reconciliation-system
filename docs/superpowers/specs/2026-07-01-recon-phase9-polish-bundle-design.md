# Phase 9 — Polish Bundle (Design)

**Date:** 2026-07-01
**Status:** Approved design, pending implementation plan
**Branch:** `feat/phase9-polish-bundle`
**Slice:** Four isolated polish sub-features shipped in one branch with per-item commits (like the Phase 7 bundle).

## 1. Goal & scope

Clear four deferred polish items from the platform backlog, each as a self-contained, independently-testable sub-feature:

1. **Counterparty-aware matching** — feed the already-populated (but ignored) counterparty identifier fields into the match score.
2. **Format auto-detection** — a `format=auto` upload mode that sniffs the file to pick a parser.
3. **Per-upload dialect/profile override** — optionally override a source's stored MT94x dialect / PDF profile for a single upload.
4. **Soft-delete / archive sources** — retire a source without deleting its history.

**Out of scope (deferred):** more bank profiles / dialects (the registries already support them; adding entries is a documented, fixture-driven follow-up), fuzzy counterparty-name matching, CSV auto-detection, format auto-detection replacing the explicit format field.

Next migration number is `0008`. As of `master` HEAD the backend has ~138 unit tests + 3 proptest properties (200 cases each); the matching engine is deterministic and replayable.

## 2. Sub-feature 1 — Counterparty-aware matching

### 2.1 Current state
`recon-matching/src/score.rs` `score_pair(a, b) -> f64`:
- Hard gate: returns `0.0` on direction or currency mismatch.
- `0.6·amount + 0.3·date + 0.1·ref`, where amount = `1.0 - |Δamount|/amount_a`, date = `1.0 - |Δdays|/30` (clamped), ref = `1.0` iff `external_ref` equal.
- `counterparty` / `counterparty_bic` / `counterparty_account` are never referenced (deliberately YAGNI'd). Every parser populates them where available (CSV mapping, CAMT.053 `<RltdPties>/<RltdAgts>`, MT94x Subfielded `?32`/`?33`).

### 2.2 Design — exact-identifier signal
Add a counterparty term to `score_pair`:
- `cpty = 1.0` if (`bic_a` and `bic_b` both non-empty and equal) **OR** (`account_a` and `account_b` both non-empty and equal); else `0.0`. The free-text `counterparty` name is **not** used (too noisy; no fuzzy matching — preserves determinism without a similarity algorithm or thresholds).
- **Presence test:** a side "has an identifier" if `counterparty_bic` or `counterparty_account` is `Some(non-empty)`.
- **Graceful degradation:** if *both* sides lack any identifier, drop the counterparty weight and renormalize the remaining weights to today's ratios — so data-less pairs score exactly as they do now (no regression on fixtures without counterparty data).

Weights:
- Both sides have ≥1 identifier: `0.5·amount + 0.25·date + 0.1·ref + 0.15·cpty`.
- At least one side has no identifier: `0.6·amount + 0.3·date + 0.1·ref` (counterparty term omitted; identical to current behavior).

This keeps every existing score in `[0,1]` (sub-scores ∈ `[0,1]`, weights sum to `1.0` in both branches) and is fully deterministic — the three proptest properties (`deterministic_and_replayable`, `no_txn_used_twice_and_conservation`, `scores_in_unit_interval`) continue to hold unchanged.

### 2.3 Versioning
`MatchConfig` version (`recon-matching/src/config.rs`, currently `"v1.0"`) bumps to `"v1.1"`. Runs persist their `config_version` (`recon-store/src/runs.rs`); historical runs keep their immutable `v1.0` decisions, new runs record `v1.1`. No data migration — the change only affects newly-created runs.

### 2.4 Tests
- Update the two `score.rs` unit tests for the new weights (data-less cases must produce the same relative ordering as today).
- Add unit tests: BIC-match boosts score; account-match boosts score; identifier-mismatch does not; one-side-missing falls back to the 3-term formula.
- Proptests unchanged — they must still pass (add counterparty fields to a couple of generated cases to exercise the new branch, keeping the invariants).

## 3. Sub-feature 2 — Format auto-detection

### 3.1 Design
Introduce a `format=auto` value handled in `recon-api/src/routes.rs` `ingest_source` **before** the existing dispatch. A new pure helper `recon_ingest::detect_format(bytes: &[u8]) -> Option<DetectedFormat>` sniffs the leading bytes:

| Format | Signature |
|---|---|
| pdf | starts with `%PDF` |
| camt053 | first non-whitespace/BOM byte is `<` |
| bai2 | first line starts with `01,` or `02,` |
| mt942 | contains `:20:` **and** an MT942-only tag (`:34F:` floor limit, or `:90D:`/`:90C:` totals) |
| mt940 | contains `:20:` and is not MT942 |

- CSV has no reliable signature and requires a column mapping, so `detect_format` never returns CSV. If nothing matches (or the sniff yields CSV-like data), `ingest_source` returns `400` with a message: *"could not auto-detect format; select CSV (with a column mapping) or a specific format explicitly."*
- Once detected, the route dispatches exactly as if the user had named that format — including reading the source's dialect/profile (or the per-upload override from §4). MT94x/PDF still need a dialect/profile; auto-detect resolves them the same way an explicit format does.

### 3.2 Frontend
Upload dialog `IngestFormat` gains `"auto"`; an **"Auto-detect"** SelectItem (listed first). When selected, the format-specific controls (CSV mapping, dialect notice) hide, replaced by a one-line hint. The amber "no profile / dialect set" guidance still applies where relevant (auto-detected MT94x/PDF against a source lacking a dialect/profile behaves like the explicit case).

### 3.3 Tests
- `detect_format` unit tests: one per format signature + the no-match (CSV-like / garbage) → `None` case.
- API integration: upload each of PDF/CAMT/MT940/MT942/BAI2 with `format=auto` and assert the right parser ran; upload CSV-ish bytes with `auto` → 400.

## 4. Sub-feature 3 — Per-upload dialect/profile override

### 4.1 Design
`ingest_source` parses two new **optional** multipart fields: `dialect` (MT94x) and `pdfProfile` (PDF). For each format that consults a source setting:
- **Effective value = override (if provided) else the source's stored value.**
- The override is validated exactly like create/PATCH (`dialect` ∈ {`generic`,`subfielded`}; `pdfProfile` ∈ `profile_names()`); an invalid override → `400`.

This lets an operator ingest an odd file (e.g. a Subfielded MT940 into a source marked Generic) without editing the source. Works with `format=auto` too (auto picks the parser; the override supplies its dialect/profile).

### 4.2 Frontend
Upload dialog: for MT940/MT942/PDF (and auto), an optional **"Override for this upload"** control (a dialect `Select` / PDF-profile `Select`) pre-filled from the source's value but editable. Sent as the `dialect` / `pdfProfile` multipart field only when it differs from the source default.

### 4.3 Audit
Not separately audited beyond the existing `data.ingest.completed` event — the file SHA-256 + `format` already pin the ingest content, and dialect/profile are parse-time metadata (consistent with Phase 8 excluding `pdf_profile` from audit payloads). No `AuditPayload` change.

### 4.4 Tests
- API integration: upload MT940 with `dialect=subfielded` overriding a Generic source and assert subfielded parsing; invalid override → 400.

## 5. Sub-feature 4 — Soft-delete / archive sources

### 5.1 Schema & model
- **Migration `0008_source_disabled.sql`:** `ALTER TABLE sources ADD COLUMN disabled BOOLEAN NOT NULL DEFAULT false;` (mirrors `users.disabled`).
- `Source.disabled: bool` in `recon-domain`; threaded through `SourceRow` + `From`, `create_source` (defaults false), `get_source`, `list_sources` — same mechanical pattern as the Phase 8 `pdf_profile` threading.

### 5.2 Filtering & guards
- `list_sources` **excludes** disabled sources by default. A new `?includeArchived=1` query param (parsed in the route) returns all, so the UI can render an "Archived" view. The new-run source pickers only ever offer enabled sources (they consume the default list).
- `ingest_source` rejects uploads to a disabled source with `409` (`"source is archived"`), checked right after `get_source`.

### 5.3 Archive/restore path & audit
A dedicated path (kept separate from the general PATCH so the audit stays clean and the existing `DataSourceUpdated` payload is untouched):
- `POST /api/sources/:id/archive` and `POST /api/sources/:id/restore` — ManageData-gated. Each sets `sources.disabled` and emits a **new** audit kind inside the same transaction.
- **New `AuditKind::DataSourceArchived`** (wire form `data.source.archived`), payload `AuditPayload::DataSourceArchived { source_id, disabled }` (`disabled=true` for archive, `false` for restore). Adding a *new* variant (never mutating an existing one) means all existing audit chains verify unchanged. Requires the 5 established sync points: `AuditKind` enum, `as_str`, `from_str`, the `AuditPayload` variant, and the variant→kind matcher. The golden-vector test is unaffected (it locks a logout genesis entry, not this kind).

### 5.4 Frontend
- `Source` Zod schema gains `disabled: z.boolean()` (default false for older literals if needed).
- Client/mock/http gain `archiveSource(tenantId, id)` / `restoreSource(tenantId, id)`; `listSources` gains an optional `includeArchived` argument.
- Sources page: an **Archive** row action (→ `restoreSource` when already archived), a **"Show archived"** toggle, and muted styling + an "Archived" badge on disabled rows. Archived sources are hidden from the New-run source selects.

### 5.5 Tests
- Store: `list_sources` hides disabled; `includeArchived` returns them; ingest-to-disabled guard; archive→restore round-trip persists the flag.
- Audit: archiving emits a `data.source.archived` event; chain still verifies.
- API integration: archive a source → it disappears from the default list, 409 on upload, restore brings it back.
- Frontend vitest: archive action calls `archiveSource`; archived toggle shows/hides.

## 6. Cross-cutting summary

- **Migrations:** one (`0008_source_disabled.sql`, additive with a default — safe on populated tables).
- **Crates touched:** `recon-matching` (score + config), `recon-ingest` (`detect_format`), `recon-store` (sources threading + archive), `recon-api` (ingest auto/override, archive routes), `recon-audit` (new kind), `recon-domain` (`Source.disabled`).
- **Audit:** exactly one new `AuditKind` (`DataSourceArchived`); no existing payload mutated; golden vector intact.
- **Frontend:** `IngestFormat` +`"auto"`; `Source` +`disabled`; upload dialog auto + override controls; sources page archive action + archived view.
- **Testing:** score unit + proptest; detect_format unit; store archive/guard; audit new-kind; API integration (auto, override, archive); frontend vitest; one Playwright archive step.
- **Docs:** README formats table notes Auto-detect; a short "Archiving a source" README subsection.

## 7. Footprint & risk notes

- Highest-risk item is counterparty scoring (mutates the audited, replayable matching engine). Mitigated by: exact-identifier-only (deterministic, no fuzzy matching), graceful degradation that leaves data-less scoring identical to today, `config_version` bump so historical runs are untouched, and the proptest invariants preserved.
- The new audit kind is additive-only, preserving chain verifiability — the critical compliance invariant.
- Auto-detect deliberately never guesses CSV, avoiding silent mis-parse of ambiguous data.
