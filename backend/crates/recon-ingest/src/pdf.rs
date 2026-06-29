//! Text-layer PDF bank-statement parsing via per-bank profiles.

use crate::RowError;

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
