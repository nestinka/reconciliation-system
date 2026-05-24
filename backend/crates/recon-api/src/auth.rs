use crate::error::ApiError;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

/// Establishes the caller's tenant from the X-Tenant-Id header.
/// This is the auth seam: a JWT validator will later populate the same struct.
pub struct AuthContext {
    pub tenant_id: String,
    pub user_id: Option<String>,
}

#[axum::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AuthContext {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let tenant_id = parts
            .headers
            .get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ApiError::unauthorized("missing X-Tenant-Id"))?
            .to_string();
        let user_id = parts
            .headers
            .get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        Ok(AuthContext { tenant_id, user_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    #[tokio::test]
    async fn extracts_tenant() {
        let req = Request::builder()
            .header("x-tenant-id", "tenant-acme")
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let ctx = AuthContext::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(ctx.tenant_id, "tenant-acme");
    }

    #[tokio::test]
    async fn missing_header_is_unauthorized() {
        let req = Request::builder().body(()).unwrap();
        let (mut parts, _) = req.into_parts();
        assert!(AuthContext::from_request_parts(&mut parts, &())
            .await
            .is_err());
    }

    #[tokio::test]
    async fn extracts_user_id_when_present() {
        let req = Request::builder()
            .header("x-tenant-id", "t")
            .header("x-user-id", "user-mia")
            .body(())
            .unwrap();
        let (mut parts, _) = req.into_parts();
        let ctx = AuthContext::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(ctx.user_id.as_deref(), Some("user-mia"));
    }
}
