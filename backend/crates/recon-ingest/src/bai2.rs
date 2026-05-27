//! BAI v2 (Bank Administration Institute version 2) parser.
//!
//! Record-based format used by US banks. Each record:
//!   <2-digit type code>,<field>,<field>,...<field>/
//!
//! Type codes used by this parser:
//!   01 File Header     (skipped, but recognised)
//!   02 Group Header    (field 4 = as-of-date YYMMDD — captured; flows into every
//!                       16 in the group as its value-date)
//!   03 Account Header  (sets account context; field 2 is currency)
//!   16 Transaction Detail (the actual transaction — fields documented in parse_16)
//!   88 Continuation    (appends to the most recent 16's description)
//!   49 Account Trailer (skipped)
//!   98 Group Trailer   (skipped)
//!   99 File Trailer    (skipped)
//!
//! BAI2 amount is in lowest currency unit (cents for USD) — no decimal.
//!
//! ## Field order on the `16` record
//!
//! Per the BAI Cash Management Balance Reporting Specifications, the `16` record
//! field order is:
//!   1. Record type (`16`)
//!   2. Type code (3 digits, mapped to debit/credit by `direction_for_type_code`)
//!   3. Amount (digits in lowest currency unit)
//!   4. Funds type (V, S, D, etc.)
//!   5. **Bank reference number**
//!   6. **Customer reference number**
//!   7. Text (free-form description; may contain commas — joined back together
//!      so embedded commas are not silently truncated)
//!
//! `external_ref` derivation: prefers the customer reference (more stable for
//! reconciling against the counterparty ledger); falls back to the bank
//! reference; if both are absent the row is rejected.

use crate::{Parser, ParsedTxn, RowError};
use recon_domain::Direction;

pub struct Bai2Parser;

impl Parser for Bai2Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        if let Some(off) = bytes.iter().position(|&b| !b.is_ascii()) {
            return Err(vec![RowError::new(0, "encoding", format!("non-ASCII byte at offset {off}"))]);
        }
        let text = std::str::from_utf8(bytes).expect("ASCII-checked above is valid UTF-8");

        let mut txns: Vec<ParsedTxn> = Vec::new();
        let mut errors: Vec<RowError> = Vec::new();
        let mut account_currency: Option<String> = None;
        // The 02 Group Header's as-of-date (YYMMDD) flows into every 16 in the
        // same group as its value-date. None means "no 02 seen yet".
        let mut group_value_date: Option<String> = None;
        let mut last_txn_idx: Option<usize> = None;

        for (i, raw) in text.lines().enumerate() {
            let line_no = i + 1;
            let line = raw.trim_end_matches('/').trim();
            if line.is_empty() { continue; }
            let fields: Vec<&str> = line.split(',').map(|f| f.trim()).collect();
            if fields.is_empty() { continue; }
            match fields[0] {
                "01" | "49" | "98" | "99" => { /* structural — ignore */ }
                "02" => {
                    // 02 = Group Header. Field 4 (index 4) is the as-of-date in YYMMDD.
                    // Format: 02,ultimate_receiver,originator,group_status,as_of_date,as_of_time,currency,...
                    if let Some(d) = fields.get(4).copied().filter(|s| !s.is_empty()) {
                        match parse_yymmdd(d) {
                            Ok(iso) => group_value_date = Some(iso),
                            Err(msg) => errors.push(RowError::new(line_no, "as_of_date", msg)),
                        }
                    }
                }
                "03" => {
                    // Field 2 (index 2) is currency.
                    if fields.len() >= 3 && !fields[2].is_empty() {
                        account_currency = Some(fields[2].to_string());
                    }
                }
                "16" => {
                    last_txn_idx = None;
                    // Description text (everything after the 6th comma) is taken
                    // from the original line so embedded commas AND internal
                    // whitespace survive the per-field trim that the numeric
                    // fields rely on. comma_index 6 = end of bank_ref/customer_ref;
                    // everything after is free-form description text.
                    let raw_description = nth_comma_tail(line, 6);
                    match parse_16(&fields, account_currency.as_deref(), group_value_date.as_deref(), raw_description) {
                        Ok(t) => {
                            txns.push(t);
                            last_txn_idx = Some(txns.len() - 1);
                        }
                        Err((field, msg)) => {
                            errors.push(RowError::new(line_no, field, msg));
                        }
                    }
                }
                "88" => {
                    // Continuation of preceding 16's description. Fields after the
                    // type code are joined back with commas so embedded commas are
                    // preserved verbatim.
                    if let Some(idx) = last_txn_idx {
                        let cont = fields[1..].join(",");
                        let txn = &mut txns[idx];
                        if !txn.description.is_empty() && !cont.is_empty() {
                            txn.description.push(' ');
                        }
                        txn.description.push_str(&cont);
                    }
                }
                other => {
                    errors.push(RowError::new(line_no, "type_code", format!("unsupported record type '{other}'")));
                }
            }
        }

        if errors.is_empty() { Ok(txns) } else { Err(errors) }
    }
}

