//! SWIFT MT940 (Customer Statement Message) parser.
//!
//! Tag-based block format. Each statement message begins at `:20:` and ends at
//! `:62F:` / `:62M:`. A file may contain multiple messages back-to-back; all
//! transactions across messages fold into one upload.
//!
//! `:61:` is the statement line (one per transaction). `:86:` immediately after
//! is the information-to-account-owner field (description). Two dialects:
//!   Generic    — `:86:` is opaque description text.
//!   Subfielded — `:86:` is parsed for `?nn` subfield codes (DE/NL/BE banks).
//!
//! Encoding: try UTF-8; on failure, fall back to Latin-1 (always succeeds —
//! every byte maps to a char in Latin-1). The fallback is silent.

use crate::{ParsedTxn, Parser, RowError};
use recon_domain::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mt940Dialect {
    Generic,
    Subfielded,
}

pub struct Mt940Parser {
    pub dialect: Mt940Dialect,
}

impl Parser for Mt940Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        let text = decode(bytes);
        let mut txns: Vec<ParsedTxn> = Vec::new();
        let mut errors: Vec<RowError> = Vec::new();

        // State: current pending :61:; concatenated :86: lines.
        let mut pending: Option<(usize, Mt61)> = None; // (line, parsed :61:)
        let mut info_buf: Vec<String> = Vec::new();

        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0usize;
        while i < lines.len() {
            let raw = lines[i];
            let line_no = i + 1;
            // Continuation: tag lines start with `:`; everything else is a
            // continuation of the previous tag's content. MT940 wraps long
            // `:86:` across newlines. We only need to handle :86: continuation;
            // :61: is single-line.
            if !raw.starts_with(':') {
                if pending.is_some() {
                    info_buf.push(raw.to_string());
                }
                i += 1;
                continue;
            }
            let (tag, content) = parse_tag(raw);
            match tag {
                ":20:" | ":25:" | ":28C:" | ":60F:" | ":60M:" | ":62F:" | ":62M:" | ":64:"
                | ":65:" => {
                    // Statement boundary or balance — flush any pending :61:.
                    if let Some((line, p61)) = pending.take() {
                        let info = std::mem::take(&mut info_buf);
                        match build_txn(&p61, &info, self.dialect) {
                            Ok(t) => txns.push(t),
                            Err(e) => errors.push(RowError::new(line, e.0, e.1)),
                        }
                    }
                }
                ":61:" => {
                    // Flush previous :61: with whatever :86: we have for it.
                    if let Some((line, p61)) = pending.take() {
                        let info = std::mem::take(&mut info_buf);
                        match build_txn(&p61, &info, self.dialect) {
                            Ok(t) => txns.push(t),
                            Err(e) => errors.push(RowError::new(line, e.0, e.1)),
                        }
                    }
                    match parse_61(content) {
                        Ok(p) => pending = Some((line_no, p)),
                        Err(e) => errors.push(RowError::new(line_no, e.0, e.1)),
                    }
                }
                // :86: not following a :61: is statement-level info — ignored.
                ":86:" if pending.is_some() => {
                    info_buf.push(content.to_string());
                }
                _ => {
                    // Unknown tag — skip.
                }
            }
            i += 1;
        }
        // Flush any trailing pending :61: (file ended without a :62F:).
        if let Some((line, p61)) = pending {
            let info = std::mem::take(&mut info_buf);
            match build_txn(&p61, &info, self.dialect) {
                Ok(t) => txns.push(t),
                Err(e) => errors.push(RowError::new(line, e.0, e.1)),
            }
        }

        if errors.is_empty() {
            Ok(txns)
        } else {
            Err(errors)
        }
    }
}

fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

fn parse_tag(raw: &str) -> (&str, &str) {
    // A tag line is like ":20:REF12345" — find the second ':' to split.
    let mut idx = 0usize;
    let mut count = 0;
    for (i, c) in raw.char_indices() {
        if c == ':' {
            count += 1;
            if count == 2 {
                idx = i + 1;
                break;
            }
        }
    }
    if count < 2 {
        return (raw, "");
    }
    let tag = &raw[..idx];
    let content = &raw[idx..];
    (tag, content)
}

struct Mt61 {
    value_date: String, // ISO 8601 YYYY-MM-DD
    direction: Direction,
    amount_minor: i64,
    customer_ref: Option<String>,
    bank_ref: Option<String>,
}

