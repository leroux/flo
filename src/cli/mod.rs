use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::client::FloClient;
use crate::models::{CreateTask, UpdateTask};

#[derive(Parser)]
#[command(name = "flo", about = "Executive function prosthetic")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Server port
    #[arg(long, default_value = "4242", global = true)]
    pub port: u16,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the API server
    Server,
    /// Show current frame: title, notes, children
    Status,
    /// Create child task and enter it
    Push {
        title: String,
    },
    /// Move to parent frame
    Pop,
    /// Move to parent frame (alias for pop)
    Up,
    /// Move into a child by index (1-based)
    Down {
        index: usize,
    },
    /// Go to root (clear cursor)
    Top,
    /// Add a child task without entering it
    Add {
        title: String,
    },
    /// Mark a task complete (current frame or child by index)
    Done {
        /// Child index (1-based). Omit to complete current frame.
        index: Option<usize>,
    },
    /// View or set notes on current frame
    Note {
        /// Set notes to this text. Omit to view.
        text: Option<String>,
    },
    /// Edit a task's title
    Edit {
        /// Child index (1-based)
        index: usize,
        /// New title
        title: String,
    },
    /// Show tree from current frame
    Tree,
    /// Delete a child task by index (1-based)
    Delete {
        index: usize,
    },
    /// Make a child a subtask of a sibling (indent)
    Indent {
        /// Child index to indent (1-based)
        index: usize,
        /// Target parent index (1-based). Defaults to sibling above.
        #[arg(short, long)]
        under: Option<usize>,
    },
    /// Defer a task (toggle). Deferred tasks are hidden from default view.
    Defer {
        /// Child index (1-based). Omit to defer current frame.
        index: Option<usize>,
    },
    /// Review deferred tasks due for check-in
    Review,
    /// Touch a task (update timestamp + log sample)
    Touch {
        /// Child index (1-based). Omit to touch current frame.
        index: Option<usize>,
        /// Optional response text for the sample
        #[arg(short, long)]
        text: Option<String>,
    },
    /// Toggle focus on a task (max 3 WIP slots)
    Focus {
        /// Child index (1-based). Omit to show focused tasks.
        index: Option<usize>,
        /// Time budget in minutes for this focus session
        #[arg(short, long)]
        budget: Option<i64>,
    },
    /// Log what you're doing right now
    Log {
        /// What are you doing?
        text: String,
    },
    /// Interactive experience sampling prompt
    Ping,
    /// Show today's experience samples
    Mirror,
    /// Launch interactive TUI
    #[cfg(feature = "tui")]
    Tui,
}

fn data_dir() -> PathBuf {
    let home = dirs::home_dir().expect("could not find home directory");
    home.join(".flo")
}

fn cursor_path() -> PathBuf {
    data_dir().join("cursor")
}

fn read_cursor() -> Option<String> {
    std::fs::read_to_string(cursor_path())
        .ok()
        .and_then(|s| {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() { None } else { Some(trimmed) }
        })
}

fn write_cursor(id: Option<&str>) {
    let path = cursor_path();
    std::fs::create_dir_all(path.parent().unwrap()).ok();
    match id {
        Some(id) => std::fs::write(&path, id).ok(),
        None => std::fs::write(&path, "").ok(),
    };
}

