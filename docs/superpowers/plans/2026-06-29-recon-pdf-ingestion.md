# PDF Bank-Statement Ingestion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add text-layer PDF bank statements as a 6th ingestion format, parsed by per-source bank "profiles", shipping the parser framework plus one synthetic `AcmeBank` profile end-to-end.

**Architecture:** A generic `PdfParser` (in `recon-ingest`) extracts the PDF text layer with `pdf-extract`, normalizes it to lines, and delegates field-mapping to a selected `PdfProfile` (a registry keyed by name). The source carries a new nullable `pdf_profile` column (threaded exactly like the existing `format_dialect`); the ingest route dispatches `format=pdf` to the profile named on the source. Frontend gains a PDF format option, a per-source PDF-profile selector, and an amber "no profile set" guard. Same atomic, fail-loud, per-row-rejection contract as every other parser.

**Tech Stack:** Rust (Axum, sqlx, `pdf-extract` 0.12 runtime, `printpdf` 0.9 dev-only for fixture generation, `chrono`), PostgreSQL, Next.js 16 / React 19 / TypeScript / Zod / react-hook-form / Base UI / TanStack Query, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-06-29-recon-pdf-ingestion-design.md`

---

## File Structure

**Backend — create:**
- `backend/crates/recon-ingest/src/pdf.rs` — `PdfParser`, `PdfProfile` trait, `resolve_profile`/`profile_names`, `AcmeBankProfile`, extraction + line-split helpers, all unit tests.
- `backend/crates/recon-ingest/tests/fixtures/pdf-acmebank.pdf` — generated synthetic statement (committed).
- `backend/crates/recon-ingest/tests/fixtures/pdf-acmebank.txt` — captured extracted text (committed; for fast unit tests).
- `backend/migrations/0007_pdf_profile.sql` — additive `pdf_profile` column.

**Backend — modify:**
- `backend/crates/recon-ingest/src/lib.rs` — `pub mod pdf;`.
- `backend/crates/recon-ingest/Cargo.toml` — add `pdf-extract`, dev `printpdf`.
- `backend/crates/recon-domain/src/types.rs` — `Source.pdf_profile`.
- `backend/crates/recon-store/src/rows.rs` — `SourceRow.pdf_profile` + `From`.
- `backend/crates/recon-store/src/sources.rs` — thread `pdf_profile` through create/get/list/update.
- `backend/crates/recon-api/src/dto.rs` — `CreateSourceReq` / `UpdateSourceReq` gain `pdf_profile`.
- `backend/crates/recon-api/src/routes.rs` — create/patch validation, `pdf` ingest arm, `GET /api/pdf-profiles`.
- Test call-site updates: `recon-store/tests/{format_dialect_schema,audit_schema,ingest,patch_source}.rs`.
- `backend/crates/recon-api/tests/ingest_api.rs` — PDF upload integration tests.

**Frontend — modify:**
- `web/lib/domain/types.ts` — `sourceSchema.pdfProfile`.
- `web/lib/api/client.ts` — `IngestFormat`, `CreateSourceInput`, `UpdateSourceInput`, `listPdfProfiles`.
- `web/lib/api/http.ts` + `web/lib/api/mock.ts` — implement the above.
- `web/components/app/upload-dialog.tsx` — PDF option, amber guard, file-accept.
- `web/app/(app)/sources/page.tsx` — new-source PDF-profile Select.
- `web/components/app/edit-source-dialog.tsx` — edit-source PDF-profile Select.
- `web/tests/*` — vitest for the dialogs.
- `web/e2e/*` + `web/e2e/fixtures/` — Playwright PDF upload step.

**Docs:**
- `docs/adding-a-pdf-bank-profile.md` — how-to.
- `web/README.md` — formats table PDF row.

---

## Task 1: Dependencies, fixture generator & extraction spike

**Purpose:** Confirm `pdf-extract` round-trips a `printpdf`-generated statement before building profile logic on it. This task is the de-risking spike named in the spec.

**Files:**
- Modify: `backend/crates/recon-ingest/Cargo.toml`
- Modify: `backend/crates/recon-ingest/src/lib.rs`
- Create: `backend/crates/recon-ingest/src/pdf.rs`
- Create: `backend/crates/recon-ingest/tests/fixtures/pdf-acmebank.pdf` (generated)
- Create: `backend/crates/recon-ingest/tests/fixtures/pdf-acmebank.txt` (generated)

- [ ] **Step 1: Add dependencies**

In `backend/crates/recon-ingest/Cargo.toml`, under `[dependencies]` add:
```toml
pdf-extract = "0.12"
```
Add a `[dev-dependencies]` entry (the section already exists with `proptest = "1"`):
```toml
printpdf = "0.9"
```

- [ ] **Step 2: Register the module**

In `backend/crates/recon-ingest/src/lib.rs`, add to the module list at the top (after `pub mod mt94x_shared;`):
```rust
pub mod pdf;
```

- [ ] **Step 3: Create the module with the extraction helper only**

Create `backend/crates/recon-ingest/src/pdf.rs`:
```rust
//! Text-layer PDF bank-statement parsing via per-bank profiles.

use crate::{RowError};

/// Extract the PDF text layer. Maps any reader failure to a single document-level
/// RowError (row 0). A scanned/image-only PDF yields empty/whitespace text, which
/// the caller treats as "no text layer".
pub(crate) fn extract_text(bytes: &[u8]) -> Result<String, Vec<RowError>> {
    pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| vec![RowError::new(0, "document", format!("could not read PDF: {e}"))])
}

#[cfg(test)]
mod tests {
    use super::*;

    // Regenerates the committed PDF fixture. Ignored in normal runs.
    // Run with: cargo test -p recon-ingest --lib pdf::tests::generate_acmebank_fixture -- --ignored
    #[test]
    #[ignore = "regenerates the committed PDF fixture"]
    fn generate_acmebank_fixture() {
        use printpdf::*;
        // ASCII-only (builtin fonts are WinAnsi). Columns separated by 4 spaces so
        // the separator survives pdf-extract's glyph-gap-based spacing as >=2 spaces;
        // descriptions use single spaces only.
        let lines = [
            "AcmeBank Statement",
            "Account 12345678",
            "Date    Description    Ref    Amount    Dr/Cr",
            "12/03/2026    CARD PURCHASE TESCO STORES 1234    A1B2C3    45.20    DR",
            "13/03/2026    FASTER PAYMENT FROM J SMITH    Z9Y8X7    500.00    CR",
            "14/03/2026    DIRECT DEBIT BRITISH GAS    D4E5F6    88.10    DR",
            "Balance carried forward    366.70",
        ];
        let mut ops = vec![
            Op::StartTextSection,
            Op::SetTextCursor { pos: Point::new(Mm(12.0), Mm(285.0)) },
            // Courier is monospaced -> predictable spacing on extraction.
            Op::SetFont { font: PdfFontHandle::Builtin(BuiltinFont::Courier), size: Pt(11.0) },
            Op::SetLineHeight { lh: Pt(14.0) },
        ];
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                ops.push(Op::AddLineBreak);
            }
            ops.push(Op::ShowText { items: vec![TextItem::Text(line.to_string())] });
        }
        ops.push(Op::EndTextSection);

        let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
        let mut doc = PdfDocument::new("AcmeBank Statement");
        let bytes = doc
            .with_pages(vec![page])
            .save(&PdfSaveOptions::default(), &mut Vec::new());
        std::fs::write("tests/fixtures/pdf-acmebank.pdf", bytes).expect("write fixture");
    }

    #[test]
    fn pdf_extract_roundtrips_acmebank_fixture() {
        let bytes = std::fs::read("tests/fixtures/pdf-acmebank.pdf").expect("fixture file");
        let text = extract_text(&bytes).expect("extract ok");
        // Sanity: the substantive content survives extraction.
        assert!(text.contains("CARD PURCHASE TESCO STORES 1234"), "got:\n{text}");
        assert!(text.contains("A1B2C3"), "got:\n{text}");
        assert!(text.contains("FASTER PAYMENT FROM J SMITH"), "got:\n{text}");
    }
}
```

- [ ] **Step 4: Generate the PDF fixture**

Run:
```bash
cd backend && cargo test -p recon-ingest --lib pdf::tests::generate_acmebank_fixture -- --ignored
```
Expected: PASS; `backend/crates/recon-ingest/tests/fixtures/pdf-acmebank.pdf` now exists.

> **Spike note:** If `printpdf` 0.9's `Op`/`save` API differs from the code above (it is a fast-moving crate), adjust the generator using the in-tree `printpdf` docs until this test produces a valid PDF. The goal is a PDF whose text layer contains the 7 lines above. If `pdf_extract::extract_text_from_mem` is named differently in 0.12, fix `extract_text` accordingly — the next step will reveal it.

- [ ] **Step 5: Run the spike test to verify extraction works**

Run:
```bash
cd backend && cargo test -p recon-ingest --lib pdf::tests::pdf_extract_roundtrips_acmebank_fixture
```
Expected: PASS. If it fails because the column separators collapsed to single spaces, increase the separator width in the generator (Step 3) to 6 spaces, regenerate (Step 4), and re-run.

- [ ] **Step 6: Capture the extracted text as a committed fixture**

Add a temporary throwaway print to confirm the exact extraction, then save it. Run:
```bash
cd backend && cargo test -p recon-ingest --lib pdf::tests::pdf_extract_roundtrips_acmebank_fixture -- --nocapture 2>/dev/null
```
Then write the extracted text to `backend/crates/recon-ingest/tests/fixtures/pdf-acmebank.txt` by extending the spike test once to dump it:

Temporarily add to the end of `pdf_extract_roundtrips_acmebank_fixture`:
```rust
        std::fs::write("tests/fixtures/pdf-acmebank.txt", &text).unwrap();
```
Run the test once to write the file, then **remove that line** (the `.txt` is now committed; tests must not rewrite it on every run).

- [ ] **Step 7: Commit**

```bash
cd backend && git add crates/recon-ingest/Cargo.toml crates/recon-ingest/Cargo.lock ../backend/Cargo.lock crates/recon-ingest/src/lib.rs crates/recon-ingest/src/pdf.rs crates/recon-ingest/tests/fixtures/pdf-acmebank.pdf crates/recon-ingest/tests/fixtures/pdf-acmebank.txt
git commit -m "feat(ingest/pdf): add pdf-extract + printpdf, AcmeBank fixture, extraction spike"
```
(If `Cargo.lock` lives only at `backend/Cargo.lock`, add that path; drop the non-existent one.)

---

## Task 2: `PdfProfile` trait, registry & `PdfParser` (no-text rejection)

**Files:**
- Modify: `backend/crates/recon-ingest/src/pdf.rs`

- [ ] **Step 1: Write failing tests for the trait/registry/parser skeleton**

Add to the `tests` module in `pdf.rs`:
```rust
    use crate::Parser;

    #[test]
    fn resolve_known_and_unknown_profiles() {
        assert!(resolve_profile("acmebank").is_some());
        assert!(resolve_profile("nope").is_none());
        assert_eq!(profile_names(), &["acmebank"]);
    }

    #[test]
    fn empty_pdf_rejects_with_document_error() {
        // A real (tiny) PDF with no text layer: reuse an empty byte slice path.
        // pdf-extract on non-PDF bytes errors -> document RowError row 0.
        let err = PdfParser { profile: Box::new(AcmeBankProfile) }
            .parse(b"not a pdf")
            .unwrap_err();
        assert_eq!(err[0].row, 0);
        assert_eq!(err[0].field, "document");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd backend && cargo test -p recon-ingest --lib pdf::`
Expected: FAIL to compile (`resolve_profile`, `profile_names`, `PdfParser`, `AcmeBankProfile` undefined).

- [ ] **Step 3: Implement the trait, registry, parser, and line normalizer**

In `pdf.rs`, above the `tests` module, add:
```rust
use crate::{ParsedTxn, Parser};

/// Per-bank PDF layout knowledge. Given the already-extracted, normalized lines,
/// produce transaction drafts. Atomic & fail-loud, like every `Parser`.
pub trait PdfProfile {
    fn name(&self) -> &'static str;
    fn parse_lines(&self, lines: &[String]) -> Result<Vec<ParsedTxn>, Vec<RowError>>;
}

/// Generic text-layer PDF parser. Bank-specific mapping lives in `profile`.
pub struct PdfParser {
    pub profile: Box<dyn PdfProfile>,
}

/// Resolve a profile by its stored name. Unknown name -> None (API maps to 400).
pub fn resolve_profile(name: &str) -> Option<Box<dyn PdfProfile>> {
    match name {
        "acmebank" => Some(Box::new(AcmeBankProfile)),
        _ => None,
    }
}

/// The single source of truth for available profile names (API validation + listing).
pub fn profile_names() -> &'static [&'static str] {
    &["acmebank"]
}

/// Trim each line and drop blank lines, preserving order.
fn normalize_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

impl Parser for PdfParser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        let text = extract_text(bytes)?;
        let lines = normalize_lines(&text);
        if lines.is_empty() {
            return Err(vec![RowError::new(
                0,
                "document",
                "no extractable text layer (scanned PDF?)",
            )]);
        }
        self.profile.parse_lines(&lines)
    }
}
```
Also add a minimal `AcmeBankProfile` so the tests compile (real logic in Task 3):
```rust
/// Synthetic columnar layout: `Date  Description  Ref  Amount  Dr/Cr`.
pub struct AcmeBankProfile;

impl PdfProfile for AcmeBankProfile {
    fn name(&self) -> &'static str {
        "acmebank"
    }
    fn parse_lines(&self, _lines: &[String]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        Ok(Vec::new())
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd backend && cargo test -p recon-ingest --lib pdf::`
Expected: PASS (the two new tests + the Task 1 tests).

- [ ] **Step 5: Commit**

```bash
cd backend && git add crates/recon-ingest/src/pdf.rs
git commit -m "feat(ingest/pdf): PdfProfile trait, profile registry, PdfParser with no-text-layer rejection"
```

---

## Task 3: `AcmeBankProfile` happy-path parsing

**Files:**
- Modify: `backend/crates/recon-ingest/src/pdf.rs`

- [ ] **Step 1: Write the failing happy-path tests**

Add to the `tests` module:
```rust
    use recon_domain::Direction;

    fn lines(raw: &[&str]) -> Vec<String> {
        raw.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_acme_rows_with_dr_cr() {
        let input = lines(&[
            "Date    Description    Ref    Amount    Dr/Cr",
            "12/03/2026    CARD PURCHASE TESCO STORES 1234    A1B2C3    45.20    DR",
            "13/03/2026    FASTER PAYMENT FROM J SMITH    Z9Y8X7    500.00    CR",
        ]);
        let txns = AcmeBankProfile.parse_lines(&input).unwrap();
        assert_eq!(txns.len(), 2);

        assert_eq!(txns[0].external_ref, "A1B2C3");
        assert_eq!(txns[0].value_date, "2026-03-12");
        assert_eq!(txns[0].amount_minor, 4520);
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[0].description, "CARD PURCHASE TESCO STORES 1234");
        assert_eq!(txns[0].currency, None);
        assert_eq!(txns[0].counterparty, None);

        assert_eq!(txns[1].external_ref, "Z9Y8X7");
        assert_eq!(txns[1].amount_minor, 50000);
        assert_eq!(txns[1].direction, Direction::Credit);
    }

    #[test]
    fn skips_metadata_before_header_and_footer_lines() {
        let input = lines(&[
            "AcmeBank Statement",
            "Account 12345678",
            "Date    Description    Ref    Amount    Dr/Cr",
            "14/03/2026    DIRECT DEBIT BRITISH GAS    D4E5F6    88.10    DR",
            "Balance carried forward    366.70",
        ]);
        let txns = AcmeBankProfile.parse_lines(&input).unwrap();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].external_ref, "D4E5F6");
    }

    #[test]
    fn parses_the_real_extracted_fixture() {
        let text = std::fs::read_to_string("tests/fixtures/pdf-acmebank.txt").expect("txt fixture");
        let ls = normalize_lines(&text);
        let txns = AcmeBankProfile.parse_lines(&ls).unwrap();
        assert_eq!(txns.len(), 3, "fixture has 3 transaction rows");
        assert_eq!(txns[0].external_ref, "A1B2C3");
        assert_eq!(txns[2].external_ref, "D4E5F6");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd backend && cargo test -p recon-ingest --lib pdf::`
Expected: FAIL (`parse_lines` returns empty; assertions on len/fields fail).

- [ ] **Step 3: Implement the parsing logic**

Replace the stub `AcmeBankProfile` impl in `pdf.rs` with:
```rust
use crate::money::parse_decimal_to_minor;
use recon_domain::Direction;

impl PdfProfile for AcmeBankProfile {
    fn name(&self) -> &'static str {
        "acmebank"
    }

    fn parse_lines(&self, lines: &[String]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        // The header row (contains both "date" and "description") anchors the table.
        let start = lines.iter().position(|l| {
            let lo = l.to_ascii_lowercase();
            lo.contains("date") && lo.contains("description")
        });
        let Some(start) = start else {
            return Err(vec![RowError::new(
                0,
                "document",
                "no transaction table header found",
            )]);
        };

        let mut out = Vec::new();
        let mut errors = Vec::new();
        for (i, line) in lines.iter().enumerate().skip(start + 1) {
            if is_footer(line) {
                continue;
            }
            // 1-based line number in the extracted text, for the row report.
            let row_no = i + 1;
            match parse_acme_row(line) {
                Ok(txn) => out.push(txn),
                Err((field, message)) => errors.push(RowError::new(row_no, field, message)),
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(out)
    }
}

/// Recognized non-transaction lines that legitimately appear after the header.
fn is_footer(line: &str) -> bool {
    let lo = line.to_ascii_lowercase();
    lo.contains("carried forward")
        || lo.contains("brought forward")
        || lo.starts_with("balance ")
        || lo.starts_with("page ")
        || lo.starts_with("statement ")
}

/// Split a row into columns on runs of 2+ spaces (descriptions use single spaces).
fn split_columns(line: &str) -> Vec<&str> {
    line.split("  ")
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_acme_row(line: &str) -> Result<ParsedTxn, (&'static str, String)> {
    let fields = split_columns(line);
    if fields.len() != 5 {
        return Err(("row", format!("expected 5 columns, got {}: {line}", fields.len())));
    }
    let (date_s, desc, reff, amount_s, drcr) =
        (fields[0], fields[1], fields[2], fields[3], fields[4]);

    let value_date = parse_uk_date(date_s).map_err(|m| ("date", m))?;
    if reff.is_empty() {
        return Err(("ref", "empty reference".to_string()));
    }
    // The amount column is always positive; direction comes from the DR/CR marker.
    let amount_minor = parse_decimal_to_minor(amount_s).map_err(|m| ("amount", m))?;
    let direction = match drcr.to_ascii_uppercase().as_str() {
        "DR" => Direction::Debit,
        "CR" => Direction::Credit,
        other => return Err(("direction", format!("expected DR or CR, got {other}"))),
    };

    Ok(ParsedTxn {
        external_ref: reff.to_string(),
        value_date,
        posted_at: None,
        amount_minor,
        currency: None,
        direction,
        counterparty: None,
        description: desc.to_string(),
        counterparty_bic: None,
        counterparty_account: None,
    })
}

/// Parse `DD/MM/YYYY` -> ISO `YYYY-MM-DD`.
fn parse_uk_date(s: &str) -> Result<String, String> {
    chrono::NaiveDate::parse_from_str(s, "%d/%m/%Y")
        .map(|d| d.format("%Y-%m-%d").to_string())
        .map_err(|_| format!("invalid date (expected DD/MM/YYYY): {s}"))
}
```
Remove the now-duplicate `use recon_domain::Direction;` if it conflicts with the one in `tests` (the `tests` module has its own `use`). Keep a single top-level `use recon_domain::Direction;`.

- [ ] **Step 4: Run to verify pass**

Run: `cd backend && cargo test -p recon-ingest --lib pdf::`
Expected: PASS (all happy-path tests, including `parses_the_real_extracted_fixture`). If the fixture test fails on count, inspect `pdf-acmebank.txt` — the extraction may have merged/split lines; adjust the generator spacing in Task 1 and regenerate.

- [ ] **Step 5: Commit**

```bash
cd backend && git add crates/recon-ingest/src/pdf.rs
git commit -m "feat(ingest/pdf): AcmeBank profile parses Date/Description/Ref/Amount/DrCr rows"
```

---

## Task 4: `AcmeBankProfile` rejection modes

**Files:**
- Modify: `backend/crates/recon-ingest/src/pdf.rs`

- [ ] **Step 1: Write the failing rejection tests**

Add to the `tests` module:
```rust
    fn header_plus(row: &str) -> Vec<String> {
        lines(&["Date    Description    Ref    Amount    Dr/Cr", row])
    }

    #[test]
    fn rejects_bad_date() {
        let err = AcmeBankProfile
            .parse_lines(&header_plus("99/99/2026    DESC    R1    10.00    DR"))
            .unwrap_err();
        assert_eq!(err[0].field, "date");
    }

    #[test]
    fn rejects_bad_amount() {
        let err = AcmeBankProfile
            .parse_lines(&header_plus("12/03/2026    DESC    R1    not-money    DR"))
            .unwrap_err();
        assert_eq!(err[0].field, "amount");
    }

    #[test]
    fn rejects_invalid_dr_cr_marker() {
        let err = AcmeBankProfile
            .parse_lines(&header_plus("12/03/2026    DESC    R1    10.00    XX"))
            .unwrap_err();
        assert_eq!(err[0].field, "direction");
    }

    #[test]
    fn rejects_empty_ref() {
        // 5 columns but ref empty is impossible via split; simulate via wrong shape:
        // a row missing the ref column collapses to 4 fields -> "row" error instead.
        // Empty ref is only reachable if the column is whitespace; assert the 4-col case.
        let err = AcmeBankProfile
            .parse_lines(&header_plus("12/03/2026    DESC    10.00    DR"))
            .unwrap_err();
        assert_eq!(err[0].field, "row");
    }

    #[test]
    fn rejects_wrong_column_count() {
        let err = AcmeBankProfile
            .parse_lines(&header_plus("12/03/2026    DESC    R1    10.00    DR    EXTRA"))
            .unwrap_err();
        assert_eq!(err[0].field, "row");
    }

    #[test]
    fn collects_all_row_errors_atomically() {
        let input = lines(&[
            "Date    Description    Ref    Amount    Dr/Cr",
            "bad-date    DESC    R1    10.00    DR",
            "12/03/2026    DESC    R2    bad-amt    CR",
        ]);
        let err = AcmeBankProfile.parse_lines(&input).unwrap_err();
        assert_eq!(err.len(), 2);
        assert_eq!(err[0].field, "date");
        assert_eq!(err[1].field, "amount");
    }
```

- [ ] **Step 2: Run to verify behavior**

Run: `cd backend && cargo test -p recon-ingest --lib pdf::`
Expected: PASS for all (the Task 3 implementation already produces these field tags). If `rejects_empty_ref` or any case fails, the implementation in Task 3 already covers `row`/`date`/`amount`/`direction`; confirm the assertions match the produced `field`. These tests document the rejection contract — they should pass against the existing logic; if one genuinely needs an extra branch, add it minimally.

- [ ] **Step 3: Commit**

```bash
cd backend && git add crates/recon-ingest/src/pdf.rs
git commit -m "test(ingest/pdf): AcmeBank rejection modes — date, amount, DR/CR, shape, atomic collection"
```

---

## Task 5: End-to-end PDF → transactions through `pdf-extract`

**Files:**
- Modify: `backend/crates/recon-ingest/src/pdf.rs`

- [ ] **Step 1: Write the failing end-to-end test**

Add to the `tests` module:
```rust
    #[test]
    fn end_to_end_pdf_bytes_to_transactions() {
        let bytes = std::fs::read("tests/fixtures/pdf-acmebank.pdf").expect("fixture file");
        let txns = PdfParser { profile: Box::new(AcmeBankProfile) }
            .parse(&bytes)
            .expect("parse ok");
        assert_eq!(txns.len(), 3);
        assert_eq!(txns[0].external_ref, "A1B2C3");
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[1].external_ref, "Z9Y8X7");
        assert_eq!(txns[1].direction, Direction::Credit);
        assert_eq!(txns[2].external_ref, "D4E5F6");
    }
```

- [ ] **Step 2: Run to verify pass**

Run: `cd backend && cargo test -p recon-ingest --lib pdf::end_to_end`
Expected: PASS. This proves bytes → `pdf-extract` → `normalize_lines` → `AcmeBankProfile` → `ParsedTxn`.

- [ ] **Step 3: Run the whole crate to confirm no regressions**

Run: `cd backend && cargo test -p recon-ingest`
Expected: all existing parser tests + the new `pdf::` tests PASS.

- [ ] **Step 4: Commit**

```bash
cd backend && git add crates/recon-ingest/src/pdf.rs
git commit -m "test(ingest/pdf): end-to-end PDF bytes -> ParsedTxn through pdf-extract"
```

---

## Task 6: Migration, domain, store threading

**Files:**
- Create: `backend/migrations/0007_pdf_profile.sql`
- Modify: `backend/crates/recon-domain/src/types.rs`
- Modify: `backend/crates/recon-store/src/rows.rs`
- Modify: `backend/crates/recon-store/src/sources.rs`
- Modify: `backend/crates/recon-store/tests/{format_dialect_schema,audit_schema,ingest,patch_source}.rs`

- [ ] **Step 1: Write the migration**

Create `backend/migrations/0007_pdf_profile.sql`:
```sql
-- Phase 8: per-source PDF bank-statement profile. Additive; no CHECK constraint
-- because valid profile names are owned by the recon-ingest registry and
-- validated at the API (avoids a migration per new profile).
ALTER TABLE sources ADD COLUMN pdf_profile TEXT NULL;
```

- [ ] **Step 2: Add the domain field**

In `backend/crates/recon-domain/src/types.rs`, in `struct Source` (currently ends with `pub format_dialect: Option<String>,`), add:
```rust
    pub pdf_profile: Option<String>,
```

- [ ] **Step 3: Compile to find every broken `Source` literal**

Run: `cd backend && cargo build -p recon-domain && cargo build -p recon-store 2>&1 | head -40`
Expected: `recon-store` fails — every `Source { .. }` literal now misses `pdf_profile`. The following steps fix each.

- [ ] **Step 4: Thread through `SourceRow`**

In `backend/crates/recon-store/src/rows.rs`, add to `struct SourceRow` (after `pub format_dialect: Option<String>,`):
```rust
    pub pdf_profile: Option<String>,
```
And in `impl From<SourceRow> for Source`, add after `format_dialect: r.format_dialect,`:
```rust
            pdf_profile: r.pdf_profile,
```

- [ ] **Step 5: Thread through `sources.rs` create/get/list/update**

In `backend/crates/recon-store/src/sources.rs`:

(a) `create_source` — add a parameter and persist it. Change the signature to add after `format_dialect: Option<&str>,`:
```rust
        pdf_profile: Option<&str>,
```
Change the INSERT to include the column:
```rust
        sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency,format_dialect,pdf_profile) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(&id)
            .bind(tenant_id)
            .bind(kind_str(kind))
            .bind(name)
            .bind(currency)
            .bind(format_dialect)
            .bind(pdf_profile)
            .execute(&mut *tx)
            .await?;
```
And the returned `Source` literal — add after `format_dialect: format_dialect.map(|s| s.to_string()),`:
```rust
            pdf_profile: pdf_profile.map(|s| s.to_string()),
```

(b) `get_source` — extend the SELECT column list:
```rust
            sqlx::query_as("SELECT id,tenant_id,kind,name,currency,format_dialect,pdf_profile FROM sources WHERE id=$1 AND tenant_id=$2")
```

(c) `list_sources` — add `pdf_profile` to the local `Row` struct, the SELECT, and the GROUP BY, and to the constructed `Source`:
```rust
        struct Row {
            id: String,
            tenant_id: String,
            kind: String,
            name: String,
            currency: String,
            format_dialect: Option<String>,
            pdf_profile: Option<String>,
            txn_count: i64,
        }
```
```rust
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT s.id, s.tenant_id, s.kind, s.name, s.currency, s.format_dialect, s.pdf_profile, \
                    COUNT(t.id) AS txn_count \
             FROM sources s \
             LEFT JOIN canonical_transactions t ON t.source_id = s.id AND t.tenant_id = s.tenant_id \
             WHERE s.tenant_id = $1 \
             GROUP BY s.id, s.tenant_id, s.kind, s.name, s.currency, s.format_dialect, s.pdf_profile \
             ORDER BY s.name",
        )
```
```rust
                source: recon_domain::Source {
                    id: r.id,
                    tenant_id: r.tenant_id,
                    kind: match r.kind.as_str() {
                        "bank" => SourceKind::Bank,
                        "ledger" => SourceKind::Ledger,
                        _ => SourceKind::CrossSystem,
                    },
                    name: r.name,
                    currency: r.currency,
                    format_dialect: r.format_dialect,
                    pdf_profile: r.pdf_profile,
                },
```

(d) `update_source` — add a double-`Option` parameter (mirrors `new_format_dialect`). Add after `new_format_dialect: Option<Option<&str>>,`:
```rust
        // None = field absent; Some(None) = clear; Some(Some(v)) = set to v.
        new_pdf_profile: Option<Option<&str>>,
```
After the `after_dialect` block, add:
```rust
        let after_pdf_profile: Option<String> = match new_pdf_profile {
            None => before.pdf_profile.clone(),
            Some(v) => v.map(|s| s.to_string()),
        };
```
Change the UPDATE to set the column too:
```rust
        sqlx::query("UPDATE sources SET name=$1, format_dialect=$2, pdf_profile=$3 WHERE id=$4 AND tenant_id=$5")
            .bind(&after_name)
            .bind(&after_dialect)
            .bind(&after_pdf_profile)
            .bind(source_id)
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;
```
The audit payload (`DataSourceUpdated`) is **unchanged** — `pdf_profile` is layout metadata, deliberately excluded from the audit chain (consistent with how the spec treats it). Finally add to the returned `Source` literal after `format_dialect: after_dialect,`:
```rust
            pdf_profile: after_pdf_profile,
```

- [ ] **Step 6: Update store test call-sites**

These callers pass the old arity and must gain the new trailing argument(s):

`backend/crates/recon-store/tests/format_dialect_schema.rs` lines ~34 and ~49 — append `, None` to each `create_source(...)` call (now ends `..., Some("subfielded"), None)` and `..., None, None)`).

`backend/crates/recon-store/tests/audit_schema.rs` line ~174 — `create_source("t", ..., "actor", None)` → append `, None`.

`backend/crates/recon-store/tests/ingest.rs` lines ~31, ~111, ~112 — append `, None` to each `create_source(...)`.

`backend/crates/recon-store/tests/patch_source.rs`:
- the `create_source(...)` call (~line 14 block) — append `, None`.
- every `update_source(...)` call (~lines 32, 44, 56, 60, 71, 96) — append a trailing `, None` (the new `new_pdf_profile` arg). E.g. `update_source(&t, &sid, &a, Some("Renamed"), None)` → `update_source(&t, &sid, &a, Some("Renamed"), None, None)`.

- [ ] **Step 7: Update the API caller so the workspace compiles**

In `backend/crates/recon-api/src/routes.rs`:
- `create_source` call (~line 235) — append `, None` for now (real validation wired in Task 7):
```rust
        .create_source(&ctx.tenant_id, body.kind, &body.name, &body.currency, &ctx.user_id, dialect, None)
```
- `update_source` call (~line 265) — append `, None`:
```rust
        .update_source(
            &ctx.tenant_id,
            &source_id,
            &ctx.user_id,
            body.name.as_deref().map(str::trim),
            dialect_patch,
            None,
        )
```

- [ ] **Step 8: Build + run store tests**

Run (DB required — postgres is already up locally):
```bash
cd backend && cargo build && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store
```
Expected: PASS. `sqlx::test` applies migrations (including `0007`) to a fresh test DB, so `pdf_profile` exists. Existing `format_dialect`/patch/ingest tests still pass.

- [ ] **Step 9: Commit**

```bash
cd backend && git add migrations/0007_pdf_profile.sql crates/recon-domain/src/types.rs crates/recon-store/src/rows.rs crates/recon-store/src/sources.rs crates/recon-store/tests/format_dialect_schema.rs crates/recon-store/tests/audit_schema.rs crates/recon-store/tests/ingest.rs crates/recon-store/tests/patch_source.rs crates/recon-api/src/routes.rs
git commit -m "feat(store): thread sources.pdf_profile through migration 0007 + create/get/list/update"
```

---

## Task 7: API — validation, ingest dispatch, `GET /api/pdf-profiles`

**Files:**
- Modify: `backend/crates/recon-api/src/dto.rs`
- Modify: `backend/crates/recon-api/src/routes.rs`
- Modify: `backend/crates/recon-api/tests/ingest_api.rs`

- [ ] **Step 1: Add `pdf_profile` to the DTOs**

In `backend/crates/recon-api/src/dto.rs`:

`CreateSourceReq` — add after `pub format_dialect: Option<String>,`:
```rust
    #[serde(default)]
    pub pdf_profile: Option<String>,
```
`UpdateSourceReq` — add after the `format_dialect` field:
```rust
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub pdf_profile: Option<Option<String>>,
```
(The existing `deserialize_double_option` helper is reused as-is.)

- [ ] **Step 2: Wire validation + the new route handler (failing integration test first)**

Add to `backend/crates/recon-api/tests/ingest_api.rs` a new test using the existing `multipart_body`, `json`, `token` helpers and the committed PDF fixture. The fixture lives in the ingest crate; reference it by relative path from the api crate test:
```rust
#[sqlx::test(migrations = "../../migrations")]
async fn pdf_ingest_pipeline(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-acme','admin')").execute(&pool).await.unwrap();

    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-acme"));

    // pdf-profiles endpoint lists the registry.
    let req = Request::builder().method("GET").uri("/api/pdf-profiles")
        .header("authorization", &auth).body(Body::empty()).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v["profiles"].as_array().unwrap().contains(&Value::from("acmebank")));

    // Create a source WITH a pdf profile.
    let req = Request::builder().method("POST").uri("/api/sources").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"kind":"bank","name":"PDF Bank","currency":"GBP","pdfProfile":"acmebank"}"#)).unwrap();
    let (st, src) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "create: {src}");
    assert_eq!(src["pdfProfile"], "acmebank");
    let src_id = src["id"].as_str().unwrap().to_string();

    // Upload the committed PDF fixture.
    let pdf = std::fs::read("../recon-ingest/tests/fixtures/pdf-acmebank.pdf").expect("fixture");
    let boundary = "BOUNDARY";
    // multipart_body takes &str values; build the PDF part as raw bytes inline instead.
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"s.pdf\"\r\n\r\n").as_bytes());
    body.extend_from_slice(&pdf);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"format\"\r\n\r\npdf\r\n").as_bytes());
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let req = Request::builder().method("POST").uri(format!("/api/sources/{src_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "pdf ingest: {v}");
    assert_eq!(v["ingested"], 3);

    // A source with NO pdf profile rejects pdf upload with 400.
    let req = Request::builder().method("POST").uri("/api/sources").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"kind":"ledger","name":"No Profile","currency":"GBP"}"#)).unwrap();
    let (_st, np) = json(&app, req).await;
    let np_id = np["id"].as_str().unwrap().to_string();
    let pdf2 = std::fs::read("../recon-ingest/tests/fixtures/pdf-acmebank.pdf").unwrap();
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"s.pdf\"\r\n\r\n").as_bytes());
    body.extend_from_slice(&pdf2);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"format\"\r\n\r\npdf\r\n").as_bytes());
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    let req = Request::builder().method("POST").uri(format!("/api/sources/{np_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, _v) = json(&app, req).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "pdf upload without profile must 400");
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api pdf_ingest_pipeline`
Expected: FAIL (no `/api/pdf-profiles` route → 404; `pdf` format → 400 "bad request" from the `_ =>` arm; `pdfProfile` not persisted).

- [ ] **Step 4: Implement create-source validation**

In `routes.rs` `create_source`, after the `format_dialect` validation block and before the `.create_source(...)` call, add:
```rust
    // Validate pdf_profile against the parser registry.
    let pdf_profile: Option<&str> = match body.pdf_profile.as_deref() {
        None => None,
        Some(name) if recon_ingest::pdf::resolve_profile(name).is_some() => Some(name),
        Some(_) => return Err(ApiError::BadRequest()),
    };
```
Change the call to pass it (replacing the temporary `None` from Task 6):
```rust
        .create_source(&ctx.tenant_id, body.kind, &body.name, &body.currency, &ctx.user_id, dialect, pdf_profile)
```

- [ ] **Step 5: Implement patch-source validation**

In `routes.rs` `patch_source`, after the `dialect_patch` block, add:
```rust
    // Validate pdf_profile patch if present.
    let pdf_profile_patch: Option<Option<&str>> = match body.pdf_profile {
        None => None,
        Some(None) => Some(None),
        Some(Some(ref name)) => match recon_ingest::pdf::resolve_profile(name) {
            Some(_) => Some(Some(name.as_str())),
            None => return Err(ApiError::BadRequest()),
        },
    };
```
Change the `update_source(...)` call's last argument from the temporary `None` to `pdf_profile_patch`.

- [ ] **Step 6: Implement the `pdf` ingest arm**

In `routes.rs` `ingest_source`, inside the `match format.as_str()`, add before the `_ =>` arm:
```rust
        "pdf" => {
            let name = source
                .pdf_profile
                .as_deref()
                .ok_or_else(ApiError::BadRequest)?;
            let profile = recon_ingest::pdf::resolve_profile(name)
                .ok_or_else(ApiError::BadRequest)?;
            recon_ingest::pdf::PdfParser { profile }.parse(&bytes)
        }
```

- [ ] **Step 7: Add the `GET /api/pdf-profiles` route + handler**

In `routes.rs` `router(...)`, add after the `/api/sources/:source_id/ingest` route:
```rust
        .route("/api/pdf-profiles", get(list_pdf_profiles))
```
Add the handler near `list_sources`:
```rust
async fn list_pdf_profiles(
    State(_s): State<AppState>,
    _ctx: AuthContext,
) -> Result<Json<Value>, ApiError> {
    // Authenticated read; profiles are not tenant-specific.
    Ok(Json(json!({ "profiles": recon_ingest::pdf::profile_names() })))
}
```

- [ ] **Step 8: Run to verify pass**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api pdf_ingest_pipeline`
Expected: PASS. Then run the full api suite for regressions:
```bash
cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api
```
Expected: all PASS (the existing `full_ingest_pipeline` still green).

- [ ] **Step 9: Commit**

```bash
cd backend && git add crates/recon-api/src/dto.rs crates/recon-api/src/routes.rs crates/recon-api/tests/ingest_api.rs
git commit -m "feat(api): pdf ingest dispatch + pdf_profile validation + GET /api/pdf-profiles"
```

- [ ] **Step 10: Workspace-wide check + clippy**

Run:
```bash
cd backend && cargo clippy --workspace --all-targets -- -D warnings && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --workspace
```
Expected: no clippy warnings; all tests pass. Fix any lints inline, then amend or add a `chore(clippy)` commit if needed.

---

## Task 8: Frontend — types, client, mock, http

**Files:**
- Modify: `web/lib/domain/types.ts`
- Modify: `web/lib/api/client.ts`
- Modify: `web/lib/api/http.ts`
- Modify: `web/lib/api/mock.ts`

- [ ] **Step 1: Extend the Zod `Source` schema**

In `web/lib/domain/types.ts`, in `sourceSchema`, add after `formatDialect: formatDialectSchema.nullable(),`:
```typescript
  pdfProfile: z.string().nullable().optional(),
```

- [ ] **Step 2: Extend client types + interface**

In `web/lib/api/client.ts`:

`IngestFormat` (line ~94):
```typescript
export type IngestFormat = "csv" | "camt053" | "mt940" | "mt942" | "bai2" | "pdf";
```
`CreateSourceInput` — add after `formatDialect?: FormatDialect | null;`:
```typescript
  // Optional per-source PDF profile name (validated server-side against the registry).
  pdfProfile?: string | null;
```
`UpdateSourceInput` — add after its `formatDialect` line:
```typescript
  // null = clear; undefined = don't change; string = set.
  pdfProfile?: string | null;
```
`ApiClient` interface — add a method (near `listSources`):
```typescript
  listPdfProfiles(tenantId: string): Promise<string[]>;
```

- [ ] **Step 3: Implement in `HttpApiClient`**

In `web/lib/api/http.ts`:

Add a `listPdfProfiles` method (mirror `listSources`, but the endpoint returns `{profiles: [...]}`):
```typescript
  async listPdfProfiles(tenantId: string): Promise<string[]> {
    const r = await this.req<{ profiles: string[] }>("/api/pdf-profiles", tenantId);
    return r.profiles;
  }
```
(If `req` is not generic in this codebase, match the existing call style used by `listSources`; the endpoint shape is `{ "profiles": string[] }`.)

Extend `updateSource` to forward `pdfProfile`:
```typescript
  updateSource(tenantId: string, sourceId: string, patch: UpdateSourceInput): Promise<Source> {
    const body: Record<string, unknown> = {};
    if (patch.name !== undefined) body.name = patch.name;
    if (patch.formatDialect !== undefined) body.formatDialect = patch.formatDialect;
    if (patch.pdfProfile !== undefined) body.pdfProfile = patch.pdfProfile;
    return this.req(`/api/sources/${sourceId}`, tenantId, { method: "PATCH", body: JSON.stringify(body) });
  }
```
`createSource` already serializes the whole `input`, so `pdfProfile` flows through automatically — no change needed.

- [ ] **Step 4: Implement in `MockApiClient`**

In `web/lib/api/mock.ts`:

Add `listPdfProfiles`:
```typescript
  async listPdfProfiles(_tenantId: string): Promise<string[]> {
    await this.delay();
    return ["acmebank"];
  }
```
`createSource` — set `pdfProfile` on the created source (after `formatDialect: input.formatDialect ?? null,`):
```typescript
    pdfProfile: input.pdfProfile ?? null,
```
`updateSource` — carry the patch (after the `formatDialect` line in the `updated` object):
```typescript
    pdfProfile: patch.pdfProfile === undefined ? existing.pdfProfile : patch.pdfProfile,
```

- [ ] **Step 5: Typecheck**

Run: `cd web && pnpm tsc --noEmit`
Expected: no errors. (If `Source` objects are constructed elsewhere in mocks/fixtures/tests without `pdfProfile`, the field is `.optional()` in the Zod type, so they remain valid; fix any non-optional TS `Source` literals that now error by adding `pdfProfile: null`.)

- [ ] **Step 6: Commit**

```bash
cd web && git add lib/domain/types.ts lib/api/client.ts lib/api/http.ts lib/api/mock.ts
git commit -m "feat(web): pdfProfile on Source + IngestFormat 'pdf' + listPdfProfiles (client/http/mock)"
```

---

## Task 9: Frontend — upload dialog & source dialogs

**Files:**
- Modify: `web/components/app/upload-dialog.tsx`
- Modify: `web/app/(app)/sources/page.tsx`
- Modify: `web/components/app/edit-source-dialog.tsx`
- Modify/Create: `web/tests/edit-source-dialog.test.tsx` (+ optionally `web/tests/upload-dialog.test.tsx`)

- [ ] **Step 1: Upload dialog — PDF option, guard, file accept**

In `web/components/app/upload-dialog.tsx`:

Add the PDF `SelectItem` to the format Select (after the `bai2` item):
```tsx
            <SelectItem value="pdf">PDF statement</SelectItem>
```
Add an amber guard for PDF sources without a profile, right after the existing MT94x dialect notice block:
```tsx
        {format === "pdf" && !source.pdfProfile && (
          <p className="text-sm text-amber-600 dark:text-amber-400">
            This source has no PDF profile set. Edit the source to choose one before uploading a PDF.
          </p>
        )}
        {format === "pdf" && source.pdfProfile && (
          <p className="text-sm text-muted-foreground">
            Using PDF profile <strong>{source.pdfProfile}</strong>.
          </p>
        )}
```
Disable the submit button when a PDF can't be ingested. Find the upload/submit `<Button>` and extend its `disabled` expression with:
```tsx
disabled={/* ...existing conditions... */ || (format === "pdf" && !source.pdfProfile)}
```
Extend the file-accept handling so `.pdf` is accepted when `format === "pdf"`. If the dialog sets an `accept` attr by format, add a `pdf` branch returning `"application/pdf,.pdf"`; if it accepts everything, no change is required.

- [ ] **Step 2: New-source dialog — PDF profile Select**

In `web/app/(app)/sources/page.tsx`:

Extend the zod `schema` (after `formatDialect: ...`):
```typescript
  pdfProfile: z.string().nullable(),
