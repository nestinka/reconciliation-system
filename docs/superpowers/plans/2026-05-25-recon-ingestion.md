# Bank-Format Ingestion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let an operator create sources, upload bank/ledger files (CSV with per-upload column mapping, or CAMT.053 XML), and trigger a reconciliation run over a date window — all through the running UI, feeding the existing matching engine.

**Architecture:** A new pure `recon-ingest` crate parses file bytes into `ParsedTxn` drafts (atomic: any bad row rejects the whole file). `recon-store` gains source creation, atomic `ingest_transactions` (unique per `(source_id, external_ref)`), and `create_run` (loads windows → `reconcile()` → persists via a new generic `persist_run`). `recon-api` adds multipart upload + source/run routes guarded by a new `ManageData` permission, mapping `ParsedTxn` → `CanonicalTransaction`. The frontend adds a Sources screen (list + create + upload dialog) and a New-run dialog.

**Tech Stack:** Rust (axum 0.7, sqlx 0.8, `csv`, `quick-xml`, `chrono`), PostgreSQL, Next.js 16 / React 19, TanStack Query, react-hook-form + zod, vitest, Playwright.

**Source of truth:** `docs/superpowers/specs/2026-05-25-recon-ingestion-design.md`.

---

## Shared type contract (authoritative — reuse verbatim across tasks)

**`recon-ingest` (Rust):**

```rust
// Parser output draft — no id / tenant / source yet.
pub struct ParsedTxn {
    pub external_ref: String,
    pub value_date: String,          // "YYYY-MM-DD"
    pub posted_at: Option<String>,   // RFC3339; None => API defaults to value_date T00:00:00Z
    pub amount_minor: i64,           // non-negative magnitude
    pub currency: Option<String>,    // None => API defaults to source currency
    pub direction: recon_domain::Direction,
    pub counterparty: Option<String>,
    pub description: String,
}

pub struct RowError { pub row: usize, pub field: String, pub message: String }

pub trait Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>>;
}

// CSV-specific (serde camelCase so the API can deserialize the multipart `mapping` field)
pub enum ColRef { Index(usize), Header(String) }          // serde: {"index":0} | {"header":"Ref"}
// NOTE: needs BOTH rename_all="camelCase" (variant names) AND
// rename_all_fields="camelCase" (struct-variant fields) so debit_when_negative -> debitWhenNegative.
pub enum AmountMapping {                                    // serde: {"signed":{"column":...,"debitWhenNegative":true}} | {"debitCredit":{"debit":...,"credit":...}}
    Signed { column: ColRef, debit_when_negative: bool },
    DebitCredit { debit: ColRef, credit: ColRef },
}
pub struct CsvMapping {
    pub has_header: bool,
    pub delimiter: u8,               // byte value, default 44 (',')
    pub external_ref: ColRef,
    pub value_date: ColRef,
    pub date_format: String,         // chrono fmt, e.g. "%d/%m/%Y"
    pub amount: AmountMapping,
    pub description: ColRef,
    pub currency: Option<ColRef>,
    pub counterparty: Option<ColRef>,
}
```

**Error envelope (consistent with the existing API):** ingest errors use the standard `{ "error": { "code", "message", ...extra } }` envelope, with structured extras merged in:
- **422** `{ "error": { "code": "parse", "message": "...", "rows": [{ "row", "field", "message" }] } }`
- **409** `{ "error": { "code": "duplicate", "message": "...", "refs": ["..."] } }`
- **200** `{ "ingested": <n>, "sourceId": "..." }`

**Frontend (`web/lib/api/client.ts`):**

```ts
export interface SourceListItem extends Source { txnCount: number }
export interface CreateSourceInput { kind: SourceKind; name: string; currency: string }
export type IngestFormat = "csv" | "camt053";
export interface IngestResult { ingested: number; sourceId: string }
export interface CreateRunInput { name: string; sourceAId: string; sourceBId: string; from: string; to: string }

// CSV mapping mirrors the Rust serde shape exactly.
export type ColRef = { index: number } | { header: string };
export type AmountMapping =
  | { signed: { column: ColRef; debitWhenNegative: boolean } }
  | { debitCredit: { debit: ColRef; credit: ColRef } };
export interface CsvMapping {
  hasHeader: boolean;
  delimiter: number;            // byte value, 44 = ','
  externalRef: ColRef;
  valueDate: ColRef;
  dateFormat: string;
  amount: AmountMapping;
  description: ColRef;
  currency?: ColRef;
  counterparty?: ColRef;
}
```

`ingestFile` rejects with an `IngestError` on 422/409 (below) so the dialog can render the report:

```ts
export class IngestError extends Error {
  constructor(
    public code: "parse" | "duplicate",
    message: string,
    public rows?: { row: number; field: string; message: string }[],
    public refs?: string[],
  ) { super(message); }
}
```

**`ApiClient` additions:**

```ts
listSources(tenantId: string): Promise<SourceListItem[]>;
createSource(tenantId: string, input: CreateSourceInput): Promise<Source>;
ingestFile(tenantId: string, sourceId: string, format: IngestFormat, file: File, mapping?: CsvMapping): Promise<IngestResult>;
createRun(tenantId: string, input: CreateRunInput): Promise<ReconciliationRun>;
```

---

# Phase A — `recon-ingest` crate (pure parsers)

### Task A1: Scaffold the crate + core types

**Files:**
- Create: `backend/crates/recon-ingest/Cargo.toml`
- Create: `backend/crates/recon-ingest/src/lib.rs`
- Modify: `backend/Cargo.toml` (workspace members + add `csv`, `quick-xml`, `chrono` to `[workspace.dependencies]`)

- [ ] **Step 1: Add workspace deps and member**

In `backend/Cargo.toml`, add to `[workspace.dependencies]`:

```toml
csv = "1"
quick-xml = "0.36"
chrono = { version = "0.4", default-features = false, features = ["std"] }
```

And add the member to `[workspace] members`:

```toml
  "crates/recon-ingest",
```

- [ ] **Step 2: Create the crate manifest**

`backend/crates/recon-ingest/Cargo.toml`:

```toml
[package]
name = "recon-ingest"
edition.workspace = true
version.workspace = true

[dependencies]
recon-domain = { path = "../recon-domain" }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
csv = { workspace = true }
quick-xml = { workspace = true }
chrono = { workspace = true }

[dev-dependencies]
proptest = { workspace = true }
```

- [ ] **Step 3: Write the failing test for the core types**

`backend/crates/recon-ingest/src/lib.rs`:

```rust
pub mod camt053;
pub mod csv;
pub mod money;

use recon_domain::Direction;

/// A parsed transaction draft. No id / tenant / source yet — the API assigns
/// those when mapping to a `CanonicalTransaction`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTxn {
    pub external_ref: String,
    pub value_date: String,
    pub posted_at: Option<String>,
    pub amount_minor: i64,
    pub currency: Option<String>,
    pub direction: Direction,
    pub counterparty: Option<String>,
    pub description: String,
}

/// One row-level parse failure. Collected so a whole file is rejected atomically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowError {
    pub row: usize,
    pub field: String,
    pub message: String,
}

impl RowError {
    pub fn new(row: usize, field: impl Into<String>, message: impl Into<String>) -> Self {
        Self { row, field: field.into(), message: message.into() }
    }
}

/// Parse raw file bytes into transaction drafts. On ANY row error, returns Err
/// with the full list (atomic: the caller stores nothing).
pub trait Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_error_constructs() {
        let e = RowError::new(3, "amount", "bad");
        assert_eq!(e.row, 3);
        assert_eq!(e.field, "amount");
        assert_eq!(e.message, "bad");
    }
}
```

- [ ] **Step 4: Create empty module files so it compiles**

Create `backend/crates/recon-ingest/src/money.rs`, `src/csv.rs`, `src/camt053.rs` each containing only a doc comment line `//! placeholder` for now (filled in later tasks). This lets A1 compile in isolation.

- [ ] **Step 5: Run the test (expect FAIL then PASS)**

Run: `cd backend && cargo test -p recon-ingest row_error_constructs`
Expected: compiles and PASSES once `lib.rs` is in place (FAIL beforehand: module files missing).

- [ ] **Step 6: Commit**

```bash
git add backend/Cargo.toml backend/crates/recon-ingest
git commit -m "feat(ingest): scaffold recon-ingest crate with Parser trait and core types

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A2: Decimal → minor-unit money parsing

**Files:**
- Modify: `backend/crates/recon-ingest/src/money.rs`

- [ ] **Step 1: Write the failing tests**

Replace `backend/crates/recon-ingest/src/money.rs` with the tests first (put `mod tests` at the bottom after you add the impl in step 3; for now write the test module and a stub):

```rust
//! Decimal-string → signed minor-unit (2 dp) parsing.

