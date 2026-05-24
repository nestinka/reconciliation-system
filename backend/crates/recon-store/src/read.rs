use crate::rows::*;
use crate::{Store, StoreError};
use recon_domain::*;
use time::OffsetDateTime;

#[derive(Default)]
pub struct RunFilter {
    pub status: Option<String>,
    pub source_id: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}
#[derive(Default)]
pub struct BreakFilter {
    pub status: Option<String>,
    pub kind: Option<String>,
    pub ageing_bucket: Option<String>,
    pub assignee_id: Option<String>,
}

pub struct RunDetail {
    pub run: ReconciliationRun,
    pub transactions: Vec<CanonicalTransaction>,
    pub matched: Vec<MatchDecision>,
    pub partial: Vec<MatchDecision>,
    pub duplicates: Vec<MatchDecision>,
    pub unmatched: Vec<Break>,
}
pub struct CaseBundle {
    pub case: Case,
    pub brk: Break,
    pub suggestions: Vec<(Vec<String>, f64, String)>,
    pub transactions: Vec<CanonicalTransaction>,
}

impl Store {
    pub async fn list_tenants(&self) -> Result<Vec<Tenant>, StoreError> {
        let rows: Vec<TenantRow> =
            sqlx::query_as("SELECT id, name, slug FROM tenants ORDER BY name")
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn list_runs(
        &self,
        tenant_id: &str,
        f: &RunFilter,
    ) -> Result<Vec<ReconciliationRun>, StoreError> {
        let rows: Vec<RunRow> = sqlx::query_as(
            "SELECT * FROM reconciliation_runs
             WHERE tenant_id = $1
               AND ($2::text IS NULL OR status = $2)
               AND ($3::text IS NULL OR source_a_id = $3 OR source_b_id = $3)
               AND ($4::text IS NULL OR started_at >= $4::timestamptz)
               AND ($5::text IS NULL OR started_at <= $5::timestamptz)
             ORDER BY started_at DESC",
        )
        .bind(tenant_id)
        .bind(&f.status)
        .bind(&f.source_id)
        .bind(&f.from)
        .bind(&f.to)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|r| ReconciliationRun::try_from(r).map_err(StoreError::from))
            .collect()
    }

    pub async fn get_run(&self, tenant_id: &str, run_id: &str) -> Result<RunDetail, StoreError> {
        let now = OffsetDateTime::now_utc();
        let run_row: Option<RunRow> =
            sqlx::query_as("SELECT * FROM reconciliation_runs WHERE id = $1 AND tenant_id = $2")
                .bind(run_id)
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?;
        let run = ReconciliationRun::try_from(run_row.ok_or(StoreError::NotFound)?)?;

        let drows: Vec<DecisionRow> = sqlx::query_as("SELECT id, run_id, type, txn_ids, score, config_version FROM match_decisions WHERE run_id = $1 AND tenant_id = $2 ORDER BY id")
            .bind(run_id).bind(tenant_id).fetch_all(&self.pool).await?;
        let decisions: Vec<MatchDecision> = drows.into_iter().map(Into::into).collect();

        let brows: Vec<BreakRow> =
            sqlx::query_as("SELECT * FROM breaks WHERE run_id = $1 AND tenant_id = $2 ORDER BY id")
                .bind(run_id)
                .bind(tenant_id)
                .fetch_all(&self.pool)
                .await?;
        let unmatched: Vec<Break> = brows.into_iter().map(|b| b.into_break(now)).collect();

        let mut ids: Vec<String> = decisions
            .iter()
            .flat_map(|d| d.txn_ids.clone())
            .chain(unmatched.iter().flat_map(|b| b.txn_ids.clone()))
            .collect();
        ids.sort();
        ids.dedup();
        let transactions = self.txns_by_ids(tenant_id, &ids).await?;

        let by = |t: MatchType| {
            decisions
                .iter()
                .filter(|d| d.match_type == t)
                .cloned()
                .collect::<Vec<_>>()
        };
        Ok(RunDetail {
            run,
            transactions,
            matched: by(MatchType::Matched),
            partial: by(MatchType::Partial),
            duplicates: by(MatchType::Duplicate),
            unmatched,
        })
    }

