use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use recon_api::{test_app, test_app_with_mailer};
use recon_domain::UserRole;
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

/// Seed the DB and return (router, cfg, capturing_mailer) for tests that inspect sent mail.
async fn auth_app_with_capture(
    pool: sqlx::PgPool,
) -> (
    axum::Router,
    std::sync::Arc<recon_api::state::AuthConfig>,
    std::sync::Arc<recon_mail::testing::CapturingMailer>,
) {
    recon_store::Store::from_pool(pool.clone())
        .seed()
        .await
        .unwrap();
    let mailer = std::sync::Arc::new(recon_mail::testing::CapturingMailer::default());
    let (router, cfg) = test_app_with_mailer(pool, mailer.clone());
    (router, cfg, mailer)
}

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

async fn post_json(
    app: &axum::Router,
    uri: &str,
    auth: Option<&str>,
    cookie: Option<&str>,
    body: serde_json::Value,
) -> (StatusCode, serde_json::Value, Option<String>) {
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(a) = auth {
        b = b.header("authorization", a);
    }
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let set_cookie = res
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
    (status, v, set_cookie)
}

async fn get_json_auth(
    app: &axum::Router,
    uri: &str,
    auth: &str,
) -> (StatusCode, serde_json::Value) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("authorization", auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, v)
}

async fn patch_json_auth(
    app: &axum::Router,
    uri: &str,
    auth: &str,
    body: serde_json::Value,
) -> StatusCode {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(uri)
                .header("authorization", auth)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    res.status()
}

