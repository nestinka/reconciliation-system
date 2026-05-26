use crate::{Store, StoreError};
use recon_audit::{chain, AuditEntry, AuditKind, AuditPayload};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
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
        //    in practice the FOR UPDATE above serializes appenders within a tenant, so
        //    the only realistic collision window is two concurrent genesis inserts on
        //    a brand-new tenant. Callers surface the 23505 as a 500 and the client
        //    retries the whole request — there is no in-store retry loop.
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

type AuditEventRow = (
    String,
    i64,
    time::OffsetDateTime,
    String,
    String,
    serde_json::Value,
    Vec<u8>,
    Vec<u8>,
);

impl Store {
    pub async fn list_audit(&self, tenant_id: &str, f: &AuditFilter) -> Result<AuditPage, StoreError> {
        let limit = f.limit.clamp(1, 500);
        let kinds_strs: Vec<String> = f.kinds.iter().map(|k| k.as_str().to_string()).collect();
        // sqlx doesn't support optional ANY() bindings as a single expression; use COALESCE pattern.
        let rows: Vec<AuditEventRow> = sqlx::query_as(
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
        let rows: Vec<AuditEventRow> = sqlx::query_as(
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
        // First: walk the chain itself. If it's already broken, surface that first.
        if let Err(e) = recon_audit::chain::verify(&entries, expected_prev_hash) {
            return Ok(recon_audit::chain::VerifyOutcome::invalid(checked, e));
        }
        // Then: cross-check against the latest anchor. An anchor locks in this
        // tenant's head as of anchor-time; if rows after the anchored seq have
        // been deleted wholesale (and the anchor row preserved or also deleted),
        // a chain-only verify would still pass. The anchor cross-check catches
        // the wholesale-deletion case the chain alone can't detect.
        //
        // We only cross-check when this is a full-tenant verify (no from/to
        // restriction) — bounded verifies are explicitly asking about a window
        // and the anchor's head may legitimately be outside that window.
        if from_seq.is_none() && to_seq.is_none() {
            if let Some(anchor_err) = self.cross_check_anchor(tenant_id, &entries).await? {
                return Ok(recon_audit::chain::VerifyOutcome::invalid(checked, anchor_err));
            }
        }
        Ok(recon_audit::chain::VerifyOutcome::valid(checked))
    }

    /// Look up the most recent anchor and confirm this tenant's chain still
    /// includes the row that was anchored. Returns `Some(VerifyError)` if the
    /// anchor points to a seq the chain has lost or whose hash no longer matches;
    /// `None` if there's no anchor yet or the anchor is satisfied.
    async fn cross_check_anchor(
        &self,
        tenant_id: &str,
        entries: &[AuditEntry],
    ) -> Result<Option<recon_audit::chain::VerifyError>, StoreError> {
        let row: Option<(serde_json::Value,)> = sqlx::query_as(
            "SELECT tenant_heads FROM audit_anchors ORDER BY anchor_seq DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await?;
        let Some((heads,)) = row else { return Ok(None) };
        let Some(head) = heads.get(tenant_id) else { return Ok(None) };
        let Some(anchored_seq) = head.get("seq").and_then(|v| v.as_i64()) else { return Ok(None) };
        let Some(anchored_hash) = head.get("hash").and_then(|v| v.as_str()) else { return Ok(None) };

        // The chain we just walked must contain the anchored (seq, hash) row.
        let found = entries.iter().find(|e| e.seq == anchored_seq);
        match found {
            None => Ok(Some(recon_audit::chain::VerifyError {
                seq: anchored_seq,
                reason: recon_audit::chain::VerifyReason::Missing,
            })),
            Some(e) if hex::encode(e.hash) != anchored_hash => Ok(Some(recon_audit::chain::VerifyError {
                seq: anchored_seq,
                reason: recon_audit::chain::VerifyReason::Tampered,
            })),
            Some(_) => Ok(None),
        }
    }
}

fn row_to_entry(r: AuditEventRow) -> Result<AuditEntry, StoreError> {
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub anchor_seq: i64,
    pub at: String,
    pub tenant_heads: BTreeMap<String, TenantHead>,
    pub prev_hash: Vec<u8>, // serialized as hex in API layer
    pub hash: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantHead {
    pub seq: i64,
    // Stored as a hex string in the tenant_heads JSONB column so the column is
    // human-readable AND the wire shape (`hash: string`) matches the frontend's
    // Anchor type. The anchor hash function feeds this same JSON form to SHA-256,
    // so the choice is load-bearing — once anchored, the format can't change
    // without invalidating prior anchors.
    pub hash: String,
}

impl Store {
    /// Snapshot every tenant's current head and append a new global anchor row.
    /// Emits one `system.anchor.created` event per affected tenant inside the
    /// same transaction (so each tenant's chain self-describes the anchor).
    pub async fn anchor_now(&self) -> Result<Anchor, StoreError> {
        let mut tx = self.pool.begin().await?;

        // 1. Each tenant's current head.
        let head_rows: Vec<(String, i64, Vec<u8>)> = sqlx::query_as(
            "SELECT ae.tenant_id, ae.seq, ae.hash FROM audit_events ae \
             WHERE ae.seq = (SELECT max(seq) FROM audit_events WHERE tenant_id = ae.tenant_id) \
             ORDER BY ae.tenant_id",
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut tenant_heads = BTreeMap::new();
        for (tid, seq, hash) in &head_rows {
            tenant_heads.insert(tid.clone(), TenantHead { seq: *seq, hash: hex::encode(hash) });
        }

        // 2. Previous anchor.
        let prev: Option<(i64, Vec<u8>)> = sqlx::query_as(
            "SELECT anchor_seq, hash FROM audit_anchors ORDER BY anchor_seq DESC LIMIT 1 FOR UPDATE",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let (prev_seq, prev_hash_vec) = prev.unwrap_or((0, vec![0u8; 32]));
        let anchor_seq = prev_seq + 1;

        let now = OffsetDateTime::now_utc();
        let at = now
            .format(&Rfc3339)
            .map_err(|_| StoreError::Db(sqlx::Error::Decode("rfc3339".into())))?;

        // 3. Compute the anchor hash: prev_hash || u64-BE(anchor_seq) || at || sorted-keys-JSON(tenant_heads).
        let tenant_heads_json = serde_json::to_value(&tenant_heads)?;
        let tenant_heads_bytes = serde_json::to_vec(&tenant_heads_json)?;
        let mut hasher = Sha256::new();
        hasher.update(&prev_hash_vec);
        hasher.update((anchor_seq as u64).to_be_bytes());
        hasher.update(at.as_bytes());
        hasher.update(&tenant_heads_bytes);
        let hash: [u8; 32] = hasher.finalize().into();

        // 4. Insert the anchor row.
        sqlx::query(
            "INSERT INTO audit_anchors(anchor_seq, at, tenant_heads, prev_hash, hash) \
             VALUES ($1, $2::timestamptz, $3, $4, $5)",
        )
        .bind(anchor_seq)
        .bind(&at)
        .bind(&tenant_heads_json)
        .bind(prev_hash_vec.as_slice())
        .bind(hash.as_slice())
        .execute(&mut *tx)
        .await?;

        // 5. Emit a per-tenant system.anchor.created so each tenant chain self-describes.
        let tenant_count = head_rows.len() as i64;
        for (tid, _seq, _h) in &head_rows {
            self.append_audit(
                &mut tx,
                tid,
                "system",
                AuditPayload::SystemAnchorCreated { anchor_seq, tenant_count },
            )
            .await?;
        }

        tx.commit().await?;

        Ok(Anchor {
            anchor_seq,
            at,
            tenant_heads,
            prev_hash: prev_hash_vec,
            hash: hash.to_vec(),
        })
    }

    pub async fn list_anchors(&self, limit: i64) -> Result<Vec<Anchor>, StoreError> {
        let limit = limit.clamp(1, 200);
        type AnchorRow = (i64, time::OffsetDateTime, serde_json::Value, Vec<u8>, Vec<u8>);
        let rows: Vec<AnchorRow> = sqlx::query_as(
            "SELECT anchor_seq, at, tenant_heads, prev_hash, hash FROM audit_anchors ORDER BY anchor_seq DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for (anchor_seq, at, tenant_heads, prev_hash, hash) in rows {
            let at = at
                .format(&Rfc3339)
                .map_err(|_| StoreError::Db(sqlx::Error::Decode("rfc3339".into())))?;
            let tenant_heads: BTreeMap<String, TenantHead> = serde_json::from_value(tenant_heads)?;
            out.push(Anchor { anchor_seq, at, tenant_heads, prev_hash, hash });
        }
        Ok(out)
    }
}
