use crate::auth::AuthContext;
use crate::dto::*;
use crate::error::ApiError;
use crate::state::AppState;
use axum::{
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use recon_ingest::Parser;
use recon_store::read::{BreakFilter, RunFilter};
use serde_json::{json, Value};

pub fn router(state: AppState) -> Router {
    let mut r = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        // Public auth endpoints (no AuthContext extractor).
        .route("/auth/login", post(crate::routes_auth::login))
        .route("/auth/refresh", post(crate::routes_auth::refresh))
        .route("/auth/logout", post(crate::routes_auth::logout))
        .route("/auth/switch-tenant", post(crate::routes_auth::switch_tenant))
        .route("/auth/password", post(crate::routes_auth::change_password))
        .route("/auth/forgot", post(crate::routes_auth::forgot))
        .route("/auth/reset", post(crate::routes_auth::reset))
        .route("/api/tenants", get(list_tenants))
        // Admin-guarded user management (replaces the old unsecured list_users).
        .route(
            "/api/users",
            get(crate::routes_users::list_users).post(crate::routes_users::create_user),
        )
        .route(
            "/api/users/:user_id",
            axum::routing::patch(crate::routes_users::patch_user)
                .delete(crate::routes_users::delete_user),
        )
        // Non-privileged member list for timeline/assignee display.
        .route("/api/members", get(crate::routes_users::list_members))
        .route("/api/dashboard", get(dashboard))
        .route("/api/sources", get(list_sources).post(create_source))
        .route(
            "/api/sources/:source_id/ingest",
            post(ingest_source).layer(DefaultBodyLimit::max(10 * 1024 * 1024)),
        )
        .route("/api/runs", get(list_runs).post(create_run))
        .route("/api/runs/:run_id", get(get_run))
        .route("/api/breaks", get(list_breaks))
        .route("/api/breaks/:break_id/assign", post(assign_break))
        .route("/api/cases/:case_id", get(get_case))
        .route("/api/cases/:case_id/events", post(append_event));
    // Dev-only: reset the DB to seeded state (used by E2E). Gated by RECON_DEV.
    if std::env::var("RECON_DEV").is_ok() {
        r = r.route("/api/dev/reseed", post(dev_reseed));
    }
    r.with_state(state)
}

async fn list_tenants(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.store.list_tenants().await?)))
}

async fn dashboard(State(s): State<AppState>, ctx: AuthContext) -> Result<Json<Value>, ApiError> {
    let d = s.store.get_dashboard(&ctx.tenant_id).await?;
    Ok(Json(json!({
        "matchRatePct": d.match_rate_pct,
        "openBreaks": d.open_breaks,
        "valueAtRiskMinor": d.value_at_risk_minor,
        "currency": d.currency,
        "slaAdherencePct": d.sla_adherence_pct,
        "breaksByType": d.breaks_by_type.iter().map(|(t,c)| json!({"type": t, "count": c})).collect::<Vec<_>>(),
        "breaksByAgeing": d.breaks_by_ageing.iter().map(|(b,c)| json!({"bucket": b, "count": c})).collect::<Vec<_>>(),
        "recentRuns": d.recent_runs,
    })))
}

async fn list_runs(
    State(s): State<AppState>,
    ctx: AuthContext,
    Query(q): Query<RunQ>,
) -> Result<Json<Value>, ApiError> {
    let f = RunFilter {
        status: q.status,
        source_id: q.source_id,
        from: q.from,
        to: q.to,
    };
    Ok(Json(json!(s.store.list_runs(&ctx.tenant_id, &f).await?)))
}

async fn get_run(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let d = s.store.get_run(&ctx.tenant_id, &run_id).await?;
    let txn_map: serde_json::Map<String, Value> = d
        .transactions
        .iter()
        .map(|t| (t.id.clone(), json!(t)))
        .collect();
    Ok(Json(json!({
        "run": d.run,
        "transactionsById": txn_map,
        "matched": d.matched,
        "partial": d.partial,
        "duplicates": d.duplicates,
        "unmatched": d.unmatched,
    })))
}

async fn list_breaks(
    State(s): State<AppState>,
    ctx: AuthContext,
    Query(q): Query<BreakQ>,
) -> Result<Json<Value>, ApiError> {
    let f = BreakFilter {
        status: q.status,
        kind: q.kind,
        ageing_bucket: q.ageing_bucket,
        assignee_id: q.assignee_id,
    };
    Ok(Json(json!(s.store.list_breaks(&ctx.tenant_id, &f).await?)))
}

async fn get_case(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(case_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let b = s.store.get_case(&ctx.tenant_id, &case_id).await?;
    let txn_map: serde_json::Map<String, Value> = b
        .transactions
        .iter()
        .map(|t| (t.id.clone(), json!(t)))
        .collect();
    let suggestions: Vec<Value> = b
        .suggestions
        .iter()
        .enumerate()
        .map(|(i, (ids, score, rat))| {
            json!({
                "id": format!("sug-{}-{}", case_id, i),
                "txnIds": ids,
                "score": score,
                "rationale": rat,
            })
        })
        .collect();
    Ok(Json(json!({
        "case": b.case,
        "brk": b.brk,
        "suggestions": suggestions,
        "transactionsById": txn_map,
    })))
}

async fn assign_break(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(break_id): Path<String>,
    Json(body): Json<AssignBody>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(
        s.store
            .assign_break(&ctx.tenant_id, &break_id, &body.user_id, &ctx.user_id)
            .await?
    )))
}
async fn append_event(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(case_id): Path<String>,
    Json(ev): Json<recon_domain::NewCaseEvent>,
) -> Result<Json<Value>, ApiError> {
    // RBAC: approval events require Approver or Admin role.
    if matches!(ev.body, recon_domain::CaseEventBody::Approved {}) {
        recon_auth::rbac::require(ctx.role, recon_auth::rbac::Permission::ApproveResolution)
            .map_err(|_| ApiError::Forbidden())?;
    }
    // Bind the event's actor to the authenticated identity (defeats body-actor impersonation).
    let ev = recon_domain::NewCaseEvent {
        actor_id: ctx.user_id.clone(),
        ..ev
    };
    Ok(Json(json!(
        s.store
            .append_case_event(&ctx.tenant_id, &case_id, ev)
            .await?
    )))
}

