use anyhow::Result;
use sqlx::SqlitePool;
use ulid::Ulid;

use crate::models::{CreateTask, ProjectPreview, Task, TaskWithChildren, UpdateTask};

pub async fn init(pool: &SqlitePool) -> Result<()> {
    sqlx::query(include_str!("../../migrations/001_init.sql"))
        .execute(pool)
        .await?;
    sqlx::query(include_str!("../../migrations/002_samples.sql"))
        .execute(pool)
        .await?;
    // Migration 003: defer + review columns (idempotent via IF NOT EXISTS-style check)
    // ALTER TABLE doesn't support IF NOT EXISTS, so we check the schema first
    let has_deferred: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('tasks') WHERE name = 'deferred'"
    )
    .fetch_one(pool)
    .await?;
    if !has_deferred {
        for stmt in include_str!("../../migrations/003_defer_review.sql").split(';') {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt).execute(pool).await?;
            }
        }
    }
    // Migration 004: ESM linking, inbox, focus slots, time budgets
    let has_acknowledged: bool = sqlx::query_scalar(
        "SELECT COUNT(*) > 0 FROM pragma_table_info('tasks') WHERE name = 'acknowledged'"
    )
    .fetch_one(pool)
    .await?;
    if !has_acknowledged {
        for stmt in include_str!("../../migrations/004_esm_linking_inbox_focus.sql").split(';') {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                sqlx::query(stmt).execute(pool).await?;
            }
        }
    }
    Ok(())
}

pub async fn create_task(pool: &SqlitePool, input: &CreateTask) -> Result<Task> {
    let id = Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    // Get next position among siblings
    let position: i64 = match &input.parent_id {
        Some(pid) => {
            sqlx::query_scalar("SELECT COALESCE(MAX(position), -1) + 1 FROM tasks WHERE parent_id = ?")
                .bind(pid)
                .fetch_one(pool)
                .await?
        }
        None => {
            sqlx::query_scalar("SELECT COALESCE(MAX(position), -1) + 1 FROM tasks WHERE parent_id IS NULL")
                .fetch_one(pool)
                .await?
        }
    };

    sqlx::query(
        "INSERT INTO tasks (id, parent_id, title, notes, completed, position, created_at, updated_at, acknowledged)
         VALUES (?, ?, ?, ?, FALSE, ?, ?, ?, FALSE)"
    )
    .bind(&id)
    .bind(&input.parent_id)
    .bind(&input.title)
    .bind(&input.notes)
    .bind(position)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    get_task(pool, &id).await
}

