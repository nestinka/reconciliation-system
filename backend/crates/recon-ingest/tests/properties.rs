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
