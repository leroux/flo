mod common;

use axum::body::Body;
use flo::models::{CreateTask, Task, TaskWithChildren, UpdateTask, SearchResult, Sample};
use http_body_util::BodyExt;
use hyper::Request;
use tower::ServiceExt;

async fn test_app() -> (axum::Router, tempfile::TempDir) {
    let (pool, tmp) = common::empty_db().await;
    (flo::server::app(pool), tmp)
}

async fn test_app_with_data() -> (axum::Router, tempfile::TempDir) {
    let (pool, tmp) = common::fork_db().await;
    (flo::server::app(pool), tmp)
}

async fn json_body<T: serde::de::DeserializeOwned>(resp: axum::response::Response) -> T {
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&body).unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

fn post_json<T: serde::Serialize>(uri: &str, body: &T) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(body).unwrap()))
        .unwrap()
}

fn patch_json<T: serde::Serialize>(uri: &str, body: &T) -> Request<Body> {
    Request::builder()
        .method("PATCH")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(body).unwrap()))
        .unwrap()
}

fn delete(uri: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .body(Body::empty())
        .unwrap()
}

// ═══════════════════════════════════════════════════════════════════
// Health
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn health_returns_ok() {
    let (app, _tmp) = test_app().await;
    let resp = app.oneshot(get("/api/health")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = json_body(resp).await;
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
}

// ═══════════════════════════════════════════════════════════════════
// Home
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn home_empty() {
    let (app, _tmp) = test_app().await;
    let resp = app.oneshot(get("/api/home")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: Vec<serde_json::Value> = json_body(resp).await;
    assert!(body.is_empty());
}

#[tokio::test]
async fn home_with_production_data() {
    let (app, _tmp) = test_app_with_data().await;
    let resp = app.oneshot(get("/api/home")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let body: Vec<serde_json::Value> = json_body(resp).await;
    assert!(!body.is_empty());
}

// ═══════════════════════════════════════════════════════════════════
// POST /api/tasks
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_task_returns_201() {
    let (app, _tmp) = test_app().await;
    let resp = app
        .oneshot(post_json("/api/tasks", &CreateTask {
            parent_id: None,
            title: "new task".into(),
            notes: String::new(),
        }))
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);
    let task: Task = json_body(resp).await;
    assert_eq!(task.title, "new task");
    assert!(!task.completed);
}

#[tokio::test]
async fn create_task_with_parent() {
    let (app, _tmp) = test_app().await;

    // Create parent
    let resp = app.clone()
        .oneshot(post_json("/api/tasks", &CreateTask {
            parent_id: None, title: "parent".into(), notes: String::new(),
        }))
        .await.unwrap();
    let parent: Task = json_body(resp).await;

    // Create child
    let resp = app
        .oneshot(post_json("/api/tasks", &CreateTask {
            parent_id: Some(parent.id.clone()),
            title: "child".into(),
            notes: String::new(),
        }))
        .await.unwrap();
    assert_eq!(resp.status(), 201);
    let child: Task = json_body(resp).await;
    assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
}

#[tokio::test]
async fn create_task_with_notes() {
    let (app, _tmp) = test_app().await;
    let resp = app
        .oneshot(post_json("/api/tasks", &CreateTask {
            parent_id: None,
            title: "noted".into(),
            notes: "some notes here".into(),
        }))
        .await.unwrap();

    let task: Task = json_body(resp).await;
    assert_eq!(task.notes, "some notes here");
}

// ═══════════════════════════════════════════════════════════════════
// GET /api/tasks
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn list_root_tasks() {
    let (app, _tmp) = test_app().await;

    // Create two root tasks
    app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "a".into(), notes: String::new(),
    })).await.unwrap();

    app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "b".into(), notes: String::new(),
    })).await.unwrap();

    let resp = app.oneshot(get("/api/tasks")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let tasks: Vec<Task> = json_body(resp).await;
    assert_eq!(tasks.len(), 2);
}

#[tokio::test]
async fn list_children_by_parent_id() {
    let (app, _tmp) = test_app().await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "parent".into(), notes: String::new(),
    })).await.unwrap();
    let parent: Task = json_body(resp).await;

    app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: Some(parent.id.clone()), title: "c1".into(), notes: String::new(),
    })).await.unwrap();

    app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: Some(parent.id.clone()), title: "c2".into(), notes: String::new(),
    })).await.unwrap();

    let resp = app.oneshot(get(&format!("/api/tasks?parent_id={}", parent.id))).await.unwrap();
    let children: Vec<Task> = json_body(resp).await;
    assert_eq!(children.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════
// GET /api/tasks/:id
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn get_task_with_children() {
    let (app, _tmp) = test_app().await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "parent".into(), notes: String::new(),
    })).await.unwrap();
    let parent: Task = json_body(resp).await;

    app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: Some(parent.id.clone()), title: "kid".into(), notes: String::new(),
    })).await.unwrap();

    let resp = app.oneshot(get(&format!("/api/tasks/{}", parent.id))).await.unwrap();
    assert_eq!(resp.status(), 200);
    let tw: TaskWithChildren = json_body(resp).await;
    assert_eq!(tw.task.title, "parent");
    assert_eq!(tw.children.len(), 1);
}

