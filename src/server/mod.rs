mod routes;

use anyhow::Result;
use axum::Router;
use sqlx::SqlitePool;
use std::sync::Arc;
use tower_http::cors::CorsLayer;

pub struct AppState {
    pub pool: SqlitePool,
}

pub async fn run(pool: SqlitePool, port: u16) -> Result<()> {
    let state = Arc::new(AppState { pool });

    let app = Router::new()
        .nest("/api", routes::api_routes())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = format!("127.0.0.1:{}", port);
    eprintln!("flo server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
