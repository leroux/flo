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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateSample {
    pub response: String,
    #[serde(default = "default_prompt_type")]
    pub prompt_type: String,
}

fn default_prompt_type() -> String {
    "activity".to_string()
}
