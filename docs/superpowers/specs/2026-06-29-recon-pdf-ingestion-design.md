# Phase 8 — PDF Bank-Statement Ingestion (Design)

**Date:** 2026-06-29
**Status:** Approved design, pending implementation plan
**Slice:** Text-layer PDF ingestion via per-source bank profiles; ships the parser framework + one synthetic profile.

## 1. Goal & scope

Add PDF bank statements as a new ingestion format alongside CSV, CAMT.053, MT940, MT942, and BAI v2. Real PDF statements flow through the existing pipeline (`POST /api/sources/:id/ingest` → `Parser` → `ingest_transactions` → reconcile) with the same atomic, fail-loud, per-row-rejection contract as every other parser.

**In scope:**
- Text-layer PDFs only (digitally generated; text is embedded and extractable).
- A generic `PdfParser` (extraction) + a `PdfProfile` registry (per-bank layout knowledge), selected per source.
- One concrete profile, `AcmeBankProfile`, targeting a synthetic columnar statement layout, with committed fixtures.
- Full wiring: migration, domain/store threading, API dispatch + validation, `GET /api/pdf-profiles`, frontend upload/new/edit dialogs, audit, docs, tests, E2E.

**Out of scope (deferred, fail-loud, not silently degraded):**
- Scanned/image PDFs / OCR (no text layer → explicit rejection). Own future slice.
- Additional real bank profiles (the slice ships the framework + one profile + a documented "how to add a profile" pattern).
- Cloud document-AI / external OCR services (financial data must stay on-box).
- PDF geometry/word-position reconstruction beyond what `pdf-extract` yields as line-oriented text.

## 2. Decisions (from brainstorming)

1. **Text-layer only** — pure-Rust extraction, no native libs; keeps the all-Rust local deploy unchanged. Scanned PDFs are rejected, not OCR'd.
2. **Per-source bank profiles** — like MT94x dialects: named layout profiles live in Rust; the source selects one. Not per-upload regex config; not heuristic auto-detect.
3. **One synthetic profile + framework** — `AcmeBankProfile` against a synthetic PDF I generate; documents the add-a-profile pattern. No dependency on real bank files.
4. **New dedicated column** — `sources.pdf_profile`, not an overload of `format_dialect` (which is CHECK-constrained to `generic|subfielded` and means MT94x dialect).
5. **`GET /api/pdf-profiles` endpoint** — profiles live in Rust; the UI fetches the list rather than hard-coding it.
6. **Playwright PDF step** — every prior slice added one E2E; this one extends ingestion coverage with a PDF upload.

## 3. Architecture

### 3.1 Two-stage parsing

`PdfParser` is generic; per-bank knowledge lives behind the `PdfProfile` trait.

```
bytes (PDF)
  └─ pdf-extract → raw text  ──(no text layer)──► Err[ RowError{row:0, field:"document",
  └─ normalize_lines → Vec<String>                     message:"no extractable text layer (scanned PDF?)"} ]
       └─ profile.parse_lines(&lines) → Result<Vec<ParsedTxn>, Vec<RowError>>
```

New module `backend/crates/recon-ingest/src/pdf.rs` (~250 LOC):

```rust
pub struct PdfParser { pub profile: Box<dyn PdfProfile> }

pub trait PdfProfile {
    fn name(&self) -> &'static str;
    /// Map already-extracted, normalized lines → transactions. Atomic & fail-loud.
    fn parse_lines(&self, lines: &[String]) -> Result<Vec<ParsedTxn>, Vec<RowError>>;
}

impl Parser for PdfParser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        let text = extract_text(bytes)?;        // pdf-extract; map failure → RowError row:0
        let lines = normalize_lines(&text);     // trim each line, drop blank lines, preserve order
        if lines.is_empty() {
            return Err(vec![RowError {
                row: 0, field: "document".into(),
                message: "no extractable text layer (scanned PDF?)".into(),
            }]);
        }
        self.profile.parse_lines(&lines)
    }
}

pub fn resolve_profile(name: &str) -> Option<Box<dyn PdfProfile>> {
    match name {
        "acmebank" => Some(Box::new(AcmeBankProfile)),
        _ => None,
    }
}

pub fn profile_names() -> &'static [&'static str] { &["acmebank"] }
```

`profile_names()` is the single source of truth shared by `resolve_profile`, API validation, and `GET /api/pdf-profiles`.

### 3.2 `AcmeBankProfile`

