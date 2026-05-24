pub mod auth;
pub mod dto;
pub mod error;
pub mod ratelimit;
pub mod routes;
pub mod routes_auth;
pub mod state;

/// Build a test app + return the AuthConfig so tests can mint matching tokens.
pub fn test_app(pool: sqlx::PgPool) -> (axum::Router, std::sync::Arc<crate::state::AuthConfig>) {
    use std::sync::Arc;
    let cfg = Arc::new(crate::state::AuthConfig::test());
    let state = crate::state::AppState {
        store: recon_store::Store::from_pool(pool),
        cfg: cfg.clone(),
        mailer: Arc::new(recon_mail::LogMailer),
        login_limiter: Arc::new(crate::ratelimit::IpLimiter::new(100.0, 1.0)),
    };
    (crate::routes::router(state), cfg)
}
