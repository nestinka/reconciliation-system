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
use crate::mt94x_shared::{decode, parse_61, parse_subfielded_86, parse_tag, Mt61};
pub use crate::mt94x_shared::Mt94xDialect as Mt940Dialect;

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

#[cfg(test)]
mod tests {
    use super::*;
    use recon_domain::Direction;

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