/// Parse a decimal money string into SIGNED minor units (hundredths).
/// Handles surrounding whitespace, thousands separators (','), a leading
/// '+'/'-', and accounting parentheses (e.g. "(50.00)" => -5000).
/// Rejects empty strings, non-numeric input, and more than 2 decimal places.
pub fn parse_decimal_to_minor(raw: &str) -> Result<i64, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Err("empty amount".into());
    }
    let (neg_paren, s) = if s.starts_with('(') && s.ends_with(')') {
        (true, &s[1..s.len() - 1])
    } else {
        (false, s)
    };
    let s = s.trim().replace(',', "");
    let (sign, digits) = match s.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => (1i64, s.strip_prefix('+').unwrap_or(&s)),
    };
    if digits.is_empty() {
        return Err(format!("not a number: {raw}"));
    }
    let (int_part, frac_part) = match digits.split_once('.') {
        Some((i, f)) => (i, f),
        None => (digits, ""),
    };
    if frac_part.len() > 2 {
        return Err(format!("more than 2 decimal places: {raw}"));
    }
    let int_part = if int_part.is_empty() { "0" } else { int_part };
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(format!("not a number: {raw}"));
    }
    let whole: i64 = int_part.parse().map_err(|_| format!("not a number: {raw}"))?;
    let frac: i64 = format!("{frac_part:0<2}").parse().unwrap_or(0);
    let magnitude = whole * 100 + frac;
    let signed = magnitude * sign * if neg_paren { -1 } else { 1 };
    Ok(signed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain() {
        assert_eq!(parse_decimal_to_minor("123.45").unwrap(), 12345);
    }
    #[test]
    fn parses_integer() {
        assert_eq!(parse_decimal_to_minor("100").unwrap(), 10000);
    }
    #[test]
    fn parses_one_decimal() {
        assert_eq!(parse_decimal_to_minor("12.5").unwrap(), 1250);
    }
    #[test]
    fn parses_thousands_separators() {
        assert_eq!(parse_decimal_to_minor("1,234.56").unwrap(), 123456);
    }
    #[test]
    fn parses_parens_as_negative() {
        assert_eq!(parse_decimal_to_minor("(50.00)").unwrap(), -5000);
    }
    #[test]
    fn parses_leading_minus() {
        assert_eq!(parse_decimal_to_minor("-7.00").unwrap(), -700);
    }
    #[test]
    fn rejects_empty() {
        assert!(parse_decimal_to_minor("   ").is_err());
    }
    #[test]
    fn rejects_three_decimals() {
        assert!(parse_decimal_to_minor("1.234").is_err());
    }
    #[test]
    fn rejects_garbage() {
        assert!(parse_decimal_to_minor("abc").is_err());
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cd backend && cargo test -p recon-ingest money::`
Expected: all 9 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/recon-ingest/src/money.rs
git commit -m "feat(ingest): decimal-to-minor money parsing with parens/thousands handling

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A3: CSV parser — happy paths (signed + debit/credit, header + index)

**Files:**
- Modify: `backend/crates/recon-ingest/src/csv.rs`

- [ ] **Step 1: Write the parser + types + happy-path tests**

Replace `backend/crates/recon-ingest/src/csv.rs`:

```rust
//! CSV parsing driven by a per-upload `CsvMapping`.

use crate::money::parse_decimal_to_minor;
use crate::{ParsedTxn, Parser, RowError};
use recon_domain::Direction;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ColRef {
    Index(usize),
    Header(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum AmountMapping {
    Signed { column: ColRef, debit_when_negative: bool },
    DebitCredit { debit: ColRef, credit: ColRef },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsvMapping {
    pub has_header: bool,
    pub delimiter: u8,
    pub external_ref: ColRef,
    pub value_date: ColRef,
    pub date_format: String,
    pub amount: AmountMapping,
    pub description: ColRef,
    #[serde(default)]
    pub currency: Option<ColRef>,
    #[serde(default)]
    pub counterparty: Option<ColRef>,
}

pub struct CsvParser {
    mapping: CsvMapping,
}

impl CsvParser {
    pub fn new(mapping: CsvMapping) -> Self {
        Self { mapping }
    }

    /// Resolve a ColRef to a field value for `record`, given the optional header row.
    fn get<'a>(
        &self,
        record: &'a csv::StringRecord,
        headers: Option<&csv::StringRecord>,
        col: &ColRef,
    ) -> Result<&'a str, String> {
        let idx = match col {
            ColRef::Index(i) => *i,
            ColRef::Header(name) => headers
                .and_then(|h| h.iter().position(|c| c == name))
                .ok_or_else(|| format!("header not found: {name}"))?,
        };
        record.get(idx).ok_or_else(|| format!("column {idx} out of range"))
    }
}

impl Parser for CsvParser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(self.mapping.has_header)
            .delimiter(self.mapping.delimiter)
            .flexible(true)
            .from_reader(bytes);

        let headers = if self.mapping.has_header {
            rdr.headers().ok().cloned()
        } else {
            None
        };

        let mut out = Vec::new();
        let mut errors = Vec::new();

        for (i, result) in rdr.records().enumerate() {
            // Row number presented to users is 1-based and counts the header.
            let row = if self.mapping.has_header { i + 2 } else { i + 1 };
            let record = match result {
                Ok(r) => r,
                Err(e) => {
                    errors.push(RowError::new(row, "row", format!("malformed CSV: {e}")));
                    continue;
                }
            };
            match self.parse_record(&record, headers.as_ref(), row) {
                Ok(txn) => out.push(txn),
                Err(mut errs) => errors.append(&mut errs),
            }
        }

        if errors.is_empty() {
            Ok(out)
        } else {
            Err(errors)
        }
    }
}

impl CsvParser {
    fn parse_record(
        &self,
        record: &csv::StringRecord,
        headers: Option<&csv::StringRecord>,
        row: usize,
    ) -> Result<ParsedTxn, Vec<RowError>> {
        let mut errs = Vec::new();

        macro_rules! field {
            ($col:expr, $name:expr) => {
                match self.get(record, headers, $col) {
                    Ok(v) => Some(v.trim().to_string()),
                    Err(m) => {
                        errs.push(RowError::new(row, $name, m));
                        None
                    }
                }
            };
        }

        let external_ref = field!(&self.mapping.external_ref, "externalRef");
        let raw_date = field!(&self.mapping.value_date, "valueDate");
        let description = field!(&self.mapping.description, "description");

        // value_date: parse with the configured chrono format, re-emit as YYYY-MM-DD.
        let value_date = raw_date.as_ref().and_then(|d| {
            match chrono::NaiveDate::parse_from_str(d, &self.mapping.date_format) {
                Ok(nd) => Some(nd.format("%Y-%m-%d").to_string()),
                Err(_) => {
                    errs.push(RowError::new(
                        row,
                        "valueDate",
                        format!("unparseable date '{d}' for format '{}'", self.mapping.date_format),
                    ));
                    None
                }
            }
        });

        let (amount_minor, direction) = self.parse_amount(record, headers, row, &mut errs);

        let currency = self
            .mapping
            .currency
            .as_ref()
            .and_then(|c| self.get(record, headers, c).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let counterparty = self
            .mapping
            .counterparty
            .as_ref()
            .and_then(|c| self.get(record, headers, c).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        if let Some(r) = &external_ref {
            if r.is_empty() {
                errs.push(RowError::new(row, "externalRef", "empty reference"));
            }
        }

        if !errs.is_empty() {
            return Err(errs);
        }

        Ok(ParsedTxn {
            external_ref: external_ref.unwrap(),
            value_date: value_date.unwrap(),
            posted_at: None,
            amount_minor: amount_minor.unwrap(),
            currency,
            direction: direction.unwrap(),
            counterparty,
            description: description.unwrap(),
        })
    }

    fn parse_amount(
        &self,
        record: &csv::StringRecord,
        headers: Option<&csv::StringRecord>,
        row: usize,
        errs: &mut Vec<RowError>,
    ) -> (Option<i64>, Option<Direction>) {
        match &self.mapping.amount {
            AmountMapping::Signed { column, debit_when_negative } => {
                let raw = match self.get(record, headers, column) {
                    Ok(v) => v.trim(),
                    Err(m) => {
                        errs.push(RowError::new(row, "amount", m));
                        return (None, None);
                    }
                };
                match parse_decimal_to_minor(raw) {
                    Ok(signed) => {
                        let is_neg = signed < 0;
                        let direction = if is_neg == *debit_when_negative {
                            Direction::Debit
                        } else {
                            Direction::Credit
                        };
                        (Some(signed.abs()), Some(direction))
                    }
                    Err(m) => {
                        errs.push(RowError::new(row, "amount", m));
                        (None, None)
                    }
                }
            }
            AmountMapping::DebitCredit { debit, credit } => {
                let d = self.get(record, headers, debit).unwrap_or("").trim().to_string();
                let c = self.get(record, headers, credit).unwrap_or("").trim().to_string();
                let d_has = !d.is_empty() && parse_decimal_to_minor(&d).map(|v| v != 0).unwrap_or(false);
                let c_has = !c.is_empty() && parse_decimal_to_minor(&c).map(|v| v != 0).unwrap_or(false);
                match (d_has, c_has) {
                    (true, false) => match parse_decimal_to_minor(&d) {
                        Ok(v) => (Some(v.abs()), Some(Direction::Debit)),
                        Err(m) => { errs.push(RowError::new(row, "amount", m)); (None, None) }
                    },
                    (false, true) => match parse_decimal_to_minor(&c) {
                        Ok(v) => (Some(v.abs()), Some(Direction::Credit)),
                        Err(m) => { errs.push(RowError::new(row, "amount", m)); (None, None) }
                    },
                    (false, false) => {
                        errs.push(RowError::new(row, "amount", "neither debit nor credit populated"));
                        (None, None)
                    }
                    (true, true) => {
                        errs.push(RowError::new(row, "amount", "both debit and credit populated"));
                        (None, None)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signed_mapping() -> CsvMapping {
        CsvMapping {
            has_header: true,
            delimiter: b',',
            external_ref: ColRef::Header("ref".into()),
            value_date: ColRef::Header("date".into()),
            date_format: "%Y-%m-%d".into(),
            amount: AmountMapping::Signed {
                column: ColRef::Header("amount".into()),
                debit_when_negative: true,
            },
            description: ColRef::Header("desc".into()),
            currency: Some(ColRef::Header("ccy".into())),
            counterparty: None,
        }
    }

    #[test]
    fn parses_signed_with_header() {
        let csv = "ref,date,amount,ccy,desc\nR1,2026-05-10,-12.50,GBP,Coffee\nR2,2026-05-11,40.00,GBP,Refund\n";
        let txns = CsvParser::new(signed_mapping()).parse(csv.as_bytes()).unwrap();
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].external_ref, "R1");
        assert_eq!(txns[0].value_date, "2026-05-10");
        assert_eq!(txns[0].amount_minor, 1250);
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[0].currency.as_deref(), Some("GBP"));
        assert_eq!(txns[1].direction, Direction::Credit);
        assert_eq!(txns[0].posted_at, None);
    }

    #[test]
    fn parses_debit_credit_columns_by_index_no_header() {
        let mapping = CsvMapping {
            has_header: false,
            delimiter: b',',
            external_ref: ColRef::Index(0),
            value_date: ColRef::Index(1),
            date_format: "%d/%m/%Y".into(),
            amount: AmountMapping::DebitCredit { debit: ColRef::Index(2), credit: ColRef::Index(3) },
            description: ColRef::Index(4),
            currency: None,
            counterparty: None,
        };
        let csv = "R1,10/05/2026,12.50,,Coffee\nR2,11/05/2026,,40.00,Refund\n";
        let txns = CsvParser::new(mapping).parse(csv.as_bytes()).unwrap();
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].value_date, "2026-05-10");
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[0].amount_minor, 1250);
        assert_eq!(txns[1].direction, Direction::Credit);
        assert_eq!(txns[1].amount_minor, 4000);
        assert_eq!(txns[0].currency, None);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cd backend && cargo test -p recon-ingest csv::tests::parses`
Expected: both tests PASS.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/recon-ingest/src/csv.rs
git commit -m "feat(ingest): CSV parser with signed and debit/credit amount mappings

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A4: CSV parser — atomic error reporting

**Files:**
- Modify: `backend/crates/recon-ingest/src/csv.rs` (append tests to the existing `mod tests`)

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `backend/crates/recon-ingest/src/csv.rs`:

```rust
    #[test]
    fn collects_all_bad_rows_and_rejects_atomically() {
        let csv = "ref,date,amount,ccy,desc\n\
                   R1,2026-05-10,-12.50,GBP,Coffee\n\
                   R2,not-a-date,40.00,GBP,Refund\n\
                   R3,2026-05-12,xx,GBP,Bad amount\n";
        let errs = CsvParser::new(signed_mapping()).parse(csv.as_bytes()).unwrap_err();
        // Two bad rows -> two errors; nothing returned.
        assert_eq!(errs.len(), 2);
        assert_eq!(errs[0].row, 4); // R2 (header + 1-based)
        assert_eq!(errs[0].field, "valueDate");
        assert_eq!(errs[1].row, 5); // R3
        assert_eq!(errs[1].field, "amount");
    }

    #[test]
    fn empty_external_ref_is_an_error() {
        let csv = "ref,date,amount,ccy,desc\n,2026-05-10,-12.50,GBP,Coffee\n";
        let errs = CsvParser::new(signed_mapping()).parse(csv.as_bytes()).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].field, "externalRef");
    }

    #[test]
    fn missing_column_index_is_an_error() {
        let mapping = CsvMapping {
            has_header: false,
            delimiter: b',',
            external_ref: ColRef::Index(0),
            value_date: ColRef::Index(1),
            date_format: "%Y-%m-%d".into(),
            amount: AmountMapping::Signed { column: ColRef::Index(9), debit_when_negative: true },
            description: ColRef::Index(2),
            currency: None,
            counterparty: None,
        };
        let csv = "R1,2026-05-10,Coffee\n";
        let errs = CsvParser::new(mapping).parse(csv.as_bytes()).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].field, "amount");
    }
```

- [ ] **Step 2: Run the tests**

Run: `cd backend && cargo test -p recon-ingest csv::tests`
Expected: all CSV tests PASS (the implementation from A3 already satisfies these — this task locks the atomic behavior with tests).

- [ ] **Step 3: Commit**

```bash
git add backend/crates/recon-ingest/src/csv.rs
git commit -m "test(ingest): CSV parser collects all row errors and rejects atomically

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A5: CAMT.053 parser — happy path

**Files:**
- Modify: `backend/crates/recon-ingest/src/camt053.rs`

- [ ] **Step 1: Write the parser + happy-path test**

Replace `backend/crates/recon-ingest/src/camt053.rs`:

```rust
//! ISO 20022 CAMT.053 (bank-to-customer statement) parsing.
//!
//! Uses quick-xml's pull parser, which does NOT perform DTD processing or
//! external-entity expansion — so XXE / billion-laughs are not reachable. We
//! never implement custom entity resolution.

use crate::{ParsedTxn, Parser, RowError};
use quick_xml::events::Event;
use quick_xml::Reader;
use recon_domain::Direction;

#[derive(Default)]
pub struct Camt053Parser;

#[derive(Default)]
struct EntryAccum {
    amount: Option<String>,
    currency: Option<String>,
    cd_dbt: Option<String>,
    value_date: Option<String>,
    booking_date: Option<String>,
    acct_svcr_ref: Option<String>,
    ntry_ref: Option<String>,
    ustrd: Option<String>,
    addtl: Option<String>,
}

impl Parser for Camt053Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        let text = match std::str::from_utf8(bytes) {
            Ok(t) => t,
            Err(_) => return Err(vec![RowError::new(0, "file", "not valid UTF-8")]),
        };
        let mut reader = Reader::from_str(text);
        reader.config_mut().trim_text(true);

        let mut out = Vec::new();
        let mut errors = Vec::new();
        let mut entry_index = 0usize;

        // path is the stack of element local-names; `in_*` flags scope the
        // sub-elements that share generic tags (e.g. <Dt> appears under both
        // <ValDt> and <BookgDt>).
        let mut path: Vec<String> = Vec::new();
        let mut accum: Option<EntryAccum> = None;
        let mut amount_ccy: Option<String> = None;
        let mut last_text = String::new();

        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    if name == "Ntry" {
                        accum = Some(EntryAccum::default());
                        entry_index += 1;
                    }
                    if name == "Amt" {
                        // capture the Ccy attribute
                        amount_ccy = e
                            .attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == b"Ccy")
                            .and_then(|a| String::from_utf8(a.value.to_vec()).ok());
                    }
                    path.push(name);
                    last_text.clear();
                }
                Ok(Event::Text(t)) => {
                    last_text = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                }
                Ok(Event::End(e)) => {
                    let name = local_name_end(&e);
                    if let Some(acc) = accum.as_mut() {
                        apply_text(acc, &path, &name, &last_text, &mut amount_ccy);
                    }
                    if name == "Ntry" {
                        if let Some(acc) = accum.take() {
                            match finalize(acc, entry_index) {
                                Ok(txn) => out.push(txn),
                                Err(mut errs) => errors.append(&mut errs),
                            }
                        }
                    }
                    path.pop();
                    last_text.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    errors.push(RowError::new(entry_index, "xml", format!("malformed XML: {e}")));
                    break;
                }
                _ => {}
            }
        }

        if errors.is_empty() {
            Ok(out)
        } else {
            Err(errors)
        }
    }
}

fn local_name(e: &quick_xml::events::BytesStart) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}
fn local_name_end(e: &quick_xml::events::BytesEnd) -> String {
    String::from_utf8_lossy(e.local_name().as_ref()).into_owned()
}

fn parent(path: &[String]) -> &str {
    // path still contains the element we're closing as the last item.
    if path.len() >= 2 { path[path.len() - 2].as_str() } else { "" }
}

fn apply_text(
    acc: &mut EntryAccum,
    path: &[String],
    name: &str,
    text: &str,
    amount_ccy: &mut Option<String>,
) {
    let p = parent(path);
    match name {
        "Amt" => {
            acc.amount = Some(text.to_string());
            acc.currency = amount_ccy.take();
        }
        "CdtDbtInd" => acc.cd_dbt = Some(text.to_string()),
        "Dt" if p == "ValDt" => acc.value_date = Some(text.to_string()),
        "Dt" if p == "BookgDt" => acc.booking_date = Some(text.to_string()),
        "AcctSvcrRef" => acc.acct_svcr_ref = Some(text.to_string()),
        "NtryRef" => acc.ntry_ref = Some(text.to_string()),
        "Ustrd" => acc.ustrd = Some(text.to_string()),
        "AddtlNtryInf" => acc.addtl = Some(text.to_string()),
        _ => {}
    }
}

fn finalize(acc: EntryAccum, idx: usize) -> Result<ParsedTxn, Vec<RowError>> {
    let mut errs = Vec::new();

    let external_ref = acc
        .acct_svcr_ref
        .or(acc.ntry_ref)
        .filter(|s| !s.is_empty());
    if external_ref.is_none() {
        errs.push(RowError::new(idx, "externalRef", "missing AcctSvcrRef/NtryRef"));
    }
    let value_date = acc.value_date.filter(|s| !s.is_empty());
    if value_date.is_none() {
        errs.push(RowError::new(idx, "valueDate", "missing ValDt/Dt"));
    }
    let direction = match acc.cd_dbt.as_deref() {
        Some("DBIT") => Some(Direction::Debit),
        Some("CRDT") => Some(Direction::Credit),
        _ => {
            errs.push(RowError::new(idx, "direction", "missing/invalid CdtDbtInd"));
            None
        }
    };
    let amount_minor = match acc.amount.as_deref() {
        Some(a) => match crate::money::parse_decimal_to_minor(a) {
            Ok(v) => Some(v.abs()),
            Err(m) => {
                errs.push(RowError::new(idx, "amount", m));
                None
            }
        },
        None => {
            errs.push(RowError::new(idx, "amount", "missing Amt"));
            None
        }
    };

    if !errs.is_empty() {
        return Err(errs);
    }

    let value_date = value_date.unwrap();
    let posted_at = acc
        .booking_date
        .filter(|s| !s.is_empty())
        .map(|d| if d.contains('T') { d } else { format!("{d}T00:00:00Z") });

    Ok(ParsedTxn {
        external_ref: external_ref.unwrap(),
        value_date,
        posted_at,
        amount_minor: amount_minor.unwrap(),
        currency: acc.currency.filter(|s| !s.is_empty()),
        direction: direction.unwrap(),
        counterparty: None,
        description: acc.ustrd.or(acc.addtl).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02">
  <BkToCstmrStmt>
    <Stmt>
      <Ntry>
        <Amt Ccy="GBP">125.00</Amt>
        <CdtDbtInd>DBIT</CdtDbtInd>
        <BookgDt><Dt>2026-05-10</Dt></BookgDt>
        <ValDt><Dt>2026-05-10</Dt></ValDt>
        <NtryDtls><TxDtls>
          <Refs><AcctSvcrRef>REF-001</AcctSvcrRef></Refs>
          <RmtInf><Ustrd>Invoice 4001</Ustrd></RmtInf>
        </TxDtls></NtryDtls>
      </Ntry>
      <Ntry>
        <Amt Ccy="GBP">90.50</Amt>
        <CdtDbtInd>CRDT</CdtDbtInd>
        <ValDt><Dt>2026-05-11</Dt></ValDt>
        <NtryRef>REF-002</NtryRef>
        <AddtlNtryInf>Customer payment</AddtlNtryInf>
      </Ntry>
    </Stmt>
  </BkToCstmrStmt>
</Document>"#;

    #[test]
    fn parses_two_entries() {
        let txns = Camt053Parser.parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(txns.len(), 2);

        assert_eq!(txns[0].external_ref, "REF-001");
        assert_eq!(txns[0].amount_minor, 12500);
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[0].value_date, "2026-05-10");
        assert_eq!(txns[0].currency.as_deref(), Some("GBP"));
        assert_eq!(txns[0].posted_at.as_deref(), Some("2026-05-10T00:00:00Z"));
        assert_eq!(txns[0].description, "Invoice 4001");

        assert_eq!(txns[1].external_ref, "REF-002");
        assert_eq!(txns[1].direction, Direction::Credit);
        assert_eq!(txns[1].amount_minor, 9050);
        assert_eq!(txns[1].posted_at, None);
        assert_eq!(txns[1].description, "Customer payment");
    }
}
```

