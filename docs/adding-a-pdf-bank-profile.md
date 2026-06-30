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
