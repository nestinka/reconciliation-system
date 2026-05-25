use crate::{Store, StoreError};
use recon_domain::{ReconciliationRun, RunStatus};
use recon_matching::{reconcile, MatchConfig, RunResult};
use time::OffsetDateTime;
use uuid::Uuid;

impl Store {
    /// Generic writer: run header + decisions + breaks + cases (status `open`,
    /// no assignee, no events). Used by `create_run`. The seed has its own
    /// specialized writer for demo fixtures.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn persist_run(
        &self,
        tx: &mut sqlx::PgConnection,
        run_id: &str,
        tenant_id: &str,
        name: &str,
        sa: &str,
        sb: &str,
        started: &str,
        result: &RunResult,
        cfg: &MatchConfig,
    ) -> Result<(), StoreError> {
        let stats = serde_json::to_value(&result.stats)?;
        sqlx::query("INSERT INTO reconciliation_runs(id,tenant_id,name,source_a_id,source_b_id,status,started_at,completed_at,config_version,stats) VALUES ($1,$2,$3,$4,$5,'completed',$6::timestamptz,$6::timestamptz,$7,$8)")
            .bind(run_id).bind(tenant_id).bind(name).bind(sa).bind(sb).bind(started).bind(&cfg.version).bind(&stats)
            .execute(&mut *tx).await?;

        for (i, d) in result.decisions.iter().enumerate() {
            let type_str = serde_json::to_value(d.match_type)?.as_str().unwrap().to_string();
            sqlx::query("INSERT INTO match_decisions(id,tenant_id,run_id,type,txn_ids,score,config_version) VALUES ($1,$2,$3,$4,$5,$6,$7)")
                .bind(format!("md-{run_id}-{i}")).bind(tenant_id).bind(run_id).bind(type_str).bind(&d.txn_ids).bind(d.score).bind(&cfg.version)
                .execute(&mut *tx).await?;
        }

        for (i, bd) in result.breaks.iter().enumerate() {
            let case_id = format!("case-{run_id}-{i}");
            let break_id = format!("break-{run_id}-{i}");
            let type_str = serde_json::to_value(bd.break_type)?.as_str().unwrap().to_string();
            sqlx::query("INSERT INTO cases(id,tenant_id,break_id,assignee_id,status) VALUES ($1,$2,$3,NULL,'open')")
                .bind(&case_id).bind(tenant_id).bind(&break_id)
                .execute(&mut *tx).await?;
            sqlx::query("INSERT INTO breaks(id,tenant_id,run_id,case_id,type,status,value_minor,currency,assignee_id,txn_ids,opened_at) VALUES ($1,$2,$3,$4,$5,'open',$6,$7,NULL,$8,$9::timestamptz)")
                .bind(&break_id).bind(tenant_id).bind(run_id).bind(&case_id).bind(type_str).bind(bd.value_minor).bind(&bd.currency).bind(&bd.txn_ids).bind(started)
                .execute(&mut *tx).await?;
        }
        Ok(())
    }

    /// Create a run reconciling two sources over a date window. Loads both
    /// windows, runs the matching engine, and persists everything atomically.
    pub async fn create_run(
        &self,
        tenant_id: &str,
        name: &str,
        source_a_id: &str,
        source_b_id: &str,
        from: &str,
        to: &str,
    ) -> Result<ReconciliationRun, StoreError> {
        // Both sources must belong to the caller's tenant.
        self.get_source(tenant_id, source_a_id).await?;
        self.get_source(tenant_id, source_b_id).await?;

        let cfg = MatchConfig::v1();
        let run_id = format!("run-{}", Uuid::new_v4());
        let started = OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let mut tx = self.pool.begin().await?;
        let a = self.load_window(&mut tx, tenant_id, source_a_id, from, to).await?;
        let b = self.load_window(&mut tx, tenant_id, source_b_id, from, to).await?;
        let result = reconcile(&a, &b, &cfg);
        self.persist_run(&mut tx, &run_id, tenant_id, name, source_a_id, source_b_id, &started, &result, &cfg)
            .await?;
        tx.commit().await?;

        Ok(ReconciliationRun {
            id: run_id,
            tenant_id: tenant_id.to_string(),
            name: name.to_string(),
            source_a_id: source_a_id.to_string(),
            source_b_id: source_b_id.to_string(),
            status: RunStatus::Completed,
            started_at: started.clone(),
            completed_at: Some(started),
            config_version: cfg.version,
            stats: result.stats,
        })
    }
}