- [ ] **Step 2: Run the test**

Run: `cd backend && cargo test -p recon-ingest camt053::tests::parses_two_entries`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/recon-ingest/src/camt053.rs
git commit -m "feat(ingest): CAMT.053 XML parser (entity-expansion-safe)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A6: CAMT.053 parser — error cases + property tests

**Files:**
- Modify: `backend/crates/recon-ingest/src/camt053.rs` (append to `mod tests`)
- Create: `backend/crates/recon-ingest/tests/properties.rs`

- [ ] **Step 1: Write CAMT error tests**

Append inside `mod tests` in `camt053.rs`:

```rust
    #[test]
    fn entry_missing_required_fields_errors() {
        let xml = r#"<Document><Stmt><Ntry>
            <Amt Ccy="GBP">10.00</Amt>
            <ValDt><Dt>2026-05-10</Dt></ValDt>
          </Ntry></Stmt></Document>"#;
        // No CdtDbtInd, no ref -> two errors, nothing returned.
        let errs = Camt053Parser.parse(xml.as_bytes()).unwrap_err();
        let fields: Vec<&str> = errs.iter().map(|e| e.field.as_str()).collect();
        assert!(fields.contains(&"direction"));
        assert!(fields.contains(&"externalRef"));
    }

    #[test]
    fn malformed_xml_errors() {
        let xml = "<Document><Ntry><Amt>oops"; // unclosed
        assert!(Camt053Parser.parse(xml.as_bytes()).is_err());
    }
```

- [ ] **Step 2: Write the property test**

`backend/crates/recon-ingest/tests/properties.rs`:

```rust
use proptest::prelude::*;
use recon_ingest::csv::{AmountMapping, ColRef, CsvMapping, CsvParser};
use recon_ingest::Parser;

fn mapping() -> CsvMapping {
    CsvMapping {
        has_header: false,
        delimiter: b',',
        external_ref: ColRef::Index(0),
        value_date: ColRef::Index(1),
        date_format: "%Y-%m-%d".into(),
        amount: AmountMapping::Signed { column: ColRef::Index(2), debit_when_negative: true },
        description: ColRef::Index(3),
        currency: None,
        counterparty: None,
    }
}

proptest! {
    // Any successfully-parsed signed amount yields a non-negative magnitude.
    #[test]
    fn amount_minor_is_non_negative(cents in -1_000_000i64..1_000_000) {
        let whole = cents / 100;
        let frac = (cents % 100).abs();
        let line = format!("R1,2026-05-10,{whole}.{frac:02},Desc\n");
        if let Ok(txns) = CsvParser::new(mapping()).parse(line.as_bytes()) {
            for t in txns {
                prop_assert!(t.amount_minor >= 0);
            }
        }
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cd backend && cargo test -p recon-ingest`
Expected: all unit tests + the property test PASS.

- [ ] **Step 4: Lint**

Run: `cd backend && cargo clippy -p recon-ingest -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/recon-ingest/src/camt053.rs backend/crates/recon-ingest/tests/properties.rs
git commit -m "test(ingest): CAMT error cases + non-negative-amount property

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase B — Store

### Task B1: Migration 0003 — uniqueness constraint

**Files:**
- Create: `backend/migrations/0003_ingest.sql`
- Create: `backend/crates/recon-store/tests/ingest.rs`

- [ ] **Step 1: Write the migration**

`backend/migrations/0003_ingest.sql`:

```sql
-- Prevent the same transaction being ingested twice into one source.
ALTER TABLE canonical_transactions
  ADD CONSTRAINT uq_txn_source_ref UNIQUE (source_id, external_ref);
```

- [ ] **Step 2: Write a failing test that the constraint exists**

`backend/crates/recon-store/tests/ingest.rs`:

```rust
use recon_store::Store;

#[sqlx::test(migrations = "../../migrations")]
async fn unique_constraint_blocks_duplicate_ref(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    // minimal tenant + source
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ('s','t','bank','Bank','GBP')")
        .execute(&store.pool).await.unwrap();
    let ins = |r: &str| sqlx::query(
        "INSERT INTO canonical_transactions(id,tenant_id,source_id,external_ref,value_date,posted_at,amount_minor,currency,direction,description) \
         VALUES ($1,'t','s',$2,'2026-05-10','2026-05-10T00:00:00Z'::timestamptz,100,'GBP','debit','x')")
        .bind(format!("txn-{r}")).bind(r).execute(&store.pool);
    ins("DUP").await.unwrap();
    let second = ins("DUP").await;
    assert!(second.is_err(), "second insert of same (source,ref) must violate the unique constraint");
}
```

- [ ] **Step 3: Run the test**

Run: `cd backend && cargo test -p recon-store --test ingest unique_constraint_blocks_duplicate_ref`
Expected: PASS (the second insert errors).

- [ ] **Step 4: Commit**

```bash
git add backend/migrations/0003_ingest.sql backend/crates/recon-store/tests/ingest.rs
git commit -m "feat(store): migration 0003 unique (source_id, external_ref)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B2: Source create / get / list

**Files:**
- Create: `backend/crates/recon-store/src/sources.rs`
- Modify: `backend/crates/recon-store/src/lib.rs` (add `pub mod sources;`)
- Modify: `backend/crates/recon-store/src/error.rs` (add `DuplicateRefs` variant — used in B3, added now)
- Modify: `backend/crates/recon-store/tests/ingest.rs` (append tests)

- [ ] **Step 1: Add the error variant**

In `backend/crates/recon-store/src/error.rs`, add to the `StoreError` enum:

```rust
    #[error("duplicate refs")]
    DuplicateRefs(Vec<String>),
```

- [ ] **Step 2: Register the module**

In `backend/crates/recon-store/src/lib.rs`, add to the module list:

```rust
pub mod sources;
```

- [ ] **Step 3: Write the source methods**

`backend/crates/recon-store/src/sources.rs`:

```rust
use crate::rows::SourceRow;
use crate::{Store, StoreError};
use recon_domain::{Source, SourceKind};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceListItem {
    #[serde(flatten)]
    pub source: Source,
    pub txn_count: i64,
}

fn kind_str(k: SourceKind) -> &'static str {
    match k {
        SourceKind::Bank => "bank",
        SourceKind::Ledger => "ledger",
        SourceKind::CrossSystem => "cross_system",
    }
}

impl Store {
    pub async fn create_source(
        &self,
        tenant_id: &str,
        kind: SourceKind,
        name: &str,
        currency: &str,
    ) -> Result<Source, StoreError> {
        let id = format!("src-{}", Uuid::new_v4());
        sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ($1,$2,$3,$4,$5)")
            .bind(&id)
            .bind(tenant_id)
            .bind(kind_str(kind))
            .bind(name)
            .bind(currency)
            .execute(&self.pool)
            .await?;
        Ok(Source { id, tenant_id: tenant_id.to_string(), kind, name: name.to_string(), currency: currency.to_string() })
    }

    pub async fn get_source(&self, tenant_id: &str, id: &str) -> Result<Source, StoreError> {
        let row: Option<SourceRow> =
            sqlx::query_as("SELECT id,tenant_id,kind,name,currency FROM sources WHERE id=$1 AND tenant_id=$2")
                .bind(id)
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?;
        row.map(Into::into).ok_or(StoreError::NotFound)
    }

    pub async fn list_sources(&self, tenant_id: &str) -> Result<Vec<SourceListItem>, StoreError> {
        let rows: Vec<SourceRow> =
            sqlx::query_as("SELECT id,tenant_id,kind,name,currency FROM sources WHERE tenant_id=$1 ORDER BY name")
                .bind(tenant_id)
                .fetch_all(&self.pool)
                .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM canonical_transactions WHERE source_id=$1",
            )
            .bind(&r.id)
            .fetch_one(&self.pool)
            .await?;
            out.push(SourceListItem { source: r.into(), txn_count: count });
        }
        Ok(out)
    }
}
```

- [ ] **Step 4: Append store tests**

Append to `backend/crates/recon-store/tests/ingest.rs`:

```rust
use recon_domain::SourceKind;

#[sqlx::test(migrations = "../../migrations")]
async fn create_and_list_sources(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    let s = store.create_source("t", SourceKind::Bank, "Acme Bank", "GBP").await.unwrap();
    assert!(s.id.starts_with("src-"));
    let got = store.get_source("t", &s.id).await.unwrap();
    assert_eq!(got.name, "Acme Bank");
    let list = store.list_sources("t").await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].txn_count, 0);
    // cross-tenant get is NotFound
    assert!(store.get_source("other", &s.id).await.is_err());
}
```

- [ ] **Step 5: Run the tests**

Run: `cd backend && cargo test -p recon-store --test ingest create_and_list_sources`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/recon-store/src/sources.rs backend/crates/recon-store/src/lib.rs backend/crates/recon-store/src/error.rs backend/crates/recon-store/tests/ingest.rs
git commit -m "feat(store): create/get/list sources + DuplicateRefs error variant

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B3: Atomic `ingest_transactions`

**Files:**
- Modify: `backend/crates/recon-store/src/sources.rs`
- Modify: `backend/crates/recon-store/tests/ingest.rs` (append)

- [ ] **Step 1: Add the ingest method**

Append to `impl Store` in `backend/crates/recon-store/src/sources.rs` (add `use recon_domain::{CanonicalTransaction, Direction};` to the imports at the top):