    pub async fn list_breaks(
        &self,
        tenant_id: &str,
        f: &BreakFilter,
    ) -> Result<Vec<Break>, StoreError> {
        let now = OffsetDateTime::now_utc();
        let rows: Vec<BreakRow> = sqlx::query_as(
            "SELECT * FROM breaks
             WHERE tenant_id = $1
               AND ($2::text IS NULL OR status = $2)
               AND ($3::text IS NULL OR type = $3)
               AND ($4::text IS NULL OR assignee_id = $4)
             ORDER BY opened_at DESC",
        )
        .bind(tenant_id)
        .bind(&f.status)
        .bind(&f.kind)
        .bind(&f.assignee_id)
        .fetch_all(&self.pool)
        .await?;
        let mut breaks: Vec<Break> = rows.into_iter().map(|b| b.into_break(now)).collect();
        if let Some(bucket) = &f.ageing_bucket {
            breaks.retain(|b| {
                serde_json::to_value(b.ageing_bucket)
                    .ok()
                    .and_then(|v| v.as_str().map(|s| s == bucket))
                    .unwrap_or(false)
            });
        }
        Ok(breaks)
    }

    pub async fn txns_by_ids(
        &self,
        tenant_id: &str,
        ids: &[String],
    ) -> Result<Vec<CanonicalTransaction>, StoreError> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let rows: Vec<TxnRow> = sqlx::query_as("SELECT * FROM canonical_transactions WHERE tenant_id = $1 AND id = ANY($2) ORDER BY id")
            .bind(tenant_id).bind(ids).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn load_case(&self, tenant_id: &str, case_id: &str) -> Result<Case, StoreError> {
        let crow: Option<CaseRow> = sqlx::query_as(
            "SELECT id, break_id, assignee_id, status FROM cases WHERE id = $1 AND tenant_id = $2",
        )
        .bind(case_id)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await?;
        let crow = crow.ok_or(StoreError::NotFound)?;
        let erows: Vec<EventRow> = sqlx::query_as("SELECT id, actor_id, at, kind, payload FROM case_events WHERE case_id = $1 AND tenant_id = $2 ORDER BY seq")
            .bind(case_id).bind(tenant_id).fetch_all(&self.pool).await?;
        let events: Vec<CaseEvent> = erows
            .into_iter()
            .map(CaseEvent::try_from)
            .collect::<Result<_, _>>()?;
        Ok(Case {
            id: crow.id,
            break_id: crow.break_id,
            assignee_id: crow.assignee_id,
            status: crate::rows::parse_break_status(&crow.status),
            events,
        })
    }

    pub async fn get_case(&self, tenant_id: &str, case_id: &str) -> Result<CaseBundle, StoreError> {
        let now = OffsetDateTime::now_utc();
        let case = self.load_case(tenant_id, case_id).await?;
        let brow: Option<BreakRow> =
            sqlx::query_as("SELECT * FROM breaks WHERE case_id = $1 AND tenant_id = $2")
                .bind(case_id)
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?;
        let brk = brow.ok_or(StoreError::NotFound)?.into_break(now);

        let brk_txns = self.txns_by_ids(tenant_id, &brk.txn_ids).await?;
        let all: Vec<TxnRow> =
            sqlx::query_as("SELECT * FROM canonical_transactions WHERE tenant_id = $1 ORDER BY id")
                .bind(tenant_id)
                .fetch_all(&self.pool)
                .await?;
        let candidates: Vec<CanonicalTransaction> = all.into_iter().map(Into::into).collect();
        let sugg = recon_matching::suggestions_for(&brk_txns, &candidates, 0.55);
        let suggestions: Vec<(Vec<String>, f64, String)> = sugg
            .into_iter()
            .take(3)
            .map(|s| (s.txn_ids, s.score, s.rationale))
            .collect();

        let mut ids: Vec<String> = brk.txn_ids.clone();
        for (tids, _, _) in &suggestions {
            ids.extend(tids.clone());
        }
        ids.sort();
        ids.dedup();
        let transactions = self.txns_by_ids(tenant_id, &ids).await?;
        Ok(CaseBundle {
            case,
            brk,
            suggestions,
            transactions,
        })
    }
}
