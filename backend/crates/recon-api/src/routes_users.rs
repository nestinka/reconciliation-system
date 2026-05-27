use axum::extract::{Path, State};
use axum::Json;
use crate::auth::AuthContext;
use crate::dto::{CreateUserReq, PatchUserReq};
use crate::error::ApiError;
use crate::state::AppState;

fn require_admin(ctx: &AuthContext) -> Result<(), ApiError> {
    recon_auth::rbac::require(ctx.role, recon_auth::rbac::Permission::ManageUsers)
        .map_err(|_| ApiError::Forbidden())
}

pub async fn list_users(
    ctx: AuthContext,
    State(state): State<AppState>,
) -> Result<Json<Vec<recon_domain::User>>, ApiError> {
    require_admin(&ctx)?;
    Ok(Json(state.store.list_users_in_tenant(&ctx.tenant_id).await?))
}

/// Non-privileged read: any authenticated tenant member can fetch the member list
/// (used by the case timeline and assignee dropdown).
pub async fn list_members(
    ctx: AuthContext,
    State(state): State<AppState>,
) -> Result<Json<Vec<recon_domain::User>>, ApiError> {
    Ok(Json(state.store.list_users_in_tenant(&ctx.tenant_id).await?))
}

pub async fn create_user(
    ctx: AuthContext,
    State(state): State<AppState>,
    Json(req): Json<CreateUserReq>,
) -> Result<(axum::http::StatusCode, Json<recon_domain::User>), ApiError> {
    require_admin(&ctx)?;
    if req.password.len() < 8 {
        return Err(ApiError::BadRequest());
    }
    let id = format!("user-{}", &recon_auth::refresh::generate().1[..12]);
    let hash = recon_auth::password::hash_password(&req.password).map_err(|_| ApiError::BadRequest())?;
    state
        .store
        .create_user_with_membership(
            &id,
            &req.name,
            &req.email,
            &hash,
            &ctx.tenant_id,
            req.role,
            &ctx.user_id,
        )
        .await?;
    let user = recon_domain::User {
        id,
        name: req.name,
        email: req.email,
        disabled: false,
        role: req.role,
    };
    Ok((axum::http::StatusCode::CREATED, Json(user)))
}

pub async fn patch_user(
    ctx: AuthContext,
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Json(req): Json<PatchUserReq>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_admin(&ctx)?;
    // Verify the target user belongs to the admin's tenant before any mutation.
    if state.store.role_in_tenant(&user_id, &ctx.tenant_id).await?.is_none() {
        return Err(ApiError::NotFound());
    }
    if let Some(role) = req.role {
        if state
            .store
            .update_membership_role(&user_id, &ctx.tenant_id, role, &ctx.user_id)
            .await?
            == 0
        {
            return Err(ApiError::NotFound());
        }
    }
    if let Some(disabled) = req.disabled {
        state
            .store
            .set_user_disabled(&user_id, disabled, &ctx.tenant_id, &ctx.user_id)
            .await?;
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn delete_user(
    ctx: AuthContext,
    State(state): State<AppState>,
    Path(user_id): Path<String>,
) -> Result<axum::http::StatusCode, ApiError> {
    require_admin(&ctx)?;
    if state
        .store
        .remove_membership(&user_id, &ctx.tenant_id, &ctx.user_id)
        .await?
        == 0
    {
        return Err(ApiError::NotFound());
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}
