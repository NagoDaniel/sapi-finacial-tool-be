mod config;
mod db;
mod api;

use anyhow::Result;
use axum::{routing::get, Router};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Settings;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();
    let settings = Settings::from_env()?;
    let pool = db::connect(&settings.database_url).await?;
    db::migrate(&pool).await?;

    let app = Router::new()
        .merge(api::incomes::router())
        .route("/health", get(api::health::health_check))
        .layer(TraceLayer::new_for_http())
        .with_state(pool);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", settings.port)).await?;
    tracing::info!("Server listening on http://0.0.0.0:{}", settings.port);

    axum::serve(listener, app).await?;
    Ok(())
}
