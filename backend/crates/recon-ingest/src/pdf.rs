//! Text-layer PDF bank-statement parsing via per-bank profiles.

use crate::{money::parse_decimal_to_minor, ParsedTxn, Parser, RowError};
use recon_domain::Direction;

/// Extract the PDF text layer. Maps any reader failure to a single document-level
/// RowError (row 0). A scanned/image-only PDF yields empty/whitespace text, which
/// the caller treats as "no text layer".
pub(crate) fn extract_text(bytes: &[u8]) -> Result<String, Vec<RowError>> {
    pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| vec![RowError::new(0, "document", format!("could not read PDF: {e}"))])
}

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

/// Synthetic columnar layout: `Date  Description  Ref  Amount  Dr/Cr`.
pub struct AcmeBankProfile;

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
            // 1-based position among the non-blank extracted lines (blank lines were
            // already dropped by normalize_lines), used as the row locator in errors.
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

/// Parse one transaction row: `Date  Description  Ref  Amount  Dr/Cr` → ParsedTxn.
/// Returns `(field, message)` on the first failure for the row.
fn parse_acme_row(line: &str) -> Result<ParsedTxn, (&'static str, String)> {
    let fields = split_columns(line);
    if fields.len() != 5 {
        return Err(("row", format!("expected 5 columns, got {}: {line}", fields.len())));
    }
    let (date_s, desc, reff, amount_s, drcr) =
        (fields[0], fields[1], fields[2], fields[3], fields[4]);

    let value_date = parse_uk_date(date_s).map_err(|m| ("date", m))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;
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

    #[test]
    fn resolve_known_and_unknown_profiles() {
        assert!(resolve_profile("acmebank").is_some());
        assert!(resolve_profile("nope").is_none());
        assert_eq!(profile_names(), &["acmebank"]);
        for name in profile_names() {
            assert!(
                resolve_profile(name).is_some(),
                "profile_names lists {name} but resolve_profile returns None",
            );
        }
    }

    #[test]
    fn empty_pdf_rejects_with_document_error() {
        // pdf-extract on non-PDF bytes errors -> document RowError row 0.
        let err = PdfParser { profile: Box::new(AcmeBankProfile) }
            .parse(b"not a pdf")
            .unwrap_err();
        assert_eq!(err[0].row, 0);
        assert_eq!(err[0].field, "document");
    }

    #[test]
    #[ignore = "regenerates the committed PDF fixture"]
    fn generate_acmebank_fixture() {
        use printpdf::*;
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
            Op::SetFont { font: PdfFontHandle::Builtin(BuiltinFont::Courier), size: Pt(11.0) },
            Op::SetLineHeight { lh: Pt(14.0) },
        ];
        for (i, line) in lines.iter().enumerate() {
            if i > 0 { ops.push(Op::AddLineBreak); }
            ops.push(Op::ShowText { items: vec![TextItem::Text(line.to_string())] });
        }
        ops.push(Op::EndTextSection);
        let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
        let mut doc = PdfDocument::new("AcmeBank Statement");
        let bytes = doc.with_pages(vec![page]).save(&PdfSaveOptions::default(), &mut Vec::new());
        std::fs::write("tests/fixtures/pdf-acmebank.pdf", bytes).expect("write fixture");
    }

    #[test]
    fn pdf_extract_roundtrips_acmebank_fixture() {
        let bytes = std::fs::read("tests/fixtures/pdf-acmebank.pdf").expect("fixture file");
        let text = extract_text(&bytes).expect("extract ok");
        assert!(text.contains("CARD PURCHASE TESCO STORES 1234"), "got:\n{text}");
        assert!(text.contains("A1B2C3"), "got:\n{text}");
        assert!(text.contains("FASTER PAYMENT FROM J SMITH"), "got:\n{text}");
    }

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
        // A row missing the ref column collapses to 4 fields -> "row" error
        // (split_columns filters empty cells, so an empty ref is unreachable as "ref").
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
}
