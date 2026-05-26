# Bank-format Ingestion — MT940 + BAI2 (Phase 6 Design)

## Context

The reconciliation platform supports CSV (with per-upload column mapping) and
ISO 20022 CAMT.053 (XML) ingestion as of Phase 4 (PR #1). The `Parser` trait in
`recon-ingest` was designed to be additive — new formats are new module-level
implementations against the same `parse(&[u8]) -> Result<Vec<ParsedTxn>,
Vec<RowError>>` contract.

Two formats remain on the deferred list and are the highest-value next addition:

- **SWIFT MT940** — Customer Statement Message. De facto European bank-statement
  format. Tag-based block structure (`:20:`, `:25:`, `:60F:`, `:61:`, `:86:`,
  `:62F:`, ...). Banks routinely send a week's worth of statements bundled in
  one file.
- **BAI v2 (BAI2)** — Bank Administration Institute version 2. De facto US-bank
  cash-management format. Record-based, type-coded; supported by Bank of America,
  JPMorgan, Wells Fargo, and others.

This slice adds both, plus a per-source dialect for MT940 (Generic vs DE/NL/BE
subfielded `:86:`).

## Scope

In:
- `Mt940Parser` with two dialects: **Generic** (`:86:` treated as opaque
  description) and **Subfielded** (`?20`/`?21`/.../`?29` codes extracted into
  structured `description`/`counterparty`).
- `Bai2Parser` — single dialect; type-code → direction via a built-in static
  table.
- New `format_dialect` column on `sources` (nullable; `'generic' | 'subfielded'`
  for MT940; NULL for other formats).
- Source creation UI gains a dialect select.
- Upload dialog gains MT940 and BAI2 in the format dropdown.
- Backend + frontend types updated to reflect the wider `IngestFormat` union.

Out:
- **MT942** (intra-day update). Same tag structure as MT940 but operational use
  case (reconciliation runs against finalized statements). YAGNI.
- **PDF** parsing. OCR is its own slice with its own quality bar.
- Bank-name-specific dialects (Deutsche Bank–specific, ING-specific, etc.).
  Each named dialect is its own maintenance burden; the Subfielded variant
  covers ~80% of European MT940 traffic with one code path.
- Per-upload dialect override. The source's stored `format_dialect` is
  authoritative; if it's NULL and the user uploads MT940, the parser defaults
  to Generic and the UI surfaces a notice.
- Auto-detection of format from file bytes. The user always picks the format
  at upload time (matches existing CSV/CAMT.053 affordance).
- Balance validation (cross-checking `:60F:` opening balance vs sum of `:61:`
  movements vs `:62F:` closing balance). MT940's balance arithmetic doesn't
  always close exactly due to forex/fees not broken out per movement; surfacing
  this as an error would be noisy.

## Architecture

### File structure

```
backend/crates/recon-ingest/src/
├── lib.rs               (existing — Parser trait, ParsedTxn, RowError)
├── csv.rs               (existing)
├── camt053.rs           (existing)
├── mt940.rs             (NEW)
└── bai2.rs              (NEW)
```

Each new parser is a struct implementing `Parser` with its own config struct.
Module boundaries are tight: no cross-parser sharing of helpers beyond what's
already in `recon-ingest::money` and the common `RowError` constructor.

### Parser shapes

```rust
// mt940.rs
pub enum Mt940Dialect { Generic, Subfielded }

pub struct Mt940Parser { pub dialect: Mt940Dialect }

impl Parser for Mt940Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> { ... }
}

// bai2.rs
pub struct Bai2Parser;   // no config; single dialect

impl Parser for Bai2Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> { ... }
}
```

### MT940 parsing strategy

1. Decode bytes as UTF-8; on failure, fall back to Latin-1 (always succeeds —
   Latin-1 maps every byte). Note this in the error envelope if Latin-1 was
   used.
2. Walk lines. A "message" begins at a `:20:` tag and ends at the following
   `:62F:` or `:62M:` tag (closing balance, final/intermediate).
3. Within a message, accumulate `:61:` (statement line) records. Each `:61:`
   is followed by an optional `:86:` (information to account owner — typically
   the description).
4. For each `:61:`:
   - Parse `YYMMDD` value-date.
   - Optional `MMDD` entry-date (skip; value-date is canonical).
   - D/C mark: `D` (debit), `C` (credit), `RD` (reverse debit), `RC` (reverse
     credit). Direction is `debit` for D/RC, `credit` for C/RD (reverse marks
     flip direction).
   - Amount: comma-decimal (European) or period-decimal. Parse via existing
     `parse_decimal_to_minor`.
   - Transaction type code: 3 letters following amount. Captured as part of
     description but not interpreted.
   - Customer reference: chars after type code, before `//`.
   - Bank reference: chars after `//`. Optional.
   - external_ref = customer-ref (preferred) ?? bank-ref. Both absent → row
     error.
5. For the following `:86:`:
   - **Generic dialect**: store the full `:86:` text as `description`.
   - **Subfielded dialect**: walk `?nn` separators. Common subfield codes:
     - `?20`–`?29`: free-form description lines (concatenate with spaces)
     - `?30`: BIC of counterparty bank
     - `?31`: account number of counterparty
     - `?32`–`?33`: counterparty name (concatenate)
     - `?34`: textual reason code
     - Unrecognized subfields preserved into description for round-tripping.
6. Multi-message files: keep accumulating across `:20:` boundaries.
7. Produce `Vec<ParsedTxn>`. Row errors collected; atomic reject if any.

### BAI2 parsing strategy

1. Decode bytes as ASCII (BAI2 is strict ASCII; non-ASCII bytes → row error).
2. Walk lines. Each line is comma-separated fields, terminated by `/`. Some
   lines have a `,` before `/` for empty trailing fields; this is fine.
3. Track current `02` group and `03` account context.
4. For each `16` (Transaction Detail) record:
   - Type code (3-digit BAI standard): map to direction via static table.
     The static table is built into `bai2.rs` and covers the common ~50 codes
     (e.g. `175` Other Deposit Item = credit; `475` Other Disbursement = debit;
     full mapping in the implementation).
   - Amount: digits with no decimal point (cents). Convert to minor units
     directly (no parse_decimal needed).
   - Customer reference: field 5 (1-based) of the `16` record.
   - Bank reference: field 4.
   - external_ref = customer-ref (preferred) ?? bank-ref. Both absent → row
     error.
   - Description: field 6 + any subsequent `88` Continuation records merged.
5. Produce `Vec<ParsedTxn>`. Atomic reject as above.

### Encoding handling

- **MT940**: try UTF-8 first; on `Utf8Error`, fall back to Latin-1. This is
  pragmatic: banks send mixed encodings and Latin-1 always decodes (lossless
  byte→char map). The fallback is silent; no user-facing toggle.
- **BAI2**: strict ASCII. Non-ASCII bytes return a row error pointing to the
  byte offset.

### Multi-message MT940

A single file may contain multiple `:20:`...`:62F:` blocks. All transactions
across all blocks fold into one upload. Each transaction has its own
`external_ref` for dedup; re-uploading the file returns 409 with the duplicate
refs (same envelope as CSV/CAMT.053).

### `external_ref` derivation summary

| Format    | Primary           | Fallback        | Both absent |
| --------- | ----------------- | --------------- | ----------- |
| CSV       | column mapping    | (none)          | row error   |
| CAMT.053  | `<EndToEndId>`    | `<TxId>`        | row error   |
| MT940     | `:61:` customer-ref | `:61:` bank-ref | row error   |
| BAI2      | `16` customer-ref | `16` bank-ref   | row error   |

## Data model

### Migration `0005_format_dialect.sql`

```sql
-- Phase 6: bank-format dialect annotation on sources.
-- Only meaningful for MT940 sources today. NULL for all other formats.
ALTER TABLE sources ADD COLUMN format_dialect TEXT NULL;
ALTER TABLE sources ADD CONSTRAINT chk_format_dialect
  CHECK (format_dialect IS NULL OR format_dialect IN ('generic', 'subfielded'));
```

Additive; safe to apply to any populated DB. No data migration needed
(existing rows get NULL).

### Store types

```rust
// recon-domain (or wherever Source lives — currently recon-domain)
pub struct Source {
    pub id: String,
    pub tenant_id: String,
    pub kind: SourceKind,
    pub name: String,
    pub currency: String,
    pub format_dialect: Option<String>,   // NEW
}

// IngestFormat is currently in recon-store / API DTO
pub enum IngestFormat { Csv, Camt053, Mt940, Bai2 }  // extended
```

### Store API changes

`create_source` signature gains an optional `format_dialect`:

```rust
pub async fn create_source(
    &self,
    tenant_id: &str,
    kind: SourceKind,
    name: &str,
    currency: &str,
    actor_id: &str,
    format_dialect: Option<&str>,   // NEW
) -> Result<Source, StoreError>
```

`list_sources` / `get_source` already SELECT-star equivalent; the new column
flows out automatically once the `Source` struct gains the field.

`ingest_transactions` is unchanged structurally — the parser config (MT940
dialect) is resolved at the API layer from the source's stored
`format_dialect` and passed in via the constructed parser instance.

## API surface

### Routes (unchanged URLs)

- `POST /api/sources` body gains optional `formatDialect`:
  ```json
  { "kind": "bank", "name": "Acme Bank Statement", "currency": "GBP",
    "formatDialect": "subfielded" }
  ```
  Validation: if present, must be `"generic"` or `"subfielded"`; else 400 with
  `code: "invalid_dialect"`.

- `POST /api/sources/:id/ingest?format=mt940|bai2` — same handler signature;
  the handler now branches on the four formats. For MT940, the handler reads
  the source's `format_dialect` (defaulting to `"generic"` if NULL) and
  constructs an `Mt940Parser` with that dialect.

- `GET /api/sources` and `GET /api/sources/:id` — `Source` DTO now includes
  `formatDialect: string | null`.

### Error envelope

Unchanged. Parse errors continue to flow through the existing 422 with
`details.rows: [{ row, field, message }]`. The `row` field for MT940 is the
line number; for BAI2 it's the line number of the offending record (or `0` for
file-level errors).

