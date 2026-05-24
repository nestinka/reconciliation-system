use crate::rows::{BreakRow, RunRow};
use crate::{Store, StoreError};
use recon_domain::*;
use time::OffsetDateTime;

pub struct DashboardSummary {
    pub match_rate_pct: f64,
    pub open_breaks: i64,
    pub value_at_risk_minor: i64,
    pub currency: String,
    pub sla_adherence_pct: f64,
    pub breaks_by_type: Vec<(BreakType, i64)>,
    pub breaks_by_ageing: Vec<(AgeingBucket, i64)>,
    pub recent_runs: Vec<ReconciliationRun>,
}

fn is_open(s: BreakStatus) -> bool {
    matches!(
        s,
        BreakStatus::Open | BreakStatus::Investigating | BreakStatus::PendingApproval
    )
}

impl Store {
    pub async fn get_dashboard(&self, tenant_id: &str) -> Result<DashboardSummary, StoreError> {
        let now = OffsetDateTime::now_utc();
        let brows: Vec<BreakRow> = sqlx::query_as("SELECT * FROM breaks WHERE tenant_id = $1")
            .bind(tenant_id)
            .fetch_all(&self.pool)
            .await?;
        let breaks: Vec<Break> = brows.into_iter().map(|b| b.into_break(now)).collect();
        let rrows: Vec<RunRow> = sqlx::query_as(
            "SELECT * FROM reconciliation_runs WHERE tenant_id = $1 ORDER BY started_at DESC",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        let runs: Vec<ReconciliationRun> = rrows
            .into_iter()
            .map(ReconciliationRun::try_from)
            .collect::<Result<_, _>>()?;

        let open: Vec<&Break> = breaks.iter().filter(|b| is_open(b.status)).collect();
        let value_at_risk_minor = open.iter().map(|b| b.value_minor).sum();
        let currency = breaks
            .first()
            .map(|b| b.currency.clone())
            .unwrap_or_else(|| "GBP".into());

        let completed: Vec<&ReconciliationRun> = runs
            .iter()
            .filter(|r| r.status == RunStatus::Completed)
            .collect();
        let match_rate_pct = if completed.is_empty() {
            0.0
        } else {
            let avg = completed
                .iter()
                .map(|r| r.stats.match_rate_pct)
                .sum::<f64>()
                / completed.len() as f64;
            (avg * 10.0).round() / 10.0
        };

        let resolved: Vec<&Break> = breaks
            .iter()
            .filter(|b| matches!(b.status, BreakStatus::Resolved | BreakStatus::WrittenOff))
            .collect();
        let sla_adherence_pct = if resolved.is_empty() {
            100.0
        } else {
            let ok = resolved.iter().filter(|b| b.ageing_days <= 7).count();
            ((ok as f64 / resolved.len() as f64) * 1000.0).round() / 10.0
        };

        let breaks_by_type = [
            BreakType::Unmatched,
            BreakType::Partial,
            BreakType::Duplicate,
            BreakType::Break,
        ]
        .into_iter()
        .map(|t| {
            (
                t,
                breaks.iter().filter(|b| b.break_type == t).count() as i64,
            )
        })
        .collect();
        let breaks_by_ageing = [
            AgeingBucket::ZeroToOne,
            AgeingBucket::TwoToSeven,
            AgeingBucket::EightToThirty,
            AgeingBucket::ThirtyPlus,
        ]
        .into_iter()
        .map(|bk| {
            (
                bk,
                open.iter().filter(|b| b.ageing_bucket == bk).count() as i64,
            )
        })
        .collect();

        let recent_runs = completed.into_iter().take(5).cloned().collect();

        Ok(DashboardSummary {
            match_rate_pct,
            open_breaks: open.len() as i64,
            value_at_risk_minor,
            currency,
            sla_adherence_pct,
            breaks_by_type,
            breaks_by_ageing,
            recent_runs,
        })
    }
}
