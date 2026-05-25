use crate::rows::SourceRow;
use crate::{Store, StoreError};
use recon_domain::{Source, SourceKind};
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
