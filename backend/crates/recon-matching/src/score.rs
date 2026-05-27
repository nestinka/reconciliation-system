use recon_domain::CanonicalTransaction;

/// Parse "YYYY-MM-DD" to a day number (proleptic) for stable, timezone-free diffs.
fn day_number(value_date: &str) -> i64 {
    let mut parts = value_date.split('-');
    let y: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let m: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let d: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    // Howard Hinnant's days-from-civil algorithm.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Similarity in [0,1] for two transactions. Hard gate on direction + currency.
pub fn score_pair(a: &CanonicalTransaction, b: &CanonicalTransaction) -> f64 {
    if a.direction != b.direction || a.currency != b.currency {
        return 0.0;
    }
    let amt_a = a.amount_minor.max(1) as f64;
    let amt_diff = (a.amount_minor - b.amount_minor).abs() as f64;
    let amount_score = (1.0 - amt_diff / amt_a).clamp(0.0, 1.0);

    let date_diff = (day_number(&a.value_date) - day_number(&b.value_date)).abs() as f64;
    let date_score = (1.0 - date_diff / 30.0).clamp(0.0, 1.0);

    let ref_score = if a.external_ref == b.external_ref {
        1.0
    } else {
        0.0
    };

    let raw = 0.6 * amount_score + 0.3 * date_score + 0.1 * ref_score;
    raw.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use recon_domain::{CanonicalTransaction, Direction};

    fn txn(id: &str, amt: i64, date: &str, dir: Direction, cur: &str) -> CanonicalTransaction {
        CanonicalTransaction {
            id: id.into(),
            tenant_id: "t".into(),
            source_id: "s".into(),
            external_ref: id.into(),
            value_date: date.into(),
            posted_at: format!("{date}T00:00:00Z"),
            amount_minor: amt,
            currency: cur.into(),
            direction: dir,
            counterparty: None,
            description: "d".into(),
            counterparty_bic: None,
            counterparty_account: None,
        }
    }

    #[test]
    fn identical_amount_and_date_scores_one() {
        // Both transactions share external_ref "R1" so the ref component
        // contributes its full 0.1, making amount_score=1 + date_score=1 + ref_score=1 → 1.0.
        let a = CanonicalTransaction {
            id: "a".into(),
            tenant_id: "t".into(),
            source_id: "s".into(),
            external_ref: "R1".into(),
            value_date: "2026-05-01".into(),
            posted_at: "2026-05-01T00:00:00Z".into(),
            amount_minor: 1000,
            currency: "GBP".into(),
            direction: Direction::Debit,
            counterparty: None,
            description: "d".into(),
            counterparty_bic: None,
            counterparty_account: None,
        };
        let b = CanonicalTransaction {
            id: "b".into(),
            tenant_id: "t".into(),
            source_id: "s".into(),
            external_ref: "R1".into(),
            value_date: "2026-05-01".into(),
            posted_at: "2026-05-01T00:00:00Z".into(),
            amount_minor: 1000,
            currency: "GBP".into(),
            direction: Direction::Debit,
            counterparty: None,
            description: "d".into(),
            counterparty_bic: None,
            counterparty_account: None,
        };
        assert!((score_pair(&a, &b) - 1.0).abs() < 1e-9);
    }
    #[test]
    fn opposite_direction_or_currency_scores_zero() {
        let a = txn("a", 1000, "2026-05-01", Direction::Debit, "GBP");
        let b = txn("b", 1000, "2026-05-01", Direction::Credit, "GBP");
        assert_eq!(score_pair(&a, &b), 0.0);
        let c = txn("c", 1000, "2026-05-01", Direction::Debit, "USD");
        assert_eq!(score_pair(&a, &c), 0.0);
    }
    #[test]
    fn score_is_always_in_unit_interval() {
        let a = txn("a", 1000, "2026-05-01", Direction::Debit, "GBP");
        let b = txn("b", 950, "2026-05-09", Direction::Debit, "GBP");
        let s = score_pair(&a, &b);
        assert!((0.0..=1.0).contains(&s), "score {s} out of range");
    }
}
