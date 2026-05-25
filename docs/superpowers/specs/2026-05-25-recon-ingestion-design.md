# Recon Bank-Format Ingestion Slice — Design

**Date:** 2026-05-25
**Status:** Approved (brainstorming) → ready for implementation plan
**Depends on:** UI slice (`2026-05-23-recon-ui-slice-design.md`), Backend slice (`2026-05-24-recon-backend-slice-design.md`), Auth & RBAC slice (`2026-05-24-recon-auth-rbac-design.md`)

## Goal

Turn the platform from seed-only data into one that reconciles real files. An
operator creates **sources**, uploads bank/ledger files (**CSV** with per-upload
column mapping, or **CAMT.053** ISO 20022 XML), and triggers a reconciliation
**run** over a date window — all through the running UI, feeding the existing
matching engine. Today the only thing that creates transactions or runs is the
seed; this slice adds the real ingestion and run-creation path the seed faked.

## Decisions (locked during brainstorming)

1. **Formats:** CSV (configurable, universal) + CAMT.053 (a real bank standard).
   The parser layer is a trait, so MT940 / BAI2 / PDF are additive later.
2. **Ingestion path:** operator UI upload → multipart `POST` to the API, which
   parses, validates, and stores. Endpoint is also callable programmatically.
3. **Upload → run:** decoupled. Upload loads transactions into a source; a
   separate **New run** action picks source A + source B + date window and
   triggers reconciliation (new `POST /api/runs`).
4. **CSV mapping:** per-upload (transient). The operator maps columns on the
   upload form; nothing is persisted. (Saved per-source profiles are a later slice.)
5. **Malformed rows:** atomic. Validate the whole file; if any row is invalid,
   store nothing and return a full error report so the feed is fixed and re-uploaded.
6. **Dedup:** unique per `(source_id, external_ref)`, enforced by a DB constraint.
   A file containing a ref already loaded — or duplicated within itself — is
   rejected atomically with a report.
7. **Sources:** include lightweight source creation (`name` + `kind` + `currency`)
   so the feature is usable end-to-end from empty sources.

---

## Section 1 — Architecture & code organization

A new **`recon-ingest`** crate holds the parsing layer, kept IO-light and pure so
every format is unit/property-testable in isolation (mirrors how `recon-auth`
isolates security primitives). Layering stays inward-pointing.

**Crate layout:**

- **`recon-ingest`** (new) — depends only on `recon-domain`:
  - A `Parser` trait: `parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>>`.
  - `ParsedTxn` — a parser output draft (no id / tenant / source yet).
  - `RowError { row, field, message }` — collected for the atomic report.
  - `csv` module — `CsvParser` driven by a per-upload `CsvMapping` spec.
  - `camt053` module — `Camt053Parser` (self-describing; no mapping).
- **`recon-store`** — new methods: `create_source`, `list_sources`,
  `ingest_transactions` (atomic insert with conflict → reject), `create_run`
  (loads windows → `reconcile()` → persists run + decisions + breaks + cases).
  The run-persistence logic currently inlined in `seed.rs` is factored into a
  shared helper that both `seed` and `create_run` call.
- **`recon-api`** — new routes (Section 4); a thin mapper turns `ParsedTxn` →
  `CanonicalTransaction` (assigns ids, tenant/source, default `posted_at` from
  `value_date`, default currency from the source). This keeps `recon-store`
  independent of `recon-ingest`.
- **`web`** — a new **Sources** screen (list + New source + per-source Upload
  dialog with the mapping form and result/error report) and a **New run** dialog
  on the Runs list.

**Dependency direction:** `recon-domain` → `recon-ingest` → `recon-api`;
`recon-store` → `recon-matching` / `recon-domain`; `recon-api` → `recon-ingest`,
`recon-store`, `recon-matching`. The API is the only place that knows about both
ingest and store.

---

## Section 2 — Data model & migration

No new tables — only a uniqueness guarantee. **Migration `0003_ingest.sql`** adds:

```sql
ALTER TABLE canonical_transactions
  ADD CONSTRAINT uq_txn_source_ref UNIQUE (source_id, external_ref);
```

This backs the dedup decision: re-uploading a statement collides and is rejected.
Seed data already uses distinct `external_ref`s per source, so the constraint
applies cleanly to fresh DBs.

Existing schema is otherwise unchanged. Reference points:

- `sources(id, tenant_id, kind, name, currency, created_at)` — `currency` becomes
  the default for CSV rows that do not map a currency column.