Targets a clean columnar statement: `Date | Description | Ref | Money-in | Money-out`. Example transaction lines after extraction:

```
12/03/2026  CARD PURCHASE TESCO STORES 1234  REF A1B2C3      —      45.20
13/03/2026  FASTER PAYMENT FROM J SMITH      REF Z9Y8X7   500.00      —
```

Logic:
- A header/anchor row (e.g. a line containing `Date` and `Description`) marks the start of the transaction table; lines before it (statement metadata) are skipped.
- Each transaction row is matched with a regex capturing `date · description · ref · money_in · money_out`.
- `money_in` present → `Direction::Credit`; `money_out` present → `Direction::Debit`. Exactly one of the two columns is populated per row; both-empty or both-populated → `RowError` (`field:"amount"`). This is the two-column encoding the codebase already models as `debitCredit`.
- `date` parsed with fixed `%d/%m/%Y` (UK statement convention) → `value_date` ISO `YYYY-MM-DD`.
- Amounts parsed via the shared `parse_decimal_to_minor` helper → `amount_minor: i64`.
- `external_ref` = the captured `REF …` token (required; missing → `RowError` `field:"ref"`).
- `description` = the description column, trimmed.
- `currency` = `None` from the parser (the `Source.currency` is authoritative, consistent with the other parsers); `counterparty`/`counterparty_bic`/`counterparty_account`/`posted_at` = `None` for v1 (YAGNI — counterparty extraction can fold into a later polish round).
- Recognized non-transaction lines (page footers, `Balance carried forward`, blank separators) are skipped; any **other** unmatched line inside the table → `RowError` (`field:"row"`, message includes the offending text). Fail-loud, no silent skips.

### 3.3 Extraction crate

`pdf-extract` (pure Rust, wraps `lopdf`) → text with line breaks roughly preserved. No native system libraries; deploy footprint unchanged. **Risk:** real PDFs extract messily — mitigated by (a) text-layer-only scope and (b) the first profile targeting a synthetic PDF we control, so extraction output is predictable. The implementation plan's first step is a spike confirming `pdf-extract` round-trips the generated fixture before profile logic is built on it.

## 4. Fixtures & dependencies

`backend/crates/recon-ingest/Cargo.toml`:
- Runtime: `pdf-extract = "0.7"` (validate exact version during the spike; pin to the resolved version).
- Dev-only: `printpdf = "0.7"` — used by a small `#[ignore]`d generator test/helper that deterministically produces the synthetic fixture, so the committed PDF is reproducible and reviewable rather than an opaque blob.

Fixtures in `backend/crates/recon-ingest/tests/fixtures/` (existing `{format}-{scenario}.{ext}` convention):
- `pdf-acmebank.pdf` — committed synthetic statement (happy path: a few credits + debits).
- `pdf-acmebank.txt` — the expected extracted text, for fast/deterministic profile unit tests that don't invoke `pdf-extract` each run.
- Additional `.txt` fixtures for rejection cases (bad date, both-amount-columns, missing ref, unmatched row) authored directly as text.

One end-to-end test exercises `pdf-acmebank.pdf` bytes → `pdf-extract` → `ParsedTxn` to prove the whole chain; profile-logic tests run on the `.txt` fixtures.

## 5. Schema, domain & store

- **Migration `0007_pdf_profile.sql`** (purely additive, no data backfill):
  ```sql
  ALTER TABLE sources ADD COLUMN pdf_profile TEXT NULL;
  ```
  No CHECK constraint: profile names are validated at the API against `profile_names()` (the registry is the source of truth; avoids a migration each time a profile is added).
- **`recon-domain` `Source`**: add `pub pdf_profile: Option<String>`.
- **`recon-store::sources`**: thread `pdf_profile` through `create_source` / `get_source` / `list_sources` / `update_source`, exactly as `format_dialect` is threaded, including the double-`Option` PATCH semantics (`None`=absent, `Some(None)`=clear, `Some(Some(v))`=set) so it can be set and cleared.

## 6. API (`recon-api`)

- **Ingest dispatch** (`routes.rs`, `ingest_source`): new `format` arm
  ```rust
  "pdf" => {
      let name = source.pdf_profile.as_deref()
          .ok_or_else(|| ApiError::bad_request("source has no PDF profile set"))?;
      let profile = recon_ingest::pdf::resolve_profile(name)
          .ok_or_else(|| ApiError::bad_request("unknown PDF profile"))?;
      recon_ingest::pdf::PdfParser { profile }.parse(&bytes)
  }
  ```
  PDF has no safe generic default (unlike MT940→Generic), so a missing profile is a 400, not a silent fallback. Parse errors flow through the existing envelope unchanged: 422 with `rows` for parse failures, 409 with `refs` for duplicate `(source_id, external_ref)`.