```rust
fn direction_str(d: Direction) -> &'static str {
    match d {
        Direction::Debit => "debit",
        Direction::Credit => "credit",
    }
}

impl Store {
    /// Insert fully-formed transactions into a source, atomically. Rejects the
    /// whole batch (storing nothing) if any external_ref is duplicated within
    /// the batch or already present in the source.
    pub async fn ingest_transactions(
        &self,
        tenant_id: &str,
        source_id: &str,
        txns: &[CanonicalTransaction],
    ) -> Result<usize, StoreError> {
        // Source must belong to the caller's tenant.
        self.get_source(tenant_id, source_id).await?;

        // Within-batch duplicates.
        let mut seen = std::collections::HashSet::new();
        let mut dups: Vec<String> = Vec::new();
        for t in txns {
            if !seen.insert(t.external_ref.as_str()) {
                dups.push(t.external_ref.clone());
            }
        }
        if !dups.is_empty() {
            dups.sort();
            dups.dedup();
            return Err(StoreError::DuplicateRefs(dups));
        }

        // Already-present refs.
        let refs: Vec<String> = txns.iter().map(|t| t.external_ref.clone()).collect();
        let existing: Vec<String> = sqlx::query_scalar(
            "SELECT external_ref FROM canonical_transactions WHERE source_id=$1 AND external_ref = ANY($2)",
        )
        .bind(source_id)
        .bind(&refs)
        .fetch_all(&self.pool)
        .await?;
        if !existing.is_empty() {
            return Err(StoreError::DuplicateRefs(existing));
        }

        let mut tx = self.pool.begin().await?;
        for t in txns {
            sqlx::query(
                "INSERT INTO canonical_transactions(id,tenant_id,source_id,external_ref,value_date,posted_at,amount_minor,currency,direction,counterparty,description) \
                 VALUES ($1,$2,$3,$4,$5::date,$6::timestamptz,$7,$8,$9,$10,$11)",
            )
            .bind(&t.id)
            .bind(tenant_id)
            .bind(source_id)
            .bind(&t.external_ref)
            .bind(&t.value_date)
            .bind(&t.posted_at)
            .bind(t.amount_minor)
            .bind(&t.currency)
            .bind(direction_str(t.direction))
            .bind(&t.counterparty)
            .bind(&t.description)
            .execute(&mut *tx)
            .await
            .map_err(|e| match e {
                sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
                    StoreError::DuplicateRefs(vec![t.external_ref.clone()])
                }
                other => StoreError::Db(other),
            })?;
        }
        tx.commit().await?;
        Ok(txns.len())
    }
}
```

> Note: keep `direction_str` as a free function above the `impl` block (Rust allows multiple `impl Store` blocks in one file).

- [ ] **Step 2: Write the tests (happy + both dup paths + cross-tenant)**

Append to `backend/crates/recon-store/tests/ingest.rs` (add `use recon_domain::{CanonicalTransaction, Direction};` near the top):

```rust
fn txn(id: &str, eref: &str) -> CanonicalTransaction {
    CanonicalTransaction {
        id: id.into(),
        tenant_id: "t".into(),
        source_id: "s".into(),
        external_ref: eref.into(),
        value_date: "2026-05-10".into(),
        posted_at: "2026-05-10T00:00:00Z".into(),
        amount_minor: 100,
        currency: "GBP".into(),
        direction: Direction::Debit,
        counterparty: None,
        description: "x".into(),
    }
}

async fn seed_source(store: &Store) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ('s','t','bank','Bank','GBP')").execute(&store.pool).await.unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_happy_path(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_source(&store).await;
    let n = store.ingest_transactions("t", "s", &[txn("txn-1", "R1"), txn("txn-2", "R2")]).await.unwrap();
    assert_eq!(n, 2);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM canonical_transactions WHERE source_id='s'")
        .fetch_one(&store.pool).await.unwrap();
    assert_eq!(count, 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_rejects_within_batch_dup(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_source(&store).await;
    let err = store.ingest_transactions("t", "s", &[txn("txn-1", "R1"), txn("txn-2", "R1")]).await.unwrap_err();
    match err {
        recon_store::StoreError::DuplicateRefs(refs) => assert_eq!(refs, vec!["R1".to_string()]),
        other => panic!("expected DuplicateRefs, got {other:?}"),
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM canonical_transactions WHERE source_id='s'")
        .fetch_one(&store.pool).await.unwrap();
    assert_eq!(count, 0, "nothing stored on rejection");
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_rejects_existing_ref(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_source(&store).await;
    store.ingest_transactions("t", "s", &[txn("txn-1", "R1")]).await.unwrap();
    let err = store.ingest_transactions("t", "s", &[txn("txn-2", "R1")]).await.unwrap_err();
    assert!(matches!(err, recon_store::StoreError::DuplicateRefs(_)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_into_foreign_source_is_not_found(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_source(&store).await;
    let err = store.ingest_transactions("other", "s", &[txn("txn-1", "R1")]).await.unwrap_err();
    assert!(matches!(err, recon_store::StoreError::NotFound));
}
```

- [ ] **Step 3: Run the tests**

Run: `cd backend && cargo test -p recon-store --test ingest`
Expected: all ingest tests PASS.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/recon-store/src/sources.rs backend/crates/recon-store/tests/ingest.rs
git commit -m "feat(store): atomic ingest_transactions with duplicate-ref rejection

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B4: `persist_run` + `create_run`

**Files:**
- Create: `backend/crates/recon-store/src/runs.rs`
- Modify: `backend/crates/recon-store/src/lib.rs` (add `pub mod runs;`)
- Modify: `backend/crates/recon-store/src/seed.rs` (make `load_window` callable: `async fn load_window` → `pub(crate) async fn load_window`)
- Modify: `backend/crates/recon-store/tests/ingest.rs` (append)

> **Scope note:** `seed.rs` keeps its own specialized run writer because it produces fixed demo ids (`case-pending` / `break-pending`) and the four-eyes pending case that many backend/frontend/E2E tests pin to. `create_run` uses the new generic `persist_run`. This is a deliberate, documented deviation from the spec's "both call it" to avoid destabilizing seeded data; the generic insert logic is small.

- [ ] **Step 1: Make `load_window` crate-visible**

In `backend/crates/recon-store/src/seed.rs`, change:

```rust
    async fn load_window(
```

to:

```rust
    pub(crate) async fn load_window(
```

- [ ] **Step 2: Register the module**

In `backend/crates/recon-store/src/lib.rs` add:

```rust
pub mod runs;
```

- [ ] **Step 3: Write `persist_run` + `create_run`**

`backend/crates/recon-store/src/runs.rs`:

```rust
use crate::{Store, StoreError};
use recon_domain::{ReconciliationRun, RunStatus};
use recon_matching::{reconcile, MatchConfig, RunResult};
use time::OffsetDateTime;
use uuid::Uuid;

impl Store {
    /// Generic writer: run header + decisions + breaks + cases (status `open`,
    /// no assignee, no events). Used by `create_run`. The seed has its own
    /// specialized writer for demo fixtures.
    pub(crate) async fn persist_run(
        &self,
        tx: &mut sqlx::PgConnection,
        run_id: &str,
        tenant_id: &str,
        name: &str,
        sa: &str,
        sb: &str,
        started: &str,
        result: &RunResult,
        cfg: &MatchConfig,
    ) -> Result<(), StoreError> {
        let stats = serde_json::to_value(&result.stats)?;
        sqlx::query("INSERT INTO reconciliation_runs(id,tenant_id,name,source_a_id,source_b_id,status,started_at,completed_at,config_version,stats) VALUES ($1,$2,$3,$4,$5,'completed',$6::timestamptz,$6::timestamptz,$7,$8)")
            .bind(run_id).bind(tenant_id).bind(name).bind(sa).bind(sb).bind(started).bind(&cfg.version).bind(&stats)
            .execute(&mut *tx).await?;

        for (i, d) in result.decisions.iter().enumerate() {
            let type_str = serde_json::to_value(d.match_type)?.as_str().unwrap().to_string();
            sqlx::query("INSERT INTO match_decisions(id,tenant_id,run_id,type,txn_ids,score,config_version) VALUES ($1,$2,$3,$4,$5,$6,$7)")
                .bind(format!("md-{run_id}-{i}")).bind(tenant_id).bind(run_id).bind(type_str).bind(&d.txn_ids).bind(d.score).bind(&cfg.version)
                .execute(&mut *tx).await?;
        }

        for (i, bd) in result.breaks.iter().enumerate() {
            let case_id = format!("case-{run_id}-{i}");
            let break_id = format!("break-{run_id}-{i}");
            let type_str = serde_json::to_value(bd.break_type)?.as_str().unwrap().to_string();
            sqlx::query("INSERT INTO cases(id,tenant_id,break_id,assignee_id,status) VALUES ($1,$2,$3,NULL,'open')")
                .bind(&case_id).bind(tenant_id).bind(&break_id)
                .execute(&mut *tx).await?;
            sqlx::query("INSERT INTO breaks(id,tenant_id,run_id,case_id,type,status,value_minor,currency,assignee_id,txn_ids,opened_at) VALUES ($1,$2,$3,$4,$5,'open',$6,$7,NULL,$8,$9::timestamptz)")
                .bind(&break_id).bind(tenant_id).bind(run_id).bind(&case_id).bind(type_str).bind(bd.value_minor).bind(&bd.currency).bind(&bd.txn_ids).bind(started)
                .execute(&mut *tx).await?;
        }
        Ok(())
    }

    /// Create a run reconciling two sources over a date window. Loads both
    /// windows, runs the matching engine, and persists everything atomically.
    pub async fn create_run(
        &self,
        tenant_id: &str,
        name: &str,
        source_a_id: &str,
        source_b_id: &str,
        from: &str,
        to: &str,
    ) -> Result<ReconciliationRun, StoreError> {
        // Both sources must belong to the caller's tenant.
        self.get_source(tenant_id, source_a_id).await?;
        self.get_source(tenant_id, source_b_id).await?;

        let cfg = MatchConfig::v1();
        let run_id = format!("run-{}", Uuid::new_v4());
        let started = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let mut tx = self.pool.begin().await?;
        let a = self.load_window(&mut tx, tenant_id, source_a_id, from, to).await?;
        let b = self.load_window(&mut tx, tenant_id, source_b_id, from, to).await?;
        let result = reconcile(&a, &b, &cfg);
        self.persist_run(&mut tx, &run_id, tenant_id, name, source_a_id, source_b_id, &started, &result, &cfg)
            .await?;
        tx.commit().await?;

        Ok(ReconciliationRun {
            id: run_id,
            tenant_id: tenant_id.to_string(),
            name: name.to_string(),
            source_a_id: source_a_id.to_string(),
            source_b_id: source_b_id.to_string(),
            status: RunStatus::Completed,
            started_at: started.clone(),
            completed_at: Some(started),
            config_version: cfg.version,
            stats: result.stats,
        })
    }
}
```

- [ ] **Step 4: Write the test**

Append to `backend/crates/recon-store/tests/ingest.rs`:

```rust
#[sqlx::test(migrations = "../../migrations")]
async fn create_run_reconciles_and_persists(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')").execute(&store.pool).await.unwrap();
    let bank = store.create_source("t", SourceKind::Bank, "Bank", "GBP").await.unwrap();
    let ledger = store.create_source("t", SourceKind::Ledger, "Ledger", "GBP").await.unwrap();

    // One matching pair (same amount/date) and one bank-only break.
    let mk = |id: &str, src: &str, eref: &str, amt: i64| CanonicalTransaction {
        id: id.into(), tenant_id: "t".into(), source_id: src.into(), external_ref: eref.into(),
        value_date: "2026-05-10".into(), posted_at: "2026-05-10T00:00:00Z".into(),
        amount_minor: amt, currency: "GBP".into(), direction: Direction::Debit,
        counterparty: None, description: "x".into(),
    };
    store.ingest_transactions("t", &bank.id, &[mk("txn-a1", &bank.id, "A1", 1000), mk("txn-a2", &bank.id, "A2", 9999)]).await.unwrap();
    store.ingest_transactions("t", &ledger.id, &[mk("txn-b1", &ledger.id, "B1", 1000)]).await.unwrap();

    let run = store.create_run("t", "Test run", &bank.id, &ledger.id, "2026-05-01", "2026-05-31").await.unwrap();
    assert_eq!(run.status, recon_domain::RunStatus::Completed);

    // The run is readable back with breaks.
    let detail = store.get_run("t", &run.id).await.unwrap();
    assert_eq!(detail.run.id, run.id);
    assert!(!detail.unmatched.is_empty(), "the bank-only txn should be a break");

    // Foreign tenant cannot create runs against these sources.
    assert!(store.create_run("other", "x", &bank.id, &ledger.id, "2026-05-01", "2026-05-31").await.is_err());
}
```

- [ ] **Step 5: Run the test**

Run: `cd backend && cargo test -p recon-store --test ingest create_run_reconciles_and_persists`
Expected: PASS.

- [ ] **Step 6: Verify the seed still passes (regression guard)**

Run: `cd backend && cargo test -p recon-store`
Expected: all store tests PASS (seed unchanged).

- [ ] **Step 7: Commit**

```bash
git add backend/crates/recon-store/src/runs.rs backend/crates/recon-store/src/lib.rs backend/crates/recon-store/src/seed.rs backend/crates/recon-store/tests/ingest.rs
git commit -m "feat(store): persist_run helper + create_run (load windows, reconcile, persist)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase C — API

### Task C1: `ManageData` permission

**Files:**
- Modify: `backend/crates/recon-auth/src/rbac.rs`

- [ ] **Step 1: Add the permission + a test**

In `backend/crates/recon-auth/src/rbac.rs`, extend the enum and matrix:

```rust
pub enum Permission { ViewRecon, AssignBreak, ProposeResolution, ApproveResolution, ManageUsers, ManageData }
```

In `permitted`, add `ManageData` to the always-allowed arm:

```rust
    match perm {
        ViewRecon | AssignBreak | ProposeResolution | ManageData => true,
        ApproveResolution => matches!(role, Approver | Admin),
        ManageUsers => matches!(role, Admin),
    }
