//! SWIFT MT942 (Interim Transaction Report / intra-day) parser.
//!
//! Tag-based block format, very close to MT940. Differences:
//!  - No `:60F:`/`:62F:` opening/closing balance tags — intra-day has no balance.
//!  - Adds `:34F:` (floor-limit indicator — parsed and discarded for state-machine cleanliness).
//!  - Adds `:13D:` (date/time of the statement — parsed and discarded).
//!  - Adds `:90D:` / `:90C:` totals — used for a sanity check that the parsed
//!    debit/credit count and minor-amount sums match what the file claims.
//!
//! Reuses MT940's `parse_61`, `parse_subfielded_86`, `parse_tag`, `decode`, and the
//! `Mt94xDialect` enum via the `mt94x_shared` module.

use crate::mt94x_shared::{decode, parse_61, parse_subfielded_86, parse_tag, Mt61, Mt94xDialect, SubfieldedInfo};
use crate::{ParsedTxn, Parser, RowError};
use recon_domain::Direction;

pub struct Mt942Parser {
    pub dialect: Mt94xDialect,
}

impl Parser for Mt942Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        let text = decode(bytes);
        let mut txns: Vec<ParsedTxn> = Vec::new();
        let mut errors: Vec<RowError> = Vec::new();

        let mut pending: Option<(usize, Mt61)> = None;
        let mut info_buf: Vec<String> = Vec::new();

        // For the :90D:/:90C: sanity check.
        let mut declared_debit_count: Option<i64> = None;
        let mut declared_credit_count: Option<i64> = None;
        let mut declared_debit_minor: Option<i64> = None;
        let mut declared_credit_minor: Option<i64> = None;

        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0usize;
        while i < lines.len() {
            let raw = lines[i];
            let line_no = i + 1;
            if !raw.starts_with(':') {
                if pending.is_some() {
                    info_buf.push(raw.to_string());
                }
                i += 1;
                continue;
            }
            let (tag, content) = parse_tag(raw);
            match tag {
                // MT940 balance tags are illegal in MT942 — reject loudly.
                ":60F:" | ":60M:" | ":62F:" | ":62M:" | ":64:" | ":65:" => {
                    errors.push(RowError::new(
                        line_no,
                        "tag",
                        format!("balance tag {tag} is not valid in MT942"),
                    ));
                }
                ":20:" | ":25:" | ":28C:" => {
                    if let Some((line, p61)) = pending.take() {
                        let info = std::mem::take(&mut info_buf);
                        match build_txn(&p61, &info, self.dialect) {
                            Ok(t) => txns.push(t),
                            Err(e) => errors.push(RowError::new(line, e.0, e.1)),
                        }
                    }
                }
                ":34F:" | ":13D:" => {
                    // Parsed and discarded.
                }
                ":61:" => {
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
                ":86:" if pending.is_some() => {
                    info_buf.push(content.to_string());
                }
                ":90D:" | ":90C:" => {
                    if let Some((line, p61)) = pending.take() {
                        let info = std::mem::take(&mut info_buf);
                        match build_txn(&p61, &info, self.dialect) {
                            Ok(t) => txns.push(t),
                            Err(e) => errors.push(RowError::new(line, e.0, e.1)),
                        }
                    }
                    match parse_90_totals(content) {
                        Ok((count, minor)) => {
                            if tag == ":90D:" {
                                declared_debit_count = Some(count);
                                declared_debit_minor = Some(minor);
                            } else {
                                declared_credit_count = Some(count);
                                declared_credit_minor = Some(minor);
                            }
                        }
                        Err(e) => errors.push(RowError::new(line_no, e.0, e.1)),
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if let Some((line, p61)) = pending {
            let info = std::mem::take(&mut info_buf);
            match build_txn(&p61, &info, self.dialect) {
                Ok(t) => txns.push(t),
                Err(e) => errors.push(RowError::new(line, e.0, e.1)),
            }
        }

        if let (Some(dc), Some(dm)) = (declared_debit_count, declared_debit_minor) {
            let pc = txns.iter().filter(|t| t.direction == Direction::Debit).count() as i64;
            let pm: i64 = txns
                .iter()
                .filter(|t| t.direction == Direction::Debit)
                .map(|t| t.amount_minor)
                .sum();
            if pc != dc || pm != dm {
                errors.push(RowError::new(
                    0,
                    "totals",
                    format!(
                        ":90D: declared {dc} debits totalling {dm} minor; parsed {pc} totalling {pm}"
                    ),
                ));
            }
        }
        if let (Some(cc), Some(cm)) = (declared_credit_count, declared_credit_minor) {
            let pc = txns.iter().filter(|t| t.direction == Direction::Credit).count() as i64;
            let pm: i64 = txns
                .iter()
                .filter(|t| t.direction == Direction::Credit)
                .map(|t| t.amount_minor)
                .sum();
            if pc != cc || pm != cm {
                errors.push(RowError::new(
                    0,
                    "totals",
                    format!(
                        ":90C: declared {cc} credits totalling {cm} minor; parsed {pc} totalling {pm}"
                    ),
                ));
            }
        }

        if errors.is_empty() {
            Ok(txns)
        } else {
            Err(errors)
        }
    }
}

/// Parse `:90D:` / `:90C:` content like `3EUR1500,00` → (count, minor).
fn parse_90_totals(content: &str) -> Result<(i64, i64), (&'static str, String)> {
    let bytes = content.as_bytes();
    let n = bytes.len();
    let mut idx = 0;
    while idx < n && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == 0 {
        return Err(("totals", "missing count".into()));
    }
    let count: i64 = content[..idx]
        .parse()
        .map_err(|_| ("totals", "bad count".to_string()))?;
    if idx + 3 > n || !bytes[idx..idx + 3].iter().all(|b| b.is_ascii_alphabetic()) {
        return Err(("totals", "missing currency".into()));
    }
    idx += 3;
    let amount_str = content[idx..].replace(',', ".");
    let minor = crate::money::parse_decimal_to_minor(&amount_str).map_err(|e| ("totals", e))?;
    Ok((count, minor))
}

fn build_txn(
    p: &Mt61,
    info_lines: &[String],
    dialect: Mt94xDialect,
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
        Mt94xDialect::Generic => (raw_info, None, None, None),
        Mt94xDialect::Subfielded => {
            let SubfieldedInfo {
                description,
                counterparty,
                counterparty_bic,
                counterparty_account,
            } = parse_subfielded_86(&raw_info);
            (description, counterparty, counterparty_bic, counterparty_account)
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

    fn load(name: &str) -> Vec<u8> {
        std::fs::read(format!("tests/fixtures/{name}")).expect("fixture file")
    }

    #[test]
    fn generic_parses_two_txns_and_passes_sanity_check() {
        let bytes = load("mt942-single-message.sta");
        let txns = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(&bytes)
        .unwrap();
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[0].amount_minor, 25000);
        assert_eq!(txns[1].direction, Direction::Credit);
        assert_eq!(txns[1].amount_minor, 50000);
    }

    #[test]
    fn balance_tag_rejected() {
        let bad = b":20:R\n:25:A\n:28C:1/1\n:60F:C260101EUR100,00\n:61:260101D50,00NTRFREF\n";
        let err = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(bad)
        .unwrap_err();
        assert!(err.iter().any(|e| e.field == "tag"));
    }

    #[test]
    fn declared_totals_mismatch_returns_error() {
        let bad = b":20:R\n:25:A\n:28C:1/1\n:61:260101D250,00NTRFREF\n:90D:2EUR500,00\n";
        let err = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(bad)
        .unwrap_err();
        assert!(err.iter().any(|e| e.field == "totals"));
    }

    #[test]
    fn floor_limit_and_date_time_tags_silently_consumed() {
        let bytes = load("mt942-single-message.sta");
        let txns = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(&bytes)
        .unwrap();
        assert!(txns.iter().all(|t| !t.description.contains("34F")));
        assert!(txns.iter().all(|t| !t.description.contains("13D")));
    }

    #[test]
    fn subfielded_dialect_extracts_counterparty_account_and_bic() {
        let bytes = load("mt942-subfielded.sta");
        let txns = Mt942Parser {
            dialect: Mt94xDialect::Subfielded,
        }
        .parse(&bytes)
        .unwrap();
        assert_eq!(txns.len(), 1);
        let t = &txns[0];
        assert_eq!(t.counterparty_account.as_deref(), Some("DE89370400440532013000"));
        assert_eq!(t.counterparty_bic.as_deref(), Some("DEUTDEFF"));
        assert!(t.description.contains("Intra-day invoice"));
    }

    #[test]
    fn no_external_ref_returns_row_error() {
        let bad = b":20:R\n:25:A\n:28C:1/1\n:61:260101D50,00NTRF//\n:90D:1EUR50,00\n:90C:0EUR0,00\n";
        let err = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(bad)
        .unwrap_err();
        assert!(err.iter().any(|e| e.field == "external_ref"));
    }

    #[test]
    fn empty_file_returns_empty_txn_list() {
        let txns = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(b"")
        .unwrap();
        assert!(txns.is_empty());
    }

    #[test]
    fn bad_dc_mark_returns_row_error() {
        let bad = b":20:R\n:25:A\n:28C:1/1\n:61:260101X50,00NTRFREF\n";
        let err = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(bad)
        .unwrap_err();
        assert!(err.iter().any(|e| e.field == "dc_mark"));
    }
}