pub async fn ensure_server(port: u16) -> Result<()> {
    let client = FloClient::new(port);
    let expected_version = crate::version();

    // Check if a server is already running
    if client.health().await.unwrap_or(false) {
        // Verify it's the right version
        let remote_version = client.health_version().await.ok().flatten();
        match remote_version {
            Some(v) if v == expected_version => return Ok(()),
            Some(v) => {
                eprintln!("stale server detected (running {}, expected {}), restarting...", v, expected_version);
                kill_server();
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
            None => {
                eprintln!("unknown server on port {}, restarting...", port);
                kill_server();
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }

    // Auto-start server as background daemon
    let exe = std::env::current_exe()?;
    let child = std::process::Command::new(exe)
        .args(["server", "--port", &port.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .context("failed to spawn server")?;

    // Write PID for later
    let pid_path = data_dir().join("server.pid");
    std::fs::create_dir_all(data_dir()).ok();
    std::fs::write(&pid_path, child.id().to_string()).ok();

    // Wait for server to be ready
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if client.health().await.unwrap_or(false) {
            return Ok(());
        }
    }

    anyhow::bail!("server failed to start within 5 seconds")
}

fn kill_server() {
    let pid_path = data_dir().join("server.pid");
    if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            unsafe { libc::kill(pid, libc::SIGTERM); }
        }
    }
    std::fs::remove_file(&pid_path).ok();
}

pub async fn run(cli: Cli) -> Result<()> {
    let port = cli.port;

    match cli.command {
        None => cmd_home(port).await,
        Some(Command::Server) => {
            let _log_guard = crate::logging::init();
            let db_path = data_dir().join("flo.db");
            std::fs::create_dir_all(data_dir())?;
            let db_url = format!("sqlite:{}?mode=rwc", db_path.display());
            let pool = sqlx::SqlitePool::connect(&db_url).await?;
            crate::db::init(&pool).await?;
            crate::server::run(pool, port).await
        }
        Some(Command::Status) => {
            ensure_server(port).await?;
            cmd_status(port).await
        }
        Some(Command::Push { title }) => {
            ensure_server(port).await?;
            cmd_push(port, &title).await
        }
        Some(Command::Pop) | Some(Command::Up) => {
            ensure_server(port).await?;
            cmd_pop(port).await
        }
        Some(Command::Down { index }) => {
            ensure_server(port).await?;
            cmd_down(port, index).await
        }
        Some(Command::Top) => {
            write_cursor(None);
            println!("At root.");
            Ok(())
        }
        Some(Command::Add { title }) => {
            ensure_server(port).await?;
            cmd_add(port, &title).await
        }
        Some(Command::Done { index }) => {
            ensure_server(port).await?;
            cmd_done(port, index).await
        }
        Some(Command::Note { text }) => {
            ensure_server(port).await?;
            cmd_note(port, text.as_deref()).await
        }
        Some(Command::Edit { index, title }) => {
            ensure_server(port).await?;
            cmd_edit(port, index, &title).await
        }
        Some(Command::Tree) => {
            ensure_server(port).await?;
            cmd_tree(port).await
        }
        Some(Command::Delete { index }) => {
            ensure_server(port).await?;
            cmd_delete(port, index).await
        }
        Some(Command::Indent { index, under }) => {
            ensure_server(port).await?;
            cmd_indent(port, index, under).await
        }
        Some(Command::Defer { index }) => {
            ensure_server(port).await?;
            cmd_defer(port, index).await
        }
        Some(Command::Review) => {
            ensure_server(port).await?;
            cmd_review(port).await
        }
        Some(Command::Touch { index, text }) => {
            ensure_server(port).await?;
            cmd_touch(port, index, text.as_deref()).await
        }
        Some(Command::Focus { index, budget }) => {
            ensure_server(port).await?;
            cmd_focus(port, index, budget).await
        }
        // Mirror commands
        Some(Command::Log { text }) => {
            ensure_server(port).await?;
            cmd_log(port, &text).await
        }
        Some(Command::Ping) => {
            ensure_server(port).await?;
            cmd_ping(port).await
        }
        Some(Command::Mirror) => {
            ensure_server(port).await?;
            cmd_mirror(port).await
        }
        #[cfg(feature = "tui")]
        Some(Command::Tui) => {
            ensure_server(port).await?;
            crate::tui::run(port).await
        }
    }
}

async fn cmd_home(port: u16) -> Result<()> {
    ensure_server(port).await?;
    let client = FloClient::new(port);
    let previews = client.home().await?;

    if previews.is_empty() {
        println!("No projects yet. Create one with: flo add <title>");
        return Ok(());
    }

    println!("Projects");
    println!("{}", "─".repeat(40));
    for (i, p) in previews.iter().enumerate() {
        let next = p
            .next_actions
            .first()
            .map(|t| format!("→ \"{}\"", t.title))
            .unwrap_or_else(|| "  (no pending tasks)".to_string());
        println!("{}. {}  {}", i + 1, p.title, next);
    }
    let total_pending: i64 = previews.iter().map(|p| p.pending_count).sum();
    println!(
        "\n{} projects, {} total pending tasks",
        previews.len(),
        total_pending
    );
    Ok(())
}

async fn cmd_status(port: u16) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();

    match cursor {
        None => cmd_home(port).await,
        Some(id) => {
            let tw = client.get_task(&id).await?;
            let ancestors = client.get_ancestors(&id).await?;

            // Breadcrumb
            let breadcrumb: Vec<&str> = ancestors.iter().map(|t| t.title.as_str()).collect();
            println!("{}", breadcrumb.join(" > "));
            println!("{}", "─".repeat(40));

            // Notes
            if !tw.task.notes.is_empty() {
                println!("{}", tw.task.notes);
                println!();
            }

            // Children
            if tw.children.is_empty() {
                println!("  (no children)");
            } else {
                for (i, child) in tw.children.iter().enumerate() {
                    let check = if child.completed {
                        "x"
                    } else if !child.acknowledged {
                        "?"
                    } else {
                        " "
                    };
                    let mut suffix = String::new();
                    if child.focused {
                        suffix.push_str(" [F]");
                    }
                    if child.deferred {
                        suffix.push_str(" [zzz]");
                    }
                    println!("  {}. [{}] {}{}", i + 1, check, child.title, suffix);
                }
            }
            Ok(())
        }
    }
}

async fn cmd_push(port: u16, title: &str) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();
    let task = client
        .create_task(&CreateTask {
            parent_id: cursor,
            title: title.to_string(),
            notes: String::new(),
        })
        .await?;
    write_cursor(Some(&task.id));
    println!("Pushed into: {}", task.title);
    Ok(())
}