- `canonical_transactions(id, tenant_id, source_id, external_ref, value_date,
  posted_at, amount_minor, currency, direction, counterparty, description,
  created_at)` — the ingest target.
- `reconciliation_runs`, `match_decisions`, `breaks`, `cases` — written by
  `create_run` exactly as the seed writes them today.

---

## Section 3 — Parser layer (`recon-ingest`)

One trait, two implementations, all pure and atomic:

```rust
pub trait Parser {
    /// Parse raw file bytes. On ANY row error, returns Err with the full list
    /// (atomic: caller stores nothing). Ok => every row parsed cleanly.
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>>;
}

pub struct ParsedTxn {
    pub external_ref: String,        // dedup key within a source
    pub value_date: String,          // "YYYY-MM-DD"
    pub posted_at: Option<String>,   // RFC3339; defaults to value_date T00:00:00Z
    pub amount_minor: i64,           // non-negative magnitude
    pub currency: Option<String>,    // defaults to the source's currency
    pub direction: Direction,        // Debit | Credit (from recon-domain)
    pub counterparty: Option<String>,
    pub description: String,
}

pub struct RowError { pub row: usize, pub field: String, pub message: String }
```

**`CsvParser`** is driven by a per-upload spec the operator fills in on the upload
form:

```rust
pub struct CsvMapping {
    pub has_header: bool,
    pub delimiter: u8,               // default b','
    pub external_ref: ColRef,
    pub value_date: ColRef,
    pub date_format: String,         // chrono fmt, e.g. "%d/%m/%Y" or "%Y-%m-%d"
    pub amount: AmountMapping,
    pub description: ColRef,
    pub currency: Option<ColRef>,
    pub counterparty: Option<ColRef>,
}
pub enum ColRef { Index(usize), Header(String) }
pub enum AmountMapping {
    // one signed column; sign picks direction (configurable which sign = debit)
    Signed { column: ColRef, debit_when_negative: bool },
    // two columns, exactly one populated per row
    DebitCredit { debit: ColRef, credit: ColRef },
}
```

Decimal amounts (`"1,234.56"`, `"(50.00)"`) are normalized (strip thousands
separators; treat parentheses as negative) and scaled to **minor units at 2
decimal places** — the slice's assumption; per-currency scale is a later concern.
Failure cases that produce a `RowError` (and reject the whole file): unparseable
date for the given `date_format`, unparseable amount, both/neither of the
debit/credit columns populated, a `ColRef` out of range or a missing header, or an
empty required field. `external_ref` must be present and non-empty per row.

**`Camt053Parser`** reads ISO 20022 XML with `quick-xml`, **entity expansion
disabled** (no XXE / billion-laughs). Per `<Ntry>`:

- `<Amt Ccy="…">` → `amount_minor` (scaled) + `currency`.
- `<CdtDbtInd>` (`CRDT` / `DBIT`) → `direction`.
- `<ValDt><Dt>` → `value_date`; `<BookgDt><Dt>`/`<DtTm>` → `posted_at`.
- `<AcctSvcrRef>` (fallback `<NtryRef>`) → `external_ref`.
- `<RmtInf>` (fallback `<AddtlNtryInf>`) → `description`.

A document that is not well-formed XML, or an `<Ntry>` missing a required element,
yields a `RowError` (entry index as `row`) and rejects the file.

---

## Section 4 — API surface