```

Add a test inside `mod tests`:

```rust
    #[test]
    fn manage_data_open_to_all_roles() {
        for r in [Operator, Approver, Admin] {
            assert!(permitted(r, Permission::ManageData));
        }
    }
```

- [ ] **Step 2: Run the tests**

Run: `cd backend && cargo test -p recon-auth rbac`
Expected: PASS (including existing matrix tests).

- [ ] **Step 3: Commit**

```bash
git add backend/crates/recon-auth/src/rbac.rs
git commit -m "feat(auth): add ManageData permission (operational write, all roles)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task C2: `ApiError` structured details + DTOs + multipart feature

**Files:**
- Modify: `backend/crates/recon-api/src/error.rs`
- Modify: `backend/crates/recon-api/src/dto.rs`
- Modify: `backend/crates/recon-api/Cargo.toml` (axum `multipart` feature + `uuid`, `recon-ingest`)

- [ ] **Step 1: Add deps + multipart feature**

In `backend/crates/recon-api/Cargo.toml`, change the `axum` line and add deps under `[dependencies]`:

```toml
axum = { workspace = true, features = ["multipart"] }
recon-ingest = { path = "../recon-ingest" }
uuid = { workspace = true }
```

- [ ] **Step 2: Add `details` to `ApiError`**

In `backend/crates/recon-api/src/error.rs`, add the field and a constructor, update every existing constructor to set `details: None`, and merge details in `into_response`. The struct becomes:

```rust
#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub details: Option<serde_json::Value>,
}
```

Add `details: None` to each existing constructor (`Unauthorized`, `Forbidden`, `NotFound`, `Conflict`, `TooManyRequests`, `BadRequest`, `unauthorized`) and to both `From<StoreError>` arms. Add a new constructor:

```rust
    pub fn with_details(status: StatusCode, code: &'static str, message: impl Into<String>, details: serde_json::Value) -> Self {
        Self { status, code, message: message.into(), details: Some(details) }
    }
```

Map the new store variant in `From<StoreError>`:

```rust
            StoreError::DuplicateRefs(refs) => ApiError {
                status: StatusCode::CONFLICT,
                code: "duplicate",
                message: "duplicate transaction references".into(),
                details: Some(json!({ "refs": refs })),
            },
```

(Add `use serde_json::json;` at the top if not present.)

Update `into_response`:

```rust
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut err = json!({ "code": self.code, "message": self.message });
        if let Some(serde_json::Value::Object(map)) = self.details {
            if let serde_json::Value::Object(target) = &mut err {
                for (k, v) in map {
                    target.insert(k, v);
                }
            }
        }
        (self.status, Json(json!({ "error": err }))).into_response()
    }
}
```

- [ ] **Step 3: Add request DTOs**

Append to `backend/crates/recon-api/src/dto.rs`:

```rust
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSourceReq {
    pub kind: recon_domain::SourceKind,
    pub name: String,
    pub currency: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRunReq {
    pub name: String,
    pub source_a_id: String,
    pub source_b_id: String,
    pub from: String,
    pub to: String,
}
```

- [ ] **Step 4: Build to verify**

Run: `cd backend && cargo build -p recon-api`
Expected: compiles (no routes yet use these — that is the next task).

- [ ] **Step 5: Commit**

```bash
git add backend/crates/recon-api/Cargo.toml backend/crates/recon-api/src/error.rs backend/crates/recon-api/src/dto.rs
git commit -m "feat(api): ApiError structured details + ingest DTOs + multipart feature

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task C3: Source + run routes

**Files:**
- Modify: `backend/crates/recon-api/src/routes.rs`
- Modify: `backend/crates/recon-api/tests/api.rs` (append)

- [ ] **Step 1: Add the routes + handlers**

In `backend/crates/recon-api/src/routes.rs`, add to the imports:

```rust
use axum::extract::{DefaultBodyLimit, Multipart};
use recon_ingest::Parser;
```

Register the routes in `router(...)` (before `.with_state(state)`), and add a 10 MB body limit on the ingest route:

```rust
        .route("/api/sources", get(list_sources).post(create_source))
        .route(
            "/api/sources/:source_id/ingest",
            post(ingest_source).layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        .route("/api/runs", get(list_runs).post(create_run))
```

> Note: change the existing `.route("/api/runs", get(list_runs))` line to the `.get(list_runs).post(create_run)` form shown above (do not add a duplicate route).

Add the handlers at the end of the file:

```rust
fn require_manage_data(ctx: &AuthContext) -> Result<(), ApiError> {
    recon_auth::rbac::require(ctx.role, recon_auth::rbac::Permission::ManageData)
        .map_err(|_| ApiError::Forbidden())
}

async fn list_sources(State(s): State<AppState>, ctx: AuthContext) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;
    Ok(Json(json!(s.store.list_sources(&ctx.tenant_id).await?)))
}

async fn create_source(
    State(s): State<AppState>,
    ctx: AuthContext,
    Json(body): Json<CreateSourceReq>,
) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;
    let src = s
        .store
        .create_source(&ctx.tenant_id, body.kind, &body.name, &body.currency)
        .await?;
    Ok(Json(json!(src)))
}

fn valid_date(s: &str) -> bool {
    time::Date::parse(
        s,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .is_ok()
}

async fn create_run(
    State(s): State<AppState>,
    ctx: AuthContext,
    Json(body): Json<CreateRunReq>,
) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;
    if !valid_date(&body.from) || !valid_date(&body.to) || body.to < body.from {
        return Err(ApiError::BadRequest());
    }
    let run = s
        .store
        .create_run(&ctx.tenant_id, &body.name, &body.source_a_id, &body.source_b_id, &body.from, &body.to)
        .await?;
    Ok(Json(json!(run)))
}

async fn ingest_source(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(source_id): Path<String>,
    mut mp: Multipart,
) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;

    // Source must exist in tenant; also gives us the default currency.
    let source = s.store.get_source(&ctx.tenant_id, &source_id).await?;

    let mut file: Option<Vec<u8>> = None;
    let mut format: Option<String> = None;
    let mut mapping_json: Option<String> = None;
    while let Some(field) = mp.next_field().await.map_err(|_| ApiError::BadRequest())? {
        match field.name() {
            Some("file") => file = Some(field.bytes().await.map_err(|_| ApiError::BadRequest())?.to_vec()),
            Some("format") => format = Some(field.text().await.map_err(|_| ApiError::BadRequest())?),
            Some("mapping") => mapping_json = Some(field.text().await.map_err(|_| ApiError::BadRequest())?),
            _ => {}
        }
    }
    let bytes = file.ok_or_else(ApiError::BadRequest)?;
    let format = format.ok_or_else(ApiError::BadRequest)?;

    let parsed = match format.as_str() {
        "csv" => {
            let raw = mapping_json.ok_or_else(ApiError::BadRequest)?;
            let mapping: recon_ingest::csv::CsvMapping =
                serde_json::from_str(&raw).map_err(|_| ApiError::BadRequest())?;
            recon_ingest::csv::CsvParser::new(mapping).parse(&bytes)
        }
        "camt053" => recon_ingest::camt053::Camt053Parser.parse(&bytes),
        _ => return Err(ApiError::BadRequest()),
    };

    let parsed = match parsed {
        Ok(p) => p,
        Err(rows) => {
            let rows: Vec<Value> = rows
                .iter()
                .map(|e| json!({ "row": e.row, "field": e.field, "message": e.message }))
                .collect();
            return Err(ApiError::with_details(
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                "parse",
                "file contains invalid rows",
                json!({ "rows": rows }),
            ));
        }
    };

    // Map ParsedTxn -> CanonicalTransaction (assign ids + defaults).
    let txns: Vec<recon_domain::CanonicalTransaction> = parsed
        .into_iter()
        .map(|p| recon_domain::CanonicalTransaction {
            id: format!("txn-{}", uuid::Uuid::new_v4()),
            tenant_id: ctx.tenant_id.clone(),
            source_id: source_id.clone(),
            external_ref: p.external_ref,
            value_date: p.value_date.clone(),
            posted_at: p.posted_at.unwrap_or_else(|| format!("{}T00:00:00Z", p.value_date)),
            amount_minor: p.amount_minor,
            currency: p.currency.unwrap_or_else(|| source.currency.clone()),
            direction: p.direction,
            counterparty: p.counterparty,
            description: p.description,
        })
        .collect();

    let n = s.store.ingest_transactions(&ctx.tenant_id, &source_id, &txns).await?;
    Ok(Json(json!({ "ingested": n, "sourceId": source_id })))
}
```

- [ ] **Step 2: Run the existing API tests (regression)**

Run: `cd backend && cargo test -p recon-api --test api`
Expected: existing tests still PASS (routes added, none removed).

- [ ] **Step 3: Commit**

```bash
git add backend/crates/recon-api/src/routes.rs
git commit -m "feat(api): source create/list, multipart ingest, create-run routes

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task C4: API integration tests for the full pipeline

**Files:**
- Create: `backend/crates/recon-api/tests/ingest_api.rs`

- [ ] **Step 1: Write the integration test**

Look at the top of `backend/crates/recon-api/tests/api.rs` to copy its helper style (it uses `recon_api::test_app(pool)`, `tower::ServiceExt::oneshot`, `http_body_util::BodyExt`, and mints a token via `recon_auth::token::encode_access` with `AuthConfig::test()`). Mirror those helpers here.

`backend/crates/recon-api/tests/ingest_api.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

fn token(cfg: &recon_api::state::AuthConfig, tenant: &str) -> String {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    recon_auth::token::encode_access(
        &cfg.jwt_secret,
        "user-ada",
        tenant,
        recon_domain::UserRole::Admin,
        cfg.access_ttl_secs,
        now,
    )
    .unwrap()
}

async fn json(app: &axum::Router, req: Request<Body>) -> (StatusCode, Value) {
    let res = app.clone().oneshot(req).await.unwrap();
    let st = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (st, v)
}

fn multipart_body(boundary: &str, parts: &[(&str, Option<&str>, &str)]) -> Vec<u8> {
    // parts: (name, filename, value)
    let mut body = Vec::new();
    for (name, filename, value) in parts {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(fname) => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{fname}\"\r\n\r\n").as_bytes(),
            ),
            None => body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
            ),
        }
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

#[sqlx::test(migrations = "../../migrations")]
async fn full_ingest_pipeline(pool: sqlx::PgPool) {
    // Seed a tenant + the admin user so the token's tenant exists.
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-acme','admin')").execute(&pool).await.unwrap();

    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-acme"));

    // 1. Create two sources.
    let mk_source = |name: &str, kind: &str| {
        Request::builder().method("POST").uri("/api/sources").header("authorization", &auth)
            .header("content-type", "application/json")
            .body(Body::from(format!("{{\"kind\":\"{kind}\",\"name\":\"{name}\",\"currency\":\"GBP\"}}"))).unwrap()
    };
    let (st, bank) = json(&app, mk_source("Bank", "bank")).await;
    assert_eq!(st, StatusCode::OK, "create bank source");
    let bank_id = bank["id"].as_str().unwrap().to_string();
    let (_st, ledger) = json(&app, mk_source("Ledger", "ledger")).await;
    let ledger_id = ledger["id"].as_str().unwrap().to_string();

    // 2. Ingest a CSV into the bank source.
    let boundary = "BOUNDARY";
    let mapping = r#"{"hasHeader":true,"delimiter":44,"externalRef":{"header":"ref"},"valueDate":{"header":"date"},"dateFormat":"%Y-%m-%d","amount":{"signed":{"column":{"header":"amount"},"debitWhenNegative":true}},"description":{"header":"desc"}}"#;
    let csv = "ref,date,amount,desc\nA1,2026-05-10,-10.00,Coffee\nA2,2026-05-11,-99.99,Lunch\n";
    let body = multipart_body(boundary, &[
        ("file", Some("bank.csv"), csv),
        ("format", None, "csv"),
        ("mapping", None, mapping),
    ]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{bank_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "ingest csv: {v}");
    assert_eq!(v["ingested"], 2);

    // 3. Re-uploading the same CSV is a 409 duplicate.
    let body = multipart_body(boundary, &[
        ("file", Some("bank.csv"), csv),
        ("format", None, "csv"),
        ("mapping", None, mapping),
    ]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{bank_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::CONFLICT, "duplicate ingest");
    assert_eq!(v["error"]["code"], "duplicate");
    assert!(v["error"]["refs"].as_array().unwrap().contains(&Value::from("A1")));

    // 4. A bad CSV row -> 422 with the parse report.
    let bad = "ref,date,amount,desc\nB1,not-a-date,-1.00,Bad\n";
    let body = multipart_body(boundary, &[
        ("file", Some("bad.csv"), bad),
        ("format", None, "csv"),
        ("mapping", None, mapping),
    ]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{ledger_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::UNPROCESSABLE_ENTITY, "bad row");
    assert_eq!(v["error"]["code"], "parse");
    assert_eq!(v["error"]["rows"][0]["field"], "valueDate");

    // 5. Ingest a CAMT.053 into the ledger source.
    let camt = r#"<Document><Stmt><Ntry><Amt Ccy="GBP">10.00</Amt><CdtDbtInd>DBIT</CdtDbtInd><ValDt><Dt>2026-05-10</Dt></ValDt><NtryRef>A1</NtryRef><AddtlNtryInf>Coffee</AddtlNtryInf></Ntry></Stmt></Document>"#;
    let body = multipart_body(boundary, &[
        ("file", Some("ledger.xml"), camt),
        ("format", None, "camt053"),
    ]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{ledger_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "ingest camt: {v}");
    assert_eq!(v["ingested"], 1);

    // 6. Create a run over the window and read it back.
    let req = Request::builder().method("POST").uri("/api/runs").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(format!("{{\"name\":\"R\",\"sourceAId\":\"{bank_id}\",\"sourceBId\":\"{ledger_id}\",\"from\":\"2026-05-01\",\"to\":\"2026-05-31\"}}"))).unwrap();
    let (st, run) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "create run: {run}");
    let run_id = run["id"].as_str().unwrap();

    let req = Request::builder().method("GET").uri(format!("/api/runs/{run_id}")).header("authorization", &auth).body(Body::empty()).unwrap();
    let (st, detail) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(detail["run"]["id"], run_id);

    // 7. Invalid date range -> 400.
    let req = Request::builder().method("POST").uri("/api/runs").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(format!("{{\"name\":\"R\",\"sourceAId\":\"{bank_id}\",\"sourceBId\":\"{ledger_id}\",\"from\":\"2026-05-31\",\"to\":\"2026-05-01\"}}"))).unwrap();
    let (st, _) = json(&app, req).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "../../migrations")]
async fn cross_tenant_ingest_is_not_found(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme'),('tenant-globex','Globex','globex')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ('s-acme','tenant-acme','bank','Bank','GBP')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-globex','admin')").execute(&pool).await.unwrap();

    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-globex"));
    let boundary = "B";
    let body = multipart_body(boundary, &[("file", Some("x.xml"), "<Document></Document>"), ("format", None, "camt053")]);
    let req = Request::builder().method("POST").uri("/api/sources/s-acme/ingest")
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, _) = json(&app, req).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run the tests**

Run: `cd backend && cargo test -p recon-api --test ingest_api`
Expected: both tests PASS.

- [ ] **Step 3: Full backend suite + clippy**

Run: `cd backend && cargo test && cargo clippy --workspace -- -D warnings`
Expected: all tests PASS; no clippy warnings.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/recon-api/tests/ingest_api.rs
git commit -m "test(api): full ingest pipeline + cross-tenant isolation

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase D — Frontend

### Task D1: Domain type + `ApiClient` interface additions

**Files:**
- Modify: `web/lib/domain/types.ts` (no change needed if `Source` already exists; confirm and add nothing)
- Modify: `web/lib/api/client.ts`

- [ ] **Step 1: Extend the client interface + types**

In `web/lib/api/client.ts`, add the imports `ReconciliationRun, Source, SourceKind` to the existing `@/lib/domain/types` import if not present, then append the new types from the **Shared type contract** block above (`SourceListItem`, `CreateSourceInput`, `IngestFormat`, `IngestResult`, `CreateRunInput`, `ColRef`, `AmountMapping`, `CsvMapping`, `IngestError`) and add the four methods to the `ApiClient` interface:

```ts
  listSources(tenantId: string): Promise<SourceListItem[]>;
  createSource(tenantId: string, input: CreateSourceInput): Promise<Source>;
  ingestFile(tenantId: string, sourceId: string, format: IngestFormat, file: File, mapping?: CsvMapping): Promise<IngestResult>;
  createRun(tenantId: string, input: CreateRunInput): Promise<ReconciliationRun>;