async fn delete_auth(app: &axum::Router, uri: &str, auth: &str) -> StatusCode {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(uri)
                .header("authorization", auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    res.status()
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

// ──────────────────────────────────────────────────────────────────────────────
// New tests: switch-tenant, change-password, forgot/reset, admin users, approval RBAC
// ──────────────────────────────────────────────────────────────────────────────

#[sqlx::test]
async fn switch_tenant_ok_and_forbidden(pool: sqlx::PgPool) {
    let (app, cfg) = auth_app(pool).await;

    // Login as ada (member of both tenant-acme and tenant-globex).
    let (status, login_body, set_cookie) = do_login(&app, "ada@acme.test", "Password123!").await;
    assert_eq!(status, StatusCode::OK, "ada login: {login_body}");
    let cookie_header = set_cookie.expect("set-cookie from ada login");
    let refresh_value = extract_refresh_value(&cookie_header).expect("recon_refresh");

    // Mint ada's bearer for tenant-acme (she logged in there).
    let ada_bearer = bearer(&cfg, "user-ada", "tenant-acme", UserRole::Admin);

    // Switch to tenant-globex (ada is also admin there) → 200.
    let (status, body, _) = post_json(
        &app,
        "/auth/switch-tenant",
        Some(&ada_bearer),
        Some(&format!("recon_refresh={refresh_value}")),
        serde_json::json!({ "tenantId": "tenant-globex" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "switch to globex: {body}");
    assert!(
        body["accessToken"].is_string(),
        "expected accessToken after switch: {body}"
    );

    // Ada is NOT a member of a non-existent tenant → 403.
    let (status, _, _) = post_json(
        &app,
        "/auth/switch-tenant",
        Some(&ada_bearer),
        None,
        serde_json::json!({ "tenantId": "tenant-nonexistent" }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "switch to unknown tenant must be 403"
    );
}

#[sqlx::test]
async fn change_password_flow(pool: sqlx::PgPool) {
    let (app, cfg) = auth_app(pool).await;
    let mia_bearer = bearer(&cfg, "user-mia", "tenant-acme", UserRole::Operator);

    // Wrong current password → 403.
    let (status, _, _) = post_json(
        &app,
        "/auth/password",
        Some(&mia_bearer),
        None,
        serde_json::json!({ "currentPassword": "WrongPass!", "newPassword": "NewPass123!" }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "wrong current password should be 403");

    // Correct current password, new ≥ 8 chars → 204.
    let (status, _, _) = post_json(
        &app,
        "/auth/password",
        Some(&mia_bearer),
        None,
        serde_json::json!({ "currentPassword": "Password123!", "newPassword": "NewPass123!" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "valid password change should be 204");

    // Re-login with the new password → 200.
    let (status, body, _) = do_login(&app, "mia@acme.test", "NewPass123!").await;
    assert_eq!(status, StatusCode::OK, "re-login after pw change: {body}");
}

#[sqlx::test]
async fn forgot_and_reset(pool: sqlx::PgPool) {
    let (app, _cfg, mailer) = auth_app_with_capture(pool).await;

    // /auth/forgot for a known email → 202 and exactly 1 email captured.
    let (status, _, _) = post_json(
        &app,
        "/auth/forgot",
        None,
        None,
        serde_json::json!({ "email": "mia@acme.test" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "forgot should be 202");

    let sent_count = mailer.sent.lock().unwrap().len();
    assert_eq!(sent_count, 1, "expected exactly 1 email sent, got {sent_count}");

    let token = {
        let sent = mailer.sent.lock().unwrap();
        let body = &sent[0].body;
        assert!(
            body.contains("/reset?token="),
            "email body should contain /reset?token=: {body}"
        );
        // Extract token from the link.
        body.split("/reset?token=")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or("")
            .to_string()
    };
    assert!(!token.is_empty(), "token should be non-empty");

    // /auth/reset with the extracted token and a new password → 204.
    let (status, _, _) = post_json(
        &app,
        "/auth/reset",
        None,
        None,
        serde_json::json!({ "token": token, "newPassword": "ResetPass1!" }),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "reset should be 204");

    // Re-login with the new password → 200.
    let (status, body, _) = do_login(&app, "mia@acme.test", "ResetPass1!").await;
    assert_eq!(status, StatusCode::OK, "re-login after reset: {body}");

    // /auth/forgot for unknown email → 202 and NO new captured email.
    let (status, _, _) = post_json(
        &app,
        "/auth/forgot",
        None,
        None,
        serde_json::json!({ "email": "unknown@nowhere.test" }),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "forgot unknown should be 202");
    let final_count = mailer.sent.lock().unwrap().len();
    assert_eq!(final_count, 1, "no new email should be sent for unknown: got {final_count}");
}

#[sqlx::test]
async fn admin_users_rbac(pool: sqlx::PgPool) {
    let (app, cfg) = auth_app(pool).await;

    // Operator (mia) → GET /api/users → 403.
    let mia_bearer = bearer(&cfg, "user-mia", "tenant-acme", UserRole::Operator);
    let (status, _) = get_json_auth(&app, "/api/users", &mia_bearer).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "operator should get 403 on /api/users");

    // Admin (ada) → GET /api/users → 200 with non-empty list.
    let ada_bearer = bearer(&cfg, "user-ada", "tenant-acme", UserRole::Admin);
    let (status, body) = get_json_auth(&app, "/api/users", &ada_bearer).await;
    assert_eq!(status, StatusCode::OK, "admin should get 200: {body}");
    assert!(
        body.as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "user list should be non-empty: {body}"
    );

    // POST /api/users → 201.
    let (status, created_body, _) = post_json(
        &app,
        "/api/users",
        Some(&ada_bearer),
        None,
        serde_json::json!({
            "name": "New User",
            "email": "newuser@acme.test",
            "role": "operator",
            "password": "NewUser123!"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "create user should be 201: {created_body}");
    let new_user_id = created_body["id"].as_str().expect("created user should have id").to_string();

    // PATCH role → 204.
    let patch_status = patch_json_auth(
        &app,
        &format!("/api/users/{new_user_id}"),
        &ada_bearer,
        serde_json::json!({ "role": "approver" }),
    )
    .await;
    assert_eq!(patch_status, StatusCode::NO_CONTENT, "patch role should be 204");

    // DELETE → 204.
    let del_status = delete_auth(&app, &format!("/api/users/{new_user_id}"), &ada_bearer).await;
    assert_eq!(del_status, StatusCode::NO_CONTENT, "delete user should be 204");
}

/// Fix 1: admin of tenant-acme cannot disable a user who is only in tenant-globex.
#[sqlx::test]
async fn patch_disable_cross_tenant_user_returns_404(pool: sqlx::PgPool) {
    let (app, cfg) = auth_app(pool.clone()).await;

    // Seed a globex-only user (not a member of tenant-acme).
    sqlx::query(
        "INSERT INTO users(id,name,email,disabled) VALUES ('user-globex-only','GlobexUser','globexuser@globex.test',false)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO user_credentials(user_id,password_hash) VALUES ('user-globex-only','$argon2id$dummy')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-globex-only','tenant-globex','operator')",
    )
    .execute(&pool)
    .await
    .unwrap();

    // ada is admin of tenant-acme; globex-only user is NOT a member of tenant-acme.
    let ada_bearer = bearer(&cfg, "user-ada", "tenant-acme", UserRole::Admin);

    let status = patch_json_auth(
        &app,
        "/api/users/user-globex-only",
        &ada_bearer,
        serde_json::json!({ "disabled": true }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "cross-tenant disable should be 404");

    // Verify the user remains enabled in the DB.
    let disabled: bool = sqlx::query_scalar("SELECT disabled FROM users WHERE id='user-globex-only'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(!disabled, "globex-only user should still be enabled");
}

/// Fix 2: non-admin can call GET /api/members and get the tenant's user list.
#[sqlx::test]
async fn non_admin_can_list_members(pool: sqlx::PgPool) {
    let (app, cfg) = auth_app(pool).await;

    // mia is an operator in tenant-acme.
    let mia_bearer = bearer(&cfg, "user-mia", "tenant-acme", UserRole::Operator);
    let (status, body) = get_json_auth(&app, "/api/members", &mia_bearer).await;
    assert_eq!(status, StatusCode::OK, "operator should get 200 on /api/members: {body}");
    assert!(
        body.as_array().map(|a| !a.is_empty()).unwrap_or(false),
        "members list should be non-empty: {body}"
    );

    // /api/users is still admin-only.
    let (status, _) = get_json_auth(&app, "/api/users", &mia_bearer).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "operator should get 403 on /api/users");
}

/// Fix 3: duplicate email on POST /api/users → 409.
#[sqlx::test]
async fn create_user_duplicate_email_returns_409(pool: sqlx::PgPool) {
    let (app, cfg) = auth_app(pool).await;
    let ada_bearer = bearer(&cfg, "user-ada", "tenant-acme", UserRole::Admin);

    // First creation succeeds.
    let (status, body, _) = post_json(
        &app,
        "/api/users",
        Some(&ada_bearer),
        None,
        serde_json::json!({
            "name": "Dup User",
            "email": "dup@acme.test",
            "role": "operator",
            "password": "Password123!"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "first create should be 201: {body}");

    // Second creation with same email → 409.
    let (status, body, _) = post_json(
        &app,
        "/api/users",
        Some(&ada_bearer),
        None,
        serde_json::json!({
            "name": "Dup User 2",
            "email": "dup@acme.test",
            "role": "operator",
            "password": "Password123!"
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "duplicate email should be 409: {body}");
}

#[sqlx::test]
async fn approval_requires_approver_role(pool: sqlx::PgPool) {
    let (app, cfg) = auth_app(pool).await;

    let approval_body = serde_json::json!({ "actorId": "user-mia", "kind": "approved", "payload": {} });

    // Operator (mia) → append approval event → 403.
    let mia_bearer = bearer(&cfg, "user-mia", "tenant-acme", UserRole::Operator);
    let (status, _, _) = post_json(
        &app,
        "/api/cases/case-pending/events",
        Some(&mia_bearer),
        None,
        approval_body.clone(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "operator cannot approve"
    );

    // Approver (theo) as checker → 200 (four-eyes: mia was maker, theo is checker).
    let theo_bearer = bearer(&cfg, "user-theo", "tenant-acme", UserRole::Approver);
    let (status, body, _) = post_json(
        &app,
        "/api/cases/case-pending/events",
        Some(&theo_bearer),
        None,
        serde_json::json!({ "actorId": "user-theo", "kind": "approved", "payload": {} }),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "approver should be able to approve: {body}"
    );
    assert_eq!(body["status"], "resolved", "case should be resolved: {body}");
}
