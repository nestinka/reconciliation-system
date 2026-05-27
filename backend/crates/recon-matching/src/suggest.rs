use crate::score::score_pair;
use recon_domain::CanonicalTransaction;

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub txn_ids: Vec<String>,
    pub score: f64,
    pub rationale: String,
}

/// Near-miss candidates for a break's transactions, sorted by descending score.
/// Used by the case screen. Deterministic: ties broken by candidate id.
pub fn suggestions_for(
    break_txns: &[CanonicalTransaction],
    candidates: &[CanonicalTransaction],
    min_score: f64,
) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    for bt in break_txns {
        for c in candidates {
            if c.id == bt.id {
                continue;
            }
            let s = score_pair(bt, c);
            if s >= min_score {
                out.push(Suggestion {
                    txn_ids: vec![bt.id.clone(), c.id.clone()],
                    score: (s * 100.0).round() / 100.0,
                    rationale: format!(
                        "Amount/date similarity {:.0}% under config tolerance.",
                        s * 100.0
                    ),
                });
            }
        }
    }
    out.sort_by(|x, y| {
        y.score
            .partial_cmp(&x.score)
            .unwrap()
            .then(x.txn_ids.cmp(&y.txn_ids))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use recon_domain::{CanonicalTransaction, Direction};
    fn txn(id: &str, amt: i64) -> CanonicalTransaction {
        CanonicalTransaction {
            id: id.into(),
            tenant_id: "t".into(),
            source_id: "s".into(),
            external_ref: id.into(),
            value_date: "2026-05-01".into(),
            posted_at: "2026-05-01T00:00:00Z".into(),
            amount_minor: amt,
            currency: "GBP".into(),
            direction: Direction::Debit,
            counterparty: None,
            description: "d".into(),
            counterparty_bic: None,
            counterparty_account: None,
        }
    }
    #[test]
    fn returns_sorted_candidates() {
        let brk = vec![txn("brk", 1000)];
        let cands = vec![txn("c1", 990), txn("c2", 500)];
        let s = suggestions_for(&brk, &cands, 0.5);
        assert!(s[0].score >= s.last().unwrap().score);
        assert_eq!(s[0].txn_ids[1], "c1"); // closest amount first
    }
}