fn parse_61(content: &str) -> Result<Mt61, (&'static str, String)> {
    // Format: YYMMDD [MMDD]? <D|C|RD|RC> [3rd-char-currency-mark]? amount_with_comma_decimal
    //         transaction-type-code(3 chars, starts with letter) customer-ref [//bank-ref] [extra]
    // Examples:
    //   250601D100,00NTRFCUSTREF-1//BNKREF-A
    //   250601D100,00NTRFCUSTREF-1
    //   250601C50,00NTRFREF
    let mut idx;
    let bytes = content.as_bytes();
    let n = bytes.len();

    // value-date YYMMDD
    if n < 6 {
        return Err(("date", "too short".into()));
    }
    let yy = parse_digits(&content[0..2]).map_err(|_| ("date", "yy not digits".to_string()))?;
    let mm = parse_digits(&content[2..4]).map_err(|_| ("date", "mm not digits".to_string()))?;
    let dd = parse_digits(&content[4..6]).map_err(|_| ("date", "dd not digits".to_string()))?;
    let year = 2000 + yy; // YY → 20YY (MT940 is post-2000 in practice)
    let value_date = format!("{:04}-{:02}-{:02}", year, mm, dd);
    idx = 6;

    // optional entry-date MMDD
    if idx + 4 <= n
        && bytes[idx].is_ascii_digit()
        && bytes[idx + 1].is_ascii_digit()
        && bytes[idx + 2].is_ascii_digit()
        && bytes[idx + 3].is_ascii_digit()
    {
        idx += 4;
    }

    // D/C/RD/RC mark
    let direction = if idx < n && bytes[idx] == b'R' {
        idx += 1;
        if idx >= n {
            return Err(("dc_mark", "expected D or C after R".into()));
        }
        match bytes[idx] {
            b'D' => {
                idx += 1;
                Direction::Credit
            } // RD = reverse-debit → effectively credit
            b'C' => {
                idx += 1;
                Direction::Debit
            } // RC = reverse-credit → effectively debit
            _ => return Err(("dc_mark", "expected D or C after R".into())),
        }
    } else if idx < n {
        match bytes[idx] {
            b'D' => {
                idx += 1;
                Direction::Debit
            }
            b'C' => {
                idx += 1;
                Direction::Credit
            }
            _ => return Err(("dc_mark", "expected D, C, RD or RC".into())),
        }
    } else {
        return Err(("dc_mark", "missing D/C mark".into()));
    };

    // optional 3rd-char currency mark — a single letter (rarely used). Skip if next char is alpha
    // but only when followed by a digit (amount starts with digit).
    if idx < n
        && bytes[idx].is_ascii_alphabetic()
        && idx + 1 < n
        && bytes[idx + 1].is_ascii_digit()
    {
        idx += 1;
    }

    // Amount — digits and one comma. Reads until first non-digit-non-comma.
    let amt_start = idx;
    while idx < n && (bytes[idx].is_ascii_digit() || bytes[idx] == b',') {
        idx += 1;
    }
    if idx == amt_start {
        return Err(("amount", "missing amount".into()));
    }
    // MT940 uses ',' as decimal separator; the shared parser expects '.'.
    let amount_str = content[amt_start..idx].replace(',', ".");
    let amount_minor =
        crate::money::parse_decimal_to_minor(&amount_str).map_err(|e| ("amount", e))?;

    // Transaction type code: 'N' followed by 3 alpha (e.g. "NTRF", "NCHK"). Skip 4 chars.
    if idx + 4 <= n
        && bytes[idx] == b'N'
        && bytes[idx + 1..idx + 4]
            .iter()
            .all(|b| b.is_ascii_alphabetic())
    {
        idx += 4;
    } else if idx + 4 <= n
        && bytes[idx] == b'S'
        && bytes[idx + 1..idx + 4]
            .iter()
            .all(|b| b.is_ascii_alphabetic())
    {
        // 'S' instead of 'N' is allowed (SWIFT code variant).
        idx += 4;
    } else {
        return Err(("type_code", "missing/invalid transaction type code".into()));
    }

    // References: customer-ref [//bank-ref] [\n description]
    let tail = &content[idx..];
    let (customer_ref, bank_ref) = if let Some(slash_idx) = tail.find("//") {
        let cust = tail[..slash_idx].trim();
        let bank = tail[slash_idx + 2..].trim();
        let cust = if cust.is_empty() {
            None
        } else {
            Some(cust.to_string())
        };
        let bank = if bank.is_empty() {
            None
        } else {
            Some(bank.to_string())
        };
        (cust, bank)
    } else {
        let cust = tail.trim();
        let cust = if cust.is_empty() {
            None
        } else {
            Some(cust.to_string())
        };
        (cust, None)
    };

    Ok(Mt61 {
        value_date,
        direction,
        amount_minor,
        customer_ref,
        bank_ref,
    })
}