async fn dev_reseed(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    s.store.seed().await?;
    Ok(Json(json!({ "ok": true })))
}

fn require_manage_data(ctx: &AuthContext) -> Result<(), ApiError> {
    recon_auth::rbac::require(ctx.role, recon_auth::rbac::Permission::ManageData)
        .map_err(|_| ApiError::Forbidden())
}

async fn list_sources(State(s): State<AppState>, ctx: AuthContext) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;
    Ok(Json(json!(s.store.list_sources(&ctx.tenant_id).await?)))
}

async fn create_source(
    State(s): State<AppState>,
    ctx: AuthContext,
    Json(body): Json<CreateSourceReq>,
) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;
    if body.name.trim().is_empty() || body.currency.trim().len() != 3 {
        return Err(ApiError::BadRequest());
    }
    let src = s
        .store
        .create_source(&ctx.tenant_id, body.kind, &body.name, &body.currency)
        .await?;
    Ok(Json(json!(src)))
}

// --- validation helpers ---
fn valid_date(s: &str) -> bool {
    time::Date::parse(
        s,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .is_ok()
}

async fn create_run(
    State(s): State<AppState>,
    ctx: AuthContext,
    Json(body): Json<CreateRunReq>,
) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;
    if !valid_date(&body.from) || !valid_date(&body.to) || body.to < body.from {
        return Err(ApiError::BadRequest());
    }
    if body.source_a_id == body.source_b_id {
        return Err(ApiError::BadRequest());
    }
    let run = s
        .store
        .create_run(&ctx.tenant_id, &body.name, &body.source_a_id, &body.source_b_id, &body.from, &body.to)
        .await?;
    Ok(Json(json!(run)))
}

async fn ingest_source(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(source_id): Path<String>,
    mut mp: Multipart,
) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;

    // Source must exist in tenant; also gives us the default currency.
    let source = s.store.get_source(&ctx.tenant_id, &source_id).await?;

    let mut file: Option<Vec<u8>> = None;
    let mut format: Option<String> = None;
    let mut mapping_json: Option<String> = None;
    while let Some(field) = mp.next_field().await.map_err(|_| ApiError::BadRequest())? {
        match field.name() {
            Some("file") => file = Some(field.bytes().await.map_err(|_| ApiError::BadRequest())?.to_vec()),
            Some("format") => format = Some(field.text().await.map_err(|_| ApiError::BadRequest())?),
            Some("mapping") => mapping_json = Some(field.text().await.map_err(|_| ApiError::BadRequest())?),
            _ => {}
        }
    }
    let bytes = file.ok_or_else(ApiError::BadRequest)?;
    let format = format.ok_or_else(ApiError::BadRequest)?;

    let parsed = match format.as_str() {
        "csv" => {
            let raw = mapping_json.ok_or_else(ApiError::BadRequest)?;
            let mapping: recon_ingest::csv::CsvMapping =
                serde_json::from_str(&raw).map_err(|_| ApiError::BadRequest())?;
            recon_ingest::csv::CsvParser::new(mapping).parse(&bytes)
        }
        "camt053" => recon_ingest::camt053::Camt053Parser.parse(&bytes),
        _ => return Err(ApiError::BadRequest()),
    };

    let parsed = match parsed {
        Ok(p) => p,
        Err(rows) => {
            let rows: Vec<Value> = rows
                .iter()
                .map(|e| json!({ "row": e.row, "field": e.field, "message": e.message }))
                .collect();
            return Err(ApiError::with_details(
                axum::http::StatusCode::UNPROCESSABLE_ENTITY,
                "parse",
                "file contains invalid rows",
                json!({ "rows": rows }),
            ));
        }
    };

    // Map ParsedTxn -> CanonicalTransaction (assign ids + defaults).
    let txns: Vec<recon_domain::CanonicalTransaction> = parsed
        .into_iter()
        .map(|p| recon_domain::CanonicalTransaction {
            id: format!("txn-{}", uuid::Uuid::new_v4()),
            tenant_id: ctx.tenant_id.clone(),
            source_id: source_id.clone(),
            external_ref: p.external_ref,
            value_date: p.value_date.clone(),
            posted_at: p.posted_at.unwrap_or_else(|| format!("{}T00:00:00Z", p.value_date)),
            amount_minor: p.amount_minor,
            currency: p.currency.unwrap_or_else(|| source.currency.clone()),
            direction: p.direction,
            counterparty: p.counterparty,
            description: p.description,
        })
        .collect();

    let n = s.store.ingest_transactions(&ctx.tenant_id, &source_id, &txns).await?;
    Ok(Json(json!({ "ingested": n, "sourceId": source_id })))
}
