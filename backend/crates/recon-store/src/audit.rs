use crate::{Store, StoreError};
use recon_audit::{chain, AuditEntry, AuditKind, AuditPayload};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

impl Store {
    /// Append an audit event to a tenant's chain INSIDE the caller's transaction.
    /// Fetches the current tail row with `FOR UPDATE`, computes the next hash,
    /// and inserts. If the caller's transaction rolls back (for any reason),
    /// the audit row rolls back with it.
    pub async fn append_audit(
        &self,
        tx: &mut sqlx::PgConnection,
        tenant_id: &str,
        actor_id: &str,
        payload: AuditPayload,
    ) -> Result<AuditEntry, StoreError> {
        // 1. Lock the tail (or genesis).
        let row: Option<(i64, Vec<u8>)> = sqlx::query_as(
            "SELECT seq, hash FROM audit_events WHERE tenant_id=$1 ORDER BY seq DESC LIMIT 1 FOR UPDATE",
        )
        .bind(tenant_id)
        .fetch_optional(&mut *tx)
        .await?;
        let (prev_seq, prev_hash) = match row {
            Some((s, h)) => (s, vec_to_arr32(h)?),
            None => (0, [0u8; 32]),
        };
        let seq = prev_seq + 1;

        // 2. Timestamp + hash.
        // Truncate to microsecond precision so the in-memory timestamp matches what
        // Postgres TIMESTAMPTZ round-trips (microseconds, not nanoseconds). Without
        // this, list_audit reformats the DB-loaded value to a different RFC3339 string
        // than the one that fed compute_hash → verify reports Tampered on roundtrip.
        let now = OffsetDateTime::now_utc();
        let micros = now.microsecond();
        let now = now
            .replace_nanosecond(micros * 1_000)
            .expect("microsecond * 1000 fits in nanosecond range");
        let at = now
            .format(&Rfc3339)
            .map_err(|_| StoreError::Db(sqlx::Error::Decode("rfc3339".into())))?;
        let kind = payload.kind();
        let hash = chain::compute_hash(&prev_hash, seq, tenant_id, &at, actor_id, kind, &payload);

        // 3. Insert. The composite PK rejects a colliding concurrent insert with 23505;
        //    the caller's transaction will be retried at the action layer.
        let payload_json = serde_json::to_value(&payload)?;
        sqlx::query(
            "INSERT INTO audit_events(tenant_id,seq,at,actor_id,kind,payload,prev_hash,hash) \
             VALUES ($1,$2,$3::timestamptz,$4,$5,$6,$7,$8)",
        )
        .bind(tenant_id)
        .bind(seq)
        .bind(&at)
        .bind(actor_id)
        .bind(kind.as_str())
        .bind(&payload_json)
        .bind(prev_hash.as_slice())
        .bind(hash.as_slice())
        .execute(&mut *tx)
        .await?;

        Ok(AuditEntry {
            tenant_id: tenant_id.into(),
            seq,
            at,
            actor_id: actor_id.into(),
            kind,
            payload,
            prev_hash,
            hash,
        })
    }
}

fn vec_to_arr32(v: Vec<u8>) -> Result<[u8; 32], StoreError> {
    let arr: [u8; 32] = v
        .try_into()
        .map_err(|_| StoreError::Db(sqlx::Error::Decode("hash len".into())))?;
    Ok(arr)
}

#[derive(Default, Debug, Clone)]
pub struct AuditFilter {
    pub from: Option<String>,        // ISO 8601 date or datetime
    pub to: Option<String>,
    pub kinds: Vec<AuditKind>,
    pub actor_id: Option<String>,
    pub limit: i64,                  // <= 500
    pub before: Option<i64>,         // cursor: return seq < before
}

#[derive(Debug, Clone)]
pub struct AuditPage {
    pub items: Vec<AuditEntry>,
    pub next_cursor: Option<i64>,
}

