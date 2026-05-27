use crate::auth::AuthContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::Json;
use recon_audit::CONTROLS;
use recon_store::audit::AuditFilter;
use serde::Deserialize;
use serde_json::{json, Value};

fn require_view_audit(ctx: &AuthContext) -> Result<(), ApiError> {
    recon_auth::rbac::require(ctx.role, recon_auth::rbac::Permission::ViewAudit)
        .map_err(|_| ApiError::Forbidden())
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListAuditQ {
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(default)]
    pub kind: Vec<String>,
    pub actor_id: Option<String>,
    pub limit: Option<i64>,
    pub before: Option<i64>,
}

pub async fn list_audit(
    State(s): State<AppState>,
    ctx: AuthContext,
    Query(q): Query<ListAuditQ>,
) -> Result<Json<Value>, ApiError> {
    require_view_audit(&ctx)?;
    let kinds = q
        .kind
        .iter()
        .filter_map(|k| recon_audit::AuditKind::from_str(k))
        .collect::<Vec<_>>();
    let f = AuditFilter {
        from: q.from,
        to: q.to,
        kinds,
        actor_id: q.actor_id,
        limit: q.limit.unwrap_or(100),
        before: q.before,
    };
    let page = s.store.list_audit(&ctx.tenant_id, &f).await?;
    Ok(Json(json!({
        "items": page.items.iter().map(audit_entry_json).collect::<Vec<_>>(),
        "nextCursor": page.next_cursor,
    })))
}

fn audit_entry_json(e: &recon_audit::AuditEntry) -> Value {
    // Drop the outer serde tag (`kind`) and emit just the inner `data` payload —
    // `kind` is already a top-level field on the wire.
    let payload_data = serde_json::to_value(&e.payload)
        .ok()
        .and_then(|v| v.get("data").cloned())
        .unwrap_or(Value::Null);
    json!({
        "tenantId": e.tenant_id,
        "seq": e.seq,
        "at": e.at,
        "actorId": e.actor_id,
        "kind": e.kind.as_str(),
        "payload": payload_data,
        "prevHash": hex::encode(e.prev_hash),
        "hash": hex::encode(e.hash),
    })
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReq {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub expected_prev_hash: Option<String>, // hex
}

pub async fn verify_audit(
    State(s): State<AppState>,
    ctx: AuthContext,
    Json(body): Json<VerifyReq>,
) -> Result<Json<Value>, ApiError> {
    require_view_audit(&ctx)?;
    let expected = match body.expected_prev_hash {
        Some(h) => Some(hex_to_arr32(&h)?),
        None => None,
    };
    let outcome = s
        .store
        .verify_audit(&ctx.tenant_id, body.from, body.to, expected)
        .await?;
    Ok(Json(serde_json::to_value(&outcome).unwrap()))
}

fn hex_to_arr32(s: &str) -> Result<[u8; 32], ApiError> {
    let v = hex::decode(s).map_err(|_| ApiError::BadRequest())?;
    if v.len() != 32 {
        return Err(ApiError::BadRequest());
    }
    let mut a = [0u8; 32];
    a.copy_from_slice(&v);
    Ok(a)
}

pub async fn anchor_audit(
    State(s): State<AppState>,
    ctx: AuthContext,
) -> Result<Json<Value>, ApiError> {
    require_view_audit(&ctx)?;
    let a = s.store.anchor_now().await?;
    Ok(Json(json!({
        "anchorSeq": a.anchor_seq,
        "hash": hex::encode(&a.hash),
    })))
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListAnchorsQ {
    pub limit: Option<i64>,
}

pub async fn list_anchors(
    State(s): State<AppState>,
    ctx: AuthContext,
    Query(q): Query<ListAnchorsQ>,
) -> Result<Json<Value>, ApiError> {
    require_view_audit(&ctx)?;
    let limit = q.limit.unwrap_or(50);
    let anchors = s.store.list_anchors(limit).await?;
    let items: Vec<Value> = anchors
        .into_iter()
        .map(|a| {
            json!({
                "anchorSeq": a.anchor_seq,
                "at": a.at,
                "tenantHeads": serde_json::to_value(&a.tenant_heads).unwrap_or(Value::Null),
                "prevHash": hex::encode(&a.prev_hash),
                "hash": hex::encode(&a.hash),
            })
        })
        .collect();
    Ok(Json(Value::Array(items)))
}

/// Open to any authenticated member — controls metadata is non-sensitive.
pub async fn list_controls(_ctx: AuthContext) -> Result<Json<Value>, ApiError> {
    Ok(Json(serde_json::to_value(CONTROLS).unwrap()))
}