For BAI2 ASCII-encoding failures, the row error reports `field: "encoding"`,
message `"non-ASCII byte at offset N"`.

For MT940 fallback to Latin-1, no error is emitted (silent).

### Wire types (frontend `client.ts`)

```ts
export type SourceKind = "bank" | "ledger";
export type FormatDialect = "generic" | "subfielded";

export interface Source {
  id: string;
  tenantId: string;
  kind: SourceKind;
  name: string;
  currency: string;
  formatDialect: FormatDialect | null;   // NEW
}

export interface CreateSourceInput {
  kind: SourceKind;
  name: string;
  currency: string;
  formatDialect?: FormatDialect | null;   // NEW (optional)
}

export type IngestFormat = "csv" | "camt053" | "mt940" | "bai2";   // extended
```

The existing `IngestError`, `IngestResult`, etc. are unchanged.

## Frontend

### New-source dialog

Add a select labeled "MT940 dialect (optional)" with three options:
- **Not applicable** (default; sends `formatDialect: null`)
- **Generic** (sends `"generic"`)
- **Subfielded — DE/NL/BE** (sends `"subfielded"`)

Help tooltip:
> Set this only if this source will receive MT940 statements. For Deutsche
> Bank, ING, ABN AMRO, Rabobank, and most other European banks, choose
> Subfielded. For other MT940 senders, choose Generic. For CSV / CAMT.053 /
> BAI2 sources, leave as Not applicable.

