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
        actor_id: &str,
        format_dialect: Option<&str>,
    ) -> Result<Source, StoreError> {
        let id = format!("src-{}", Uuid::new_v4());
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency,format_dialect) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(&id)
            .bind(tenant_id)
            .bind(kind_str(kind))
            .bind(name)
            .bind(currency)
            .bind(format_dialect)
            .execute(&mut *tx)
            .await?;
        self.append_audit(
            &mut tx,
            tenant_id,
            actor_id,
            recon_audit::AuditPayload::DataSourceCreated {
                source_id: id.clone(),
                kind: kind_str(kind).to_string(),
                currency: currency.to_string(),
                name: name.to_string(),
            },
        )
        .await?;
        tx.commit().await?;
        Ok(Source {
            id,
            tenant_id: tenant_id.to_string(),
            kind,
            name: name.to_string(),
            currency: currency.to_string(),
            format_dialect: format_dialect.map(|s| s.to_string()),
        })
    }

    pub async fn get_source(&self, tenant_id: &str, id: &str) -> Result<Source, StoreError> {
        let row: Option<SourceRow> =
            sqlx::query_as("SELECT id,tenant_id,kind,name,currency,format_dialect FROM sources WHERE id=$1 AND tenant_id=$2")
                .bind(id)
                .bind(tenant_id)
                .fetch_optional(&self.pool)
                .await?;
        row.map(Into::into).ok_or(StoreError::NotFound)
    }

    pub async fn list_sources(&self, tenant_id: &str) -> Result<Vec<SourceListItem>, StoreError> {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: String,
            tenant_id: String,
            kind: String,
            name: String,
            currency: String,
            format_dialect: Option<String>,
            txn_count: i64,
        }
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT s.id, s.tenant_id, s.kind, s.name, s.currency, s.format_dialect, \
                    COUNT(t.id) AS txn_count \
             FROM sources s \
             LEFT JOIN canonical_transactions t ON t.source_id = s.id AND t.tenant_id = s.tenant_id \
             WHERE s.tenant_id = $1 \
             GROUP BY s.id, s.tenant_id, s.kind, s.name, s.currency, s.format_dialect \
             ORDER BY s.name",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| SourceListItem {
                source: recon_domain::Source {
                    id: r.id,
                    tenant_id: r.tenant_id,
                    kind: match r.kind.as_str() {
                        "bank" => SourceKind::Bank,
                        "ledger" => SourceKind::Ledger,
                        _ => SourceKind::CrossSystem,
                    },
                    name: r.name,
                    currency: r.currency,
                    format_dialect: r.format_dialect,
                },
                txn_count: r.txn_count,
            })
            .collect())
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
    #[allow(clippy::too_many_arguments)]
    pub async fn ingest_transactions(
        &self,
        tenant_id: &str,
        source_id: &str,
        txns: &[CanonicalTransaction],
        actor_id: &str,
        file_sha256: &str,
        file_format: &str,
        file_bytes: i64,
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
                "INSERT INTO canonical_transactions(id,tenant_id,source_id,external_ref,value_date,posted_at,amount_minor,currency,direction,counterparty,description,counterparty_bic,counterparty_account) \
                 VALUES ($1,$2,$3,$4,$5::date,$6::timestamptz,$7,$8,$9,$10,$11,$12,$13)",
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
            .bind(&t.counterparty_bic)
            .bind(&t.counterparty_account)
            .execute(&mut *tx)
            .await
            .map_err(|e| match e {
                sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
                    StoreError::DuplicateRefs(vec![t.external_ref.clone()])
                }
                other => StoreError::Db(other),
            })?;
        }
        self.append_audit(
            &mut tx,
            tenant_id,
            actor_id,
            recon_audit::AuditPayload::DataIngestCompleted {
                source_id: source_id.to_string(),
                format: file_format.to_string(),
                file_sha256: file_sha256.to_string(),
                bytes: file_bytes,
                ingested: txns.len() as i64,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(txns.len())
    }
}
