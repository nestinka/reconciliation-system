use axum::extract::State;
use axum::http::request::Parts;
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

/// Pull the originating IP from the first `X-Forwarded-For` value (if present).
/// Returns `None` when the header is absent or empty. Audit payloads accept
/// `Option<String>`, so callers don't need a fallback. Exposed in both shapes
/// (Parts and HeaderMap) because axum's auth handlers vary in how they get
/// access to the inbound request metadata.
#[allow(dead_code)]
pub(crate) fn extract_ip(parts: &Parts) -> Option<String> {
    extract_ip_headers(&parts.headers)
}

fn extract_ip_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Issue an access token + refresh cookie for (user, tenant, role) INSIDE an open tx.
/// Returns (jar_with_cookie, access_token).
async fn issue_session_tx(
    state: &AppState,
    tx: &mut sqlx::PgConnection,
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
        .insert_refresh_tx(
            tx,
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
    let audit_ip = extract_ip_headers(&headers);
    if !state.login_limiter.check(&ip) {
        // Rate-limited: try to attribute to a known user's chain if email is known.
        if let Some((user, _)) = state.store.find_credential_by_email(&req.email).await? {
            if let Some(tenant_id) = primary_tenant_for(&state, &user.id).await? {
                let mut tx = state.store.pool.begin().await.map_err(recon_store::StoreError::from)?;
                state
                    .store
                    .append_audit(
                        &mut tx,
                        &tenant_id,
                        &user.id,
                        recon_audit::AuditPayload::AuthLoginFailure {
                            email: req.email.clone(),
                            ip: audit_ip.clone(),
                            reason: recon_audit::LoginFailureReason::RateLimited,
                        },
                    )
                    .await?;
                tx.commit().await.map_err(recon_store::StoreError::from)?;
            }
        }
        return Err(ApiError::TooManyRequests());
    }

    let found = state.store.find_credential_by_email(&req.email).await?;
    let (user, cred) = match found {
        Some(x) => x,
        None => {
            // Equalize timing to prevent email enumeration.
            let _ = recon_auth::password::verify_password(&req.password, dummy_hash());
            // Unknown email → no tenant chain to attach an audit to; skip emission.
            return Err(ApiError::Unauthorized());
        }
    };
    let now = now_unix();
    if recon_auth::lockout::is_locked(cred.locked_until, now) {
        // Emit a Locked failure to the user's primary tenant chain.
        if let Some(tenant_id) = primary_tenant_for(&state, &user.id).await? {
            let mut tx = state.store.pool.begin().await.map_err(recon_store::StoreError::from)?;
            state
                .store
                .append_audit(
                    &mut tx,
                    &tenant_id,
                    &user.id,
                    recon_audit::AuditPayload::AuthLoginFailure {
                        email: req.email.clone(),
                        ip: audit_ip.clone(),
                        reason: recon_audit::LoginFailureReason::Locked,
                    },
                )
                .await?;
            tx.commit().await.map_err(recon_store::StoreError::from)?;
        }
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
        let primary_tenant = primary_tenant_for(&state, &user.id).await?;

        let mut tx = state.store.pool.begin().await.map_err(recon_store::StoreError::from)?;
        state
            .store
            .record_login_failure_tx(&mut tx, &user.id, decision.locked_until_unix)
            .await?;
        if let Some(tenant_id) = primary_tenant.as_deref() {
            state
                .store
                .append_audit(
                    &mut tx,
                    tenant_id,
                    &user.id,
                    recon_audit::AuditPayload::AuthLoginFailure {
                        email: req.email.clone(),
                        ip: audit_ip.clone(),
                        reason: recon_audit::LoginFailureReason::BadCredentials,
                    },
                )
                .await?;
            if let Some(lu) = decision.locked_until_unix {
                let locked_until = time::OffsetDateTime::from_unix_timestamp(lu)
                    .ok()
                    .and_then(|t| t.format(&time::format_description::well_known::Rfc3339).ok())
                    .unwrap_or_default();
                state
                    .store
                    .append_audit(
                        &mut tx,
                        tenant_id,
                        &user.id,
                        recon_audit::AuditPayload::AuthLockout {
                            user_id: user.id.clone(),
                            email: user.email.clone(),
                            locked_until,
                        },
                    )
                    .await?;
            }
        }
        tx.commit().await.map_err(recon_store::StoreError::from)?;

        return Err(if decision.locked_until_unix.is_some() {
            ApiError::TooManyRequests()
        } else {
            ApiError::Unauthorized()
        });
    }

    // Success path: reads outside the tx (idempotent), then a single tx for
    // reset-failures + insert-refresh + audit + commit.
    let memberships = state.store.memberships_for(&user.id).await?;
    let active = memberships.first().cloned().ok_or_else(ApiError::Unauthorized)?;
    let tenant = state
        .store
        .get_tenant(&active.tenant_id)
        .await?
        .ok_or_else(ApiError::Unauthorized)?;

    let mut tx = state.store.pool.begin().await.map_err(recon_store::StoreError::from)?;
    state
        .store
        .reset_login_failures_tx(&mut tx, &user.id)
        .await?;
    let (jar, access) =
        issue_session_tx(&state, &mut tx, jar, &user.id, &active.tenant_id, active.role).await?;
    state
        .store
        .append_audit(
            &mut tx,
            &active.tenant_id,
            &user.id,
            recon_audit::AuditPayload::AuthLoginSuccess {
                user_id: user.id.clone(),
                email: user.email.clone(),
                ip: audit_ip,
            },
        )
        .await?;
    tx.commit().await.map_err(recon_store::StoreError::from)?;

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

/// Resolve a deterministic "primary" tenant for the given user — the first
/// membership alphabetically by tenant name. Returns None if the user has no
/// memberships. Used by audit emission for events that fire before we know which
/// tenant the user is acting under (e.g. login failures).
async fn primary_tenant_for(state: &AppState, user_id: &str) -> Result<Option<String>, ApiError> {
    let m = state.store.memberships_for(user_id).await?;
    Ok(m.into_iter().next().map(|m| m.tenant_id))
}

pub async fn refresh(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
) -> Result<(CookieJar, Json<AccessTokenResp>), ApiError> {
    let cookie = jar
        .get(REFRESH_COOKIE)
        .map(|c| c.value().to_string())
        .ok_or_else(ApiError::Unauthorized)?;
    let h = recon_auth::refresh::hash(&cookie);
    let now = now_unix();
    let audit_ip = extract_ip_headers(&headers);

    let mut tx = state.store.pool.begin().await.map_err(recon_store::StoreError::from)?;

    // Reuse detection: a presented-but-revoked token means theft → nuke the chain.
    if state.store.refresh_is_revoked_tx(&mut tx, &h).await? {
        if let Some(owner) = state.store.refresh_owner_tx(&mut tx, &h).await? {
            state.store.revoke_all_refresh_tx(&mut tx, &owner).await?;
            // Emit refresh.reused into the user's primary tenant chain.
            if let Some(tenant_id) = primary_tenant_for(&state, &owner).await? {
                // Token id (not the hash) — recover it for the audit row.
                let token_id: Option<String> = sqlx::query_scalar(
                    "SELECT id FROM refresh_tokens WHERE token_hash=$1 LIMIT 1",
                )
                .bind(&h)
                .fetch_optional(&mut *tx)
                .await
                .map_err(recon_store::StoreError::from)?;
                state
                    .store
                    .append_audit(
                        &mut tx,
                        &tenant_id,
                        &owner,
                        recon_audit::AuditPayload::AuthRefreshReused {
                            user_id: owner.clone(),
                            token_id: token_id.unwrap_or_default(),
                            ip: audit_ip,
                        },
                    )
                    .await?;
            }
        }
        tx.commit().await.map_err(recon_store::StoreError::from)?;
        return Err(ApiError::Unauthorized());
    }
    let (old_id, user_id, tenant_id) = state
        .store
        .find_live_refresh_tx(&mut tx, &h, now)
        .await?
        .ok_or_else(ApiError::Unauthorized)?;
    let role = state
        .store
        .role_in_tenant(&user_id, &tenant_id)
        .await?
        .ok_or_else(ApiError::Unauthorized)?;
    state.store.revoke_refresh_tx(&mut tx, &old_id).await?;
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
        .insert_refresh_tx(
            &mut tx,
            &new_id,
            &user_id,
            &tenant_id,
            &new_hash,
            now + state.cfg.refresh_ttl_secs,
            Some(&old_id),
        )
        .await?;
    tx.commit().await.map_err(recon_store::StoreError::from)?;
    let jar = jar.add(refresh_cookie(plaintext, &state.cfg));
    Ok((jar, Json(AccessTokenResp { access_token: access })))
}

pub async fn logout(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: axum::http::HeaderMap,
) -> Result<(CookieJar, axum::http::StatusCode), ApiError> {
    let audit_ip = extract_ip_headers(&headers);
    if let Some(c) = jar.get(REFRESH_COOKIE) {
        let h = recon_auth::refresh::hash(c.value());

        let mut tx = state.store.pool.begin().await.map_err(recon_store::StoreError::from)?;
        // Capture owner+tenant BEFORE revoking so we can emit the audit even if the
        // row is already gone after the UPDATE.
        let owner: Option<(String, String)> = sqlx::query_as(
            "SELECT user_id, tenant_id FROM refresh_tokens WHERE token_hash=$1 LIMIT 1",
        )
        .bind(&h)
        .fetch_optional(&mut *tx)
        .await
        .map_err(recon_store::StoreError::from)?;
        state.store.revoke_refresh_by_hash_tx(&mut tx, &h).await?;
        if let Some((user_id, tenant_id)) = owner {
            state
                .store
                .append_audit(
                    &mut tx,
                    &tenant_id,
                    &user_id,
                    recon_audit::AuditPayload::AuthLogout {
                        user_id: user_id.clone(),
                        ip: audit_ip,
                    },
                )
                .await?;
        }
        tx.commit().await.map_err(recon_store::StoreError::from)?;
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

    let from_tenant = ctx.tenant_id.clone();
    let to_tenant = req.tenant_id.clone();

    let mut tx = state.store.pool.begin().await.map_err(recon_store::StoreError::from)?;
    if let Some(c) = jar.get(REFRESH_COOKIE) {
        let h = recon_auth::refresh::hash(c.value());
        state.store.revoke_refresh_by_hash_tx(&mut tx, &h).await?;
    }
    let (jar, access) =
        issue_session_tx(&state, &mut tx, jar, &ctx.user_id, &req.tenant_id, role).await?;
    // Emit into the destination tenant's chain (the chain the user is now acting under).
    state
        .store
        .append_audit(
            &mut tx,
            &to_tenant,
            &ctx.user_id,
            recon_audit::AuditPayload::AuthTenantSwitched {
                user_id: ctx.user_id.clone(),
                from_tenant,
                to_tenant: to_tenant.clone(),
            },
        )
        .await?;
    tx.commit().await.map_err(recon_store::StoreError::from)?;
    Ok((jar, Json(AccessTokenResp { access_token: access })))
}

pub async fn change_password(
    ctx: crate::auth::AuthContext,
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ChangePasswordReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    if req.new_password.len() < 8 {
        return Err(ApiError::BadRequest());
    }
    let audit_ip = extract_ip_headers(&headers);
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

    let mut tx = state.store.pool.begin().await.map_err(recon_store::StoreError::from)?;
    state.store.set_password_tx(&mut tx, &ctx.user_id, &new_hash).await?;
    state.store.revoke_all_refresh_tx(&mut tx, &ctx.user_id).await?;
    state
        .store
        .append_audit(
            &mut tx,
            &ctx.tenant_id,
            &ctx.user_id,
            recon_audit::AuditPayload::AuthPasswordChanged {
                user_id: ctx.user_id.clone(),
                ip: audit_ip,
            },
        )
        .await?;
    tx.commit().await.map_err(recon_store::StoreError::from)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn forgot(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ForgotReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    let audit_ip = extract_ip_headers(&headers);
    if let Some((user, _cred)) = state.store.find_credential_by_email(&req.email).await? {
        let (plaintext, hash) = recon_auth::refresh::generate();
        let id = recon_auth::refresh::generate().1;
        let primary_tenant = primary_tenant_for(&state, &user.id).await?;

        let mut tx = state.store.pool.begin().await.map_err(recon_store::StoreError::from)?;
        state
            .store
            .insert_reset_token_tx(&mut tx, &id, &user.id, &hash, now_unix() + 3600)
            .await?;
        if let Some(tenant_id) = primary_tenant.as_deref() {
            state
                .store
                .append_audit(
                    &mut tx,
                    tenant_id,
                    &user.id,
                    recon_audit::AuditPayload::AuthPasswordResetRequested {
                        email: req.email.clone(),
                        ip: audit_ip.clone(),
                    },
                )
                .await?;
        }
        tx.commit().await.map_err(recon_store::StoreError::from)?;

        // Email send happens OUTSIDE the tx (external side-effect). If this fails,
        // the user retries; the audit row already accurately reflects the request.
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
    headers: axum::http::HeaderMap,
    Json(req): Json<ResetReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    if req.new_password.len() < 8 {
        return Err(ApiError::BadRequest());
    }
    let audit_ip = extract_ip_headers(&headers);
    let h = recon_auth::refresh::hash(&req.token);

    let mut tx = state.store.pool.begin().await.map_err(recon_store::StoreError::from)?;
    let user_id = state
        .store
        .consume_reset_token_tx(&mut tx, &h, now_unix())
        .await?
        .ok_or_else(ApiError::BadRequest)?;
    let new_hash =
        recon_auth::password::hash_password(&req.new_password).map_err(|_| ApiError::BadRequest())?;
    state.store.set_password_tx(&mut tx, &user_id, &new_hash).await?;
    state.store.revoke_all_refresh_tx(&mut tx, &user_id).await?;
    // For audit emission, resolve the user's primary tenant inside the tx (cheap read).
    let primary_tenant: Option<String> = sqlx::query_scalar(
        "SELECT tenant_id FROM memberships m JOIN tenants t ON t.id=m.tenant_id \
         WHERE m.user_id=$1 ORDER BY t.name LIMIT 1",
    )
    .bind(&user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(recon_store::StoreError::from)?;
    if let Some(tenant_id) = primary_tenant {
        state
            .store
            .append_audit(
                &mut tx,
                &tenant_id,
                &user_id,
                recon_audit::AuditPayload::AuthPasswordResetCompleted {
                    user_id: user_id.clone(),
                    ip: audit_ip,
                },
            )
            .await?;
    }
    tx.commit().await.map_err(recon_store::StoreError::from)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
