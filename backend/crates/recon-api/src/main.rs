use recon_api::routes::router;
use recon_api::state::AppState;
use recon_store::Store;
use tower_http::cors::{Any, CorsLayer};
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
        .allow_origin(web_origin.parse::<axum::http::HeaderValue>().unwrap())
        .allow_methods(Any)
        .allow_headers(Any);

    let app = router(AppState { store })
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let bind = std::env::var("API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "recon-api listening");
    axum::serve(listener, app).await?;
    Ok(())
}
