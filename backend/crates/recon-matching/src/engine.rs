use crate::config::MatchConfig;
use crate::score::score_pair;
use recon_domain::{BreakType, CanonicalTransaction, MatchType, RunStats};

/// A match decision produced by the engine (no DB identity yet).
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionDraft {
    pub match_type: MatchType,
    pub txn_ids: Vec<String>,
    pub score: f64,
}

/// An unmatched transaction that becomes a break (no DB identity yet).
#[derive(Debug, Clone, PartialEq)]
pub struct BreakDraft {
    pub break_type: BreakType,
    pub txn_ids: Vec<String>,
    pub value_minor: i64,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunResult {
    pub decisions: Vec<DecisionDraft>,
    pub breaks: Vec<BreakDraft>,
    pub stats: RunStats,
}

/// Deterministic, replayable reconciliation of source A against source B.
pub fn reconcile(
    a: &[CanonicalTransaction],
    b: &[CanonicalTransaction],
    cfg: &MatchConfig,
) -> RunResult {
    let mut a: Vec<&CanonicalTransaction> = a.iter().collect();
    let mut b: Vec<&CanonicalTransaction> = b.iter().collect();
    a.sort_by(|x, y| x.id.cmp(&y.id));
    b.sort_by(|x, y| x.id.cmp(&y.id));

    let mut decisions: Vec<DecisionDraft> = Vec::new();
    let mut consumed_a = vec![false; a.len()];
    let mut consumed_b = vec![false; b.len()];

    detect_duplicates(&a, &mut consumed_a, &mut decisions);
    detect_duplicates(&b, &mut consumed_b, &mut decisions);

    for (i, ta) in a.iter().enumerate() {
        if consumed_a[i] {
            continue;
        }
        let mut best: Option<(usize, f64)> = None;
        for (j, tb) in b.iter().enumerate() {
            if consumed_b[j] {
                continue;
            }
            let s = score_pair(ta, tb);
            if s >= cfg.fuzzy_threshold && best.is_none_or(|(_, bs)| s > bs) {
                best = Some((j, s));
            }
        }
        if let Some((j, s)) = best {
            consumed_a[i] = true;
            consumed_b[j] = true;
            let exact = (ta.amount_minor - b[j].amount_minor).abs() == 0
                && ta.value_date == b[j].value_date;
            let match_type = if exact && s >= 0.999 {
                MatchType::Matched
            } else {
                MatchType::Partial
            };
            decisions.push(DecisionDraft {
                match_type,
                txn_ids: vec![ta.id.clone(), b[j].id.clone()],
                score: s,
            });
        }
    }

    let mut breaks: Vec<BreakDraft> = Vec::new();
    for (i, ta) in a.iter().enumerate() {
        if !consumed_a[i] {
            breaks.push(BreakDraft {
                break_type: BreakType::Unmatched,
                txn_ids: vec![ta.id.clone()],
                value_minor: ta.amount_minor,
                currency: ta.currency.clone(),
            });
        }
    }
    for (j, tb) in b.iter().enumerate() {
        if !consumed_b[j] {
            breaks.push(BreakDraft {
                break_type: BreakType::Unmatched,
                txn_ids: vec![tb.id.clone()],
                value_minor: tb.amount_minor,
                currency: tb.currency.clone(),
            });
        }
    }
    breaks.sort_by(|x, y| x.txn_ids.cmp(&y.txn_ids));

    let stats = compute_stats(&decisions, &breaks);
    RunResult {
        decisions,
        breaks,
        stats,
    }
}

fn detect_duplicates(
    txns: &[&CanonicalTransaction],
    consumed: &mut [bool],
    out: &mut Vec<DecisionDraft>,
) {
    for i in 0..txns.len() {
        if consumed[i] {
            continue;
        }
        for j in (i + 1)..txns.len() {
            if consumed[j] {
                continue;
            }
            let (x, y) = (txns[i], txns[j]);
            if x.amount_minor == y.amount_minor
                && x.external_ref
                    .split('-')
                    .take(2)
                    .eq(y.external_ref.split('-').take(2))
                && x.value_date == y.value_date
            {
                consumed[i] = true;
                consumed[j] = true;
                out.push(DecisionDraft {
                    match_type: MatchType::Duplicate,
                    txn_ids: vec![x.id.clone(), y.id.clone()],
                    score: 0.99,
                });
                break;
            }
        }
    }
}

fn compute_stats(decisions: &[DecisionDraft], breaks: &[BreakDraft]) -> RunStats {
    let count = |t: MatchType| decisions.iter().filter(|d| d.match_type == t).count() as i64;
    let matched = count(MatchType::Matched);
    let partial = count(MatchType::Partial);
    let duplicate = count(MatchType::Duplicate);
    let unmatched = breaks.len() as i64;
    let denom = (matched + partial + duplicate + unmatched).max(1);
    let value_at_risk_minor = breaks.iter().map(|b| b.value_minor).sum();
    RunStats {
        matched,
        unmatched,
        partial,
        duplicate,
        break_count: breaks.len() as i64,
        match_rate_pct: (matched as f64 / denom as f64 * 1000.0).round() / 10.0,
        value_at_risk_minor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MatchConfig;
    use recon_domain::{BreakType, CanonicalTransaction, Direction, MatchType};

    fn txn(id: &str, src: &str, amt: i64, date: &str, eref: &str) -> CanonicalTransaction {
        CanonicalTransaction {
            id: id.into(),
            tenant_id: "t".into(),
            source_id: src.into(),
            external_ref: eref.into(),
            value_date: date.into(),
            posted_at: format!("{date}T00:00:00Z"),
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
    fn exact_pair_matches_unmatched_breaks() {
        let a = vec![
            txn("a1", "A", 1000, "2026-05-01", "R1"),
            txn("a2", "A", 2000, "2026-05-02", "R2"),
        ];
        let b = vec![txn("b1", "B", 1000, "2026-05-01", "R1")];
        let r = reconcile(&a, &b, &MatchConfig::v1());
        assert_eq!(
            r.decisions
                .iter()
                .filter(|d| d.match_type == MatchType::Matched)
                .count(),
            1
        );
        assert_eq!(r.breaks.len(), 1);
        assert_eq!(r.breaks[0].txn_ids, vec!["a2".to_string()]);
        assert_eq!(r.breaks[0].break_type, BreakType::Unmatched);
    }

    #[test]
    fn within_tolerance_is_partial() {
        let a = vec![txn("a1", "A", 1000, "2026-05-01", "R1")];
        let b = vec![txn("b1", "B", 1300, "2026-05-02", "R9")];
        let r = reconcile(&a, &b, &MatchConfig::v1());
        assert_eq!(r.decisions.len(), 1);
        assert_eq!(r.decisions[0].match_type, MatchType::Partial);
    }

    #[test]
    fn duplicate_within_source_detected() {
        let a = vec![
            txn("a1", "A", 950, "2026-05-10", "D1"),
            txn("a2", "A", 950, "2026-05-10", "D1"),
        ];
        let b: Vec<CanonicalTransaction> = vec![];
        let r = reconcile(&a, &b, &MatchConfig::v1());
        assert!(r
            .decisions
            .iter()
            .any(|d| d.match_type == MatchType::Duplicate));
    }

    #[test]
    fn stats_are_consistent() {
        let a = vec![
            txn("a1", "A", 1000, "2026-05-01", "R1"),
            txn("a2", "A", 5, "2026-05-01", "X"),
        ];
        let b = vec![txn("b1", "B", 1000, "2026-05-01", "R1")];
        let r = reconcile(&a, &b, &MatchConfig::v1());
        assert_eq!(r.stats.matched, 1);
        assert_eq!(r.stats.break_count, r.breaks.len() as i64);
        assert!(r.stats.match_rate_pct >= 0.0 && r.stats.match_rate_pct <= 100.0);
    }
}