fn parse_digits(s: &str) -> Result<u32, ()> {
    if s.chars().all(|c| c.is_ascii_digit()) {
        s.parse().map_err(|_| ())
    } else {
        Err(())
    }
}

fn build_txn(
    p: &Mt61,
    info_lines: &[String],
    dialect: Mt940Dialect,
) -> Result<ParsedTxn, (&'static str, String)> {
    let external_ref = p
        .customer_ref
        .clone()
        .or_else(|| p.bank_ref.clone())
        .ok_or((
            "external_ref",
            "no customer-ref or bank-ref on :61:".to_string(),
        ))?;
    let raw_info = info_lines.join("");
    let (description, counterparty, counterparty_bic, counterparty_account) = match dialect {
        Mt940Dialect::Generic => (raw_info, None, None, None),
        Mt940Dialect::Subfielded => {
            let s = parse_subfielded_86(&raw_info);
            (s.description, s.counterparty, s.counterparty_bic, s.counterparty_account)
        }
    };
    Ok(ParsedTxn {
        external_ref,
        value_date: p.value_date.clone(),
        posted_at: None,
        amount_minor: p.amount_minor,
        currency: None,
        direction: p.direction,
        counterparty,
        description,
        counterparty_bic,
        counterparty_account,
    })
}

/// Parsed result of a Subfielded `:86:` field.
struct SubfieldedInfo {
    description: String,
    counterparty: Option<String>,
    counterparty_bic: Option<String>,
    counterparty_account: Option<String>,
}

