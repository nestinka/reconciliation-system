use axum::extract::State;
use axum::Json;
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};

use crate::dto::{AccessTokenResp, ChangePasswordReq, ForgotReq, LoginReq, LoginResp, ResetReq, SwitchTenantReq};
use crate::error::ApiError;
use crate::state::{AppState, AuthConfig};

const REFRESH_COOKIE: &str = "recon_refresh";

fn refresh_cookie(value: String, cfg: &AuthConfig) -> Cookie<'static> {
    Cookie::build((REFRESH_COOKIE, value))
        .path("/auth")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg.secure_cookie)
        .max_age(time::Duration::seconds(cfg.refresh_ttl_secs))
        .build()
}

fn cleared_cookie(cfg: &AuthConfig) -> Cookie<'static> {
    Cookie::build((REFRESH_COOKIE, ""))
        .path("/auth")
        .http_only(true)
        .same_site(SameSite::Strict)
        .secure(cfg.secure_cookie)
        .max_age(time::Duration::seconds(0))
        .build()
}

/// Dummy hash to equalize timing for unknown emails.
fn dummy_hash() -> &'static str {
    use std::sync::OnceLock;
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| recon_auth::password::hash_password("dummy-equalize").unwrap())
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// Issue an access token + refresh cookie for (user, tenant, role). Returns (jar_with_cookie, access_token).
async fn issue_session(
    state: &AppState,
    jar: CookieJar,
    user_id: &str,
    tenant_id: &str,
    role: recon_domain::UserRole,
) -> Result<(CookieJar, String), ApiError> {
    let now = now_unix();
    let access = recon_auth::token::encode_access(
        &state.cfg.jwt_secret,
        user_id,
        tenant_id,
        role,
        state.cfg.access_ttl_secs,
        now,
    )
    .map_err(|_| ApiError::Unauthorized())?;
    let (plaintext, hash) = recon_auth::refresh::generate();
    // Use a fresh random value as the refresh token row id.
    let (_, id) = recon_auth::refresh::generate();
    state
        .store
        .insert_refresh(
            &id,
            user_id,
            tenant_id,
            &hash,
            now + state.cfg.refresh_ttl_secs,
            None,
        )
        .await?;
    Ok((jar.add(refresh_cookie(plaintext, &state.cfg)), access))
}

pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
    Json(req): Json<LoginReq>,
) -> Result<(CookieJar, Json<LoginResp>), ApiError> {
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("local")
        .to_string();
    if !state.login_limiter.check(&ip) {
        return Err(ApiError::TooManyRequests());
    }

    let found = state.store.find_credential_by_email(&req.email).await?;
    let (user, cred) = match found {
        Some(x) => x,
        None => {
            // Equalize timing to prevent email enumeration.
            let _ = recon_auth::password::verify_password(&req.password, dummy_hash());
            return Err(ApiError::Unauthorized());
        }
    };
    let now = now_unix();
    if recon_auth::lockout::is_locked(cred.locked_until, now) {
        return Err(ApiError::TooManyRequests());
    }
    if user.disabled {
        return Err(ApiError::Unauthorized());
    }

    let ok =
        recon_auth::password::verify_password(&req.password, &cred.password_hash).unwrap_or(false);
    if !ok {
        let attempts_after = cred.failed_attempts + 1;
        let decision = recon_auth::lockout::on_failure(attempts_after, now);
        state
            .store
            .record_login_failure(&user.id, decision.locked_until_unix)
            .await?;
        return Err(if decision.locked_until_unix.is_some() {
            ApiError::TooManyRequests()
        } else {
            ApiError::Unauthorized()
        });
    }
    state.store.reset_login_failures(&user.id).await?;

    let memberships = state.store.memberships_for(&user.id).await?;
    let active = memberships.first().cloned().ok_or_else(ApiError::Unauthorized)?;
    let tenant = state
        .store
        .get_tenant(&active.tenant_id)
        .await?
        .ok_or_else(ApiError::Unauthorized)?;
    let (jar, access) =
        issue_session(&state, jar, &user.id, &active.tenant_id, active.role).await?;
    let user = recon_domain::User {
        role: active.role,
        ..user
    };
    Ok((
        jar,
        Json(LoginResp {
            access_token: access,
            user,
            active_tenant: tenant,
            memberships,
        }),
    ))
}

