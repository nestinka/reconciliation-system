use recon_api::ratelimit::IpLimiter;
use recon_api::routes::router;
use recon_api::state::{AppState, AuthConfig};
use recon_store::Store;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "recon_api=debug,info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let store = Store::connect(&database_url).await?;

    match std::env::args().nth(1).as_deref() {
        Some("seed") => {
            store.seed().await?;
            tracing::info!("seed complete");
            return Ok(());
        }
        Some("serve") | None => {}
        Some(other) => {
            eprintln!("unknown command: {other}; use serve|seed");
            std::process::exit(2);
        }
    }

    store.migrate().await?;

    let web_origin = std::env::var("WEB_ORIGIN").unwrap_or_else(|_| "http://localhost:3100".into());

    let cors = CorsLayer::new()
        .allow_origin(web_origin.parse::<axum::http::HeaderValue>().expect("WEB_ORIGIN must be a valid origin header value"))
        .allow_credentials(true)
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PATCH,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::AUTHORIZATION,
            axum::http::header::CONTENT_TYPE,
        ]);

    // Choose mailer based on environment.
    let mailer: Arc<dyn recon_mail::Mailer> = if let Ok(host) = std::env::var("SMTP_HOST") {
        let port: u16 = std::env::var("SMTP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(1025);
        let from = std::env::var("SMTP_FROM").unwrap_or_else(|_| "recon@example.com".into());
        Arc::new(recon_mail::SmtpMailer::new(host, port, from))
    } else {
        Arc::new(recon_mail::LogMailer)
    };

    // In RECON_DEV mode use a very large bucket so E2E test suites aren't
    // blocked by the IP rate limiter when running many login flows in quick
    // succession.  Production keeps the tight 10-req/min limit.
    let login_limiter = if std::env::var("RECON_DEV").is_ok() {
        Arc::new(IpLimiter::new(1000.0, 1000.0))
    } else {
        Arc::new(IpLimiter::new(10.0, 10.0 / 60.0))
    };

    let app = router(AppState {
        store,
        cfg: Arc::new(AuthConfig::from_env()),
        mailer,
        login_limiter,
    })
    .layer(TraceLayer::new_for_http())
    .layer(cors);

    let bind = std::env::var("API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "recon-api listening");
    axum::serve(listener, app).await?;
    Ok(())
}
