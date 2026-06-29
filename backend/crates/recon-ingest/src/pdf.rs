//! Text-layer PDF bank-statement parsing via per-bank profiles.

use crate::{ParsedTxn, Parser, RowError};

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
    fn parse_lines(&self, _lines: &[String]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Parser;

    #[test]
    fn resolve_known_and_unknown_profiles() {
        assert!(resolve_profile("acmebank").is_some());
        assert!(resolve_profile("nope").is_none());
        assert_eq!(profile_names(), &["acmebank"]);
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
}