The field is always visible (not conditionally hidden) — the dropdown is
self-documenting.

### Upload dialog

The format dropdown gains two new options: **MT940** and **BAI2**. For both,
the CSV mapping form (column index mapping, debit/credit encoding) is hidden.

If the user picks MT940 and the source's `formatDialect` is NULL, show an
inline notice:
> This source has no MT940 dialect set. Using Generic. Edit the source to
> choose a dialect.

The notice does not block submission — the parser proceeds with Generic.

### Sources table

For sources with `formatDialect` set, render a small badge next to the source
name: `MT940 · Subfielded` or `MT940 · Generic`. Sources with NULL show no
badge.

## Audit emission

No new `AuditKind` needed. Every successful ingest emits the existing:

```rust
AuditPayload::DataIngestCompleted {
    source_id,
    format: file_format.to_string(),     // "mt940" or "bai2"
    file_sha256,
    bytes,
    ingested,
}
```

The `format` field's value space simply extends to include `"mt940"` and
`"bai2"`. Same-tx wiring in `ingest_transactions` carries over.

The `data.source.created` payload (existing) does NOT gain a `format_dialect`
field — dialect is metadata, not a security-relevant decision. If an auditor
wants to know a source's dialect, they query the `sources` table directly.

## Testing

### Backend unit tests (per parser)

`backend/crates/recon-ingest/src/mt940.rs` `#[cfg(test)] mod tests`:
- `happy_path_single_message_five_txns` — golden file → 5 ParsedTxn.
- `multi_message_file_three_statements` — 3 `:20:` blocks → 12 ParsedTxn total.
- `subfielded_86_extracts_counterparty` — Subfielded dialect, `?32`/`?33`
  populate counterparty.
- `generic_86_passed_through_unchanged` — Generic dialect, `:86:` is the
  full description verbatim.
- `customer_ref_missing_falls_back_to_bank_ref` — `:61:` with `//bank-ref`
  only → external_ref = bank-ref.