pub async fn refresh(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, Json<AccessTokenResp>), ApiError> {
    let cookie = jar
        .get(REFRESH_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(ApiError::Unauthorized)?;
    let h = recon_auth::refresh::hash(&cookie);
    let now = now_unix();
    // Reuse detection: a presented-but-revoked token means theft → nuke the chain.
    if state.store.refresh_is_revoked(&h).await? {
        if let Some(owner) = state.store.refresh_owner(&h).await? {
            state.store.revoke_all_refresh(&owner).await?;
        }
        return Err(ApiError::Unauthorized());
    }
    let (old_id, user_id, tenant_id) = state
        .store
        .find_live_refresh(&h, now)
        .await?
        .ok_or_else(ApiError::Unauthorized)?;
    let role = state
        .store
        .role_in_tenant(&user_id, &tenant_id)
        .await?
        .ok_or_else(ApiError::Unauthorized)?;
    state.store.revoke_refresh(&old_id).await?;
    // Issue new rotated token.
    let access = recon_auth::token::encode_access(
        &state.cfg.jwt_secret,
        &user_id,
        &tenant_id,
        role,
        state.cfg.access_ttl_secs,
        now,
    )
    .map_err(|_| ApiError::Unauthorized())?;
    let (plaintext, new_hash) = recon_auth::refresh::generate();
    // Use a fresh random value as the new row id.
    let (_, new_id) = recon_auth::refresh::generate();
    state
        .store
        .insert_refresh(
            &new_id,
            &user_id,
            &tenant_id,
            &new_hash,
            now + state.cfg.refresh_ttl_secs,
            Some(&old_id),
        )
        .await?;
    let jar = jar.add(refresh_cookie(plaintext, &state.cfg));
    Ok((jar, Json(AccessTokenResp { access_token: access })))
}

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
) -> Result<(CookieJar, axum::http::StatusCode), ApiError> {
    if let Some(c) = jar.get(REFRESH_COOKIE) {
        let h = recon_auth::refresh::hash(c.value());
        state.store.revoke_refresh_by_hash(&h).await?;
    }
    Ok((
        jar.add(cleared_cookie(&state.cfg)),
        axum::http::StatusCode::NO_CONTENT,
    ))
}

pub async fn switch_tenant(
    ctx: crate::auth::AuthContext,
    State(state): State<AppState>,
    jar: CookieJar,
    Json(req): Json<SwitchTenantReq>,
) -> Result<(CookieJar, Json<AccessTokenResp>), ApiError> {
    let role = state
        .store
        .role_in_tenant(&ctx.user_id, &req.tenant_id)
        .await?
        .ok_or_else(ApiError::Forbidden)?;
    if let Some(c) = jar.get(REFRESH_COOKIE) {
        let h = recon_auth::refresh::hash(c.value());
        state.store.revoke_refresh_by_hash(&h).await?;
    }
    let (jar, access) = issue_session(&state, jar, &ctx.user_id, &req.tenant_id, role).await?;
    Ok((jar, Json(AccessTokenResp { access_token: access })))
}

pub async fn change_password(
    ctx: crate::auth::AuthContext,
    State(state): State<AppState>,
    Json(req): Json<ChangePasswordReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    if req.new_password.len() < 8 {
        return Err(ApiError::BadRequest());
    }
    let hash = state
        .store
        .password_hash_for(&ctx.user_id)
        .await?
        .ok_or_else(ApiError::Unauthorized)?;
    if !recon_auth::password::verify_password(&req.current_password, &hash).unwrap_or(false) {
        return Err(ApiError::Forbidden());
    }
    let new_hash =
        recon_auth::password::hash_password(&req.new_password).map_err(|_| ApiError::BadRequest())?;
    state.store.set_password(&ctx.user_id, &new_hash).await?;
    state.store.revoke_all_refresh(&ctx.user_id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn forgot(
    State(state): State<AppState>,
    Json(req): Json<ForgotReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    if let Some((user, _cred)) = state.store.find_credential_by_email(&req.email).await? {
        let (plaintext, hash) = recon_auth::refresh::generate();
        let id = recon_auth::refresh::generate().1;
        state
            .store
            .insert_reset_token(&id, &user.id, &hash, now_unix() + 3600)
            .await?;
        let link = format!("{}/reset?token={}", state.cfg.app_base_url, plaintext);
        let _ = state
            .mailer
            .send(recon_mail::Email {
                to: req.email.clone(),
                subject: "Reset your Recon password".into(),
                body: format!(
                    "Reset your password using this link: {link}\nThis link expires in 1 hour."
                ),
            })
            .await;
    }
    Ok(axum::http::StatusCode::ACCEPTED)
}

pub async fn reset(
    State(state): State<AppState>,
    Json(req): Json<ResetReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    if req.new_password.len() < 8 {
        return Err(ApiError::BadRequest());
    }
    let h = recon_auth::refresh::hash(&req.token);
    let user_id = state
        .store
        .consume_reset_token(&h, now_unix())
        .await?
        .ok_or_else(ApiError::BadRequest)?;
    let new_hash =
        recon_auth::password::hash_password(&req.new_password).map_err(|_| ApiError::BadRequest())?;
    state.store.set_password(&user_id, &new_hash).await?;
    state.store.revoke_all_refresh(&user_id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