All new routes are authed, gated by a new `Permission::ManageData` (granted to
operator / approver / admin — operational write), and tenant-scoped (a source not
in the caller's tenant → 404).

| Method · Path | Body → Result | Notes |
|---|---|---|
| `GET /api/sources` | → `Source[]` (with txn count) | Powers the Sources screen + run-creation pickers |
| `POST /api/sources` | `{kind, name, currency}` → `Source` | `kind` = `bank` \| `ledger` \| `cross_system` |
| `POST /api/sources/:id/ingest` | multipart: `file` + `format` (`csv`\|`camt053`) + `mapping` (JSON, CSV only) → `IngestResult` | Atomic; see below |
| `POST /api/runs` | `{name, sourceAId, sourceBId, from, to}` → `ReconciliationRun` | Runs `reconcile()` over the window, persists run + decisions + breaks + cases |

**`IngestResult` / errors** — success and both failure modes are explicit so the
UI can render a precise report:

- **200** `{ ingested: <n>, sourceId }` — all rows stored in one transaction.
- **422** `{ error: "parse", rows: [{row, field, message}, …] }` — bad rows;
  nothing stored.
- **409** `{ error: "duplicate", refs: [<external_ref>, …] }` — refs already
  present in the source, or duplicated within the file; nothing stored.

`POST /api/runs` returns **404** if either source is not in the caller's tenant,
and **400** if `from`/`to` are not valid `YYYY-MM-DD` or `to` precedes `from`.

A request **body-size limit** (`DefaultBodyLimit`, 10 MB) guards the upload
endpoint against memory exhaustion. `format` is an allowlisted enum; the CSV
`mapping` is validated (column refs in range / header present) before parsing.

---

## Section 5 — Frontend

A new **Sources** area plus a **New run** action, reusing the existing
list/table/dialog components and the `ApiClient` seam.

- **`/sources` screen** (`app/(app)/sources`) — a table of the tenant's sources
  (name, kind, currency, transaction count) with two actions:
  - **New source** dialog → `{kind, name, currency}` → `POST /api/sources`.
  - **Upload** dialog (per row) → pick **format** (CSV / CAMT.053); for CSV,
    reveal the mapping form (delimiter, has-header, a field→column picker for each
    canonical field, the amount-encoding choice, date format). Submits
    `multipart/form-data`. Renders the `IngestResult`: a success summary
    (*"N transactions ingested"*) or the error report — bad rows with reasons
    (422) or colliding refs (409).
- **New run** dialog on the **Runs list** (`/runs`) → pick source A, source B, a
  date range, and a name → `POST /api/runs` → navigate to the new run's detail page.
- **`ApiClient` additions** (and `MockApiClient` test double): `listSources`,
  `createSource`, `ingestFile` (uses `FormData`), `createRun`.
  `HttpApiClient.ingestFile` sends `FormData` (no JSON `Content-Type`) but keeps
  the `Authorization: Bearer` header and the 401 → refresh → retry behavior.
- **Navigation** — add a **Sources** link to the app nav. The Sources screen,
  Upload, New source, and New run actions are gated through the same `useAuth()`
  role check used elsewhere (visible to roles with `ManageData` — all current
  roles, but it tightens cleanly later).

---

## Section 6 — Security & testing

**Security:**

- **RBAC + tenant isolation:** every new route requires `ManageData` and scopes
  to the token's `tid`; ingesting into / running against a source you don't own
  returns 404.
- **Upload hardening:** body-size limit; `format` is an allowlisted enum; the CSV
  mapping is validated before parsing; CAMT.053 XML parsed with entity expansion
  off (no XXE / billion-laughs).
- **Atomicity:** ingest and run-creation each run in a single DB transaction — a
  rejected file or a failed run leaves no partial state.
- **No code/secret exposure:** parsing is pure data transformation; files are
  parsed in memory and not persisted to disk.

**Testing (TDD throughout):**

- **`recon-ingest`** (unit + property): CSV — signed vs debit/credit encodings,
  multiple date formats, decimal→minor scaling, `(parens)`/thousands separators,
  header-vs-index refs, and every bad-row case returning the full `RowError` list
  atomically; CAMT.053 — a real sample document → expected `ParsedTxn`s, malformed
  XML → error. Property: parsed `amount_minor` is always non-negative; `direction`
  always set.
- **`recon-store`** (`#[sqlx::test]`): `create_source`, `list_sources` (with
  counts), `ingest_transactions` happy path, collision rejection (existing ref +
  within-file dup), `create_run` produces run + decisions + breaks + cases, and
  tenant-scoping guards.
- **`recon-api`** (integration): full flow — create source → ingest CSV → ingest
  CAMT.053 → create run → `GET /api/runs/:id` shows matches/breaks; multipart
  handling; 422 bad-file and 409 duplicate reports; RBAC 403 / cross-tenant 404;
  body-limit rejection.
- **Frontend** (vitest): Sources table, New-source form, Upload dialog (mapping
  inputs + success and error-report rendering), New-run dialog; `MockApiClient`
  extensions.
- **E2E** (Playwright, live stack): operator creates two sources, uploads a CSV
  fixture into one and a CAMT.053 fixture into the other, creates a run over the
  window, and sees matched/unmatched results — proving the whole pipeline.

---

## Out of scope (candidate later slices)

- MT940 / BAI2 / PDF parsers (the `Parser` trait makes them additive).
- Saved per-source mapping profiles (this slice maps per upload).
- Watched-folder / SFTP connectors and scheduled ingestion.
- Per-currency decimal scales (this slice assumes 2 dp).
- Very large / streaming file ingestion (this slice parses in memory under a
  body-size limit).