impl Store {
    pub async fn list_audit(&self, tenant_id: &str, f: &AuditFilter) -> Result<AuditPage, StoreError> {
        let limit = f.limit.clamp(1, 500);
        let kinds_strs: Vec<String> = f.kinds.iter().map(|k| k.as_str().to_string()).collect();
        // sqlx doesn't support optional ANY() bindings as a single expression; use COALESCE pattern.
        let rows: Vec<(String, i64, time::OffsetDateTime, String, String, serde_json::Value, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT tenant_id, seq, at, actor_id, kind, payload, prev_hash, hash \
             FROM audit_events \
             WHERE tenant_id = $1 \
               AND ($2::timestamptz IS NULL OR at >= $2::timestamptz) \
               AND ($3::timestamptz IS NULL OR at <= $3::timestamptz) \
               AND (cardinality($4::text[]) = 0 OR kind = ANY($4::text[])) \
               AND ($5::text IS NULL OR actor_id = $5) \
               AND ($6::bigint IS NULL OR seq < $6) \
             ORDER BY seq DESC \
             LIMIT $7",
        )
        .bind(tenant_id)
        .bind(f.from.as_deref())
        .bind(f.to.as_deref())
        .bind(&kinds_strs)
        .bind(f.actor_id.as_deref())
        .bind(f.before)
        .bind(limit + 1) // fetch one extra to detect a next page
        .fetch_all(&self.pool)
        .await?;

        let has_more = rows.len() as i64 > limit;
        let items: Vec<AuditEntry> = rows
            .into_iter()
            .take(limit as usize)
            .map(row_to_entry)
            .collect::<Result<_, _>>()?;
        let next_cursor = if has_more { items.last().map(|e| e.seq) } else { None };
        Ok(AuditPage { items, next_cursor })
    }

    /// Load a range in seq order (ascending) and run chain::verify on it.
    pub async fn verify_audit(
        &self,
        tenant_id: &str,
        from_seq: Option<i64>,
        to_seq: Option<i64>,
        expected_prev_hash: Option<[u8; 32]>,
    ) -> Result<recon_audit::chain::VerifyOutcome, StoreError> {
        let rows: Vec<(String, i64, time::OffsetDateTime, String, String, serde_json::Value, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT tenant_id, seq, at, actor_id, kind, payload, prev_hash, hash \
             FROM audit_events \
             WHERE tenant_id = $1 \
               AND ($2::bigint IS NULL OR seq >= $2) \
               AND ($3::bigint IS NULL OR seq <= $3) \
             ORDER BY seq ASC",
        )
        .bind(tenant_id)
        .bind(from_seq)
        .bind(to_seq)
        .fetch_all(&self.pool)
        .await?;
        let entries: Vec<AuditEntry> = rows
            .into_iter()
            .map(row_to_entry)
            .collect::<Result<_, _>>()?;
        let checked = entries.len() as i64;
        match recon_audit::chain::verify(&entries, expected_prev_hash) {
            Ok(()) => Ok(recon_audit::chain::VerifyOutcome::valid(checked)),
            Err(e) => Ok(recon_audit::chain::VerifyOutcome::invalid(checked, e)),
        }
    }
}

fn row_to_entry(
    r: (String, i64, time::OffsetDateTime, String, String, serde_json::Value, Vec<u8>, Vec<u8>),
) -> Result<AuditEntry, StoreError> {
    let (tenant_id, seq, at, actor_id, kind_str, payload, prev_hash, hash) = r;
    let kind = AuditKind::from_str(&kind_str)
        .ok_or_else(|| StoreError::Db(sqlx::Error::Decode(format!("unknown audit kind {kind_str}").into())))?;
    let payload: AuditPayload = serde_json::from_value(payload)?;
    let prev_hash = vec_to_arr32(prev_hash)?;
    let hash = vec_to_arr32(hash)?;
    let at = at
        .format(&Rfc3339)
        .map_err(|_| StoreError::Db(sqlx::Error::Decode("rfc3339".into())))?;
    Ok(AuditEntry { tenant_id, seq, at, actor_id, kind, payload, prev_hash, hash })
}