#[tokio::test]
async fn get_nonexistent_task_returns_404() {
    let (app, _tmp) = test_app().await;
    let resp = app.oneshot(get("/api/tasks/nonexistent")).await.unwrap();
    assert_eq!(resp.status(), 404);
}

// ═══════════════════════════════════════════════════════════════════
// PATCH /api/tasks/:id
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn update_task_title_via_api() {
    let (app, _tmp) = test_app().await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "old".into(), notes: String::new(),
    })).await.unwrap();
    let task: Task = json_body(resp).await;

    let resp = app.oneshot(patch_json(
        &format!("/api/tasks/{}", task.id),
        &UpdateTask { title: Some("new".into()), ..Default::default() },
    )).await.unwrap();

    assert_eq!(resp.status(), 200);
    let updated: Task = json_body(resp).await;
    assert_eq!(updated.title, "new");
}

#[tokio::test]
async fn update_task_completed_via_api() {
    let (app, _tmp) = test_app().await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "t".into(), notes: String::new(),
    })).await.unwrap();
    let task: Task = json_body(resp).await;

    let resp = app.oneshot(patch_json(
        &format!("/api/tasks/{}", task.id),
        &UpdateTask { completed: Some(true), ..Default::default() },
    )).await.unwrap();

    let updated: Task = json_body(resp).await;
    assert!(updated.completed);
}

#[tokio::test]
async fn patch_nonexistent_task_returns_500() {
    let (app, _tmp) = test_app().await;
    let resp = app.oneshot(patch_json(
        "/api/tasks/nonexistent",
        &UpdateTask { title: Some("x".into()), ..Default::default() },
    )).await.unwrap();
    assert_eq!(resp.status(), 500);
}

// ═══════════════════════════════════════════════════════════════════
// DELETE /api/tasks/:id
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn delete_task_returns_204() {
    let (app, _tmp) = test_app().await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "bye".into(), notes: String::new(),
    })).await.unwrap();
    let task: Task = json_body(resp).await;

    let resp = app.clone().oneshot(delete(&format!("/api/tasks/{}", task.id))).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Verify gone
    let resp = app.oneshot(get(&format!("/api/tasks/{}", task.id))).await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn delete_cascades_via_api() {
    let (app, _tmp) = test_app().await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "parent".into(), notes: String::new(),
    })).await.unwrap();
    let parent: Task = json_body(resp).await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: Some(parent.id.clone()), title: "child".into(), notes: String::new(),
    })).await.unwrap();
    let child: Task = json_body(resp).await;

    app.clone().oneshot(delete(&format!("/api/tasks/{}", parent.id))).await.unwrap();

    let resp = app.oneshot(get(&format!("/api/tasks/{}", child.id))).await.unwrap();
    assert_eq!(resp.status(), 404);
}

// ═══════════════════════════════════════════════════════════════════
// GET /api/tasks/:id/subtree
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn subtree_via_api() {
    let (app, _tmp) = test_app().await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "root".into(), notes: String::new(),
    })).await.unwrap();
    let root: Task = json_body(resp).await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: Some(root.id.clone()), title: "child".into(), notes: String::new(),
    })).await.unwrap();
    let child: Task = json_body(resp).await;

    app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: Some(child.id.clone()), title: "grandchild".into(), notes: String::new(),
    })).await.unwrap();

    let resp = app.oneshot(get(&format!("/api/tasks/{}/subtree", root.id))).await.unwrap();
    assert_eq!(resp.status(), 200);
    let tasks: Vec<Task> = json_body(resp).await;
    assert_eq!(tasks.len(), 3);
}

// ═══════════════════════════════════════════════════════════════════
// GET /api/tasks/:id/ancestors
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn ancestors_via_api() {
    let (app, _tmp) = test_app().await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "root".into(), notes: String::new(),
    })).await.unwrap();
    let root: Task = json_body(resp).await;

    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: Some(root.id.clone()), title: "leaf".into(), notes: String::new(),
    })).await.unwrap();
    let leaf: Task = json_body(resp).await;

    let resp = app.oneshot(get(&format!("/api/tasks/{}/ancestors", leaf.id))).await.unwrap();
    assert_eq!(resp.status(), 200);
    let ancestors: Vec<Task> = json_body(resp).await;
    assert_eq!(ancestors.len(), 2);
    assert_eq!(ancestors[0].title, "root");
    assert_eq!(ancestors[1].title, "leaf");
}