```

- [ ] **Step 2: Typecheck (expect failures in http.ts and mock.ts)**

Run: `pnpm -C web tsc --noEmit`
Expected: FAIL — `HttpApiClient` and `MockApiClient` no longer satisfy `ApiClient` (methods missing). That is the signal for D2/D3.

- [ ] **Step 3: Commit**

```bash
git add web/lib/api/client.ts
git commit -m "feat(web): ApiClient ingestion types + method signatures

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task D2: `HttpApiClient` implementation

**Files:**
- Modify: `web/lib/api/http.ts`

- [ ] **Step 1: Implement the four methods**

In `web/lib/api/http.ts`, import the new types and `IngestError` from `./client` and `ReconciliationRun, Source` from domain types, then add the methods to the class:

```ts
  listSources(tenantId: string): Promise<SourceListItem[]> { return this.req("/api/sources", tenantId); }
  createSource(tenantId: string, input: CreateSourceInput): Promise<Source> {
    return this.req("/api/sources", tenantId, { method: "POST", body: JSON.stringify(input) });
  }
  createRun(tenantId: string, input: CreateRunInput): Promise<ReconciliationRun> {
    return this.req("/api/runs", tenantId, { method: "POST", body: JSON.stringify(input) });
  }

  async ingestFile(_tenantId: string, sourceId: string, format: IngestFormat, file: File, mapping?: CsvMapping): Promise<IngestResult> {
    const send = async (token: string | null): Promise<Response> => {
      const fd = new FormData();
      fd.append("file", file);
      fd.append("format", format);
      if (mapping) fd.append("mapping", JSON.stringify(mapping));
      const headers: Record<string, string> = {};
      if (token) headers["Authorization"] = `Bearer ${token}`;
      // NOTE: do not set Content-Type — the browser sets the multipart boundary.
      return fetch(`${this.baseUrl}/api/sources/${sourceId}/ingest`, { method: "POST", headers, body: fd });
    };

    let res = await send(getAccessToken());
    if (res.status === 401) {
      const newToken = await runRefresh();
      if (!newToken) throw new Error("API 401: unauthorized");
      res = await send(newToken);
    }
    if (res.ok) return res.json() as Promise<IngestResult>;

    // Structured ingest errors (422 parse / 409 duplicate).
    let body: { error?: { code?: string; message?: string; rows?: { row: number; field: string; message: string }[]; refs?: string[] } } = {};
    try { body = await res.json(); } catch { /* ignore */ }
    const err = body.error;
    if (err?.code === "parse") throw new IngestError("parse", err.message ?? "parse error", err.rows);
    if (err?.code === "duplicate") throw new IngestError("duplicate", err.message ?? "duplicate", undefined, err.refs);
    throw new Error(`API ${res.status}: ${err?.code ?? err?.message ?? res.status}`);
  }
```

Add the new type imports to the existing import statements at the top of the file:

```ts
import type { ApiClient, BreakQuery, CreateUserInput, DashboardSummary, MatchSuggestion, NewCaseEvent, RunDetail, RunQuery, UpdateUserPatch, SourceListItem, CreateSourceInput, IngestFormat, IngestResult, CreateRunInput, CsvMapping } from "./client";
import { IngestError } from "./client";
import type { Break, Case, CanonicalTransaction, ReconciliationRun, Source, Tenant, User } from "@/lib/domain/types";
```

- [ ] **Step 2: Write a unit test mirroring `http.test.ts` style**

Append to `web/lib/api/http.test.ts` a test that mocks `fetch` and asserts `ingestFile` throws an `IngestError` with `code: "duplicate"` and `refs` on a 409:

```ts
import { IngestError } from "./client";

it("ingestFile throws IngestError on 409 duplicate", async () => {
  const c = new HttpApiClient("http://api.test");
  const fetchMock = vi.fn().mockResolvedValue(
    new Response(JSON.stringify({ error: { code: "duplicate", message: "dup", refs: ["A1"] } }), { status: 409 })
  );
  vi.stubGlobal("fetch", fetchMock);
  const file = new File(["x"], "f.csv", { type: "text/csv" });
  await expect(c.ingestFile("t", "s", "csv", file, undefined)).rejects.toMatchObject({ code: "duplicate", refs: ["A1"] });
  expect(IngestError).toBeDefined();
});
```

(Check the top of `web/lib/api/http.test.ts` for how it imports `HttpApiClient`, `vi`, and stubs `fetch`; mirror that exactly. If the file uses a `beforeEach`/`afterEach` to reset stubs, your test will inherit it.)

- [ ] **Step 3: Run the test + typecheck**

Run: `pnpm -C web test -- http.test` then `pnpm -C web tsc --noEmit`
Expected: the new test PASSES; typecheck still fails only on `mock.ts` (fixed next).

- [ ] **Step 4: Commit**

```bash
git add web/lib/api/http.ts web/lib/api/http.test.ts
git commit -m "feat(web): HttpApiClient ingestion methods (FormData + structured errors)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task D3: `MockApiClient` implementation

**Files:**
- Modify: `web/lib/api/mock.ts`
- Modify: `web/lib/api/mock.test.ts` (append)

- [ ] **Step 1: Implement the four methods on the mock**

In `web/lib/api/mock.ts`, add the new type imports and implement the methods using the in-memory `this.state`. Sources live in `this.state.sources` (confirm the field name by reading `lib/api/fixtures.ts`'s `Fixtures` type; it has `sources`). Implementation:

```ts
  async listSources(_tenantId: string): Promise<SourceListItem[]> {
    await this.delay();
    return this.state.sources.map((s) => ({
      ...s,
      txnCount: this.state.transactions.filter((t) => t.sourceId === s.id).length,
    }));
  }

  async createSource(_tenantId: string, input: CreateSourceInput): Promise<Source> {
    await this.delay();
    const src: Source = { id: `src-${nextId()}`, tenantId: this.state.tenants[0]?.id ?? "tenant-acme", kind: input.kind, name: input.name, currency: input.currency };
    this.state.sources.push(deepClone(src));
    return src;
  }

  async ingestFile(_tenantId: string, sourceId: string, _format: IngestFormat, _file: File, _mapping?: CsvMapping): Promise<IngestResult> {
    await this.delay();
    // The mock does not parse real bytes; it records a deterministic ingest so
    // UI flows (success summary) can be tested. One synthetic txn per call.
    const ref = `MOCK-${nextId()}`;
    this.state.transactions.push(deepClone({
      id: `txn-${nextId()}`, tenantId: this.state.tenants[0]?.id ?? "tenant-acme", sourceId,
      externalRef: ref, valueDate: "2026-05-10", postedAt: "2026-05-10T00:00:00Z",
      amountMinor: 1000, currency: "GBP", direction: "debit", description: "Mock ingest",
    }) as CanonicalTransaction);
    return { ingested: 1, sourceId };
  }

  async createRun(_tenantId: string, input: CreateRunInput): Promise<ReconciliationRun> {
    await this.delay();
    const run: ReconciliationRun = {
      id: `run-${nextId()}`, tenantId: this.state.tenants[0]?.id ?? "tenant-acme", name: input.name,
      sourceAId: input.sourceAId, sourceBId: input.sourceBId, status: "completed",
      startedAt: "2026-05-25T00:00:00Z", completedAt: "2026-05-25T00:00:00Z", configVersion: "v1.0",
      stats: { matched: 0, unmatched: 0, partial: 0, duplicate: 0, breakCount: 0, matchRatePct: 0, valueAtRiskMinor: 0 },
    };
    this.state.runs.push(deepClone(run));
    return run;
  }
```

Add to the imports at the top of `mock.ts`:

```ts
import type { ReconciliationRun, Source } from "@/lib/domain/types";
import type { SourceListItem, CreateSourceInput, IngestFormat, IngestResult, CreateRunInput, CsvMapping } from "./client";
```

> Confirm `ReconciliationRun.stats` field names against `web/lib/domain/types.ts` (camelCase) before writing — adjust if the fixture uses different keys.

- [ ] **Step 2: Append mock tests**

Append to `web/lib/api/mock.test.ts`:

```ts
it("createSource then listSources includes it with a txn count", async () => {
  const c = new MockApiClient({ latencyMs: 0 });
  const before = (await c.listSources("tenant-acme")).length;
  const src = await c.createSource("tenant-acme", { kind: "bank", name: "New Bank", currency: "GBP" });
  const after = await c.listSources("tenant-acme");
  expect(after.length).toBe(before + 1);
  expect(after.find((s) => s.id === src.id)?.txnCount).toBe(0);
});

it("ingestFile records a transaction and returns a count", async () => {
  const c = new MockApiClient({ latencyMs: 0 });
  const src = await c.createSource("tenant-acme", { kind: "bank", name: "B", currency: "GBP" });
  const res = await c.ingestFile("tenant-acme", src.id, "csv", new File(["x"], "f.csv"), undefined);
  expect(res.ingested).toBe(1);
  expect((await c.listSources("tenant-acme")).find((s) => s.id === src.id)?.txnCount).toBe(1);
});

it("createRun appends a run", async () => {
  const c = new MockApiClient({ latencyMs: 0 });
  const run = await c.createRun("tenant-acme", { name: "R", sourceAId: "a", sourceBId: "b", from: "2026-05-01", to: "2026-05-31" });
  expect(run.id).toMatch(/^run-/);
});
```

- [ ] **Step 3: Run tests + typecheck**

Run: `pnpm -C web test -- mock.test` then `pnpm -C web tsc --noEmit`
Expected: mock tests PASS; typecheck now CLEAN (both clients satisfy `ApiClient`).

- [ ] **Step 4: Commit**

```bash
git add web/lib/api/mock.ts web/lib/api/mock.test.ts
git commit -m "feat(web): MockApiClient ingestion methods + tests

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task D4: `useSources` hook + Sources page (list + New source dialog)

**Files:**
- Create: `web/lib/hooks/use-sources.ts`
- Create: `web/app/(app)/sources/page.tsx`

- [ ] **Step 1: Write the hook**

`web/lib/hooks/use-sources.ts`:

```ts
import { useQuery } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";

export function useSources() {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["sources", tenantId],
    queryFn: () => api.listSources(tenantId),
  });
}
```

- [ ] **Step 2: Write the Sources page**

`web/app/(app)/sources/page.tsx` — a client page modeled on `web/app/(app)/users/page.tsx` (table + a dialog driven by react-hook-form + zod + a TanStack mutation + sonner toasts). It lists sources (Name, Kind, Currency, Transactions, an Upload action button per row) and has a "New source" dialog (`name`, `kind` select, `currency`). The Upload button sets the selected source into state and opens the upload dialog from Task D5 (import `UploadDialog`). Gate with `useAuth()` — all roles have `ManageData`, so no redirect is needed, but read `useAuth()` to stay consistent. Use `useSources()` for data and invalidate `["sources", tenantId]` after create/upload.

