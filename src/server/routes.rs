use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use super::AppState;
use crate::db;
use crate::models::{CreateTask, UpdateTask};

#[derive(Deserialize)]
pub struct ListParams {
    parent_id: Option<String>,
}

#[derive(Deserialize)]
pub struct SearchParams {
    q: String,
}

pub fn api_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health))
        .route("/home", get(home))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/{id}", get(get_task).patch(update_task).delete(delete_task))
        .route("/tasks/{id}/subtree", get(get_subtree))
        .route("/tasks/{id}/ancestors", get(get_ancestors))
        .route("/search", get(search))
        // Mirror
        .route("/samples", get(list_samples).post(create_sample))
}

async fn health() -> &'static str {
    "ok"
}

async fn home(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match db::get_home(&state.pool).await {
        Ok(previews) => Json(previews).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_tasks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    match db::get_children(&state.pool, params.parent_id.as_deref()).await {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn create_task(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateTask>,
) -> impl IntoResponse {
    match db::create_task(&state.pool, &input).await {
        Ok(task) => (StatusCode::CREATED, Json(task)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match db::get_task_with_children(&state.pool, &id).await {
        Ok(task) => Json(task).into_response(),
        Err(e) => (StatusCode::NOT_FOUND, e.to_string()).into_response(),
    }
}

async fn update_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTask>,
) -> impl IntoResponse {
    match db::update_task(&state.pool, &id, &input).await {
        Ok(task) => Json(task).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match db::delete_task(&state.pool, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_subtree(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match db::get_subtree(&state.pool, &id).await {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_ancestors(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match db::get_ancestors(&state.pool, &id).await {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// ── Mirror ──

async fn create_sample(
    State(state): State<Arc<AppState>>,
    Json(input): Json<crate::models::CreateSample>,
) -> impl IntoResponse {
    match db::create_sample(&state.pool, &input).await {
        Ok(sample) => (StatusCode::CREATED, Json(sample)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_samples(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match db::get_samples_today(&state.pool).await {
        Ok(samples) => Json(samples).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> impl IntoResponse {
    match db::search_tasks(&state.pool, &params.q).await {
        Ok(results) => Json(results).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