pub async fn get_task(pool: &SqlitePool, id: &str) -> Result<Task> {
    let task = sqlx::query_as::<_, Task>("SELECT * FROM tasks WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await?;
    Ok(task)
}

pub async fn get_task_with_children(pool: &SqlitePool, id: &str) -> Result<TaskWithChildren> {
    let task = get_task(pool, id).await?;
    let children = get_children(pool, Some(id)).await?;
    Ok(TaskWithChildren { task, children })
}

pub async fn get_children(pool: &SqlitePool, parent_id: Option<&str>) -> Result<Vec<Task>> {
    let tasks = match parent_id {
        Some(pid) => {
            sqlx::query_as::<_, Task>(
                "SELECT * FROM tasks WHERE parent_id = ? ORDER BY position"
            )
            .bind(pid)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, Task>(
                "SELECT * FROM tasks WHERE parent_id IS NULL ORDER BY position"
            )
            .fetch_all(pool)
            .await?
        }
    };
    Ok(tasks)
}

#[allow(dead_code)]
pub async fn get_pending_children(pool: &SqlitePool, parent_id: Option<&str>) -> Result<Vec<Task>> {
    let tasks = match parent_id {
        Some(pid) => {
            sqlx::query_as::<_, Task>(
                "SELECT * FROM tasks WHERE parent_id = ? AND completed = FALSE ORDER BY position"
            )
            .bind(pid)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, Task>(
                "SELECT * FROM tasks WHERE parent_id IS NULL AND completed = FALSE ORDER BY position"
            )
            .fetch_all(pool)
            .await?
        }
    };
    Ok(tasks)
}

pub async fn update_task(pool: &SqlitePool, id: &str, input: &UpdateTask) -> Result<Task> {
    let existing = get_task(pool, id).await?;
    let now = chrono::Utc::now().to_rfc3339();

    let title = input.title.as_deref().unwrap_or(&existing.title);
    let notes = input.notes.as_deref().unwrap_or(&existing.notes);
    let completed = input.completed.unwrap_or(existing.completed);
    let position = input.position.unwrap_or(existing.position);
    let deferred = input.deferred.unwrap_or(existing.deferred);
    let acknowledged = input.acknowledged.unwrap_or(existing.acknowledged);
    // parent_id: None = keep existing, Some("") = set to root (NULL), Some(id) = reparent
    let parent_id = match input.parent_id.as_deref() {
        None => existing.parent_id.as_deref(),
        Some("") => None,
        Some(id) => Some(id),
    };

    sqlx::query(
        "UPDATE tasks SET title = ?, notes = ?, completed = ?, position = ?, parent_id = ?, deferred = ?, acknowledged = ?, updated_at = ? WHERE id = ?"
    )
    .bind(title)
    .bind(notes)
    .bind(completed)
    .bind(position)
    .bind(parent_id)
    .bind(deferred)
    .bind(acknowledged)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await?;

    get_task(pool, id).await
}

pub async fn delete_task(pool: &SqlitePool, id: &str) -> Result<()> {
    // CASCADE handles children
    sqlx::query("DELETE FROM tasks WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_ancestors(pool: &SqlitePool, id: &str) -> Result<Vec<Task>> {
    let tasks = sqlx::query_as::<_, Task>(
        "WITH RECURSIVE ancestors AS (
            SELECT * FROM tasks WHERE id = ?
            UNION ALL
            SELECT t.* FROM tasks t
            JOIN ancestors a ON t.id = a.parent_id
        )
        SELECT * FROM ancestors ORDER BY created_at"
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    Ok(tasks)
}

pub async fn get_home(pool: &SqlitePool) -> Result<Vec<ProjectPreview>> {
    let roots = get_children(pool, None).await?;
    let mut previews = Vec::new();

    for root in roots {
        let pending_count: i64 = sqlx::query_scalar(
            "WITH RECURSIVE subtree AS (
                SELECT id FROM tasks WHERE parent_id = ?
                UNION ALL
                SELECT t.id FROM tasks t
                JOIN subtree s ON t.parent_id = s.id
            )
            SELECT COUNT(*) FROM subtree s
            JOIN tasks t ON t.id = s.id
            WHERE t.completed = FALSE"
        )
        .bind(&root.id)
        .fetch_one(pool)
        .await?;

        let next_actions = sqlx::query_as::<_, Task>(
            "SELECT * FROM tasks WHERE parent_id = ? AND completed = FALSE ORDER BY position LIMIT 2"
        )
        .bind(&root.id)
        .fetch_all(pool)
        .await?;

        previews.push(ProjectPreview {
            id: root.id,
            title: root.title,
            pending_count,
            next_actions,
        });
    }

    Ok(previews)
}

pub async fn get_subtree(pool: &SqlitePool, id: &str) -> Result<Vec<Task>> {
    let tasks = sqlx::query_as::<_, Task>(
        "WITH RECURSIVE subtree AS (
            SELECT *, 0 as depth FROM tasks WHERE id = ?
            UNION ALL
            SELECT t.*, s.depth + 1 FROM tasks t
            JOIN subtree s ON t.parent_id = s.id
        )
        SELECT id, parent_id, title, notes, completed, position, created_at, updated_at,
               deferred, review_interval, next_review_at, acknowledged, focused, focused_at, budget_minutes
        FROM subtree ORDER BY depth, position"
    )
    .bind(id)
    .fetch_all(pool)
    .await?;
    Ok(tasks)
}

