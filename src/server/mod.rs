pub mod routes;

use anyhow::Result;
use axum::Router;
use sqlx::SqlitePool;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tracing::info;

pub struct AppState {
    pub pool: SqlitePool,
}

/// Build the axum Router (useful for testing without binding to a port).
pub fn app(pool: SqlitePool) -> Router {
    let state = Arc::new(AppState { pool });
    Router::new()
        .nest("/api", routes::api_routes())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub async fn run(pool: SqlitePool, port: u16) -> Result<()> {
    let app = app(pool);

    let addr = format!("127.0.0.1:{}", port);
    info!(port, "flo server starting");
    eprintln!("flo server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
