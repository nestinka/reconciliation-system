//! Shared parser helpers for the MT94x family (MT940 customer statement,
//! MT942 intra-day statement). The two messages differ in tag set and
//! state machine, but share several lexical helpers.

use recon_domain::Direction;

/// Per-source dialect (re-used by MT940 and MT942 — they share the same
/// `?nn` subfield grammar in the `:86:` info field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mt94xDialect {
    Generic,
    Subfielded,
}

/// Decode bytes as UTF-8; on failure fall back to Latin-1 (one byte → one
/// char). The fallback is silent and always succeeds.
pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Split a tag line like `:20:REF12345` into `(":20:", "REF12345")`.
pub fn parse_tag(raw: &str) -> (&str, &str) {
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

/// Parsed result of a single `:61:` statement line — common to MT940 and MT942.
pub struct Mt61 {
    pub value_date: String, // ISO 8601 YYYY-MM-DD
    pub direction: Direction,
    pub amount_minor: i64,
    pub customer_ref: Option<String>,
    pub bank_ref: Option<String>,
}

/// Parse the content portion of a `:61:` tag line.
pub fn parse_61(content: &str) -> Result<Mt61, (&'static str, String)> {
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

/// Parsed result of a Subfielded `:86:` field.
pub struct SubfieldedInfo {
    pub description: String,
    pub counterparty: Option<String>,
    pub counterparty_bic: Option<String>,
    pub counterparty_account: Option<String>,
}

/// Parse a Subfielded `:86:` field. Subfields are `?nn` separated.
///
/// `?32` → `counterparty_account` (trimmed); `?33` → `counterparty_bic`
/// (trimmed + uppercased). Both fields are structural in Phase 7. `?20`–`?29`
/// fold into description; `?30`/`?31` (counterparty bank BLZ + account) and
/// unknown subfields are preserved verbatim inside the description.
pub fn parse_subfielded_86(raw: &str) -> SubfieldedInfo {
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

    #[test]
    fn parse_tag_splits_at_second_colon() {
        assert_eq!(parse_tag(":20:REF"), (":20:", "REF"));
        assert_eq!(parse_tag(":86:?20stuff"), (":86:", "?20stuff"));
    }

    #[test]
    fn decode_falls_back_to_latin1_for_invalid_utf8() {
        let bytes = b"Caf\xe9";
        let s = decode(bytes);
        assert!(s.contains('é'));
    }
}