/// Resolve a ULID prefix to a full ID. Returns error if ambiguous.
#[allow(dead_code)]
pub async fn resolve_id(pool: &SqlitePool, prefix: &str) -> Result<String> {
    let pattern = format!("{}%", prefix);
    let matches: Vec<String> = sqlx::query_scalar("SELECT id FROM tasks WHERE id LIKE ?")
        .bind(&pattern)
        .fetch_all(pool)
        .await?;

    match matches.len() {
        0 => anyhow::bail!("no task found matching '{}'", prefix),
        1 => Ok(matches.into_iter().next().unwrap()),
        _ => anyhow::bail!("ambiguous prefix '{}' matches {} tasks", prefix, matches.len()),
    }
}

// ── Defer & Review ──

pub async fn defer_task(pool: &SqlitePool, id: &str) -> Result<Task> {
    let existing = get_task(pool, id).await?;
    let now = chrono::Utc::now();
    let now_str = now.to_rfc3339();

    if existing.deferred {
        // Un-defer: reset interval, clear next_review_at
        sqlx::query(
            "UPDATE tasks SET deferred = FALSE, review_interval = 7, next_review_at = NULL, updated_at = ? WHERE id = ?"
        )
        .bind(&now_str)
        .bind(id)
        .execute(pool)
        .await?;
    } else {
        // Defer: set deferred, schedule next review
        let next_review = now + chrono::Duration::days(existing.review_interval);
        let next_review_str = next_review.to_rfc3339();
        sqlx::query(
            "UPDATE tasks SET deferred = TRUE, next_review_at = ?, updated_at = ? WHERE id = ?"
        )
        .bind(&next_review_str)
        .bind(&now_str)
        .bind(id)
        .execute(pool)
        .await?;
    }

    get_task(pool, id).await
}

pub async fn snooze_review(pool: &SqlitePool, id: &str) -> Result<Task> {
    let existing = get_task(pool, id).await?;
    let now = chrono::Utc::now();
    let now_str = now.to_rfc3339();
    let new_interval = (existing.review_interval * 2).min(90); // cap at 90 days
    let next_review = now + chrono::Duration::days(new_interval);
    let next_review_str = next_review.to_rfc3339();

    sqlx::query(
        "UPDATE tasks SET review_interval = ?, next_review_at = ?, updated_at = ? WHERE id = ?"
    )
    .bind(new_interval)
    .bind(&next_review_str)
    .bind(&now_str)
    .bind(id)
    .execute(pool)
    .await?;

    get_task(pool, id).await
}

pub async fn get_review_tasks(pool: &SqlitePool) -> Result<Vec<Task>> {
    let now = chrono::Utc::now().to_rfc3339();
    let tasks = sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE deferred = TRUE AND next_review_at IS NOT NULL AND next_review_at <= ? ORDER BY next_review_at LIMIT 20"
    )
    .bind(&now)
    .fetch_all(pool)
    .await?;
    Ok(tasks)
}

// ── Search ──

use crate::models::SearchResult;

pub async fn search_tasks(pool: &SqlitePool, query: &str) -> Result<Vec<SearchResult>> {
    let pattern = format!("%{}%", query);
    let tasks = sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE title LIKE ? OR notes LIKE ? ORDER BY updated_at DESC LIMIT 30",
    )
    .bind(&pattern)
    .bind(&pattern)
    .fetch_all(pool)
    .await?;

    let mut results = Vec::new();
    for task in tasks {
        let ancestors = get_ancestors(pool, &task.id).await?;
        // ancestors includes the task itself; path = all ancestor titles except the task
        let path: Vec<String> = ancestors
            .iter()
            .take_while(|a| a.id != task.id)
            .map(|a| a.title.clone())
            .collect();
        results.push(SearchResult { task, path });
    }

    Ok(results)
}

// ── Mirror (Experience Sampling) ──

use crate::models::{CreateSample, Sample, SampleWithTask};