Key wiring (the file is ~150 lines; build it following the users-page pattern):

```tsx
"use client";
import { useState } from "react";
import { useForm } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { PlusCircle, Upload as UploadIcon, Database } from "lucide-react";
import { PageHeader } from "@/components/app/page-header";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { Skeleton } from "@/components/ui/skeleton";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import { useSources } from "@/lib/hooks/use-sources";
import { UploadDialog } from "@/components/app/upload-dialog";
import type { SourceListItem } from "@/lib/api/client";
import type { SourceKind } from "@/lib/domain/types";

const KIND_OPTIONS: { value: SourceKind; label: string }[] = [
  { value: "bank", label: "Bank" },
  { value: "ledger", label: "Ledger" },
  { value: "cross_system", label: "Cross-system" },
];
const schema = z.object({
  name: z.string().min(1, "Name is required"),
  kind: z.enum(["bank", "ledger", "cross_system"]),
  currency: z.string().min(3, "3-letter currency").max(3),
});
type FormValues = z.infer<typeof schema>;

export default function SourcesPage() {
  const api = useApi();
  const { tenantId } = useTenant();
  const queryClient = useQueryClient();
  const { data: sources, isLoading } = useSources();
  const [showNew, setShowNew] = useState(false);
  const [uploadTarget, setUploadTarget] = useState<SourceListItem | null>(null);

  const createMutation = useMutation({
    mutationFn: (input: FormValues) => api.createSource(tenantId, input),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["sources", tenantId] });
      toast.success("Source created.");
      setShowNew(false);
      reset();
    },
    onError: () => toast.error("Failed to create source."),
  });

  const { register, handleSubmit, reset, setValue, watch, formState: { errors } } =
    useForm<FormValues>({ resolver: zodResolver(schema), defaultValues: { kind: "bank", currency: "GBP" } });
  const kind = watch("kind");

  return (
    <>
      <div className="flex flex-col gap-6">
        <div className="flex items-center justify-between">
          <PageHeader title="Sources" description="Manage data sources and ingest bank/ledger files." />
          <Button onClick={() => setShowNew(true)} className="gap-2"><PlusCircle className="size-4" />New source</Button>
        </div>
        {isLoading ? (
          <div className="flex flex-col gap-2">{Array.from({ length: 3 }).map((_, i) => <Skeleton key={i} className="h-12 w-full" />)}</div>
        ) : (
          <div className="rounded-lg border border-border overflow-hidden">
            <Table>
              <TableHeader><TableRow>
                <TableHead>Name</TableHead><TableHead>Kind</TableHead><TableHead>Currency</TableHead>
                <TableHead className="text-right">Transactions</TableHead><TableHead className="text-right">Actions</TableHead>
              </TableRow></TableHeader>
              <TableBody>
                {sources?.map((s) => (
                  <TableRow key={s.id}>
                    <TableCell className="font-medium">{s.name}</TableCell>
                    <TableCell className="capitalize text-muted-foreground">{s.kind.replace("_", " ")}</TableCell>
                    <TableCell>{s.currency}</TableCell>
                    <TableCell className="text-right tabular-nums">{s.txnCount}</TableCell>
                    <TableCell className="text-right">
                      <Button variant="outline" size="sm" className="gap-1.5" onClick={() => setUploadTarget(s)}>
                        <UploadIcon className="size-3.5" />Upload
                      </Button>
                    </TableCell>
                  </TableRow>
                ))}
                {sources?.length === 0 && (
                  <TableRow><TableCell colSpan={5} className="text-center text-muted-foreground py-8">
                    <Database className="size-5 mx-auto mb-2 opacity-50" />No sources yet. Create one to start ingesting.
                  </TableCell></TableRow>
                )}
              </TableBody>
            </Table>
          </div>
        )}
      </div>

      <Dialog open={showNew} onOpenChange={setShowNew}>
        <DialogContent>
          <DialogHeader><DialogTitle>New source</DialogTitle></DialogHeader>
          <form onSubmit={handleSubmit((v) => createMutation.mutate(v))} className="flex flex-col gap-4" noValidate>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="src-name">Name</Label>
              <Input id="src-name" {...register("name")} aria-invalid={!!errors.name} />
              {errors.name && <p className="text-xs text-destructive" role="alert">{errors.name.message}</p>}
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="src-kind">Kind</Label>
              <Select value={kind} onValueChange={(v) => setValue("kind", v as SourceKind)}>
                <SelectTrigger id="src-kind"><SelectValue /></SelectTrigger>
                <SelectContent>{KIND_OPTIONS.map((o) => <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>)}</SelectContent>
              </Select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="src-ccy">Currency</Label>
              <Input id="src-ccy" {...register("currency")} aria-invalid={!!errors.currency} />
              {errors.currency && <p className="text-xs text-destructive" role="alert">{errors.currency.message}</p>}
            </div>
            <DialogFooter><Button type="submit" disabled={createMutation.isPending}>{createMutation.isPending ? "Creating…" : "Create source"}</Button></DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      {uploadTarget && (
        <UploadDialog source={uploadTarget} open={!!uploadTarget} onOpenChange={(o) => !o && setUploadTarget(null)} />
      )}
    </>
  );
}
```

- [ ] **Step 3: Typecheck (UploadDialog missing — expected)**

Run: `pnpm -C web tsc --noEmit`
Expected: FAIL only on the missing `@/components/app/upload-dialog` import (created in D5).

- [ ] **Step 4: Commit**

```bash
git add web/lib/hooks/use-sources.ts "web/app/(app)/sources/page.tsx"
git commit -m "feat(web): Sources page (list + new-source dialog) and useSources hook

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task D5: Upload dialog (format + CSV mapping + result/error report)

**Files:**
- Create: `web/components/app/upload-dialog.tsx`
- Create: `web/tests/upload-dialog.test.tsx`

- [ ] **Step 1: Build the dialog**

`web/components/app/upload-dialog.tsx` — a client component. Props: `{ source: SourceListItem; open: boolean; onOpenChange: (o: boolean) => void }`. State: `format` ("csv"|"camt053"), a `file`, and for CSV the mapping fields (hasHeader, delimiter, and column **index** numbers for externalRef/valueDate/description, an amount-encoding select with index inputs, a `dateFormat` text input, optional currency/counterparty indices). On submit, build a `CsvMapping` (using `{ index }` ColRefs) and call `api.ingestFile`. Render the result: success summary or, on `IngestError`, a scrollable list of `rows` (parse) or `refs` (duplicate). Use a TanStack mutation and invalidate `["sources", tenantId]` on success.

```tsx
"use client";
import { useState } from "react";
import { toast } from "sonner";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import { IngestError } from "@/lib/api/client";
import type { SourceListItem, IngestFormat, CsvMapping, AmountMapping } from "@/lib/api/client";