async fn cmd_pop(port: u16) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();
    match cursor {
        None => println!("Already at root."),
        Some(id) => {
            let task = client.get_task(&id).await?;
            match task.task.parent_id {
                Some(pid) => {
                    let parent = client.get_task(&pid).await?;
                    write_cursor(Some(&pid));
                    println!("Popped to: {}", parent.task.title);
                }
                None => {
                    write_cursor(None);
                    println!("Popped to root.");
                }
            }
        }
    }
    Ok(())
}

async fn cmd_down(port: u16, index: usize) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();
    let children = client.list_tasks(cursor.as_deref()).await?;

    if index == 0 || index > children.len() {
        anyhow::bail!("invalid index {}. Have {} children.", index, children.len());
    }

    let child = &children[index - 1];
    // Auto-acknowledge on enter
    if !child.acknowledged {
        client.acknowledge_task(&child.id).await?;
    }
    write_cursor(Some(&child.id));
    println!("Entered: {}", child.title);
    Ok(())
}

async fn cmd_add(port: u16, title: &str) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();
    let task = client
        .create_task(&CreateTask {
            parent_id: cursor,
            title: title.to_string(),
            notes: String::new(),
        })
        .await?;
    println!("Added: {}", task.title);
    Ok(())
}

async fn cmd_done(port: u16, index: Option<usize>) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();

    let id = match index {
        None => {
            cursor.context("no current frame to complete (at root)")?
        }
        Some(idx) => {
            let children = client.list_tasks(cursor.as_deref()).await?;
            if idx == 0 || idx > children.len() {
                anyhow::bail!("invalid index {}. Have {} children.", idx, children.len());
            }
            children[idx - 1].id.clone()
        }
    };

    let task = client
        .update_task(&id, &UpdateTask {
            completed: Some(true),
            ..Default::default()
        })
        .await?;
    println!("Done: {}", task.title);
    Ok(())
}

async fn cmd_note(port: u16, text: Option<&str>) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor().context("no current frame (at root)")?;

    match text {
        None => {
            let tw = client.get_task(&cursor).await?;
            if tw.task.notes.is_empty() {
                println!("(no notes)");
            } else {
                println!("{}", tw.task.notes);
            }
        }
        Some(t) => {
            client
                .update_task(&cursor, &UpdateTask {
                    notes: Some(t.to_string()),
                    ..Default::default()
                })
                .await?;
            println!("Notes updated.");
        }
    }
    Ok(())
}

async fn cmd_edit(port: u16, index: usize, title: &str) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();
    let children = client.list_tasks(cursor.as_deref()).await?;

    if index == 0 || index > children.len() {
        anyhow::bail!("invalid index {}. Have {} children.", index, children.len());
    }

    let id = &children[index - 1].id;
    let task = client
        .update_task(id, &UpdateTask {
            title: Some(title.to_string()),
            ..Default::default()
        })
        .await?;
    println!("Renamed to: {}", task.title);
    Ok(())
}

async fn cmd_tree(port: u16) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();

    match cursor {
        None => {
            // Show all root tasks and their subtrees
            let roots = client.list_tasks(None).await?;
            for root in &roots {
                print_tree_node(&client, root, 0).await?;
            }
        }
        Some(id) => {
            let task = client.get_task(&id).await?;
            print_tree_node(&client, &task.task, 0).await?;
        }
    }
    Ok(())
}

