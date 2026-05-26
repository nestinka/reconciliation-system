use crate::{Store, StoreError};
use recon_audit::{chain, AuditEntry, AuditPayload};
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
        let now = OffsetDateTime::now_utc();
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