pub async fn create_sample(pool: &SqlitePool, input: &CreateSample) -> Result<Sample> {
    let id = Ulid::new().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO samples (id, prompt_type, response, created_at, task_id) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&input.prompt_type)
    .bind(&input.response)
    .bind(&now)
    .bind(&input.task_id)
    .execute(pool)
    .await?;

    let sample = sqlx::query_as::<_, Sample>("SELECT * FROM samples WHERE id = ?")
        .bind(&id)
        .fetch_one(pool)
        .await?;
    Ok(sample)
}

pub async fn get_samples_today(pool: &SqlitePool) -> Result<Vec<Sample>> {
    let samples = sqlx::query_as::<_, Sample>(
        "SELECT * FROM samples WHERE date(created_at) = date('now') ORDER BY created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(samples)
}

#[allow(dead_code)]
pub async fn get_samples_today_with_tasks(pool: &SqlitePool) -> Result<Vec<SampleWithTask>> {
    let samples = sqlx::query_as::<_, SampleWithTask>(
        "SELECT s.id, s.prompt_type, s.response, s.created_at, s.task_id, t.title as task_title
         FROM samples s LEFT JOIN tasks t ON s.task_id = t.id
         WHERE date(s.created_at) = date('now') ORDER BY s.created_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(samples)
}

#[allow(dead_code)]
pub async fn get_samples_range(
    pool: &SqlitePool,
    from: &str,
    to: &str,
) -> Result<Vec<Sample>> {
    let samples = sqlx::query_as::<_, Sample>(
        "SELECT * FROM samples WHERE created_at >= ? AND created_at < ? ORDER BY created_at",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;
    Ok(samples)
}

// ── Touch ──

pub async fn touch_task(pool: &SqlitePool, id: &str, response: Option<&str>) -> Result<Task> {
    let task = get_task(pool, id).await?;
    let now = chrono::Utc::now().to_rfc3339();

    // Update updated_at
    sqlx::query("UPDATE tasks SET updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;

    // Create a linked sample
    let sample_response = response.unwrap_or(&task.title);
    create_sample(pool, &CreateSample {
        response: sample_response.to_string(),
        prompt_type: "touch".to_string(),
        task_id: Some(id.to_string()),
    }).await?;

    get_task(pool, id).await
}

// ── Inbox / Acknowledge ──

pub async fn acknowledge_task(pool: &SqlitePool, id: &str) -> Result<Task> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query("UPDATE tasks SET acknowledged = TRUE, updated_at = ? WHERE id = ?")
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    get_task(pool, id).await
}

// ── Focus Slots ──

pub async fn focus_task(pool: &SqlitePool, id: &str, budget_minutes: Option<i64>) -> Result<Task> {
    let existing = get_task(pool, id).await?;
    let now = chrono::Utc::now().to_rfc3339();

    if existing.focused {
        // Unfocus
        sqlx::query(
            "UPDATE tasks SET focused = FALSE, focused_at = NULL, budget_minutes = NULL, updated_at = ? WHERE id = ?"
        )
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    } else {
        // Check WIP limit
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM tasks WHERE focused = TRUE"
        )
        .fetch_one(pool)
        .await?;
        if count >= 3 {
            anyhow::bail!("already 3 focused tasks (max). Unfocus one first.");
        }
        sqlx::query(
            "UPDATE tasks SET focused = TRUE, focused_at = ?, budget_minutes = ?, updated_at = ? WHERE id = ?"
        )
        .bind(&now)
        .bind(budget_minutes)
        .bind(&now)
        .bind(id)
        .execute(pool)
        .await?;
    }

    get_task(pool, id).await
}

pub async fn get_focused_tasks(pool: &SqlitePool) -> Result<Vec<Task>> {
    let tasks = sqlx::query_as::<_, Task>(
        "SELECT * FROM tasks WHERE focused = TRUE ORDER BY focused_at"
    )
    .fetch_all(pool)
    .await?;
    Ok(tasks)
}
