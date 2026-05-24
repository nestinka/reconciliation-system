use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use recon_api::test_app;
use tower::ServiceExt;

/// Seed the DB and return (router, cfg) for auth tests.
/// Users seeded by seed.rs: mia@acme.test / Password123!
async fn auth_app(
    pool: sqlx::PgPool,
) -> (axum::Router, std::sync::Arc<recon_api::state::AuthConfig>) {
    recon_store::Store::from_pool(pool.clone())
        .seed()
        .await
        .unwrap();
    test_app(pool)
}

/// POST /auth/login and return (status, body, set-cookie header value).
async fn do_login(
    app: &axum::Router,
    email: &str,
    password: &str,
) -> (StatusCode, serde_json::Value, Option<String>) {
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
    let cookie = res
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, v, cookie)
}

/// Extract `recon_refresh=<value>` from a set-cookie header string.
fn extract_refresh_value(set_cookie: &str) -> Option<String> {
    // The set-cookie header looks like: recon_refresh=<value>; Path=/auth; HttpOnly; ...
    set_cookie
        .split(';')
        .next()
        .and_then(|part| part.trim().strip_prefix("recon_refresh="))
        .map(|v| v.to_string())
}

/// POST /auth/refresh with the given refresh token cookie value. Returns (status, body, new set-cookie).
async fn do_refresh(
    app: &axum::Router,
    refresh_value: &str,
) -> (StatusCode, serde_json::Value, Option<String>) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/refresh")
                .header("cookie", format!("recon_refresh={refresh_value}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let new_cookie = res
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, v, new_cookie)
}

/// POST /auth/logout with the given refresh token cookie value. Returns status.
async fn do_logout(app: &axum::Router, refresh_value: &str) -> StatusCode {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/logout")
                .header("cookie", format!("recon_refresh={refresh_value}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    res.status()
}

#[sqlx::test]
async fn login_wrong_password_401(pool: sqlx::PgPool) {
    let (app, _cfg) = auth_app(pool).await;
    let (status, _body, _cookie) = do_login(&app, "mia@acme.test", "WrongPassword!").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn login_success_sets_cookie(pool: sqlx::PgPool) {
    let (app, _cfg) = auth_app(pool).await;
    let (status, body, cookie) = do_login(&app, "mia@acme.test", "Password123!").await;
    assert_eq!(status, StatusCode::OK, "body: {body}");
    // Body has accessToken.
    assert!(
        body["accessToken"].is_string(),
        "expected accessToken in body, got: {body}"
    );
    // Response sets a cookie containing recon_refresh and HttpOnly.
    let cookie_str = cookie.expect("expected set-cookie header");
    assert!(
        cookie_str.contains("recon_refresh="),
        "cookie missing recon_refresh: {cookie_str}"
    );
    assert!(
        cookie_str.to_lowercase().contains("httponly"),
        "cookie missing HttpOnly: {cookie_str}"
    );
}

#[sqlx::test]
async fn lockout_after_5_failures(pool: sqlx::PgPool) {
    let (app, _cfg) = auth_app(pool).await;
    // First 4 wrong attempts should return 401 (not locked yet).
    for i in 1..5 {
        let (status, _, _) = do_login(&app, "mia@acme.test", "WrongPassword!").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "attempt {i} should be 401");
    }
    // 5th attempt triggers lockout → 429.
    let (status, _, _) = do_login(&app, "mia@acme.test", "WrongPassword!").await;
    assert_eq!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "5th failed attempt should be 429"
    );
}

#[sqlx::test]
async fn refresh_rotates_and_detects_reuse(pool: sqlx::PgPool) {
    let (app, _cfg) = auth_app(pool).await;

    // Login to get initial refresh cookie.
    let (status, _body, set_cookie) = do_login(&app, "mia@acme.test", "Password123!").await;
    assert_eq!(status, StatusCode::OK);
    let old_cookie_header = set_cookie.expect("expected set-cookie from login");
    let old_refresh = extract_refresh_value(&old_cookie_header)
        .expect("expected recon_refresh value in set-cookie");

    // First refresh: should succeed and return a new cookie.
    let (status, body, new_cookie_header) = do_refresh(&app, &old_refresh).await;
    assert_eq!(status, StatusCode::OK, "first refresh should succeed: {body}");
    assert!(body["accessToken"].is_string(), "expected accessToken: {body}");
    let new_cookie_str = new_cookie_header.expect("expected new set-cookie after refresh");
    let _new_refresh = extract_refresh_value(&new_cookie_str)
        .expect("expected new recon_refresh value");

    // Second refresh with OLD cookie → reuse detected → 401.
    let (status, _, _) = do_refresh(&app, &old_refresh).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "reuse of old refresh token should be 401"
    );
}

#[sqlx::test]
async fn logout_revokes(pool: sqlx::PgPool) {
    let (app, _cfg) = auth_app(pool).await;

    // Login.
    let (status, _, set_cookie) = do_login(&app, "mia@acme.test", "Password123!").await;
    assert_eq!(status, StatusCode::OK);
    let cookie_header = set_cookie.expect("expected set-cookie from login");
    let refresh_value =
        extract_refresh_value(&cookie_header).expect("expected recon_refresh value");

    // Logout with cookie.
    let status = do_logout(&app, &refresh_value).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "logout should return 204");

    // Refresh with that (now revoked) cookie → 401.
    let (status, _, _) = do_refresh(&app, &refresh_value).await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "refresh after logout should be 401"
    );
}