- `both_refs_missing_returns_row_error` — `:61:` with no reference → RowError.
- `bad_dc_mark_returns_row_error` — `:61:` with unknown D/C mark → RowError.
- `latin1_fallback_decodes` — Latin-1 file with non-UTF-8 bytes parses
  successfully.
- `reverse_marks_flip_direction` — `RC` reverses credit to debit and vice
  versa.

`backend/crates/recon-ingest/src/bai2.rs` `#[cfg(test)] mod tests`:
- `happy_path_single_account_five_txns` — golden file → 5 ParsedTxn.
- `type_code_175_maps_to_credit` and a few representative debit/credit codes.
- `continuation_88_records_merge_into_preceding_16` — `88` lines append to
  the prior `16` description.
- `customer_ref_missing_falls_back_to_bank_ref`.
- `non_ascii_byte_returns_row_error_with_offset`.
- `unknown_type_code_returns_row_error`.

### Backend integration tests

`backend/crates/recon-store/tests/sources.rs` (or wherever source CRUD is
tested):
- `create_source_with_dialect_round_trips` — create with
  `format_dialect: "subfielded"`, get_source returns the same.
- `create_source_with_invalid_dialect_fails_check_constraint` — direct SQL
  insert with `format_dialect = "wat"` fails.

`backend/crates/recon-api/tests/ingest_api.rs` (existing):
- `mt940_happy_path_ingest` — POST `/api/sources/:id/ingest?format=mt940`
  with a fixture file → 200; transactions in DB.
- `bai2_happy_path_ingest` — same, format=bai2.
- `mt940_subfielded_uses_source_dialect` — source has
  `format_dialect: "subfielded"`; upload an MT940 with `?nn` subfields;
  verify counterparty extraction in stored rows.
- `dedup_rejects_reupload` — same MT940 file twice → 409 with refs.

### Frontend tests

`web/tests/upload-dialog.test.tsx` — extend:
- Format dropdown shows MT940 and BAI2.
- Picking MT940 hides the CSV mapping form.
- Picking MT940 on a source with `formatDialect: null` shows the inline
  notice.

`web/tests/new-source-dialog.test.tsx`:
- Dialect select renders all three options.
- Submitting "Subfielded" sends `formatDialect: "subfielded"`.
- Submitting "Not applicable" omits `formatDialect` (or sends `null`).

### E2E test

`web/tests/e2e/ingest.spec.ts` — append one scenario:
- Create a source with `formatDialect: "subfielded"`.
- Upload a fixture MT940 file via the Upload dialog.
- Navigate to the source detail or run page; assert ingested-count > 0.

### Fixture files

Create `backend/crates/recon-ingest/tests/fixtures/`:
- `mt940-single-message.sta`
- `mt940-multi-message.sta`
- `mt940-subfielded.sta`
- `bai2-single-account.bai`
- `bai2-continuation.bai`

Real-world examples scrubbed of sensitive data. Each ≤ 5KB.

## Migration safety

Migration `0005` is purely additive (one nullable column + one CHECK
constraint). No data migration needed. Safe to apply to any populated DB.

Backward compatibility:
- Existing sources have `format_dialect = NULL`. Their behavior is unchanged.
- API responses now include `formatDialect: null` for old sources. Frontend
  handles the null case (no badge rendered).
- The frontend type `Source` adds `formatDialect: FormatDialect | null`; the
  null is the safe default for any deserialized older response.

## Out-of-scope (explicit YAGNI list)

- MT942 intra-day messages
- PDF parsing
- Bank-name-specific dialect tables (Deutsche Bank-specific, etc.)
- Format auto-detection from file bytes
- Per-upload dialect override
- Balance validation (opening + movements = closing)
- Editing a source's `format_dialect` after creation (would require a new
  PATCH endpoint and a "Are you sure? This won't re-parse existing data."
  confirmation flow)
- MT940 cross-message-balance reconciliation
- Multi-currency MT940 (the existing model is one currency per source;
  multi-currency on one statement would require revisiting `Source.currency`)

## Success criteria

- `cargo test --workspace` passes (existing tests unchanged; new tests pass).
- `cargo clippy --workspace -- -D warnings` clean.
- `pnpm -C web tsc --noEmit` clean.
- `pnpm -C web test` passes.
- `pnpm -C web e2e` passes including the new MT940 scenario.
- Migration `0005` applies cleanly to the dev DB.
- A real-world MT940 file (from a European bank — any in the engineer's reach)
  ingests cleanly and produces transactions that match against a parallel
  ledger source in a recon run.
- A real-world BAI2 file (from a US bank — any reachable) ingests cleanly
  similarly.