async fn print_tree_node(client: &FloClient, task: &crate::models::Task, depth: usize) -> Result<()> {
    let indent = "  ".repeat(depth);
    let check = if task.completed { "x" } else { " " };
    println!("{}[{}] {}", indent, check, task.title);

    let children = client.list_tasks(Some(&task.id)).await?;
    for child in &children {
        Box::pin(print_tree_node(client, child, depth + 1)).await?;
    }
    Ok(())
}

async fn cmd_delete(port: u16, index: usize) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();
    let children = client.list_tasks(cursor.as_deref()).await?;

    if index == 0 || index > children.len() {
        anyhow::bail!("invalid index {}. Have {} children.", index, children.len());
    }

    let child = &children[index - 1];
    let title = child.title.clone();
    client.delete_task(&child.id).await?;
    println!("Deleted: {}", title);
    Ok(())
}

async fn cmd_indent(port: u16, index: usize, under: Option<usize>) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();
    let children = client.list_tasks(cursor.as_deref()).await?;

    if index == 0 || index > children.len() {
        anyhow::bail!("invalid index {}. Have {} children.", index, children.len());
    }

    let target_index = match under {
        Some(t) => t,
        None => {
            if index <= 1 {
                anyhow::bail!("no sibling above index 1 to indent under. Use --under to specify.");
            }
            index - 1
        }
    };

    if target_index == 0 || target_index > children.len() {
        anyhow::bail!("invalid target index {}. Have {} children.", target_index, children.len());
    }
    if target_index == index {
        anyhow::bail!("cannot indent a task under itself.");
    }

    let child = &children[index - 1];
    let new_parent = &children[target_index - 1];

    let task = client
        .update_task(&child.id, &UpdateTask {
            parent_id: Some(new_parent.id.clone()),
            ..Default::default()
        })
        .await?;
    println!("Moved \"{}\" under \"{}\"", task.title, new_parent.title);
    Ok(())
}

// ── Defer & Review ──

async fn cmd_defer(port: u16, index: Option<usize>) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();

    let id = match index {
        None => cursor.context("no current frame to defer (at root)")?,
        Some(idx) => {
            let children = client.list_tasks(cursor.as_deref()).await?;
            if idx == 0 || idx > children.len() {
                anyhow::bail!("invalid index {}. Have {} children.", idx, children.len());
            }
            children[idx - 1].id.clone()
        }
    };

    let task = client.defer_task(&id).await?;
    if task.deferred {
        println!("Deferred: {} (review in {} days)", task.title, task.review_interval);
    } else {
        println!("Undeferred: {}", task.title);
    }
    Ok(())
}

async fn cmd_review(port: u16) -> Result<()> {
    use std::io::{self, Write};

    let client = FloClient::new(port);
    let tasks = client.get_review_tasks().await?;

    if tasks.is_empty() {
        println!("Nothing to review. All caught up!");
        return Ok(());
    }

    println!("Review ({} tasks due)", tasks.len());
    println!("{}", "─".repeat(40));

    for task in &tasks {
        println!("\n  {} (interval: {}d)", task.title, task.review_interval);
        if !task.notes.is_empty() {
            for line in task.notes.lines().take(3) {
                println!("    {}", line);
            }
        }
        print!("  [k]eep  [s]nooze  [d]one  [q]uit > ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        match input.trim() {
            "k" | "keep" => {
                // Un-defer: bring it back to active
                client.defer_task(&task.id).await?;
                println!("  → Activated");
            }
            "s" | "snooze" => {
                let updated = client.snooze_task(&task.id).await?;
                println!("  → Snoozed (next review in {}d)", updated.review_interval);
            }
            "d" | "done" => {
                client
                    .update_task(
                        &task.id,
                        &UpdateTask {
                            completed: Some(true),
                            ..Default::default()
                        },
                    )
                    .await?;
                println!("  → Done");
            }
            "q" | "quit" => {
                println!("  → Stopped review");
                break;
            }
            _ => {
                println!("  → Skipped");
            }
        }
    }
    Ok(())
}

// ── Touch ──

