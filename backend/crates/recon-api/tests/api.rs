use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use recon_api::test_app;
use recon_domain::UserRole;
use tower::ServiceExt;

/// Mint a Bearer token string for the given identity.
fn bearer(
    cfg: &recon_api::state::AuthConfig,
    user_id: &str,
    tenant_id: &str,
    role: UserRole,
) -> String {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let t = recon_auth::token::encode_access(
        &cfg.jwt_secret,
        user_id,
        tenant_id,
        role,
        cfg.access_ttl_secs,
        now,
    )
    .unwrap();
    format!("Bearer {t}")
}

/// Build a seeded test app; returns (router, cfg).
async fn seeded_app(
    pool: sqlx::PgPool,
) -> (axum::Router, std::sync::Arc<recon_api::state::AuthConfig>) {
    recon_store::Store::from_pool(pool.clone())
        .seed()
        .await
        .unwrap();
    test_app(pool)
}

async fn get_json(
    app: &axum::Router,
    uri: &str,
    auth: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut b = Request::builder().uri(uri);
    if let Some(a) = auth {
        b = b.header(axum::http::header::AUTHORIZATION, a);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, v)
}

async fn post_json_as(
    app: &axum::Router,
    uri: &str,
    auth: &str,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(axum::http::header::AUTHORIZATION, auth)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, v)
}

#[sqlx::test]
async fn dashboard_shape(pool: sqlx::PgPool) {
    let (app, cfg) = seeded_app(pool).await;
    let auth = bearer(&cfg, "user-mia", "tenant-acme", UserRole::Operator);
    let (st, v) = get_json(&app, "/api/dashboard", Some(&auth)).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v["matchRatePct"].is_number());
    assert!(v["breaksByType"].is_array());
    assert!(v["openBreaks"].is_number());
}

