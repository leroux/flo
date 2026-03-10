use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{error, info};

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
        .route("/tasks/{id}/defer", post(defer_task_route))
        .route("/tasks/{id}/snooze", post(snooze_task))
        .route("/tasks/{id}/touch", post(touch_task_route))
        .route("/tasks/{id}/ack", post(ack_task_route))
        .route("/tasks/{id}/focus", post(focus_task_route))
        .route("/review", get(review_tasks))
        .route("/focus", get(get_focused))
        .route("/search", get(search))
        // Mirror
        .route("/samples", get(list_samples).post(create_sample))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": crate::version(),
    }))
}

async fn home(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match db::get_home(&state.pool).await {
        Ok(previews) => Json(previews).into_response(),
        Err(e) => {
            error!(error = %e, "GET /home failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn list_tasks(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListParams>,
) -> impl IntoResponse {
    info!(parent_id = ?params.parent_id, "GET /tasks");
    match db::get_children(&state.pool, params.parent_id.as_deref()).await {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => {
            error!(error = %e, "GET /tasks failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn create_task(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateTask>,
) -> impl IntoResponse {
    info!(title = %input.title, parent_id = ?input.parent_id, "POST /tasks");
    match db::create_task(&state.pool, &input).await {
        Ok(task) => (StatusCode::CREATED, Json(task)).into_response(),
        Err(e) => {
            error!(error = %e, "POST /tasks failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn get_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    info!(id = %id, "GET /tasks/:id");
    match db::get_task_with_children(&state.pool, &id).await {
        Ok(task) => Json(task).into_response(),
        Err(e) => {
            error!(id = %id, error = %e, "GET /tasks/:id failed");
            (StatusCode::NOT_FOUND, e.to_string()).into_response()
        }
    }
}

async fn update_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(input): Json<UpdateTask>,
) -> impl IntoResponse {
    info!(id = %id, "PATCH /tasks/:id");
    match db::update_task(&state.pool, &id, &input).await {
        Ok(task) => Json(task).into_response(),
        Err(e) => {
            error!(id = %id, error = %e, "PATCH /tasks/:id failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn delete_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    info!(id = %id, "DELETE /tasks/:id");
    match db::delete_task(&state.pool, &id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            error!(id = %id, error = %e, "DELETE /tasks/:id failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn get_subtree(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    info!(id = %id, "GET /tasks/:id/subtree");
    match db::get_subtree(&state.pool, &id).await {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => {
            error!(id = %id, error = %e, "GET /tasks/:id/subtree failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn get_ancestors(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    info!(id = %id, "GET /tasks/:id/ancestors");
    match db::get_ancestors(&state.pool, &id).await {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => {
            error!(id = %id, error = %e, "GET /tasks/:id/ancestors failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

// ── Defer & Review ──

async fn defer_task_route(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    info!(id = %id, "POST /tasks/:id/defer");
    match db::defer_task(&state.pool, &id).await {
        Ok(task) => Json(task).into_response(),
        Err(e) => {
            error!(id = %id, error = %e, "POST /tasks/:id/defer failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn snooze_task(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    info!(id = %id, "POST /tasks/:id/snooze");
    match db::snooze_review(&state.pool, &id).await {
        Ok(task) => Json(task).into_response(),
        Err(e) => {
            error!(id = %id, error = %e, "POST /tasks/:id/snooze failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn review_tasks(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    info!("GET /review");
    match db::get_review_tasks(&state.pool).await {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => {
            error!(error = %e, "GET /review failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

// ── Touch ──

#[derive(Deserialize, Default)]
struct TouchBody {
    response: Option<String>,
}

async fn touch_task_route(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Option<Json<TouchBody>>,
) -> impl IntoResponse {
    info!(id = %id, "POST /tasks/:id/touch");
    let response = body.and_then(|b| b.response.clone());
    match db::touch_task(&state.pool, &id, response.as_deref()).await {
        Ok(task) => Json(task).into_response(),
        Err(e) => {
            error!(id = %id, error = %e, "POST /tasks/:id/touch failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

// ── Inbox / Acknowledge ──

async fn ack_task_route(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    info!(id = %id, "POST /tasks/:id/ack");
    match db::acknowledge_task(&state.pool, &id).await {
        Ok(task) => Json(task).into_response(),
        Err(e) => {
            error!(id = %id, error = %e, "POST /tasks/:id/ack failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

// ── Focus ──

#[derive(Deserialize, Default)]
struct FocusBody {
    budget_minutes: Option<i64>,
}

async fn focus_task_route(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Option<Json<FocusBody>>,
) -> impl IntoResponse {
    info!(id = %id, "POST /tasks/:id/focus");
    let budget = body.and_then(|b| b.budget_minutes);
    match db::focus_task(&state.pool, &id, budget).await {
        Ok(task) => Json(task).into_response(),
        Err(e) => {
            error!(id = %id, error = %e, "POST /tasks/:id/focus failed");
            (StatusCode::BAD_REQUEST, e.to_string()).into_response()
        }
    }
}

async fn get_focused(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    info!("GET /focus");
    match db::get_focused_tasks(&state.pool).await {
        Ok(tasks) => Json(tasks).into_response(),
        Err(e) => {
            error!(error = %e, "GET /focus failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

// ── Mirror ──

async fn create_sample(
    State(state): State<Arc<AppState>>,
    Json(input): Json<crate::models::CreateSample>,
) -> impl IntoResponse {
    info!(prompt_type = %input.prompt_type, "POST /samples");
    match db::create_sample(&state.pool, &input).await {
        Ok(sample) => (StatusCode::CREATED, Json(sample)).into_response(),
        Err(e) => {
            error!(error = %e, "POST /samples failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn list_samples(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    info!("GET /samples");
    match db::get_samples_today(&state.pool).await {
        Ok(samples) => Json(samples).into_response(),
        Err(e) => {
            error!(error = %e, "GET /samples failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}

async fn search(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SearchParams>,
) -> impl IntoResponse {
    info!(query = %params.q, "GET /search");
    match db::search_tasks(&state.pool, &params.q).await {
        Ok(results) => Json(results).into_response(),
        Err(e) => {
            error!(query = %params.q, error = %e, "GET /search failed");
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        }
    }
}