async fn cmd_touch(port: u16, index: Option<usize>, text: Option<&str>) -> Result<()> {
    let client = FloClient::new(port);
    let cursor = read_cursor();

    let id = match index {
        None => cursor.context("no current frame to touch (at root)")?,
        Some(idx) => {
            let children = client.list_tasks(cursor.as_deref()).await?;
            if idx == 0 || idx > children.len() {
                anyhow::bail!("invalid index {}. Have {} children.", idx, children.len());
            }
            children[idx - 1].id.clone()
        }
    };

    let task = client.touch_task(&id, text).await?;
    println!("Touched: {}", task.title);
    Ok(())
}

// ── Focus ──

async fn cmd_focus(port: u16, index: Option<usize>, budget: Option<i64>) -> Result<()> {
    let client = FloClient::new(port);

    match index {
        None => {
            // Show focused tasks
            let tasks = client.get_focused_tasks().await?;
            if tasks.is_empty() {
                println!("No focused tasks. Use `flo focus N` to focus a task.");
                return Ok(());
            }
            println!("Focused tasks");
            println!("{}", "─".repeat(40));
            for (i, task) in tasks.iter().enumerate() {
                let elapsed = if let Some(ref at) = task.focused_at {
                    format_elapsed(at)
                } else {
                    String::new()
                };
                let budget_str = if let Some(mins) = task.budget_minutes {
                    format!(" (budget: {}m)", mins)
                } else {
                    String::new()
                };
                println!("  {}. {}{} {}", i + 1, task.title, budget_str, elapsed);
            }
            Ok(())
        }
        Some(idx) => {
            let cursor = read_cursor();
            let children = client.list_tasks(cursor.as_deref()).await?;
            if idx == 0 || idx > children.len() {
                anyhow::bail!("invalid index {}. Have {} children.", idx, children.len());
            }
            let id = &children[idx - 1].id;
            let task = client.focus_task(id, budget).await?;
            if task.focused {
                let budget_str = budget.map(|m| format!(" ({}m budget)", m)).unwrap_or_default();
                println!("Focused: {}{}", task.title, budget_str);
            } else {
                println!("Unfocused: {}", task.title);
            }
            Ok(())
        }
    }
}

fn format_elapsed(iso: &str) -> String {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return String::new();
    };
    let elapsed = chrono::Utc::now().signed_duration_since(dt);
    let mins = elapsed.num_minutes();
    if mins < 60 {
        format!("({}m ago)", mins)
    } else {
        let hours = mins / 60;
        format!("({}h{}m ago)", hours, mins % 60)
    }
}

// ── Mirror ──

async fn cmd_log(port: u16, text: &str) -> Result<()> {
    let client = FloClient::new(port);
    let sample = client
        .create_sample(&crate::models::CreateSample {
            response: text.to_string(),
            prompt_type: "activity".to_string(),
            task_id: None,
        })
        .await?;
    println!("Logged at {}", &sample.created_at[11..16]);
    Ok(())
}

async fn cmd_ping(port: u16) -> Result<()> {
    use std::io::{self, Write};

    print!("What are you doing right now? ");
    io::stdout().flush()?;

    let mut response = String::new();
    io::stdin().read_line(&mut response)?;
    let response = response.trim();

    if response.is_empty() {
        println!("Skipped.");
        return Ok(());
    }

    let client = FloClient::new(port);
    let sample = client
        .create_sample(&crate::models::CreateSample {
            response: response.to_string(),
            prompt_type: "ping".to_string(),
            task_id: None,
        })
        .await?;
    println!("Recorded at {}", &sample.created_at[11..16]);
    Ok(())
}

async fn cmd_mirror(port: u16) -> Result<()> {
    let client = FloClient::new(port);
    let samples = client.get_samples_today().await?;

    if samples.is_empty() {
        println!("No samples today. Use `flo log <text>` or `flo ping` to record.");
        return Ok(());
    }

    println!("Today's samples");
    println!("{}", "─".repeat(40));
    for s in &samples {
        let time = &s.created_at[11..16];
        let tag = match s.prompt_type.as_str() {
            "ping" => "ping",
            "activity" => " log",
            "touch" => "touch",
            other => other,
        };
        let task_label = s.task_id.as_ref().map(|_| {
            // For touch samples, response is the task title
            if s.prompt_type == "touch" {
                format!(" {}", s.response)
            } else {
                String::new()
            }
        }).unwrap_or_default();
        if s.prompt_type == "touch" {
            println!("  {} [{}]{}", time, tag, task_label);
        } else {
            println!("  {} [{}] {}", time, tag, s.response);
        }
    }
    println!("\n{} entries", samples.len());
    Ok(())
}