- **Create/PATCH source**: accept and validate `pdf_profile` — `None`, or a name in `profile_names()`; anything else → 400. Mirrors the existing `format_dialect` validation.
- **`GET /api/pdf-profiles`**: returns `{ profiles: ["acmebank"] }` (from `profile_names()`). Read-only; available to any authenticated member (same gate as other read endpoints).
- **Audit**: structurally unchanged. `data.ingest.completed`'s `format` value space extends to include `"pdf"`. `data.source.created` / `data.source.updated` deliberately exclude `pdf_profile` (layout metadata, not security-relevant), consistent with `format_dialect`.

## 7. Frontend (`web/`)

- **`IngestFormat`** union (`web/lib/api/client.ts`) → 6 variants (add `"pdf"`).
- **Upload dialog** (`web/components/app/upload-dialog.tsx`):
  - New `<SelectItem value="pdf">PDF statement</SelectItem>`.
  - When `format === "pdf"`: hide the CSV mapping form; show the source's PDF profile (from `GET /api/pdf-profiles` / the source record). If the source has no `pdf_profile`, show an **amber notice** ("This source has no PDF profile set — edit the source to choose one") mirroring the MT94x dialect notice, and disable submit.
  - File-accept switch extended to `application/pdf` / `.pdf`.
- **New-source + Edit-source dialogs**: a **PDF profile `Select`** (Base UI Select + `__none__` sentinel, the established pattern for the dialect field), populated from `GET /api/pdf-profiles`.
- **Types & client**: `Source` Zod type gains `pdfProfile: z.string().nullable().optional()`; `createSource` / `updateSource` inputs gain `pdfProfile` across the client interface, mock, and HTTP implementations; a `listPdfProfiles()` client method hits `GET /api/pdf-profiles`.

## 8. Error handling

Fail-loud at every layer, matching the existing parsers:
- No text layer (scanned PDF) → single `RowError{row:0, field:"document", …}` → 422.
- Unmatched transaction line in the table → `RowError{field:"row", …}` → 422.
- Bad date / bad amount / both-or-neither money column / missing ref → per-row `RowError` with the specific field → 422.
- Duplicate `(source_id, external_ref)` → `StoreError::DuplicateRefs` → 409 with `refs`.
- Unknown or missing PDF profile → 400 at the API.

## 9. Testing

- **Profile unit tests** (`pdf.rs`): happy path on `pdf-acmebank.txt`; one test per rejection mode (bad date, both money columns, missing ref, unmatched row, empty/no-text).
- **End-to-end extraction test**: `pdf-acmebank.pdf` bytes → `pdf-extract` → expected `ParsedTxn`s (proves the extraction chain).
- **Fixture generator**: `#[ignore]`d helper using `printpdf` that regenerates `pdf-acmebank.pdf` deterministically; documented so the fixture can be rebuilt/reviewed.
- **API integration** (`recon-api/tests/ingest_api.rs`): real multipart PDF upload to a source with `pdf_profile="acmebank"` (success path); missing-profile → 400 case; `GET /api/pdf-profiles` returns the list.
- **Frontend vitest**: new format variant, PDF profile Select, amber missing-profile notice, submit-disabled state.
- **Playwright E2E**: extend the ingestion spec with a PDF upload step (set source profile → upload `pdf-acmebank.pdf` → run → see matched/exception results).

## 10. Documentation

- `docs/` how-to: **"Adding a PDF bank profile"** — implement `PdfProfile`, add a fixture (`.pdf` + `.txt`), one registry line in `resolve_profile`/`profile_names`, and tests.
- `web/README.md` formats table: add a **PDF** row; note the per-source profile requirement.

## 11. Footprint summary

- 1 migration (`0007_pdf_profile.sql`, additive).
- Backend crates touched: `recon-ingest` (new `pdf.rs`), `recon-domain` (`Source`), `recon-store` (`sources`), `recon-api` (`routes`).
- ~250 LOC parser + 1 profile; 1 new runtime dep (`pdf-extract`), 1 dev-dep (`printpdf`).
- ~6 frontend files; new docs + README row.
- Audit, matching engine: unchanged (matching deliberately untouched — YAGNI).
