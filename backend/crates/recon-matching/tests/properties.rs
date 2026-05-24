use proptest::prelude::*;
use recon_domain::{CanonicalTransaction, Direction};
use recon_matching::{reconcile, MatchConfig};

fn arb_txns(prefix: &'static str) -> impl Strategy<Value = Vec<CanonicalTransaction>> {
    prop::collection::vec((1i64..1_000_000i64, 1u32..28u32, any::<bool>()), 0..12).prop_map(
        move |rows| {
            rows.into_iter()
                .enumerate()
                .map(|(i, (amt, day, debit))| CanonicalTransaction {
                    id: format!("{prefix}-{i}"),
                    tenant_id: "t".into(),
                    source_id: prefix.into(),
                    external_ref: format!("{prefix}-R{i}"),
                    value_date: format!("2026-05-{day:02}"),
                    posted_at: format!("2026-05-{day:02}T00:00:00Z"),
                    amount_minor: amt,
                    currency: "GBP".into(),
                    direction: if debit {
                        Direction::Debit
                    } else {
                        Direction::Credit
                    },
                    counterparty: None,
                    description: "d".into(),
                })
                .collect()
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn deterministic_and_replayable(a in arb_txns("a"), b in arb_txns("b")) {
        let cfg = MatchConfig::v1();
        let r1 = reconcile(&a, &b, &cfg);
        let r2 = reconcile(&a, &b, &cfg);
        prop_assert_eq!(r1, r2);
    }

    #[test]
    fn no_txn_used_twice_and_conservation(a in arb_txns("a"), b in arb_txns("b")) {
        let r = reconcile(&a, &b, &MatchConfig::v1());
        let mut seen = std::collections::HashSet::new();
        for d in &r.decisions { for id in &d.txn_ids { prop_assert!(seen.insert(id.clone()), "double-used {}", id); } }
        for bk in &r.breaks { for id in &bk.txn_ids { prop_assert!(seen.insert(id.clone()), "double-used {}", id); } }
        let total_ids: std::collections::HashSet<String> =
            a.iter().chain(b.iter()).map(|t| t.id.clone()).collect();
        prop_assert_eq!(seen, total_ids);
    }

    #[test]
    fn scores_in_unit_interval(a in arb_txns("a"), b in arb_txns("b")) {
        let r = reconcile(&a, &b, &MatchConfig::v1());
        for d in &r.decisions { prop_assert!((0.0..=1.0).contains(&d.score)); }
    }
}
