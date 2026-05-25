use crate::rows::SourceRow;
use crate::{Store, StoreError};
use recon_domain::{CanonicalTransaction, Direction, Source, SourceKind};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceListItem {
    #[serde(flatten)]
    pub source: Source,
    pub txn_count: i64,
}

fn kind_str(k: SourceKind) -> &'static str {
    match k {
        SourceKind::Bank => "bank",
        SourceKind::Ledger => "ledger",
        SourceKind::CrossSystem => "cross_system",
    }
}

impl Store {
    pub async fn create_source(
        &self,
        tenant_id: &str,
        kind: SourceKind,
        name: &str,
        currency: &str,
    ) -> Result<Source, StoreError> {
        let id = format!("src-{}", Uuid::new_v4());
        sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ($1,$2,$3,$4,$5)")
            .bind(&id)
            .bind(tenant_id)
            .bind(kind_str(kind))
            .bind(name)
            .bind(currency)
            .execute(&self.pool)
            .await?;
        Ok(Source { id, tenant_id: tenant_id.to_string(), kind, name: name.to_string(), currency: currency.to_string() })
    }

    pub async fn get_source(&self, tenant_id: &str, id: &str) -> Result<Source, StoreError> {
        let row: Option<SourceRow> =
            sqlx::query_as("SELECT id,tenant_id,kind,name,currency FROM sources WHERE id=$1 AND tenant_id=$2")
                .bind(id)
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?;
        row.map(Into::into).ok_or(StoreError::NotFound)
    }

    pub async fn list_sources(&self, tenant_id: &str) -> Result<Vec<SourceListItem>, StoreError> {
        let rows: Vec<SourceRow> =
            sqlx::query_as("SELECT id,tenant_id,kind,name,currency FROM sources WHERE tenant_id=$1 ORDER BY name")
                .bind(tenant_id)
                .fetch_all(&self.pool)
                .await?;
        let mut out = Vec::with_capacity(rows.len());
        for r in rows {
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM canonical_transactions WHERE source_id=$1",
            )
            .bind(&r.id)
            .fetch_one(&self.pool)
            .await?;
            out.push(SourceListItem { source: r.into(), txn_count: count });
        }
        Ok(out)
    }
}

fn direction_str(d: Direction) -> &'static str {
    match d {
        Direction::Debit => "debit",
        Direction::Credit => "credit",
    }
}

impl Store {
    /// Insert fully-formed transactions into a source, atomically. Rejects the
    /// whole batch (storing nothing) if any external_ref is duplicated within
    /// the batch or already present in the source.
    pub async fn ingest_transactions(
        &self,
        tenant_id: &str,
        source_id: &str,
        txns: &[CanonicalTransaction],
    ) -> Result<usize, StoreError> {
        // Source must belong to the caller's tenant.
        self.get_source(tenant_id, source_id).await?;

        // Within-batch duplicates.
        let mut seen = std::collections::HashSet::new();
        let mut dups: Vec<String> = Vec::new();
        for t in txns {
            if !seen.insert(t.external_ref.as_str()) {
                dups.push(t.external_ref.clone());
            }
        }
        if !dups.is_empty() {
            dups.sort();
            dups.dedup();
            return Err(StoreError::DuplicateRefs(dups));
        }

        // Already-present refs.
        let refs: Vec<String> = txns.iter().map(|t| t.external_ref.clone()).collect();
        let existing: Vec<String> = sqlx::query_scalar(
            "SELECT external_ref FROM canonical_transactions WHERE source_id=$1 AND external_ref = ANY($2)",
        )
        .bind(source_id)
        .bind(&refs)
        .fetch_all(&self.pool)
        .await?;
        if !existing.is_empty() {
            return Err(StoreError::DuplicateRefs(existing));
        }

        let mut tx = self.pool.begin().await?;
        for t in txns {
            sqlx::query(
                "INSERT INTO canonical_transactions(id,tenant_id,source_id,external_ref,value_date,posted_at,amount_minor,currency,direction,counterparty,description) \
                 VALUES ($1,$2,$3,$4,$5::date,$6::timestamptz,$7,$8,$9,$10,$11)",
            )
            .bind(&t.id)
            .bind(tenant_id)
            .bind(source_id)
            .bind(&t.external_ref)
            .bind(&t.value_date)
            .bind(&t.posted_at)
            .bind(t.amount_minor)
            .bind(&t.currency)
            .bind(direction_str(t.direction))
            .bind(&t.counterparty)
            .bind(&t.description)
            .execute(&mut *tx)
            .await
            .map_err(|e| match e {
                sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
                    StoreError::DuplicateRefs(vec![t.external_ref.clone()])
                }
                other => StoreError::Db(other),
            })?;
        }
        tx.commit().await?;
        Ok(txns.len())
    }
}