/// Parse a Subfielded `:86:` field. Subfields are `?nn` separated.
///
/// `?32` → `counterparty_account` (trimmed); `?33` → `counterparty_bic`
/// (trimmed + uppercased). Both fields are structural in Phase 7. `?20`–`?29`
/// fold into description; `?30`/`?31` (counterparty bank BLZ + account) and
/// unknown subfields are preserved verbatim inside the description.
fn parse_subfielded_86(raw: &str) -> SubfieldedInfo {
    let mut desc_parts: Vec<String> = Vec::new();
    let mut cpty_bic: Option<String> = None;
    let mut cpty_account: Option<String> = None;
    let mut prefix = String::new();
    let mut chunks = raw.split('?');
    if let Some(first) = chunks.next() {
        if !first.is_empty() {
            prefix.push_str(first);
        }
    }
    if !prefix.is_empty() {
        desc_parts.push(prefix);
    }
    for chunk in chunks {
        if chunk.len() < 2 {
            continue;
        }
        let code = &chunk[..2];
        let val = &chunk[2..];
        match code {
            "20" | "21" | "22" | "23" | "24" | "25" | "26" | "27" | "28" | "29" => {
                desc_parts.push(val.to_string());
            }
            "32" => {
                if cpty_account.is_none() {
                    let v = val.trim();
                    if !v.is_empty() {
                        cpty_account = Some(v.to_string());
                    }
                }
            }
            "33" => {
                if cpty_bic.is_none() {
                    let v = val.trim().to_uppercase();
                    if !v.is_empty() {
                        cpty_bic = Some(v);
                    }
                }
            }
            "30" | "31" => {
                // Counterparty bank BLZ + account — preserve into description.
                desc_parts.push(format!("[{code}:{val}]"));
            }
            _ => {
                // Unknown subfield — preserve into description verbatim.
                desc_parts.push(format!("[?{code}:{val}]"));
            }
        }
    }
    let description = desc_parts.join(" ").trim().to_string();
    SubfieldedInfo {
        description,
        counterparty: None,
        counterparty_bic: cpty_bic,
        counterparty_account: cpty_account,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Vec<u8> {
        std::fs::read(format!("tests/fixtures/{name}")).expect("fixture file")
    }

    #[test]
    fn happy_path_single_message_two_txns() {
        let bytes = load("mt940-single-message.sta");
        let txns = Mt940Parser {
            dialect: Mt940Dialect::Generic,
        }
        .parse(&bytes)
        .unwrap();
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].external_ref, "BANKREF-1"); // customer-ref before //
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[0].amount_minor, 10000); // 100.00 → 10000 minor
        assert_eq!(txns[0].value_date, "2025-06-01");
        assert_eq!(txns[0].description, "Counterparty payment 1");
        assert_eq!(txns[1].external_ref, "CUSTREF-2");
        assert_eq!(txns[1].direction, Direction::Credit);
    }

    #[test]
    fn multi_message_file_three_txns_total() {
        let bytes = load("mt940-multi-message.sta");
        let txns = Mt940Parser {
            dialect: Mt940Dialect::Generic,
        }
        .parse(&bytes)
        .unwrap();
        assert_eq!(txns.len(), 3);
        assert_eq!(txns[0].value_date, "2025-06-01");
        assert_eq!(txns[1].value_date, "2025-06-02");
        assert_eq!(txns[2].value_date, "2025-06-02");
    }

    #[test]
    fn subfielded_86_extracts_counterparty_bic_and_account() {
        let bytes = load("mt940-subfielded.sta");
        let txns = Mt940Parser {
            dialect: Mt940Dialect::Subfielded,
        }
        .parse(&bytes)
        .unwrap();
        assert_eq!(txns.len(), 1);
        let t = &txns[0];
        assert_eq!(
            t.counterparty_account.as_deref(),
            Some("DE89370400440532013000")
        );
        assert_eq!(t.counterparty_bic.as_deref(), Some("DEUTDEFF"));
        assert!(t.description.contains("Invoice payment"));
        assert!(t.description.contains("INV-12345"));
    }

    #[test]
    fn generic_86_does_not_populate_counterparty_fields() {
        let bytes = load("mt940-subfielded.sta");
        let txns = Mt940Parser {
            dialect: Mt940Dialect::Generic,
        }
        .parse(&bytes)
        .unwrap();
        assert!(txns[0].counterparty_bic.is_none());
        assert!(txns[0].counterparty_account.is_none());
    }

    #[test]
    fn generic_86_passed_through_unchanged() {
        let bytes = load("mt940-subfielded.sta");
        let txns = Mt940Parser {
            dialect: Mt940Dialect::Generic,
        }
        .parse(&bytes)
        .unwrap();
        let t = &txns[0];
        assert!(t.counterparty.is_none());
        // Whole :86: ends up in description verbatim (with the ?nn codes still in there).
        assert!(t.description.contains("?32DE89370400440532013000"));
        assert!(t.description.contains("?33DEUTDEFF"));
    }

    #[test]
    fn missing_refs_returns_row_error() {
        let bad = b":20:REF1\n:25:ACC\n:28C:1/1\n:60F:C250601GBP100,00\n:61:250601D50,00NTRF//\n:86:no refs\n:62F:C250601GBP50,00\n";
        let err = Mt940Parser {
            dialect: Mt940Dialect::Generic,
        }
        .parse(bad)
        .unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].field, "external_ref");
    }

    #[test]
    fn bad_dc_mark_returns_row_error() {
        let bad = b":20:REF1\n:25:ACC\n:28C:1/1\n:60F:C250601GBP100,00\n:61:250601X50,00NTRFREF\n:62F:C250601GBP50,00\n";
        let err = Mt940Parser {
            dialect: Mt940Dialect::Generic,
        }
        .parse(bad)
        .unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0].field, "dc_mark");
    }

    #[test]
    fn latin1_fallback_decodes() {
        let bytes = load("mt940-latin1.sta");
        let txns = Mt940Parser {
            dialect: Mt940Dialect::Generic,
        }
        .parse(&bytes)
        .unwrap();
        assert_eq!(txns.len(), 1);
        assert!(txns[0].description.contains("Caf"));
        // The é byte (0xe9 in Latin-1) maps to U+00E9 'é'. Round-tripped.
        assert!(txns[0].description.contains('é'));
    }

    #[test]
    fn rc_reverse_credit_flips_to_debit() {
        let bytes = b":20:REF\n:25:ACC\n:28C:1/1\n:60F:C250601GBP100,00\n:61:250601RC50,00NTRFREF\n:86:reversed\n:62F:C250601GBP50,00\n";
        let txns = Mt940Parser {
            dialect: Mt940Dialect::Generic,
        }
        .parse(bytes)
        .unwrap();
        assert_eq!(txns[0].direction, Direction::Debit);
    }
}
