use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Task {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub notes: String,
    pub completed: bool,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
    pub deferred: bool,
    pub review_interval: i64,
    pub next_review_at: Option<String>,
    pub acknowledged: bool,
    pub focused: bool,
    pub focused_at: Option<String>,
    pub budget_minutes: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskWithChildren {
    #[serde(flatten)]
    pub task: Task,
    pub children: Vec<Task>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectPreview {
    pub id: String,
    pub title: String,
    pub pending_count: i64,
    pub next_actions: Vec<Task>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTask {
    pub parent_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct UpdateTask {
    pub title: Option<String>,
    pub notes: Option<String>,
    pub completed: Option<bool>,
    pub position: Option<i64>,
    pub parent_id: Option<String>,
    pub deferred: Option<bool>,
    pub acknowledged: Option<bool>,
    pub focused: Option<bool>,
}

// ── Search ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub task: Task,
    pub path: Vec<String>, // ancestor titles from root to parent
}

// ── Mirror (Experience Sampling) ──

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Sample {
    pub id: String,
    pub prompt_type: String,
    pub response: String,
    pub created_at: String,
    pub task_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSample {
    pub response: String,
    #[serde(default = "default_prompt_type")]
    pub prompt_type: String,
    #[serde(default)]
    pub task_id: Option<String>,
}

fn default_prompt_type() -> String {
    "activity".to_string()
}

// ── Mirror with task title (for display) ──

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct SampleWithTask {
    pub id: String,
    pub prompt_type: String,
    pub response: String,
    pub created_at: String,
    pub task_id: Option<String>,
    pub task_title: Option<String>,
}