export function UploadDialog({ source, open, onOpenChange }: { source: SourceListItem; open: boolean; onOpenChange: (o: boolean) => void }) {
  const api = useApi();
  const { tenantId } = useTenant();
  const queryClient = useQueryClient();
  const [format, setFormat] = useState<IngestFormat>("csv");
  const [file, setFile] = useState<File | null>(null);
  const [report, setReport] = useState<{ kind: "parse" | "duplicate"; rows?: { row: number; field: string; message: string }[]; refs?: string[] } | null>(null);

  // CSV mapping fields (indices, 0-based)
  const [hasHeader, setHasHeader] = useState(true);
  const [delimiter, setDelimiter] = useState(44);
  const [dateFormat, setDateFormat] = useState("%Y-%m-%d");
  const [refCol, setRefCol] = useState(0);
  const [dateCol, setDateCol] = useState(1);
  const [descCol, setDescCol] = useState(4);
  const [amountKind, setAmountKind] = useState<"signed" | "debitCredit">("signed");
  const [amountCol, setAmountCol] = useState(2);
  const [debitWhenNegative, setDebitWhenNegative] = useState(true);
  const [debitCol, setDebitCol] = useState(2);
  const [creditCol, setCreditCol] = useState(3);

  const buildMapping = (): CsvMapping => {
    const amount: AmountMapping = amountKind === "signed"
      ? { signed: { column: { index: amountCol }, debitWhenNegative } }
      : { debitCredit: { debit: { index: debitCol }, credit: { index: creditCol } } };
    return { hasHeader, delimiter, externalRef: { index: refCol }, valueDate: { index: dateCol }, dateFormat, amount, description: { index: descCol } };
  };

  const mutation = useMutation({
    mutationFn: () => {
      if (!file) throw new Error("No file selected");
      return api.ingestFile(tenantId, source.id, format, file, format === "csv" ? buildMapping() : undefined);
    },
    onSuccess: (res) => {
      setReport(null);
      void queryClient.invalidateQueries({ queryKey: ["sources", tenantId] });
      toast.success(`${res.ingested} transaction${res.ingested === 1 ? "" : "s"} ingested.`);
      onOpenChange(false);
    },
    onError: (e) => {
      if (e instanceof IngestError) setReport({ kind: e.code, rows: e.rows, refs: e.refs });
      else toast.error("Ingestion failed.");
    },
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>Upload to {source.name}</DialogTitle></DialogHeader>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="up-format">Format</Label>
            <Select value={format} onValueChange={(v) => setFormat(v as IngestFormat)}>
              <SelectTrigger id="up-format"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="csv">CSV</SelectItem>
                <SelectItem value="camt053">CAMT.053 (XML)</SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="up-file">File</Label>
            <Input id="up-file" type="file" onChange={(e) => setFile(e.target.files?.[0] ?? null)} accept={format === "csv" ? ".csv,text/csv" : ".xml,text/xml,application/xml"} />
          </div>

          {format === "csv" && (
            <div className="flex flex-col gap-3 rounded-md border border-border p-3">
              <p className="text-xs text-muted-foreground">Map CSV columns (0-based index).</p>
              <label className="flex items-center gap-2 text-sm"><Checkbox checked={hasHeader} onCheckedChange={(c) => setHasHeader(!!c)} />Has header row</label>
              <div className="grid grid-cols-2 gap-3">
                <NumberField label="Reference col" value={refCol} onChange={setRefCol} id="m-ref" />
                <NumberField label="Date col" value={dateCol} onChange={setDateCol} id="m-date" />
                <NumberField label="Description col" value={descCol} onChange={setDescCol} id="m-desc" />
                <div className="flex flex-col gap-1.5">
                  <Label htmlFor="m-delim">Delimiter</Label>
                  <Select value={String(delimiter)} onValueChange={(v) => setDelimiter(Number(v))}>
                    <SelectTrigger id="m-delim"><SelectValue /></SelectTrigger>
                    <SelectContent>
                      <SelectItem value="44">Comma</SelectItem>
                      <SelectItem value="59">Semicolon</SelectItem>
                      <SelectItem value="9">Tab</SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="m-dfmt">Date format</Label>
                <Input id="m-dfmt" value={dateFormat} onChange={(e) => setDateFormat(e.target.value)} placeholder="%Y-%m-%d" />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="m-amtkind">Amount encoding</Label>
                <Select value={amountKind} onValueChange={(v) => setAmountKind(v as "signed" | "debitCredit")}>
                  <SelectTrigger id="m-amtkind"><SelectValue /></SelectTrigger>
                  <SelectContent>
                    <SelectItem value="signed">Single signed column</SelectItem>
                    <SelectItem value="debitCredit">Separate debit/credit columns</SelectItem>
                  </SelectContent>
                </Select>
              </div>
              {amountKind === "signed" ? (
                <div className="grid grid-cols-2 gap-3">
                  <NumberField label="Amount col" value={amountCol} onChange={setAmountCol} id="m-amt" />
                  <label className="flex items-center gap-2 text-sm mt-6"><Checkbox checked={debitWhenNegative} onCheckedChange={(c) => setDebitWhenNegative(!!c)} />Negative = debit</label>
                </div>
              ) : (
                <div className="grid grid-cols-2 gap-3">
                  <NumberField label="Debit col" value={debitCol} onChange={setDebitCol} id="m-debit" />
                  <NumberField label="Credit col" value={creditCol} onChange={setCreditCol} id="m-credit" />
                </div>
              )}
            </div>
          )}

          {report && (
            <div role="alert" className="rounded-md border border-danger/30 bg-danger/5 p-3 text-sm text-danger max-h-40 overflow-auto">
              {report.kind === "parse" ? (
                <>
                  <p className="font-medium mb-1">File rejected — fix these rows:</p>
                  <ul className="list-disc pl-4">{report.rows?.map((r, i) => <li key={i}>Row {r.row}: {r.field} — {r.message}</li>)}</ul>
                </>
              ) : (
                <>
                  <p className="font-medium mb-1">Duplicate references already loaded:</p>
                  <p>{report.refs?.join(", ")}</p>
                </>
              )}
            </div>
          )}
        </div>
        <DialogFooter>
          <Button onClick={() => mutation.mutate()} disabled={!file || mutation.isPending}>{mutation.isPending ? "Uploading…" : "Upload"}</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function NumberField({ label, value, onChange, id }: { label: string; value: number; onChange: (n: number) => void; id: string }) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={id}>{label}</Label>
      <Input id={id} type="number" min={0} value={value} onChange={(e) => onChange(Number(e.target.value))} />
    </div>
  );
}
```

- [ ] **Step 2: Write a component test**

`web/tests/upload-dialog.test.tsx` — render the dialog with a `MockApiClient`-backed provider, select a file, click Upload, and assert the success toast / dialog close path. For the error path, use a stub client whose `ingestFile` rejects with an `IngestError("parse", ..., rows)` and assert the row report renders. Check `web/tests/*.test.tsx` (e.g. `approval-bar.test.tsx`) for the exact provider-wrapping pattern (`ApiProvider`/`QueryClientProvider`/`TenantProvider`) and reuse it. Minimum assertions:

```tsx
// after rendering with a client that throws IngestError("parse", "bad", [{row:4,field:"valueDate",message:"unparseable"}])
// and clicking Upload with a file selected:
expect(await screen.findByText(/fix these rows/i)).toBeInTheDocument();
expect(screen.getByText(/Row 4: valueDate/)).toBeInTheDocument();
```

- [ ] **Step 3: Run the test + typecheck + lint**

Run: `pnpm -C web test -- upload-dialog` then `pnpm -C web tsc --noEmit` then `pnpm -C web lint`
Expected: test PASSES; typecheck CLEAN; lint CLEAN.

- [ ] **Step 4: Commit**

```bash
git add web/components/app/upload-dialog.tsx web/tests/upload-dialog.test.tsx
git commit -m "feat(web): upload dialog with CSV mapping form and error report

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task D6: New-run dialog on the Runs page

**Files:**
- Modify: `web/app/(app)/runs/page.tsx`
- Create: `web/components/app/new-run-dialog.tsx`

- [ ] **Step 1: Build the New-run dialog**

`web/components/app/new-run-dialog.tsx` — props `{ open, onOpenChange }`. Uses `useSources()` to populate two source `Select`s (A and B), a name input, and two date inputs (`from`, `to`, `type="date"`). On submit, call `api.createRun(tenantId, { name, sourceAId, sourceBId, from, to })`, invalidate `["runs", tenantId]`, toast, close, and `router.push('/runs/' + run.id)`. Disable submit unless both sources chosen (and distinct) and dates set. Mirror the dialog/mutation pattern from the users page.

```tsx
"use client";
import { useState } from "react";
import { useRouter } from "next/navigation";
import { toast } from "sonner";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import { useSources } from "@/lib/hooks/use-sources";

export function NewRunDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const api = useApi();
  const router = useRouter();
  const { tenantId } = useTenant();
  const queryClient = useQueryClient();
  const { data: sources } = useSources();
  const [name, setName] = useState("");
  const [a, setA] = useState("");
  const [b, setB] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");

  const mutation = useMutation({
    mutationFn: () => api.createRun(tenantId, { name, sourceAId: a, sourceBId: b, from, to }),
    onSuccess: (run) => {
      void queryClient.invalidateQueries({ queryKey: ["runs", tenantId] });
      toast.success("Run created.");
      onOpenChange(false);
      router.push(`/runs/${run.id}`);
    },
    onError: () => toast.error("Failed to create run."),
  });

  const valid = name && a && b && a !== b && from && to && from <= to;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader><DialogTitle>New reconciliation run</DialogTitle></DialogHeader>
        <div className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5"><Label htmlFor="run-name">Name</Label><Input id="run-name" value={name} onChange={(e) => setName(e.target.value)} /></div>
          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="run-a">Source A</Label>
              <Select value={a} onValueChange={setA}><SelectTrigger id="run-a"><SelectValue placeholder="Select" /></SelectTrigger>
                <SelectContent>{sources?.map((s) => <SelectItem key={s.id} value={s.id}>{s.name}</SelectItem>)}</SelectContent></Select>
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="run-b">Source B</Label>
              <Select value={b} onValueChange={setB}><SelectTrigger id="run-b"><SelectValue placeholder="Select" /></SelectTrigger>
                <SelectContent>{sources?.map((s) => <SelectItem key={s.id} value={s.id}>{s.name}</SelectItem>)}</SelectContent></Select>
            </div>
            <div className="flex flex-col gap-1.5"><Label htmlFor="run-from">From</Label><Input id="run-from" type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></div>
            <div className="flex flex-col gap-1.5"><Label htmlFor="run-to">To</Label><Input id="run-to" type="date" value={to} onChange={(e) => setTo(e.target.value)} /></div>
          </div>
        </div>
        <DialogFooter><Button onClick={() => mutation.mutate()} disabled={!valid || mutation.isPending}>{mutation.isPending ? "Running…" : "Create run"}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 2: Wire a "New run" button into the Runs page**

In `web/app/(app)/runs/page.tsx`, inside `RunsPageInner`, add `const [showNewRun, setShowNewRun] = useState(false);` (add `useState` to the React import), import `NewRunDialog` and `Button` (Button is already imported). Place a "New run" button in the `PageHeader` row by wrapping the header in a flex container:

```tsx
import { NewRunDialog } from "@/components/app/new-run-dialog";
// ...
      <div className="flex items-center justify-between">
        <PageHeader title="Reconciliation runs" description="Browse and inspect reconciliation run history." />
        <Button onClick={() => setShowNewRun(true)}>New run</Button>
      </div>
      <NewRunDialog open={showNewRun} onOpenChange={setShowNewRun} />
```

(Replace the existing standalone `<PageHeader .../>` element with the flex wrapper above.)

- [ ] **Step 3: Typecheck + lint + run the runs-page tests if any**

Run: `pnpm -C web tsc --noEmit && pnpm -C web lint && pnpm -C web test -- runs`
Expected: CLEAN; any existing runs tests still pass.

- [ ] **Step 4: Commit**

```bash
git add "web/app/(app)/runs/page.tsx" web/components/app/new-run-dialog.tsx
git commit -m "feat(web): New-run dialog on the runs page

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task D7: Sidebar navigation link

**Files:**
- Modify: `web/components/app/app-sidebar.tsx`

- [ ] **Step 1: Add the Sources nav item**

In `web/components/app/app-sidebar.tsx`, import an icon and add the nav entry (between Runs and Exceptions). Change the import:

```tsx
import { LayoutDashboard, ListChecks, TriangleAlert, Scale, Users, Database, type LucideIcon } from "lucide-react";
```

Add to `NAV_ITEMS`:

```tsx
  { href: "/sources", label: "Sources", icon: Database },
```

- [ ] **Step 2: Typecheck + run the full frontend suite**

Run: `pnpm -C web tsc --noEmit && pnpm -C web test`
Expected: CLEAN; all vitest tests PASS.

- [ ] **Step 3: Commit**

```bash
git add web/components/app/app-sidebar.tsx
git commit -m "feat(web): add Sources to the sidebar navigation

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase E — End-to-end + docs

### Task E1: E2E fixtures + Playwright spec

**Files:**
- Create: `web/tests/e2e/fixtures/bank.csv`
- Create: `web/tests/e2e/fixtures/ledger.camt053.xml`
- Create: `web/tests/e2e/ingestion.spec.ts`

- [ ] **Step 1: Create the fixtures**

`web/tests/e2e/fixtures/bank.csv`:

```csv
ref,date,amount,desc
BANK-1,2026-05-10,-100.00,Payment to supplier
BANK-2,2026-05-11,-250.00,Office rent
```

`web/tests/e2e/fixtures/ledger.camt053.xml`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.02">
  <BkToCstmrStmt><Stmt>
    <Ntry>
      <Amt Ccy="GBP">100.00</Amt><CdtDbtInd>DBIT</CdtDbtInd>
      <ValDt><Dt>2026-05-10</Dt></ValDt>
      <NtryDtls><TxDtls><Refs><AcctSvcrRef>LEDG-1</AcctSvcrRef></Refs>
      <RmtInf><Ustrd>Payment to supplier</Ustrd></RmtInf></TxDtls></NtryDtls>
    </Ntry>
  </Stmt></BkToCstmrStmt>
</Document>
```

- [ ] **Step 2: Write the E2E spec**

Read `web/tests/e2e/operator-loop.spec.ts` and `web/tests/e2e/helpers.ts` first to reuse the login helper and the reseed-before-each pattern. `web/tests/e2e/ingestion.spec.ts`:

```ts
import { test, expect } from "@playwright/test";
import path from "node:path";
import { loginAs } from "./helpers"; // reuse the existing login helper (confirm its exact name/signature)

test.beforeEach(async ({ request }) => {
  await request.post("http://localhost:8080/api/dev/reseed");
});

test("operator ingests two files and creates a run", async ({ page }) => {
  await loginAs(page, "ada@acme.test", "Password123!"); // admin has ManageData; adjust to helper's signature

  // Create a bank source
  await page.goto("/sources");
  await page.getByRole("button", { name: /new source/i }).click();
  await page.getByLabel("Name").fill("E2E Bank");
  await page.getByLabel("Currency").fill("GBP");
  await page.getByRole("button", { name: /create source/i }).click();
  await expect(page.getByText("E2E Bank")).toBeVisible();

  // Upload CSV into it
  await page.getByRole("row", { name: /E2E Bank/ }).getByRole("button", { name: /upload/i }).click();
  await page.getByLabel("File").setInputFiles(path.join(__dirname, "fixtures/bank.csv"));
  // default mapping matches bank.csv (ref=0,date=1,amount=2 signed neg=debit,desc=3)
  await page.getByLabel("Description col").fill("3");
  await page.getByRole("button", { name: /^upload$/i }).click();
  await expect(page.getByText(/2 transactions ingested/i)).toBeVisible();

  // Create a ledger source + upload CAMT.053
  await page.getByRole("button", { name: /new source/i }).click();
  await page.getByLabel("Name").fill("E2E Ledger");
  await page.getByLabel("Currency").fill("GBP");
  // set kind to ledger
  await page.getByLabel("Kind").click();
  await page.getByRole("option", { name: "Ledger" }).click();
  await page.getByRole("button", { name: /create source/i }).click();

  await page.getByRole("row", { name: /E2E Ledger/ }).getByRole("button", { name: /upload/i }).click();
  await page.getByLabel("Format").click();
  await page.getByRole("option", { name: /CAMT/i }).click();
  await page.getByLabel("File").setInputFiles(path.join(__dirname, "fixtures/ledger.camt053.xml"));
  await page.getByRole("button", { name: /^upload$/i }).click();
  await expect(page.getByText(/1 transaction ingested/i)).toBeVisible();

  // Create a run across the two sources
  await page.goto("/runs");
  await page.getByRole("button", { name: /new run/i }).click();
  await page.getByLabel("Name").fill("E2E Run");
  await page.getByLabel("Source A").click();
  await page.getByRole("option", { name: "E2E Bank" }).click();
  await page.getByLabel("Source B").click();
  await page.getByRole("option", { name: "E2E Ledger" }).click();
  await page.getByLabel("From").fill("2026-05-01");
  await page.getByLabel("To").fill("2026-05-31");
  await page.getByRole("button", { name: /create run/i }).click();

  // Lands on the run detail page; one pair matches, one bank txn is a break.
  await expect(page).toHaveURL(/\/runs\/run-/);
});
```

> The selectors above assume the labels/roles produced by D4–D6. If the login helper has a different signature (check `helpers.ts`), adapt the first line. Keep `playwright.config.ts` untouched — it already runs the web server on :3100 with `reuseExistingServer`.

- [ ] **Step 3: Run the E2E (requires the live backend on :8080 with RECON_DEV=1)**

Run: `pnpm -C web e2e ingestion`
Expected: the ingestion spec PASSES. (If the backend is not running, start it per `web/README.md`.)

- [ ] **Step 4: Commit**

```bash
git add web/tests/e2e/fixtures web/tests/e2e/ingestion.spec.ts
git commit -m "test(e2e): ingest CSV + CAMT.053 and create a run end-to-end

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task E2: Documentation

**Files:**
- Modify: `web/README.md`

- [ ] **Step 1: Document the ingestion flow**

Append a section to `web/README.md` after the dev-credentials block:

```markdown
### Ingesting bank/ledger files

1. Sign in (any role can ingest).
2. Go to **Sources** → **New source** (give it a name, kind, and currency).
3. Click **Upload** on the source row, choose **CSV** or **CAMT.053**, pick a file, and
   (for CSV) map the columns by 0-based index + choose how amounts are encoded
   (single signed column, or separate debit/credit columns). Bad rows reject the whole
   file with a per-row report; re-uploading an already-loaded statement is rejected as a
   duplicate.
4. Create a second source and upload its file.
5. Go to **Runs** → **New run**, pick the two sources + a date window, and **Create run**.
   You land on the run detail with matches and breaks.

Supported formats this slice: CSV (configurable mapping) and CAMT.053 (ISO 20022 XML).
```

- [ ] **Step 2: Commit**

```bash
git add web/README.md
git commit -m "docs: document the bank-format ingestion flow

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Final verification (after all tasks)

- [ ] **Backend:** `cd backend && cargo test && cargo clippy --workspace -- -D warnings` → all green.
- [ ] **Frontend:** `pnpm -C web tsc --noEmit && pnpm -C web lint && pnpm -C web test` → all green.
- [ ] **E2E:** with the live stack up, `pnpm -C web e2e` → all specs green (existing + ingestion).
- [ ] Dispatch a final code review over the whole branch, then use **superpowers:finishing-a-development-branch**.

---

## Notes for the implementer

- **Read-before-edit:** several tasks modify existing files (`error.rs`, `dto.rs`, `routes.rs`, `app-sidebar.tsx`, `runs/page.tsx`, `mock.ts`, `http.ts`). Read each file first; match its existing style.
- **Next.js 16 / React 19:** consult `web/AGENTS.md` and `node_modules/next/dist` if any App Router / Suspense behavior surprises you — this version has breaking changes vs older training data.
- **sqlx:** runtime-checked queries only (`sqlx::query`/`query_as`/`query_scalar`) — never the `query!` macros (matches the rest of `recon-store`). Tests use `#[sqlx::test(migrations = "../../migrations")]`.
- **Money:** amounts are minor units (i64), 2 decimal places this slice. `direction` is separate from the (always non-negative) magnitude.
- **Don't change the seed's fixed ids** (`case-pending` / `break-pending` / `txn-brk001`) — many tests pin to them.
```