/// BAI2 type code → direction. Built-in table covers the common codes used by
/// US-bank statement files. Extend with care: misclassifying a code silently
/// inverts direction at ingest, which then fails matching downstream. When in
/// doubt, leave a code out and let the row return a "unknown type code" error
/// — fail loud beats silent misclassification.
///
/// Reference: BAI Cash Management Balance Reporting Specifications, Appendix A.
fn direction_for_type_code(code: &str) -> Option<Direction> {
    match code {
        // Common credits (deposits / incoming)
        "100" | "108" | "115" | "175" | "195" | "301" | "399" => Some(Direction::Credit),
        // Common debits (withdrawals / outgoing)
        "400" | "408" | "409" | "475" | "495" | "555" | "595" => Some(Direction::Debit),
        // 501 ("Affiliated Bank Credits") removed from the table: classification
        // varies by bank convention and the safe default is to require an
        // explicit code-mapping decision rather than silently picking a side.
        _ => None,
    }
}

fn parse_16(
    fields: &[&str],
    _account_currency: Option<&str>,
    group_value_date: Option<&str>,
    raw_description: &str,
) -> Result<ParsedTxn, (&'static str, String)> {
    // 16,<type_code>,<amount>,<funds_type>,<bank_ref>,<customer_ref>,<text...>
    // Per BAI2 spec: field 5 (index 4) is the bank reference, field 6 (index 5)
    // is the customer reference. external_ref prefers customer-ref because it's
    // the more stable identifier for matching against the counterparty ledger.
    if fields.len() < 3 { return Err(("type_code", "16 record too short".into())); }
    let type_code = fields[1];
    let direction = direction_for_type_code(type_code)
        .ok_or(("type_code", format!("unknown BAI2 type code '{type_code}'")))?;
    let amount_str = fields[2];
    if amount_str.is_empty() { return Err(("amount", "missing amount".into())); }
    if !amount_str.chars().all(|c| c.is_ascii_digit()) {
        return Err(("amount", format!("non-numeric amount '{amount_str}'")));
    }
    let amount_minor: i64 = amount_str.parse()
        .map_err(|_| ("amount", format!("amount '{amount_str}' overflows")))?;

    // bank-ref + customer-ref are taken from the trimmed fields (whitespace
    // around them in malformed files is noise). The description (free text)
    // comes from the original line so its internal whitespace is preserved.
    let bank_ref = fields.get(4).copied().unwrap_or("").trim().to_string();
    let customer_ref = fields.get(5).copied().unwrap_or("").trim().to_string();
    let text = raw_description.to_string();

    let external_ref = if !customer_ref.is_empty() { customer_ref }
        else if !bank_ref.is_empty() { bank_ref }
        else { return Err(("external_ref", "no customer-ref or bank-ref on 16 record".into())); };

    // The 02 Group Header's as-of-date is the value-date for every 16 in the
    // group. If no 02 has been seen (malformed file), the row is rejected — a
    // value-date is required for downstream run-window filtering.
    let value_date = group_value_date
        .ok_or(("value_date", "no 02 Group Header before this 16 record".to_string()))?
        .to_string();

    Ok(ParsedTxn {
        external_ref,
        value_date,
        posted_at: None,
        amount_minor,
        currency: None,
        direction,
        counterparty: None,
        description: text,
        counterparty_bic: None,
        counterparty_account: None,
    })
}

/// Return everything in `line` after the Nth comma (0-indexed: the 6th comma
/// separates the customer-ref field from the description field on a `16`
/// record). Returns `""` if there are fewer than N commas in the line.
fn nth_comma_tail(line: &str, n: usize) -> &str {
    let mut seen = 0;
    for (i, b) in line.bytes().enumerate() {
        if b == b',' {
            seen += 1;
            if seen == n {
                return &line[i + 1..];
            }
        }
    }
    ""
}

