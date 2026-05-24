use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use recon_api::routes::router;
use recon_api::state::AppState;
use recon_store::Store;
use tower::ServiceExt;

async fn app(pool: sqlx::PgPool) -> axum::Router {
    let store = Store::from_pool(pool);
    store.seed().await.unwrap();
    router(AppState { store })
}

async fn get_json(
    app: &axum::Router,
    uri: &str,
    tenant: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    let mut b = Request::builder().uri(uri);
    if let Some(t) = tenant {
        b = b.header("x-tenant-id", t);
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

#[sqlx::test]
async fn dashboard_shape(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (st, v) = get_json(&app, "/api/dashboard", Some("tenant-acme")).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v["matchRatePct"].is_number());
    assert!(v["breaksByType"].is_array());
    assert!(v["openBreaks"].is_number());
}

#[sqlx::test]
async fn dashboard_requires_tenant_header(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (st, _) = get_json(&app, "/api/dashboard", None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn case_pending_shape(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (st, v) = get_json(&app, "/api/cases/case-pending", Some("tenant-acme")).await;
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
    let app = app(pool).await;
    let (st, _) = get_json(&app, "/api/cases/case-pending", Some("tenant-globex")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