#[sqlx::test]
async fn dashboard_requires_auth(pool: sqlx::PgPool) {
    let (app, _cfg) = test_app(pool);
    let (st, _) = get_json(&app, "/api/dashboard", None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn case_pending_shape(pool: sqlx::PgPool) {
    let (app, cfg) = seeded_app(pool).await;
    let auth = bearer(&cfg, "user-mia", "tenant-acme", UserRole::Operator);
    let (st, v) = get_json(&app, "/api/cases/case-pending", Some(&auth)).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["case"]["status"], "pending_approval");
    assert_eq!(v["brk"]["caseId"], "case-pending");
    assert!(v["case"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|e| e["kind"] == "approval_requested"));
}

#[sqlx::test]
async fn cross_tenant_case_is_not_found(pool: sqlx::PgPool) {
    let (app, cfg) = seeded_app(pool).await;
    // Token for tenant-globex — case-pending belongs to tenant-acme, so 404
    let auth = bearer(&cfg, "user-mia", "tenant-globex", UserRole::Operator);
    let (st, _) = get_json(&app, "/api/cases/case-pending", Some(&auth)).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}

#[sqlx::test]
async fn maker_approve_forbidden_then_approver_resolves(pool: sqlx::PgPool) {
    let (app, cfg) = seeded_app(pool).await;
    // Mia (maker / Operator) is forbidden from approving
    let mia_auth = bearer(&cfg, "user-mia", "tenant-acme", UserRole::Operator);
    let (st, _) = post_json_as(
        &app,
        "/api/cases/case-pending/events",
        &mia_auth,
        serde_json::json!({ "actorId": "user-mia", "kind": "approved", "payload": {} }),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // Theo (Approver) succeeds -> resolved
    let theo_auth = bearer(&cfg, "user-theo", "tenant-acme", UserRole::Approver);
    let (st, v) = post_json_as(
        &app,
        "/api/cases/case-pending/events",
        &theo_auth,
        serde_json::json!({ "actorId": "user-theo", "kind": "approved", "payload": {} }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["status"], "resolved");
}

#[sqlx::test]
async fn body_actor_cannot_impersonate_approver(pool: sqlx::PgPool) {
    let (app, cfg) = seeded_app(pool).await;
    // Caller is Mia (Operator / maker) but lies in the body claiming to be Theo.
    // The server overwrites actor_id from the JWT sub, so four-eyes still blocks it.
    let mia_auth = bearer(&cfg, "user-mia", "tenant-acme", UserRole::Operator);
    let (st, _) = post_json_as(
        &app,
        "/api/cases/case-pending/events",
        &mia_auth,
        serde_json::json!({ "actorId": "user-theo", "kind": "approved", "payload": {} }),
    )
    .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
}

#[sqlx::test]
async fn write_requires_auth(pool: sqlx::PgPool) {
    let (app, _cfg) = test_app(pool);
    // No Authorization header -> 401
    let res = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/api/cases/case-pending/events")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::json!({"actorId":"user-mia","kind":"comment","payload":{"text":"hi"}})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

// ──────────────────────────────────────────────────────────────────────────────
// Audit (D1): list / verify / anchor / controls + RBAC
// ──────────────────────────────────────────────────────────────────────────────

/// POST /auth/login as the given seeded user; returns (status, accessToken).
async fn login_as(
    app: &axum::Router,
    email: &str,
    password: &str,
) -> (StatusCode, Option<String>) {
    let body = serde_json::json!({ "email": email, "password": password });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let token = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|v| v["accessToken"].as_str().map(|s| s.to_string()));
    (status, token)
}

#[sqlx::test]
async fn non_admin_forbidden_on_audit(pool: sqlx::PgPool) {
    let (app, _cfg) = seeded_app(pool).await;
    let (st, tok) = login_as(&app, "mia@acme.test", "Password123!").await;
    assert_eq!(st, StatusCode::OK);
    let auth = format!("Bearer {}", tok.expect("access token"));
    let (st, _) = get_json(&app, "/api/audit", Some(&auth)).await;
    assert_eq!(st, StatusCode::FORBIDDEN, "operator must not list audit");
}

#[sqlx::test]
async fn admin_can_list_audit(pool: sqlx::PgPool) {
    let (app, _cfg) = seeded_app(pool).await;
    let (st, tok) = login_as(&app, "ada@acme.test", "Password123!").await;
    assert_eq!(st, StatusCode::OK);
    let auth = format!("Bearer {}", tok.expect("access token"));
    let (st, v) = get_json(&app, "/api/audit", Some(&auth)).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v["items"].is_array(), "expected items array, got: {v}");
    // The login above emitted at least one auth.login.success event into the
    // admin's tenant chain, so the list must be non-empty.
    assert!(
        !v["items"].as_array().unwrap().is_empty(),
        "expected audit items after login: {v}"
    );
    let first = &v["items"].as_array().unwrap()[0];
    assert!(first["kind"].is_string());
    assert!(first["seq"].is_number());
    assert!(first["hash"].is_string());
    assert!(first["prevHash"].is_string());
}

#[sqlx::test]
async fn verify_on_clean_chain_returns_valid(pool: sqlx::PgPool) {
    let (app, _cfg) = seeded_app(pool).await;
    let (st, tok) = login_as(&app, "ada@acme.test", "Password123!").await;
    assert_eq!(st, StatusCode::OK);
    let auth = format!("Bearer {}", tok.expect("access token"));
    // Warm up: confirm there are events to verify.
    let (st, _list) = get_json(&app, "/api/audit", Some(&auth)).await;
    assert_eq!(st, StatusCode::OK);
    let (st, v) = post_json_as(&app, "/api/audit/verify", &auth, serde_json::json!({})).await;
    assert_eq!(st, StatusCode::OK, "verify response: {v}");
    assert_eq!(v["status"], "valid", "clean chain should verify: {v}");
}

#[sqlx::test]
async fn verify_after_tamper_returns_invalid(pool: sqlx::PgPool) {
    let (app, _cfg) = seeded_app(pool.clone()).await;
    let (st, tok) = login_as(&app, "ada@acme.test", "Password123!").await;
    assert_eq!(st, StatusCode::OK);
    let auth = format!("Bearer {}", tok.expect("access token"));
    // Tamper with one row's payload directly via SQL. The chain re-hash will mismatch.
    // We mutate the row for ada's own tenant so the verify call (which uses the
    // caller's tenant_id) sees the corruption.
    let n = sqlx::query(
        "UPDATE audit_events SET payload = jsonb_set(payload, '{data,email}', '\"tampered@x.test\"') \
         WHERE tenant_id = $1 AND kind = 'auth.login.success' \
         AND seq = (SELECT min(seq) FROM audit_events WHERE tenant_id = $1 AND kind = 'auth.login.success')",
    )
    .bind("tenant-acme")
    .execute(&pool)
    .await
    .unwrap()
    .rows_affected();
    assert!(n >= 1, "expected to tamper at least one row");
    let (st, v) = post_json_as(&app, "/api/audit/verify", &auth, serde_json::json!({})).await;
    assert_eq!(st, StatusCode::OK, "verify of tampered chain should still be 200: {v}");
    assert_eq!(v["status"], "invalid", "tampered chain should be invalid: {v}");
    assert!(v["firstBrokenSeq"].is_number(), "expected firstBrokenSeq: {v}");
}

#[sqlx::test]
async fn anchor_endpoint_writes_row(pool: sqlx::PgPool) {
    let (app, _cfg) = seeded_app(pool).await;
    let (st, tok) = login_as(&app, "ada@acme.test", "Password123!").await;
    assert_eq!(st, StatusCode::OK);
    let auth = format!("Bearer {}", tok.expect("access token"));
    // Trigger an anchor.
    let (st, v) = post_json_as(&app, "/api/audit/anchor", &auth, serde_json::json!({})).await;
    assert_eq!(st, StatusCode::OK, "anchor response: {v}");
    assert!(v["anchorSeq"].is_number(), "expected anchorSeq: {v}");
    assert!(v["hash"].is_string(), "expected hex hash: {v}");
    // List anchors and confirm we have at least one row.
    let (st, anchors) = get_json(&app, "/api/audit/anchors", Some(&auth)).await;
    assert_eq!(st, StatusCode::OK);
    let arr = anchors.as_array().expect("expected anchor array");
    assert!(!arr.is_empty(), "expected at least one anchor row: {anchors}");
    assert!(arr[0]["anchorSeq"].is_number());
    assert!(arr[0]["hash"].is_string());
    assert!(arr[0]["prevHash"].is_string());
    assert!(arr[0]["tenantHeads"].is_object() || arr[0]["tenantHeads"].is_null());
}

#[sqlx::test]
async fn assign_break_sets_assignee(pool: sqlx::PgPool) {
    let (app, cfg) = seeded_app(pool).await;
    let auth = bearer(&cfg, "user-ada", "tenant-acme", UserRole::Operator);
    let (_, breaks) = get_json(&app, "/api/breaks?status=open", Some(&auth)).await;
    let break_id = breaks.as_array().unwrap()[0]["id"]
        .as_str()
        .unwrap()
        .to_string();
    let (st, v) = post_json_as(
        &app,
        &format!("/api/breaks/{break_id}/assign"),
        &auth,
        serde_json::json!({ "userId": "user-mia" }),
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["assigneeId"], "user-mia");
    assert_eq!(v["status"], "investigating");
}