fn parse_yymmdd(s: &str) -> Result<String, String> {
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("expected YYMMDD, got '{s}'"));
    }
    let yy: u32 = s[0..2].parse().map_err(|_| "yy not digits".to_string())?;
    let mm: u32 = s[2..4].parse().map_err(|_| "mm not digits".to_string())?;
    let dd: u32 = s[4..6].parse().map_err(|_| "dd not digits".to_string())?;
    Ok(format!("{:04}-{:02}-{:02}", 2000 + yy, mm, dd))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Vec<u8> {
        std::fs::read(format!("tests/fixtures/{name}")).expect("fixture file")
    }

    #[test]
    fn happy_path_three_txns() {
        let bytes = load("bai2-single-account.bai");
        let txns = Bai2Parser.parse(&bytes).unwrap();
        assert_eq!(txns.len(), 3);
        // Field order is bank-ref then customer-ref (BAI2 spec); external_ref
        // prefers customer-ref. Fixture row 1: bank=BNKREF-A, customer=CUSTREF-1.
        assert_eq!(txns[0].external_ref, "CUSTREF-1");
        assert_eq!(txns[0].direction, Direction::Credit);  // 175
        assert_eq!(txns[0].amount_minor, 25000);
        // value-date inherits from the 02 group header (250601 → 2025-06-01).
        assert_eq!(txns[0].value_date, "2025-06-01");
        assert_eq!(txns[1].direction, Direction::Debit);   // 475
        // Row 3: customer-ref empty → external_ref falls back to bank-ref.
        assert_eq!(txns[2].external_ref, "BNKREF-C");
    }

    #[test]
    fn continuation_88_merges_into_preceding_16() {
        let bytes = load("bai2-continuation.bai");
        let txns = Bai2Parser.parse(&bytes).unwrap();
        assert_eq!(txns.len(), 2);
        assert!(txns[0].description.contains("Deposit from customer"));
        assert!(txns[0].description.contains("Additional description continued"));
    }

    #[test]
    fn description_preserves_embedded_commas() {
        // Field 7 onward must be joined with `,` so commas in free text survive.
        let raw = b"01,S,R,250601,0930,1,80,2,2/\n02,A,S,1,250601,0930,USD,2/\n03,123,USD,010,0,,,015,0,,/\n16,175,500,V,BNKREF,CUSTREF,Hello, world, with commas/\n";
        let txns = Bai2Parser.parse(raw).unwrap();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].description, "Hello, world, with commas");
    }

    #[test]
    fn customer_ref_missing_falls_back_to_bank_ref() {
        // Field 4 (bank-ref) present, field 5 (customer-ref) empty → external_ref = bank-ref.
        let raw = b"01,S,R,250601,0930,1,80,2,2/\n02,A,S,1,250601,0930,USD,2/\n03,123,USD,010,0,,,015,0,,/\n16,175,500,V,BANKREF-X,,Some text/\n";
        let txns = Bai2Parser.parse(raw).unwrap();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].external_ref, "BANKREF-X");
    }

    #[test]
    fn both_refs_missing_returns_row_error() {
        let raw = b"01,S,R,250601,0930,1,80,2,2/\n02,A,S,1,250601,0930,USD,2/\n03,123,USD,010,0,,,015,0,,/\n16,175,500,V,,,Some text/\n";
        let err = Bai2Parser.parse(raw).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].field, "external_ref");
    }

    #[test]
    fn unknown_type_code_returns_row_error() {
        let raw = b"01,S,R,250601,0930,1,80,2,2/\n02,A,S,1,250601,0930,USD,2/\n03,123,USD,010,0,,,015,0,,/\n16,999,500,V,B,C,bad/\n";
        let err = Bai2Parser.parse(raw).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].field, "type_code");
    }

    #[test]
    fn type_code_501_now_unknown_after_audit() {
        // 501 was previously classified as debit; the audit removed it. Now an
        // unknown code error so future contributors must decide explicitly.
        let raw = b"01,S,R,250601,0930,1,80,2,2/\n02,A,S,1,250601,0930,USD,2/\n03,123,USD,010,0,,,015,0,,/\n16,501,500,V,B,C,affiliated/\n";
        let err = Bai2Parser.parse(raw).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].field, "type_code");
    }

    #[test]
    fn non_ascii_byte_returns_row_error() {
        let mut raw = b"01,S,R,250601,0930,1,80,2,2/\n".to_vec();
        raw.push(0xE9); // é in Latin-1 — not ASCII
        let err = Bai2Parser.parse(&raw).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].field, "encoding");
    }

    #[test]
    fn non_numeric_amount_returns_row_error() {
        let raw = b"01,S,R,250601,0930,1,80,2,2/\n02,A,S,1,250601,0930,USD,2/\n03,123,USD,010,0,,,015,0,,/\n16,175,abc,V,B,C,bad/\n";
        let err = Bai2Parser.parse(raw).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].field, "amount");
    }

    #[test]
    fn value_date_from_02_group_header() {
        let raw = b"01,S,R,250601,0930,1,80,2,2/\n02,A,S,1,260315,0930,USD,2/\n03,123,USD,010,0,,,015,0,,/\n16,175,500,V,BNK,CUST,text/\n";
        let txns = Bai2Parser.parse(raw).unwrap();
        // 260315 → 2026-03-15
        assert_eq!(txns[0].value_date, "2026-03-15");
    }

    #[test]
    fn missing_02_before_16_returns_row_error() {
        // No 02 header at all — value-date can't be determined.
        let raw = b"01,S,R,250601,0930,1,80,2,2/\n03,123,USD,010,0,,,015,0,,/\n16,175,500,V,B,C,text/\n";
        let err = Bai2Parser.parse(raw).unwrap_err();
        assert!(err.iter().any(|e| e.field == "value_date"));
    }
}
