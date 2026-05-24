use std::sync::Arc;
use recon_mail::Mailer;

#[derive(Clone)]
pub struct AppState {
    pub store: recon_store::Store,
    pub cfg: Arc<AuthConfig>,
    pub mailer: Arc<dyn Mailer>,
    pub login_limiter: Arc<crate::ratelimit::IpLimiter>,
}

pub struct AuthConfig {
    pub jwt_secret: Vec<u8>,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
    pub app_base_url: String,
    pub secure_cookie: bool,
}

impl AuthConfig {
    pub fn from_env() -> Self {
        const DEV_SECRET: &str = "dev-insecure-secret-change-me";
        let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
            tracing::warn!("JWT_SECRET unset — using insecure dev secret");
            DEV_SECRET.into()
        });
        if secret == DEV_SECRET {
            tracing::warn!("JWT_SECRET is set to the known dev fallback — use a strong secret in production");
        }
        Self {
            jwt_secret: secret.into_bytes(),
            access_ttl_secs: 900,
            refresh_ttl_secs: 2_592_000,
            app_base_url: std::env::var("APP_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:3100".into()),
            secure_cookie: std::env::var("SECURE_COOKIE")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false),
        }
    }

    /// Fixed config for tests (deterministic secret).
    pub fn test() -> Self {
        Self {
            jwt_secret: b"test-secret".to_vec(),
            access_ttl_secs: 900,
            refresh_ttl_secs: 2_592_000,
            app_base_url: "http://localhost:3100".into(),
            secure_cookie: false,
        }
    }
}
