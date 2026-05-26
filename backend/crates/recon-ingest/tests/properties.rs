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
    #[test]
    fn direction_tracks_sign(neg in any::<bool>(), units in 0u32..1_000_000, cents in 0u32..100) {
        let sign = if neg { "-" } else { "" };
        let line = format!("R1,2026-05-10,{sign}{units}.{cents:02},Desc\n");
        let txns = CsvParser::new(mapping()).parse(line.as_bytes()).unwrap();
        let amount = units as i64 * 100 + cents as i64;
        if amount == 0 { return Ok(()); } // zero has no meaningful sign
        prop_assert!(txns[0].amount_minor >= 0);
        let expected = if neg { recon_domain::Direction::Debit } else { recon_domain::Direction::Credit };
        prop_assert_eq!(txns[0].direction, expected);
    }
}