```
Add to `defaultValues`: `pdfProfile: null`.
Add a watch: `const pdfProfile = watch("pdfProfile");`
Fetch profiles with TanStack Query (near other hooks in the component):
```typescript
  const { data: pdfProfiles = [] } = useQuery({
    queryKey: ["pdf-profiles", tenantId],
    queryFn: () => api.listPdfProfiles(tenantId),
  });
```
Add a Select (place near the dialect Select), reusing the `DIALECT_NONE` sentinel constant already defined at module scope:
```tsx
              <Select
                value={pdfProfile ?? DIALECT_NONE}
                onValueChange={(v) =>
                  setValue("pdfProfile", v === DIALECT_NONE ? null : v)
                }
              >
                <SelectTrigger id="src-pdf-profile" aria-label="PDF profile">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={DIALECT_NONE}>Not applicable</SelectItem>
                  {pdfProfiles.map((p) => (
                    <SelectItem key={p} value={p}>{p}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
```
`createMutation.mutate(v)` already passes the whole form value; ensure the create mutation forwards `pdfProfile` to `api.createSource` (the input type now accepts it — include it in the object passed to `createSource` if the mutation builds an explicit object).

- [ ] **Step 3: Edit-source dialog — PDF profile Select**

In `web/components/app/edit-source-dialog.tsx`:

Extend the zod `schema` (after `formatDialect: ...`):
```typescript
  pdfProfile: z.string().nullable(),
```
Add to `defaultValues`:
```typescript
    pdfProfile: source.pdfProfile ?? null,
```
Add `const pdfProfile = watch("pdfProfile");` and the same `useQuery` for `pdfProfiles` as in Step 2 (this component receives `tenantId`).
Add the same Select markup as Step 2 (use `id="edit-src-pdf-profile"`).
Extend the patch builder (the `mutationFn`) after the `formatDialect` diff:
```typescript
      if (values.pdfProfile !== (source.pdfProfile ?? null)) {
        patch.pdfProfile = values.pdfProfile;
      }
```
And widen the local `patch` type to include `pdfProfile?: string | null`.

- [ ] **Step 4: Write a vitest for the edit dialog PDF profile**

In `web/tests/edit-source-dialog.test.tsx`, add a test mirroring the existing name test. It must provide a profiles list via the mock (the `MockApiClient.listPdfProfiles` returns `["acmebank"]`):
```typescript
it("submits pdfProfile when changed", async () => {
  const user = userEvent.setup();
  const base = new MockApiClient({ latencyMs: 0 });
  const updateSpy = vi.fn(base.updateSource.bind(base));
  const stubClient: ApiClient = Object.assign(base, { updateSource: updateSpy });

  renderDialog(stubClient);

  // Open the PDF profile select and choose acmebank.
  await user.click(screen.getByLabelText(/pdf profile/i));
  await user.click(await screen.findByRole("option", { name: "acmebank" }));
  await user.click(screen.getByRole("button", { name: /save/i }));

  await waitFor(() => expect(updateSpy).toHaveBeenCalled());
  expect(updateSpy).toHaveBeenCalledWith(
    "tenant-acme",
    "src-1",
    expect.objectContaining({ pdfProfile: "acmebank" }),
  );
});
```
(If the Base UI Select renders options lazily, the existing tests show the working interaction pattern — match it. `BASE_SOURCE` already has `formatDialect: null`; add `pdfProfile: null` to it.)

- [ ] **Step 5: Run frontend tests + typecheck**

Run:
```bash
cd web && pnpm tsc --noEmit && pnpm vitest run
```
Expected: PASS, including the new test and all existing ones. Update `BASE_SOURCE` and any other `Source` literal in tests with `pdfProfile: null` if TS/Zod requires it.

- [ ] **Step 6: Commit**

```bash
cd web && git add components/app/upload-dialog.tsx app/\(app\)/sources/page.tsx components/app/edit-source-dialog.tsx tests/edit-source-dialog.test.tsx
git commit -m "feat(web): PDF format in upload dialog + PDF-profile Select in new/edit source dialogs"
```

---

## Task 10: Playwright E2E, docs & README

**Files:**
- Create: `web/e2e/fixtures/pdf-acmebank.pdf` (copy of the committed fixture)
- Modify: an existing ingestion E2E spec under `web/e2e/` (the one that uploads CSV/MT940 today)
- Create: `docs/adding-a-pdf-bank-profile.md`
- Modify: `web/README.md`

- [ ] **Step 1: Copy the PDF fixture into the web E2E fixtures**

Run:
```bash
cp backend/crates/recon-ingest/tests/fixtures/pdf-acmebank.pdf web/e2e/fixtures/pdf-acmebank.pdf
```
(If `web/e2e/fixtures/` does not exist, find where the existing E2E reads upload fixtures and place it there; match that path in the spec below.)

- [ ] **Step 2: Add a PDF upload step to the ingestion E2E**

Locate the existing ingestion spec (grep `web/e2e` for `ingest`/`upload`/`Sources`). Add a flow that:
1. Signs in as `ada@acme.test` / `Password123!` (admin — required for ManageData).
2. Creates a source with PDF profile `acmebank` (New source dialog → set PDF profile → save), or edits an existing source to set the profile.
3. Opens that source's Upload dialog, selects format **PDF statement**, attaches `e2e/fixtures/pdf-acmebank.pdf` via `setInputFiles`, submits.
4. Asserts the success toast / ingested count (3) appears.

Concrete Playwright snippet to adapt to the existing spec's helpers:
```typescript
test("ingests a PDF bank statement via the acmebank profile", async ({ page }) => {
  await signIn(page, "ada@acme.test", "Password123!"); // use the spec's existing helper
  await page.goto("/sources");
  // ... create/edit a source named "PDF Bank" with PDF profile "acmebank" ...
  await page.getByRole("button", { name: /upload/i }).first().click();
  await page.getByLabel(/format/i).click();
  await page.getByRole("option", { name: /pdf statement/i }).click();
  await page.setInputFiles('input[type="file"]', "e2e/fixtures/pdf-acmebank.pdf");
  await page.getByRole("button", { name: /upload|ingest/i }).click();
  await expect(page.getByText(/ingested|3/i)).toBeVisible();
});
```

- [ ] **Step 3: Run the E2E**

Run (with backend + web running, per the deploy instructions; the test runner may start them — match the existing E2E command in `web/package.json`):
```bash
cd web && pnpm test:e2e
```
Expected: the new PDF test + all existing E2E PASS. (If E2E requires the dev DB reseeded, use the `RECON_DEV` reseed endpoint the existing specs already rely on.)

- [ ] **Step 4: Write the how-to doc**

Create `docs/adding-a-pdf-bank-profile.md`:
```markdown
# Adding a PDF bank profile

PDF statements are parsed by a per-bank `PdfProfile` selected on the source. To add a bank:

1. **Implement the profile** in `backend/crates/recon-ingest/src/pdf.rs` (or a submodule):
   implement `PdfProfile` — `name()` returns the registry key; `parse_lines(&[String])`
   maps the extracted, normalized text lines to `ParsedTxn`s, returning `Vec<RowError>`
   on any bad row (atomic, fail-loud).
2. **Register it** in `resolve_profile` (add a match arm) and `profile_names`
   (add the key). These are the single source of truth the API validates against.
3. **Add fixtures** under `crates/recon-ingest/tests/fixtures/`: a `<bank>.pdf`
   (generate via a `#[ignore]` printpdf test, mirroring `generate_acmebank_fixture`)
   and capture the extracted text as `<bank>.txt`.
4. **Test**: a happy-path `parse_lines` test, one test per rejection mode, and an
   end-to-end `PdfParser.parse(bytes)` test against the `.pdf`.
5. The frontend needs no change — `GET /api/pdf-profiles` surfaces the new name
   automatically in the source dialogs.

Notes:
- Only text-layer PDFs are supported. Scanned/image PDFs have no text layer and are
  rejected with a document-level error.
- Keep column separators wide (>=2 spaces) in synthetic fixtures so `pdf-extract`'s
  spacing survives; descriptions should use single spaces only.
```

- [ ] **Step 5: Add the README formats-table row**

In `web/README.md`, add a row to the formats table (matching the existing column shape, e.g. Format / Description / Notes):
```markdown
| PDF | Text-layer bank statements | Requires a per-source PDF profile (e.g. `acmebank`); set it on the source before uploading. Scanned/image PDFs are not supported. |
```

- [ ] **Step 6: Commit**

```bash
git add web/e2e/fixtures/pdf-acmebank.pdf web/e2e docs/adding-a-pdf-bank-profile.md web/README.md
git commit -m "test(e2e)+docs: PDF upload E2E, add-a-profile how-to, README formats row"
```

---

## Final verification

- [ ] **Backend:** `cd backend && cargo clippy --workspace --all-targets -- -D warnings && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --workspace` → all green.
- [ ] **Frontend:** `cd web && pnpm tsc --noEmit && pnpm vitest run` → all green.
- [ ] **E2E:** the ingestion spec including the PDF step passes.
- [ ] **Manual smoke (local deploy already running):** sign in as `ada@acme.test`, create a source with PDF profile `acmebank`, upload `pdf-acmebank.pdf`, confirm 3 transactions ingested and a run reconciles.
- [ ] **Update memory:** record Phase 8 (PDF ingestion) in `recon-ui-slice-status.md` once merged.

---

## Self-review notes

- **Spec coverage:** extraction crate + spike (Task 1); two-stage parser + registry + no-text rejection (Task 2); AcmeBank profile happy path (Task 3) + rejection modes (Task 4); end-to-end (Task 5); migration/domain/store (Task 6); DTO validation + ingest dispatch + `GET /api/pdf-profiles` + audit-unchanged (Task 7); frontend types/client/mock/http (Task 8); upload + new/edit dialogs + amber guard (Task 9); Playwright + docs + README (Task 10). All spec sections map to a task.
- **Type consistency:** `pdf_profile` (Rust) / `pdfProfile` (TS/JSON, via `rename_all = "camelCase"`); `resolve_profile`/`profile_names` are the single registry source used by both `create`/`patch` validation and the listing route; `PdfParser`/`PdfProfile`/`AcmeBankProfile` names consistent across tasks; the `DR`/`CR` marker model matches the refined spec §3.2.
- **Fail-loud:** document-level (row 0) for no-text; per-row `field` tags `row`/`date`/`amount`/`direction`/`ref`; 400 for unknown/missing profile; 422 rows / 409 dup-refs reuse the existing envelope unchanged.