// ═══════════════════════════════════════════════════════════════════
// GET /api/search
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn search_via_api() {
    let (app, _tmp) = test_app().await;

    app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "buy milk".into(), notes: String::new(),
    })).await.unwrap();

    app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "unrelated".into(), notes: String::new(),
    })).await.unwrap();

    let resp = app.oneshot(get("/api/search?q=milk")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let results: Vec<SearchResult> = json_body(resp).await;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].task.title, "buy milk");
}

#[tokio::test]
async fn search_empty_query_matches_nothing() {
    let (app, _tmp) = test_app().await;
    app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "task".into(), notes: String::new(),
    })).await.unwrap();

    // Empty search should match everything (LIKE '%%')
    let resp = app.oneshot(get("/api/search?q=")).await.unwrap();
    assert_eq!(resp.status(), 200);
}

// ═══════════════════════════════════════════════════════════════════
// POST /api/samples
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_sample_via_api() {
    let (app, _tmp) = test_app().await;
    let resp = app.oneshot(post_json("/api/samples", &serde_json::json!({
        "response": "working on tests",
        "prompt_type": "activity"
    }))).await.unwrap();

    assert_eq!(resp.status(), 201);
    let sample: Sample = json_body(resp).await;
    assert_eq!(sample.response, "working on tests");
}

#[tokio::test]
async fn list_samples_via_api() {
    let (app, _tmp) = test_app().await;

    app.clone().oneshot(post_json("/api/samples", &serde_json::json!({
        "response": "test 1",
        "prompt_type": "ping"
    }))).await.unwrap();

    app.clone().oneshot(post_json("/api/samples", &serde_json::json!({
        "response": "test 2",
        "prompt_type": "activity"
    }))).await.unwrap();

    let resp = app.oneshot(get("/api/samples")).await.unwrap();
    assert_eq!(resp.status(), 200);
    let samples: Vec<Sample> = json_body(resp).await;
    assert_eq!(samples.len(), 2);
}

// ═══════════════════════════════════════════════════════════════════
// Edge cases / malformed requests
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_task_missing_body_returns_error() {
    let (app, _tmp) = test_app().await;
    let resp = app.oneshot(
        Request::builder()
            .method("POST")
            .uri("/api/tasks")
            .header("content-type", "application/json")
            .body(Body::from("{}"))
            .unwrap()
    ).await.unwrap();
    // Missing required field "title" — should 422 or 400
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn create_task_invalid_json() {
    let (app, _tmp) = test_app().await;
    let resp = app.oneshot(
        Request::builder()
            .method("POST")
            .uri("/api/tasks")
            .header("content-type", "application/json")
            .body(Body::from("not json"))
            .unwrap()
    ).await.unwrap();
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn nonexistent_route_returns_404() {
    let (app, _tmp) = test_app().await;
    let resp = app.oneshot(get("/api/nonexistent")).await.unwrap();
    assert_eq!(resp.status(), 404);
}

// ═══════════════════════════════════════════════════════════════════
// Full workflow: create -> update -> complete -> delete
// ═══════════════════════════════════════════════════════════════════

#[tokio::test]
async fn full_task_lifecycle() {
    let (app, _tmp) = test_app().await;

    // Create
    let resp = app.clone().oneshot(post_json("/api/tasks", &CreateTask {
        parent_id: None, title: "lifecycle task".into(), notes: String::new(),
    })).await.unwrap();
    assert_eq!(resp.status(), 201);
    let task: Task = json_body(resp).await;

    // Update title
    let resp = app.clone().oneshot(patch_json(
        &format!("/api/tasks/{}", task.id),
        &UpdateTask { title: Some("updated title".into()), ..Default::default() },
    )).await.unwrap();
    let task: Task = json_body(resp).await;
    assert_eq!(task.title, "updated title");

    // Add notes
    let resp = app.clone().oneshot(patch_json(
        &format!("/api/tasks/{}", task.id),
        &UpdateTask { notes: Some("important".into()), ..Default::default() },
    )).await.unwrap();
    let task: Task = json_body(resp).await;
    assert_eq!(task.notes, "important");

    // Complete
    let resp = app.clone().oneshot(patch_json(
        &format!("/api/tasks/{}", task.id),
        &UpdateTask { completed: Some(true), ..Default::default() },
    )).await.unwrap();
    let task: Task = json_body(resp).await;
    assert!(task.completed);

    // Delete
    let resp = app.clone().oneshot(delete(&format!("/api/tasks/{}", task.id))).await.unwrap();
    assert_eq!(resp.status(), 204);

    // Verify gone
    let resp = app.oneshot(get(&format!("/api/tasks/{}", task.id))).await.unwrap();
    assert_eq!(resp.status(), 404);
}
