use crate::auth::AuthContext;
use crate::dto::*;
use crate::error::ApiError;
use crate::state::AppState;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
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
        .route("/api/runs", get(list_runs))
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
