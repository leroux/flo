use anyhow::{Context, Result};
use reqwest::Client;

use crate::models::{CreateTask, ProjectPreview, Task, TaskWithChildren, UpdateTask};

pub struct FloClient {
    base_url: String,
    http: Client,
}

impl FloClient {
    pub fn new(port: u16) -> Self {
        Self {
            base_url: format!("http://127.0.0.1:{}", port),
            http: Client::new(),
        }
    }

    pub async fn health(&self) -> Result<bool> {
        let resp = self
            .http
            .get(format!("{}/api/health", self.base_url))
            .send()
            .await;
        Ok(resp.is_ok())
    }

    pub async fn health_version(&self) -> Result<Option<String>> {
        let resp = self
            .http
            .get(format!("{}/api/health", self.base_url))
            .send()
            .await?;
        let body: serde_json::Value = resp.json().await?;
        Ok(body.get("version").and_then(|v| v.as_str()).map(String::from))
    }

    pub async fn home(&self) -> Result<Vec<ProjectPreview>> {
        let resp = self
            .http
            .get(format!("{}/api/home", self.base_url))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    pub async fn list_tasks(&self, parent_id: Option<&str>) -> Result<Vec<Task>> {
        let mut url = format!("{}/api/tasks", self.base_url);
        if let Some(pid) = parent_id {
            url.push_str(&format!("?parent_id={}", pid));
        }
        let resp = self.http.get(&url).send().await.context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    pub async fn get_task(&self, id: &str) -> Result<TaskWithChildren> {
        let resp = self
            .http
            .get(format!("{}/api/tasks/{}", self.base_url, id))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    pub async fn create_task(&self, input: &CreateTask) -> Result<Task> {
        let resp = self
            .http
            .post(format!("{}/api/tasks", self.base_url))
            .json(input)
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    pub async fn update_task(&self, id: &str, input: &UpdateTask) -> Result<Task> {
        let resp = self
            .http
            .patch(format!("{}/api/tasks/{}", self.base_url, id))
            .json(input)
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    pub async fn delete_task(&self, id: &str) -> Result<()> {
        self.http
            .delete(format!("{}/api/tasks/{}", self.base_url, id))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(())
    }

    pub async fn get_ancestors(&self, id: &str) -> Result<Vec<Task>> {
        let resp = self
            .http
            .get(format!("{}/api/tasks/{}/ancestors", self.base_url, id))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    pub async fn get_subtree(&self, id: &str) -> Result<Vec<Task>> {
        let resp = self
            .http
            .get(format!("{}/api/tasks/{}/subtree", self.base_url, id))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    // ── Defer & Review ──

    pub async fn defer_task(&self, id: &str) -> Result<Task> {
        let resp = self
            .http
            .post(format!("{}/api/tasks/{}/defer", self.base_url, id))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    pub async fn snooze_task(&self, id: &str) -> Result<Task> {
        let resp = self
            .http
            .post(format!("{}/api/tasks/{}/snooze", self.base_url, id))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    pub async fn get_review_tasks(&self) -> Result<Vec<Task>> {
        let resp = self
            .http
            .get(format!("{}/api/review", self.base_url))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    // ── Touch ──

    pub async fn touch_task(&self, id: &str, response: Option<&str>) -> Result<Task> {
        let mut req = self.http.post(format!("{}/api/tasks/{}/touch", self.base_url, id));
        if let Some(text) = response {
            req = req.json(&serde_json::json!({ "response": text }));
        }
        let resp = req.send().await.context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    // ── Inbox / Acknowledge ──

    pub async fn acknowledge_task(&self, id: &str) -> Result<Task> {
        let resp = self
            .http
            .post(format!("{}/api/tasks/{}/ack", self.base_url, id))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    // ── Focus ──

    pub async fn focus_task(&self, id: &str, budget_minutes: Option<i64>) -> Result<Task> {
        let mut req = self.http.post(format!("{}/api/tasks/{}/focus", self.base_url, id));
        if let Some(mins) = budget_minutes {
            req = req.json(&serde_json::json!({ "budget_minutes": mins }));
        }
        let resp = req.send().await.context("failed to connect to server")?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            anyhow::bail!("{}", body);
        }
        Ok(serde_json::from_str(&body)?)
    }

    pub async fn get_focused_tasks(&self) -> Result<Vec<Task>> {
        let resp = self
            .http
            .get(format!("{}/api/focus", self.base_url))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    // ── Search ──

    pub async fn search(&self, query: &str) -> Result<Vec<crate::models::SearchResult>> {
        let resp = self
            .http
            .get(format!("{}/api/search?q={}", self.base_url, urlencoding::encode(query)))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    // ── Mirror ──

    pub async fn create_sample(&self, input: &crate::models::CreateSample) -> Result<crate::models::Sample> {
        let resp = self
            .http
            .post(format!("{}/api/samples", self.base_url))
            .json(input)
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }

    pub async fn get_samples_today(&self) -> Result<Vec<crate::models::Sample>> {
        let resp = self
            .http
            .get(format!("{}/api/samples", self.base_url))
            .send()
            .await
            .context("failed to connect to server")?;
        Ok(resp.json().await?)
    }
}
