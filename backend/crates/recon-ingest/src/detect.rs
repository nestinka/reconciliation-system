//! Best-effort format sniffing for `format=auto` uploads. Never returns CSV
//! (no reliable signature; CSV needs an explicit column mapping).

/// Sniff the leading bytes to pick a parser format. `None` = could not detect
/// (e.g. CSV or unknown) — the caller must reject with guidance.
pub fn detect_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"%PDF") {
        return Some("pdf");
    }
    // First non-whitespace / non-BOM byte.
    let trimmed = {
        let mut b = bytes;
        if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
            b = &b[3..];
        }
        let start = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(b.len());
        &b[start..]
    };
    if trimmed.first() == Some(&b'<') {
        return Some("camt053");
    }
    if trimmed.starts_with(b"01,") || trimmed.starts_with(b"02,") {
        return Some("bai2");
    }
    // MT94x: must contain the mandatory :20: transaction-reference tag.
    let text = String::from_utf8_lossy(bytes);
    if text.contains(":20:") {
        // MT942-only markers: floor-limit (:34F:) or debit/credit totals (:90D:/:90C:).
        if text.contains(":34F:") || text.contains(":90D:") || text.contains(":90C:") {
            return Some("mt942");
        }
        return Some("mt940");
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_pdf() {
        assert_eq!(detect_format(b"%PDF-1.7\n..."), Some("pdf"));
    }
    #[test]
    fn detects_camt_xml() {
        assert_eq!(detect_format(b"<?xml version=\"1.0\"?><Document>"), Some("camt053"));
        assert_eq!(detect_format(b"  \n<Document>"), Some("camt053"));
    }
    #[test]
    fn detects_bai2() {
        assert_eq!(detect_format(b"01,BANK,RECIPIENT,260501,..."), Some("bai2"));
        assert_eq!(detect_format(b"02,..."), Some("bai2"));
    }
    #[test]
    fn detects_mt942_by_totals_tag() {
        let s = b":20:REF\r\n:34F:GBP0,\r\n:90D:3EUR100,\r\n";
        assert_eq!(detect_format(s), Some("mt942"));
    }
    #[test]
    fn detects_mt940_when_no_mt942_tags() {
        let s = b":20:REF\r\n:60F:C260501GBP0,\r\n:62F:C260501GBP0,\r\n";
        assert_eq!(detect_format(s), Some("mt940"));
    }
    #[test]
    fn returns_none_for_csv_or_garbage() {
        assert_eq!(detect_format(b"ref,date,amount,desc\nA1,2026-05-01,10.00,x"), None);
        assert_eq!(detect_format(b"\x00\x01\x02 random"), None);
        assert_eq!(detect_format(b""), None);
    }
}
