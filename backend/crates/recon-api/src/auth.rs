use recon_domain::UserRole;

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub user_id: String,
    pub tenant_id: String,
    pub role: UserRole,
}

#[axum::async_trait]
impl axum::extract::FromRequestParts<crate::state::AppState> for AuthContext {
    type Rejection = crate::error::ApiError;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let token = header
            .strip_prefix("Bearer ")
            .ok_or(crate::error::ApiError::unauthorized("missing or invalid Authorization header"))?;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let claims = recon_auth::token::decode_access(&state.cfg.jwt_secret, token, now)
            .map_err(|_| crate::error::ApiError::unauthorized("invalid or expired token"))?;
        Ok(AuthContext {
            user_id: claims.sub,
            tenant_id: claims.tid,
            role: claims.role,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::FromRequestParts;
    use axum::http::Request;
    use std::sync::Arc;

    fn make_state() -> crate::state::AppState {
        let cfg = Arc::new(crate::state::AuthConfig::test());
        let pool = sqlx::PgPool::connect_lazy("postgres://recon:recon@localhost:5432/recon").unwrap();
        crate::state::AppState {
            store: recon_store::Store::from_pool(pool),
            cfg,
            mailer: Arc::new(recon_mail::LogMailer),
        }
    }

    fn make_token(user_id: &str, tenant_id: &str) -> String {
        let cfg = crate::state::AuthConfig::test();
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        recon_auth::token::encode_access(
            &cfg.jwt_secret,
            user_id,
            tenant_id,
            recon_domain::UserRole::Operator,
            cfg.access_ttl_secs,
            now,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn extracts_tenant_and_user_from_bearer_token() {
        let state = make_state();
        let token = make_token("user-mia", "tenant-acme");
        let req = Request::builder()
            .header("authorization", format!("Bearer {token}"))
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let ctx = AuthContext::from_request_parts(&mut parts, &state)
            .await
            .unwrap();
        assert_eq!(ctx.tenant_id, "tenant-acme");
        assert_eq!(ctx.user_id, "user-mia");
    }

    #[tokio::test]
    async fn missing_header_is_unauthorized() {
        let state = make_state();
        let req = Request::builder().body(()).unwrap();
        let (mut parts, _) = req.into_parts();
        assert!(AuthContext::from_request_parts(&mut parts, &state)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn invalid_token_is_unauthorized() {
        let state = make_state();
        let req = Request::builder()
            .header("authorization", "Bearer not-a-valid-token")
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        assert!(AuthContext::from_request_parts(&mut parts, &state)
            .await
            .is_err());
    }
}
