use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame, Terminal,
};
use std::io;

use crate::client::FloClient;
use crate::models::{CreateTask, SearchResult, Task, UpdateTask};

enum InputMode {
    Normal,
    Adding(String),
    Editing(usize, String),
    Filtering(String),  // local filter
    Searching(String),          // global search (Ctrl-f)
    ConfirmDelete(String, String), // (task_id, title) — waiting for y/n
    WantEditorForNotes,          // signal to run loop to suspend TUI and open $EDITOR
}

enum Clipboard {
    Empty,
    Cut(Task),            // full task data (deleted from tree, recreated on paste)
    Yank(String, String), // (task_id, title) — reference copy
}

// ── Undo/Redo ──

#[derive(Clone)]
enum UndoAction {
    /// Delete this task (reverses a create)
    Delete(String),
    /// Create a task with these properties (reverses a delete)
    Create {
        parent_id: Option<String>,
        title: String,
        notes: String,
        completed: bool,
        position: i64,
    },
    /// Update a task to these exact values (reverses a change)
    Update {
        id: String,
        title: String,
        notes: String,
        completed: bool,
        position: i64,
        parent_id: Option<String>,
    },
}

#[derive(Clone)]
struct UndoEntry {
    description: String,
    actions: Vec<UndoAction>,
}

struct App {
    client: FloClient,
    // Navigation stack: list of (task_id, title) pairs. Empty = root.
    nav_stack: Vec<(String, String)>,
    // Current children displayed
    children: Vec<Task>,
    // Current frame task (None = root)
    current_task: Option<Task>,
    // List selection state
    list_state: ListState,
    // Input mode
    input_mode: InputMode,
    // Show notes panel (always read-only, shows selected task's notes)
    show_notes: bool,
    // Pending counts cache for root items (task_id -> count)
    pending_counts: std::collections::HashMap<String, i64>,
    // Show completed tasks
    show_completed: bool,
    // Tree view: show full nested subtree instead of direct children only
    show_tree: bool,
    // Flattened tree data: (task, depth) pairs for tree view
    tree_items: Vec<(Task, usize)>,
    // Pending keypress sequences (gg, dd, yy, <<, >>)
    pending_g: bool,
    pending_d: bool,
    pending_y: bool,
    pending_lt: bool,
    pending_gt: bool,
    // Clipboard for cut/yank
    clipboard: Clipboard,
    // Filter string (for / search)
    filter: String,
    // Status message
    status_msg: Option<String>,
    // Terminal height for page scrolling
    term_height: u16,
    // Show help overlay
    show_help: bool,
    // Adding mode creates+enters (push) vs just creates (add)
    adding_is_push: bool,
    // Undo/redo stacks
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    // Global search results
    search_results: Vec<SearchResult>,
    search_list_state: ListState,
}

impl App {
    fn new(port: u16) -> Self {
        let mut list_state = ListState::default();
        list_state.select(Some(0));
        Self {
            client: FloClient::new(port),
            nav_stack: Vec::new(),
            children: Vec::new(),
            current_task: None,
            list_state,
            input_mode: InputMode::Normal,
            show_notes: true,
            pending_counts: std::collections::HashMap::new(),
            show_completed: false,
            show_tree: false,
            tree_items: Vec::new(),
            pending_g: false,
            pending_d: false,
            pending_y: false,
            pending_lt: false,
            pending_gt: false,
            clipboard: Clipboard::Empty,
            filter: String::new(),
            status_msg: None,
            term_height: 24,
            show_help: false,
            search_results: Vec::new(),
            search_list_state: ListState::default(),
            adding_is_push: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    fn current_id(&self) -> Option<&str> {
        self.nav_stack.last().map(|(id, _)| id.as_str())
    }

    /// Record an undo entry and clear the redo stack
    fn push_undo(&mut self, entry: UndoEntry) {
        self.undo_stack.push(entry);
        self.redo_stack.clear();
    }

    /// Snapshot a task for undo (captures current state as an Update action)
    fn snapshot_task(task: &Task) -> UndoAction {
        UndoAction::Update {
            id: task.id.clone(),
            title: task.title.clone(),
            notes: task.notes.clone(),
            completed: task.completed,
            position: task.position,
            parent_id: task.parent_id.clone(),
        }
    }

    /// Execute an undo entry: apply all actions, return the reverse entry
    async fn execute_undo_entry(&self, entry: &UndoEntry) -> Result<UndoEntry> {
        let mut reverse_actions = Vec::new();

        for action in entry.actions.iter().rev() {
            match action {
                UndoAction::Delete(id) => {
                    // Fetch before deleting so we can recreate
                    if let Ok(tw) = self.client.get_task(id).await {
                        let task = tw.task;
                        self.client.delete_task(id).await?;
                        reverse_actions.push(UndoAction::Create {
                            parent_id: task.parent_id,
                            title: task.title,
                            notes: task.notes,
                            completed: task.completed,
                            position: task.position,
                        });
                    }
                }
                UndoAction::Create { parent_id, title, notes, completed, position } => {
                    let new_task = self.client.create_task(&CreateTask {
                        parent_id: parent_id.clone(),
                        title: title.clone(),
                        notes: notes.clone(),
                    }).await?;
                    // Restore completed and position if needed
                    if *completed || *position != new_task.position {
                        self.client.update_task(&new_task.id, &UpdateTask {
                            completed: Some(*completed),
                            position: Some(*position),
                            ..Default::default()
                        }).await?;
                    }
                    reverse_actions.push(UndoAction::Delete(new_task.id));
                }
                UndoAction::Update { id, title, notes, completed, position, parent_id } => {
                    // Snapshot current state for reverse
                    if let Ok(tw) = self.client.get_task(id).await {
                        let current = tw.task;
                        reverse_actions.push(Self::snapshot_task(&current));
                    }
                    self.client.update_task(id, &UpdateTask {
                        title: Some(title.clone()),
                        notes: Some(notes.clone()),
                        completed: Some(*completed),
                        position: Some(*position),
                        parent_id: Some(parent_id.clone().unwrap_or_default()),
                    }).await?;
                }
            }
        }

        Ok(UndoEntry {
            description: entry.description.clone(),
            actions: reverse_actions,
        })
    }

    async fn undo(&mut self) -> Result<()> {
        let Some(entry) = self.undo_stack.pop() else {
            self.status_msg = Some("Nothing to undo".to_string());
            return Ok(());
        };
        let desc = entry.description.clone();
        let redo_entry = self.execute_undo_entry(&entry).await?;
        self.redo_stack.push(redo_entry);
        self.status_msg = Some(format!("Undo: {}", desc));
        self.refresh().await?;
        Ok(())
    }

    async fn redo(&mut self) -> Result<()> {
        let Some(entry) = self.redo_stack.pop() else {
            self.status_msg = Some("Nothing to redo".to_string());
            return Ok(());
        };
        let desc = entry.description.clone();
        let undo_entry = self.execute_undo_entry(&entry).await?;
        self.undo_stack.push(undo_entry);
        self.status_msg = Some(format!("Redo: {}", desc));
        self.refresh().await?;
        Ok(())
    }

    fn breadcrumb(&self) -> String {
        if self.nav_stack.is_empty() {
            "root".to_string()
        } else {
            let mut parts = vec!["root"];
            parts.extend(self.nav_stack.iter().map(|(_, t)| t.as_str()));
            parts.join(" > ")
        }
    }

    fn selected_task(&self) -> Option<&Task> {
        self.list_state.selected().and_then(|i| {
            if self.show_tree {
                self.tree_items.get(i).map(|(t, _)| t)
            } else {
                self.children.get(i)
            }
        })
    }

    fn visible_count(&self) -> usize {
        if self.show_tree {
            self.tree_items.len()
        } else {
            self.children.len()
        }
    }

    async fn refresh(&mut self) -> Result<()> {
        let parent_id = self.current_id().map(|s| s.to_string());

        // Fetch current task details
        self.current_task = match &parent_id {
            Some(id) => {
                let tw = self.client.get_task(id).await?;
                Some(tw.task)
            }
            None => None,
        };

        // Fetch children
        let all_children = self.client.list_tasks(parent_id.as_deref()).await?;
        self.children = all_children
            .into_iter()
            .filter(|t| self.show_completed || !t.completed)
            .filter(|t| {
                self.filter.is_empty()
                    || t.title.to_lowercase().contains(&self.filter.to_lowercase())
            })
            .collect();

        // If at root, fetch pending counts
        if parent_id.is_none() {
            self.pending_counts.clear();
            let home = self.client.home().await?;
            for p in home {
                self.pending_counts.insert(p.id, p.pending_count);
            }
        }

        // Build tree view data if in tree mode
        if self.show_tree {
            self.tree_items.clear();
            for child in &self.children {
                let subtree = self.client.get_subtree(&child.id).await?;
                // subtree is ordered by depth, position — first item is the root
                // We need to compute depth relative to child
                let mut depth_map: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                for task in &subtree {
                    let depth = match &task.parent_id {
                        None => 0,
                        Some(pid) => depth_map.get(pid).map(|d| d + 1).unwrap_or(0),
                    };
                    depth_map.insert(task.id.clone(), depth);
                    if (!self.show_completed && task.completed)
                        || (!self.filter.is_empty()
                            && !task.title.to_lowercase().contains(&self.filter.to_lowercase()))
                    {
                        continue;
                    }
                    self.tree_items.push((task.clone(), depth));
                }
            }
        }

        // Fix selection bounds
        let len = self.visible_count();
        if len == 0 {
            self.list_state.select(None);
        } else if let Some(sel) = self.list_state.selected() {
            if sel >= len {
                self.list_state.select(Some(len - 1));
            }
        } else {
            self.list_state.select(Some(0));
        }

        Ok(())
    }

    async fn enter_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task().cloned() {
            if self.show_tree {
                // In tree view, navigate to the selected task by building full nav stack
                // from root to this task using ancestors
                let ancestors = self.client.get_ancestors(&task.id).await?;
                // ancestors includes the task itself, ordered from root to leaf
                // We need to build nav_stack from the first ancestor that's deeper than current frame
                self.nav_stack.clear();
                for ancestor in &ancestors {
                    self.nav_stack
                        .push((ancestor.id.clone(), ancestor.title.clone()));
                }
            } else {
                self.nav_stack.push((task.id.clone(), task.title.clone()));
            }
            self.list_state.select(Some(0));
            self.refresh().await?;
        }
        Ok(())
    }

    async fn pop(&mut self) -> Result<()> {
        if let Some((child_id, _)) = self.nav_stack.pop() {
            self.refresh().await?;
            // Select the child we just came from
            let idx = if self.show_tree {
                self.tree_items.iter().position(|(t, _)| t.id == child_id).unwrap_or(0)
            } else {
                self.children.iter().position(|t| t.id == child_id).unwrap_or(0)
            };
            self.list_state.select(Some(idx));
        }
        Ok(())
    }

    async fn add_task(&mut self, title: &str) -> Result<()> {
        let parent_id = self.current_id().map(|s| s.to_string());
        let task = self.client
            .create_task(&CreateTask {
                parent_id,
                title: title.to_string(),
                notes: String::new(),
            })
            .await?;
        self.push_undo(UndoEntry {
            description: format!("add \"{}\"", title),
            actions: vec![UndoAction::Delete(task.id)],
        });
        self.refresh().await?;
        if !self.children.is_empty() {
            self.list_state.select(Some(self.children.len() - 1));
        }
        self.status_msg = Some(format!("Added: {}", title));
        Ok(())
    }

    async fn push_task(&mut self, title: &str) -> Result<()> {
        let parent_id = self.current_id().map(|s| s.to_string());
        let task = self.client
            .create_task(&CreateTask {
                parent_id,
                title: title.to_string(),
                notes: String::new(),
            })
            .await?;
        self.push_undo(UndoEntry {
            description: format!("push \"{}\"", title),
            actions: vec![UndoAction::Delete(task.id.clone())],
        });
        self.nav_stack.push((task.id.clone(), task.title.clone()));
        self.list_state.select(Some(0));
        self.refresh().await?;
        self.status_msg = Some(format!("Pushed: {}", title));
        Ok(())
    }

    async fn toggle_done(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task().cloned() {
            let new_completed = !task.completed;
            self.push_undo(UndoEntry {
                description: format!("toggle \"{}\"", task.title),
                actions: vec![Self::snapshot_task(&task)],
            });
            self.client
                .update_task(
                    &task.id,
                    &UpdateTask {
                        completed: Some(new_completed),
                        ..Default::default()
                    },
                )
                .await?;
            let label = if new_completed { "Done" } else { "Undone" };
            self.status_msg = Some(format!("{}: {}", label, task.title));
            self.refresh().await?;
        }
        Ok(())
    }

    async fn delete_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task().cloned() {
            let children = self.client.list_tasks(Some(&task.id)).await?;
            if children.is_empty() {
                self.push_undo(UndoEntry {
                    description: format!("delete \"{}\"", task.title),
                    actions: vec![UndoAction::Create {
                        parent_id: task.parent_id.clone(),
                        title: task.title.clone(),
                        notes: task.notes.clone(),
                        completed: task.completed,
                        position: task.position,
                    }],
                });
                self.client.delete_task(&task.id).await?;
                self.status_msg = Some(format!("Deleted: {}", task.title));
                self.refresh().await?;
            } else {
                self.status_msg = Some(format!(
                    "Delete \"{}\" and {} children? (y/n)",
                    task.title,
                    children.len()
                ));
                self.input_mode = InputMode::ConfirmDelete(task.id.clone(), task.title.clone());
            }
        }
        Ok(())
    }

    async fn rename_selected(&mut self, title: &str) -> Result<()> {
        if let Some(task) = self.selected_task().cloned() {
            self.push_undo(UndoEntry {
                description: format!("rename \"{}\"", task.title),
                actions: vec![Self::snapshot_task(&task)],
            });
            self.client
                .update_task(
                    &task.id,
                    &UpdateTask {
                        title: Some(title.to_string()),
                        ..Default::default()
                    },
                )
                .await?;
            self.status_msg = Some(format!("Renamed: {}", title));
            self.refresh().await?;
        }
        Ok(())
    }

    /// Get the siblings of a task (tasks sharing the same parent_id)
    async fn get_siblings(&self, task: &Task) -> Result<Vec<Task>> {
        self.client.list_tasks(task.parent_id.as_deref()).await
    }

    async fn move_selected_up(&mut self) -> Result<()> {
        let Some(task) = self.selected_task().cloned() else { return Ok(()) };
        let siblings = self.get_siblings(&task).await?;
        let Some(idx) = siblings.iter().position(|t| t.id == task.id) else { return Ok(()) };
        if idx == 0 { return Ok(()) }
        let prev = &siblings[idx - 1];
        self.push_undo(UndoEntry {
            description: format!("move up \"{}\"", task.title),
            actions: vec![Self::snapshot_task(&task), Self::snapshot_task(prev)],
        });
        self.client.update_task(&task.id, &UpdateTask {
            position: Some(prev.position),
            ..Default::default()
        }).await?;
        self.client.update_task(&prev.id, &UpdateTask {
            position: Some(task.position),
            ..Default::default()
        }).await?;
        self.refresh().await?;
        self.select_task_by_id(&task.id);
        Ok(())
    }

    async fn move_selected_down(&mut self) -> Result<()> {
        let Some(task) = self.selected_task().cloned() else { return Ok(()) };
        let siblings = self.get_siblings(&task).await?;
        let Some(idx) = siblings.iter().position(|t| t.id == task.id) else { return Ok(()) };
        if idx + 1 >= siblings.len() { return Ok(()) }
        let next = &siblings[idx + 1];
        self.push_undo(UndoEntry {
            description: format!("move down \"{}\"", task.title),
            actions: vec![Self::snapshot_task(&task), Self::snapshot_task(next)],
        });
        self.client.update_task(&task.id, &UpdateTask {
            position: Some(next.position),
            ..Default::default()
        }).await?;
        self.client.update_task(&next.id, &UpdateTask {
            position: Some(task.position),
            ..Default::default()
        }).await?;
        self.refresh().await?;
        self.select_task_by_id(&task.id);
        Ok(())
    }

    /// Indent: make selected task a child of the sibling above it
    async fn indent(&mut self) -> Result<()> {
        let Some(task) = self.selected_task().cloned() else { return Ok(()) };
        let siblings = self.get_siblings(&task).await?;
        let Some(idx) = siblings.iter().position(|t| t.id == task.id) else { return Ok(()) };
        if idx == 0 {
            self.status_msg = Some("No sibling above to indent under".to_string());
            return Ok(());
        }
        let new_parent = &siblings[idx - 1];
        self.push_undo(UndoEntry {
            description: format!("indent \"{}\"", task.title),
            actions: vec![Self::snapshot_task(&task)],
        });
        let new_siblings = self.client.list_tasks(Some(&new_parent.id)).await?;
        let new_position = new_siblings.iter().map(|t| t.position).max().unwrap_or(-1) + 1;
        self.client.update_task(&task.id, &UpdateTask {
            parent_id: Some(new_parent.id.clone()),
            position: Some(new_position),
            ..Default::default()
        }).await?;
        self.status_msg = Some(format!("Indented under: {}", new_parent.title));
        self.refresh().await?;
        self.select_task_by_id(&task.id);
        Ok(())
    }

    /// Outdent: move selected task to be a sibling of its parent
    async fn outdent(&mut self) -> Result<()> {
        let Some(task) = self.selected_task().cloned() else { return Ok(()) };
        let Some(parent_id) = &task.parent_id else {
            self.status_msg = Some("Already at root level".to_string());
            return Ok(());
        };
        let parent = self.client.get_task(parent_id).await?;
        let grandparent_id = parent.task.parent_id.clone();
        let uncle_siblings = self.client.list_tasks(grandparent_id.as_deref()).await?;
        let parent_pos = uncle_siblings.iter()
            .find(|t| t.id == *parent_id)
            .map(|t| t.position)
            .unwrap_or(0);
        // Record undo: snapshot the task + all siblings that will be bumped
        let mut undo_actions = vec![Self::snapshot_task(&task)];
        for uncle in &uncle_siblings {
            if uncle.position > parent_pos {
                undo_actions.push(Self::snapshot_task(uncle));
            }
        }
        self.push_undo(UndoEntry {
            description: format!("outdent \"{}\"", task.title),
            actions: undo_actions,
        });
        for uncle in &uncle_siblings {
            if uncle.position > parent_pos {
                self.client.update_task(&uncle.id, &UpdateTask {
                    position: Some(uncle.position + 1),
                    ..Default::default()
                }).await?;
            }
        }
        self.client.update_task(&task.id, &UpdateTask {
            parent_id: Some(grandparent_id.unwrap_or_default()),
            position: Some(parent_pos + 1),
            ..Default::default()
        }).await?;
        self.status_msg = Some("Outdented".to_string());
        self.refresh().await?;
        self.select_task_by_id(&task.id);
        Ok(())
    }

    /// Find a task by ID in the current view and select it
    fn select_task_by_id(&mut self, id: &str) {
        let idx = if self.show_tree {
            self.tree_items.iter().position(|(t, _)| t.id == id)
        } else {
            self.children.iter().position(|t| t.id == id)
        };
        self.list_state.select(idx.or(Some(0)));
    }

    async fn paste(&mut self) -> Result<()> {
        let parent_id = self.current_id().map(|s| s.to_string());
        match std::mem::replace(&mut self.clipboard, Clipboard::Empty) {
            Clipboard::Empty => {
                self.status_msg = Some("Nothing to paste".to_string());
            }
            Clipboard::Cut(task) => {
                let title = task.title.clone();
                let new_task = self.client.create_task(&CreateTask {
                    parent_id,
                    title: task.title,
                    notes: task.notes,
                }).await?;
                self.push_undo(UndoEntry {
                    description: format!("paste \"{}\"", title),
                    actions: vec![UndoAction::Delete(new_task.id)],
                });
                self.status_msg = Some(format!("Pasted: {}", title));
                self.refresh().await?;
            }
            Clipboard::Yank(id, title) => {
                if let Ok(source) = self.client.get_task(&id).await {
                    let new_task = self.client.create_task(&CreateTask {
                        parent_id,
                        title: source.task.title.clone(),
                        notes: source.task.notes.clone(),
                    }).await?;
                    self.push_undo(UndoEntry {
                        description: format!("paste \"{}\"", title),
                        actions: vec![UndoAction::Delete(new_task.id)],
                    });
                    self.status_msg = Some(format!("Copied: {}", title));
                } else {
                    self.status_msg = Some("Source task no longer exists".to_string());
                }
                self.clipboard = Clipboard::Yank(id, title);
                self.refresh().await?;
            }
        }
        Ok(())
    }

    /// Spawn $EDITOR to edit notes for the selected task. Suspends TUI.
    async fn edit_notes_external(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        let Some(task) = self.selected_task().cloned() else {
            self.status_msg = Some("No task selected".to_string());
            return Ok(());
        };

        // Write current notes to temp file with header
        let tmp_dir = std::env::temp_dir();
        let tmp_path = tmp_dir.join(format!("flo-{}.md", &task.id));
        let file_content = format!(
            "# {}\n# Lines starting with # are stripped on save.\n\n{}",
            task.title, task.notes
        );
        std::fs::write(&tmp_path, &file_content)?;

        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .unwrap_or_else(|_| "vim".to_string());

        // Suspend TUI
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen)?;

        // Spawn editor
        let status = std::process::Command::new(&editor)
            .arg("+4") // start on line 4, after comments
            .arg(&tmp_path)
            .status();

        // Restore TUI
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        terminal.clear()?;

        match status {
            Ok(s) if s.success() => {
                if let Ok(raw) = std::fs::read_to_string(&tmp_path) {
                    let new_notes: String = raw
                        .lines()
                        .filter(|l| !l.starts_with('#'))
                        .collect::<Vec<_>>()
                        .join("\n")
                        .trim()
                        .to_string();
                    self.client
                        .update_task(
                            &task.id,
                            &UpdateTask {
                                notes: Some(new_notes),
                                ..Default::default()
                            },
                        )
                        .await?;
                    self.status_msg = Some("Notes saved".to_string());
                    self.refresh().await?;
                }
            }
            _ => {
                self.status_msg = Some("Editor failed or cancelled".to_string());
            }
        }

        // Clean up temp file
        std::fs::remove_file(&tmp_path).ok();
        Ok(())
    }

    fn move_up(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected > 0 {
                let new_sel = selected - 1;
                self.list_state.select(Some(new_sel));
                self.apply_scrolloff(new_sel);
            }
        }
    }

    fn move_down(&mut self) {
        if let Some(selected) = self.list_state.selected() {
            if selected + 1 < self.visible_count() {
                let new_sel = selected + 1;
                self.list_state.select(Some(new_sel));
                self.apply_scrolloff(new_sel);
            }
        }
    }

    fn apply_scrolloff(&mut self, selected: usize) {
        let scrolloff: usize = 4;
        let visible_height = self.term_height.saturating_sub(2) as usize;
        let total = self.visible_count();
        let offset = self.list_state.offset();

        // Scroll up: keep `scrolloff` lines above cursor
        if selected < offset + scrolloff {
            *self.list_state.offset_mut() = selected.saturating_sub(scrolloff);
        }
        // Scroll down: keep `scrolloff` lines below cursor
        if selected + scrolloff >= offset + visible_height {
            *self.list_state.offset_mut() = (selected + scrolloff + 1).saturating_sub(visible_height);
        }
        // Never scroll past content — no empty space at bottom
        let max_offset = total.saturating_sub(visible_height);
        if *self.list_state.offset_mut() > max_offset {
            *self.list_state.offset_mut() = max_offset;
        }
    }
}

pub async fn run(port: u16) -> Result<()> {
    let mut app = App::new(port);
    app.refresh().await?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        // Check if we need to open $EDITOR (must happen outside draw loop)
        if matches!(app.input_mode, InputMode::WantEditorForNotes) {
            app.input_mode = InputMode::Normal;
            app.edit_notes_external(terminal).await?;
            continue;
        }

        terminal.draw(|f| {
            app.term_height = f.area().height;
            ui(f, app);
        })?;

        if let Event::Key(key) = event::read()? {
            // Dismiss help on any key
            if app.show_help {
                app.show_help = false;
                continue;
            }

            match &mut app.input_mode {
                InputMode::Normal => {
                    if handle_normal_key(app, key).await? {
                        return Ok(());
                    }
                }
                InputMode::Adding(ref mut buf) => {
                    handle_text_input(key, buf);
                    match key.code {
                        KeyCode::Enter => {
                            let is_push = app.adding_is_push;
                            if let InputMode::Adding(text) =
                                std::mem::replace(&mut app.input_mode, InputMode::Normal)
                            {
                                if !text.trim().is_empty() {
                                    if is_push {
                                        app.push_task(text.trim()).await?;
                                    } else {
                                        app.add_task(text.trim()).await?;
                                    }
                                }
                            }
                            app.adding_is_push = false;
                        }
                        KeyCode::Esc => {
                            app.input_mode = InputMode::Normal;
                            app.adding_is_push = false;
                        }
                        _ => {}
                    }
                }
                InputMode::Editing(_, ref mut buf) => {
                    handle_text_input(key, buf);
                    match key.code {
                        KeyCode::Enter => {
                            if let InputMode::Editing(_, text) =
                                std::mem::replace(&mut app.input_mode, InputMode::Normal)
                            {
                                if !text.trim().is_empty() {
                                    app.rename_selected(text.trim()).await?;
                                }
                            }
                        }
                        KeyCode::Esc => {
                            app.input_mode = InputMode::Normal;
                        }
                        _ => {}
                    }
                }
                InputMode::Filtering(ref mut buf) => {
                    match key.code {
                        KeyCode::Esc => {
                            app.filter.clear();
                            app.input_mode = InputMode::Normal;
                            app.refresh().await?;
                        }
                        KeyCode::Enter => {
                            if let InputMode::Filtering(text) =
                                std::mem::replace(&mut app.input_mode, InputMode::Normal)
                            {
                                app.filter = text;
                                app.list_state.select(Some(0));
                                app.refresh().await?;
                            }
                        }
                        KeyCode::Char(c) => {
                            buf.push(c);
                            // Live filter as you type
                            app.filter = buf.clone();
                            app.list_state.select(Some(0));
                            app.refresh().await?;
                        }
                        KeyCode::Backspace => {
                            buf.pop();
                            app.filter = buf.clone();
                            app.list_state.select(Some(0));
                            app.refresh().await?;
                        }
                        _ => {}
                    }
                }
                InputMode::Searching(ref mut buf) => {
                    match key.code {
                        KeyCode::Esc => {
                            app.search_results.clear();
                            app.input_mode = InputMode::Normal;
                        }
                        KeyCode::Enter => {
                            // Jump to selected search result
                            if let Some(sel) = app.search_list_state.selected() {
                                if let Some(result) = app.search_results.get(sel) {
                                    let task = &result.task;
                                    // Build nav_stack from ancestors
                                    let ancestors = app.client.get_ancestors(&task.id).await?;
                                    app.nav_stack.clear();
                                    for ancestor in &ancestors {
                                        app.nav_stack.push((
                                            ancestor.id.clone(),
                                            ancestor.title.clone(),
                                        ));
                                    }
                                    app.search_results.clear();
                                    app.input_mode = InputMode::Normal;
                                    app.list_state.select(Some(0));
                                    app.refresh().await?;
                                }
                            }
                        }
                        KeyCode::Char(c) => {
                            buf.push(c);
                            let query = buf.clone();
                            if query.len() >= 2 {
                                app.search_results = app.client.search(&query).await?;
                                if !app.search_results.is_empty() {
                                    app.search_list_state.select(Some(0));
                                } else {
                                    app.search_list_state.select(None);
                                }
                            } else {
                                app.search_results.clear();
                                app.search_list_state.select(None);
                            }
                        }
                        KeyCode::Backspace => {
                            buf.pop();
                            let query = buf.clone();
                            if query.len() >= 2 {
                                app.search_results = app.client.search(&query).await?;
                                if !app.search_results.is_empty() {
                                    app.search_list_state.select(Some(0));
                                } else {
                                    app.search_list_state.select(None);
                                }
                            } else {
                                app.search_results.clear();
                                app.search_list_state.select(None);
                            }
                        }
                        KeyCode::Down | KeyCode::Tab => {
                            if let Some(sel) = app.search_list_state.selected() {
                                if sel + 1 < app.search_results.len() {
                                    app.search_list_state.select(Some(sel + 1));
                                }
                            }
                        }
                        KeyCode::Up | KeyCode::BackTab => {
                            if let Some(sel) = app.search_list_state.selected() {
                                if sel > 0 {
                                    app.search_list_state.select(Some(sel - 1));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                InputMode::ConfirmDelete(ref id, ref title) => {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            let id = id.clone();
                            let title = title.clone();
                            app.client.delete_task(&id).await?;
                            app.status_msg = Some(format!("Deleted: {}", title));
                            app.input_mode = InputMode::Normal;
                            app.refresh().await?;
                        }
                        _ => {
                            app.status_msg = None;
                            app.input_mode = InputMode::Normal;
                        }
                    }
                }
                InputMode::WantEditorForNotes => {
                    // Handled at top of loop
                }
            }
        }
    }
}

fn handle_text_input(key: KeyEvent, buf: &mut String) {
    match key.code {
        KeyCode::Char(c) => buf.push(c),
        KeyCode::Backspace => {
            buf.pop();
        }
        _ => {}
    }
}

async fn handle_normal_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Handle pending key sequences: gg, dd, yy, <<, >>
    if app.pending_g {
        app.pending_g = false;
        if key.code == KeyCode::Char('g') {
            if app.visible_count() > 0 {
                app.list_state.select(Some(0));
                app.apply_scrolloff(0);
            } else {
                app.list_state.select(None);
            }
            return Ok(false);
        }
    }
    if app.pending_d {
        app.pending_d = false;
        if key.code == KeyCode::Char('d') {
            // dd → cut (delete from tree, store for paste)
            if let Some(task) = app.selected_task().cloned() {
                app.push_undo(UndoEntry {
                    description: format!("cut \"{}\"", task.title),
                    actions: vec![UndoAction::Create {
                        parent_id: task.parent_id.clone(),
                        title: task.title.clone(),
                        notes: task.notes.clone(),
                        completed: task.completed,
                        position: task.position,
                    }],
                });
                app.client.delete_task(&task.id).await?;
                app.status_msg = Some(format!("Cut: {}", task.title));
                app.clipboard = Clipboard::Cut(task);
                app.refresh().await?;
            }
            return Ok(false);
        }
    }
    if app.pending_y {
        app.pending_y = false;
        if key.code == KeyCode::Char('y') {
            // yy → yank
            if let Some(task) = app.selected_task().cloned() {
                app.clipboard = Clipboard::Yank(task.id.clone(), task.title.clone());
                app.status_msg = Some(format!("Yanked: {}", task.title));
            }
            return Ok(false);
        }
    }
    if app.pending_lt {
        app.pending_lt = false;
        if key.code == KeyCode::Char('<') {
            app.outdent().await?;
            return Ok(false);
        }
    }
    if app.pending_gt {
        app.pending_gt = false;
        if key.code == KeyCode::Char('>') {
            app.indent().await?;
            return Ok(false);
        }
    }

    match key.code {
        KeyCode::Char('q') => return Ok(true),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),

        // ── Vim navigation ──
        KeyCode::Char('j') | KeyCode::Down => {
            app.status_msg = None;
            app.move_down();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.status_msg = None;
            app.move_up();
        }
        KeyCode::Char('g') => {
            app.pending_g = true; // wait for second g
        }
        KeyCode::Char('G') => {
            let len = app.visible_count();
            if len > 0 {
                let target = len - 1;
                app.list_state.select(Some(target));
                app.apply_scrolloff(target);
            }
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let half = (app.term_height / 2) as usize;
            if let Some(sel) = app.list_state.selected() {
                let target = (sel + half).min(app.visible_count().saturating_sub(1));
                app.list_state.select(Some(target));
                app.apply_scrolloff(target);
            }
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let half = (app.term_height / 2) as usize;
            if let Some(sel) = app.list_state.selected() {
                let target = sel.saturating_sub(half);
                app.list_state.select(Some(target));
                app.apply_scrolloff(target);
            }
        }

        // ── Frame navigation ──
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => app.enter_selected().await?,
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => app.pop().await?,
        KeyCode::Char('H') | KeyCode::Char('~') => {
            // H or ~ → home (root)
            app.nav_stack.clear();
            app.list_state.select(Some(0));
            app.filter.clear();
            app.refresh().await?;
        }

        // ── Actions ──
        KeyCode::Char('a') => {
            app.input_mode = InputMode::Adding(String::new());
        }
        KeyCode::Char('o') => {
            // push: create child + enter it
            app.input_mode = InputMode::Adding(String::new());
            app.adding_is_push = true;
        }
        KeyCode::Char('e') => {
            if let Some(task) = app.selected_task() {
                let title = task.title.clone();
                let idx = app.list_state.selected().unwrap_or(0);
                app.input_mode = InputMode::Editing(idx, title);
            }
        }
        KeyCode::Char('x') => app.toggle_done().await?,
        KeyCode::Char('d') => {
            app.pending_d = true; // wait for dd
        }
        KeyCode::Char('y') => {
            app.pending_y = true; // wait for yy
        }
        KeyCode::Char('p') => app.paste().await?,
        KeyCode::Char('D') => app.delete_selected().await?,
        KeyCode::Char('J') => app.move_selected_down().await?,
        KeyCode::Char('K') => app.move_selected_up().await?,
        KeyCode::Char('<') => { app.pending_lt = true; }
        KeyCode::Char('>') => { app.pending_gt = true; }

        // ── Notes ──
        KeyCode::Tab => {
            app.show_notes = !app.show_notes;
        }
        KeyCode::Char('n') => {
            if app.selected_task().is_some() {
                app.show_notes = true;
                app.input_mode = InputMode::WantEditorForNotes;
            }
        }

        // ── Search/filter ──
        KeyCode::Char('/') => {
            app.input_mode = InputMode::Filtering(app.filter.clone());
        }
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.search_results.clear();
            app.search_list_state.select(None);
            app.input_mode = InputMode::Searching(String::new());
        }
        KeyCode::Esc => {
            // Clear filter if active
            if !app.filter.is_empty() {
                app.filter.clear();
                app.list_state.select(Some(0));
                app.refresh().await?;
            }
        }

        // ── Toggles ──
        KeyCode::Char('t') => {
            app.show_tree = !app.show_tree;
            app.list_state.select(Some(0));
            app.refresh().await?;
        }
        KeyCode::Char('c') => {
            app.show_completed = !app.show_completed;
            app.refresh().await?;
        }

        // ── Refresh / Help ──
        KeyCode::Char('r') => app.refresh().await?,
        KeyCode::Char('?') => {
            app.show_help = true;
        }

        _ => {}
    }
    Ok(false)
}

fn ui(f: &mut Frame, app: &App) {
    let area = f.area();

    // Main layout: breadcrumb top, content middle, status bottom
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // breadcrumb
            Constraint::Min(3),   // content
            Constraint::Length(1), // status / input
        ])
        .split(area);

    // Breadcrumb
    let breadcrumb = Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(
            app.breadcrumb(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    f.render_widget(Paragraph::new(breadcrumb), outer[0]);

    // Content area: split or single depending on show_notes
    if app.show_notes {
        let content = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(outer[1]);

        render_task_list(f, app, content[0]);
        render_notes_panel(f, app, content[1]);
    } else {
        render_task_list(f, app, outer[1]);
    }

    // Status bar / input line
    render_status_bar(f, app, outer[2]);

    // Search overlay
    if matches!(app.input_mode, InputMode::Searching(_)) {
        render_search(f, app, area);
    }

    // Help overlay
    if app.show_help {
        render_help(f, area);
    }
}

fn render_task_list(f: &mut Frame, app: &App, area: Rect) {
    let is_root = app.nav_stack.is_empty();

    // Empty state
    if app.visible_count() == 0 {
        let msg = if !app.filter.is_empty() {
            "No matches. Press Esc to clear filter."
        } else if is_root {
            "No projects yet. Press 'a' to create one."
        } else {
            "No tasks. Press 'a' to add one."
        };
        let p = Paragraph::new(Line::from(Span::styled(
            format!("  {}", msg),
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(p, area);
        return;
    }

    let items: Vec<ListItem> = if app.show_tree {
        app.tree_items
            .iter()
            .map(|(task, depth)| build_task_line(task, *depth, is_root, &app.pending_counts))
            .collect()
    } else {
        app.children
            .iter()
            .map(|task| build_task_line(task, 0, is_root, &app.pending_counts))
            .collect()
    };

    let block = Block::default().borders(Borders::NONE);

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut app.list_state.clone());
}

fn build_task_line(
    task: &Task,
    depth: usize,
    is_root: bool,
    pending_counts: &std::collections::HashMap<String, i64>,
) -> ListItem<'static> {
    let indent = "  ".repeat(depth);
    let check = if task.completed { "x" } else { " " };
    let mut spans = vec![
        Span::raw(format!(" {}", indent)),
        Span::styled(
            format!("[{}] ", check),
            if task.completed {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::Green)
            },
        ),
    ];

    let title_style = if task.completed {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::default()
    };
    spans.push(Span::styled(task.title.clone(), title_style));

    // Show pending count for root items
    if is_root && depth == 0 {
        if let Some(count) = pending_counts.get(&task.id) {
            spans.push(Span::styled(
                format!("  ({})", count),
                Style::default().fg(Color::DarkGray),
            ));
        }
    }

    // Show notes indicator
    if !task.notes.is_empty() {
        spans.push(Span::styled(" [n]", Style::default().fg(Color::Yellow)));
    }

    ListItem::new(Line::from(spans))
}

fn render_notes_panel(f: &mut Frame, app: &App, area: Rect) {
    // Show notes for the SELECTED task (read-only)
    let (title_text, notes_text) = match app.selected_task() {
        Some(task) => {
            let notes = if task.notes.is_empty() {
                "No notes — press 'n' to edit".to_string()
            } else {
                task.notes.clone()
            };
            (task.title.clone(), notes)
        }
        None => ("No task selected".to_string(), String::new()),
    };

    let block = Block::default()
        .title(" Notes ")
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(Color::DarkGray));

    let mut lines = vec![
        Line::from(Span::styled(
            &title_text,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    if notes_text.is_empty() {
        lines.push(Line::from(Span::styled(
            "No task selected",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for line in notes_text.lines() {
            lines.push(Line::from(Span::raw(line)));
        }
    }

    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let content = match &app.input_mode {
        InputMode::Adding(buf) => {
            let label = if app.adding_is_push {
                " push: "
            } else {
                " add: "
            };
            Line::from(vec![
                Span::styled(label, Style::default().fg(Color::Yellow)),
                Span::raw(buf),
                Span::styled("\u{2588}", Style::default().fg(Color::Yellow)),
            ])
        }
        InputMode::Editing(_, buf) => Line::from(vec![
            Span::styled(" edit: ", Style::default().fg(Color::Yellow)),
            Span::raw(buf),
            Span::styled("\u{2588}", Style::default().fg(Color::Yellow)),
        ]),
        InputMode::Filtering(buf) => Line::from(vec![
            Span::styled(" /", Style::default().fg(Color::Yellow)),
            Span::raw(buf),
            Span::styled("\u{2588}", Style::default().fg(Color::Yellow)),
        ]),
        InputMode::Searching(_) => Line::from(Span::styled(
            " ↑↓:navigate  Enter:jump  Esc:cancel",
            Style::default().fg(Color::DarkGray),
        )),
        InputMode::ConfirmDelete(_, _) => {
            if let Some(msg) = &app.status_msg {
                Line::from(Span::styled(
                    format!(" {}", msg),
                    Style::default().fg(Color::Red),
                ))
            } else {
                Line::from("")
            }
        }
        InputMode::WantEditorForNotes => Line::from(Span::styled(
            " opening editor...",
            Style::default().fg(Color::Yellow),
        )),
        InputMode::Normal => {
            if !app.filter.is_empty() {
                Line::from(Span::styled(
                    format!(" filter: \"{}\" (Esc to clear)", app.filter),
                    Style::default().fg(Color::Yellow),
                ))
            } else if let Some(msg) = &app.status_msg {
                Line::from(Span::styled(
                    format!(" {}", msg),
                    Style::default().fg(Color::Green),
                ))
            } else {
                let dim = Style::default().fg(Color::DarkGray);
                let key_style = Style::default().fg(Color::Yellow);
                let sep = Span::styled("  ", dim);
                let mut spans = vec![Span::raw(" ")];

                let keys: &[(&str, &str)] = &[
                    ("a", "add"),
                    ("o", "push"),
                    ("x", "done"),
                    ("dd", "cut"),
                    ("yy", "yank"),
                    ("p", "paste"),
                    ("J/K", "move"),
                    ("<< >>", "in/outdent"),
                    ("D", "del"),
                    ("?", "help"),
                ];

                for (i, (k, label)) in keys.iter().enumerate() {
                    if i > 0 {
                        spans.push(sep.clone());
                    }
                    spans.push(Span::styled(*k, key_style));
                    spans.push(Span::styled(format!(":{}", label), dim));
                }

                // Show clipboard content if any
                match &app.clipboard {
                    Clipboard::Cut(t) => {
                        spans.push(Span::styled(format!("  [cut: {}]", t.title), Style::default().fg(Color::Yellow)));
                    }
                    Clipboard::Yank(_, t) => {
                        spans.push(Span::styled(format!("  [yank: {}]", t), Style::default().fg(Color::Cyan)));
                    }
                    Clipboard::Empty => {}
                }

                Line::from(spans)
            }
        }
    };

    f.render_widget(Paragraph::new(content), area);
}

fn render_search(f: &mut Frame, app: &App, area: Rect) {
    let query = match &app.input_mode {
        InputMode::Searching(q) => q.as_str(),
        _ => "",
    };

    let height = (area.height.saturating_sub(4)).min(20);
    let width = (area.width - 4).min(60);
    let x = area.width.saturating_sub(width) / 2;
    let popup_area = Rect::new(x, 1, width, height + 3);

    let block = Block::default()
        .title(" Search ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(block, popup_area);

    let inner = popup_area.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 });
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    // Search input line
    let input_line = Line::from(vec![
        Span::styled(" > ", Style::default().fg(Color::Cyan)),
        Span::raw(query),
        Span::styled("\u{2588}", Style::default().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(input_line), chunks[0]);

    // Results
    if app.search_results.is_empty() {
        let msg = if query.len() < 2 {
            "type to search..."
        } else {
            "no results"
        };
        let p = Paragraph::new(Line::from(Span::styled(
            format!(" {}", msg),
            Style::default().fg(Color::DarkGray),
        )));
        f.render_widget(p, chunks[1]);
    } else {
        let items: Vec<ListItem> = app
            .search_results
            .iter()
            .map(|r| {
                let mut spans = vec![Span::raw(" ")];
                // Show path if task has ancestors
                if !r.path.is_empty() {
                    spans.push(Span::styled(
                        format!("{} > ", r.path.join(" > ")),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                let title_style = if r.task.completed {
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(r.task.title.clone(), title_style));
                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items).highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
        f.render_stateful_widget(list, chunks[1], &mut app.search_list_state.clone());
    }
}

fn render_help(f: &mut Frame, area: Rect) {
    let help_lines = vec![
        Line::from(Span::styled(
            " Keybindings",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Navigation", Style::default().fg(Color::Yellow)),
        ]),
        Line::from("   j/k ↑/↓     move selection"),
        Line::from("   Enter/l/→   enter task"),
        Line::from("   h/←/Bksp    go to parent"),
        Line::from("   H/~         go to root"),
        Line::from("   gg          jump to first"),
        Line::from("   G           jump to last"),
        Line::from("   Ctrl-d/u    half page down/up"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Actions", Style::default().fg(Color::Yellow)),
        ]),
        Line::from("   a           add task"),
        Line::from("   o           push (add + enter)"),
        Line::from("   e           edit title"),
        Line::from("   x           toggle done"),
        Line::from("   dd          cut task"),
        Line::from("   yy          yank (copy) task"),
        Line::from("   p           paste"),
        Line::from("   J/K         move task down/up"),
        Line::from("   >>          indent (child of above)"),
        Line::from("   <<          outdent (up a level)"),
        Line::from("   D           delete task"),
        Line::from("   n           edit notes ($EDITOR)"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Views", Style::default().fg(Color::Yellow)),
        ]),
        Line::from("   Tab         toggle notes panel"),
        Line::from("   t           toggle tree/list"),
        Line::from("   c           toggle completed"),
        Line::from("   /           filter (local)"),
        Line::from("   Ctrl-f      search (global)"),
        Line::from("   r           refresh"),
        Line::from(""),
        Line::from("   q/Ctrl-c    quit"),
        Line::from(""),
        Line::from(Span::styled(
            "   press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let height = help_lines.len() as u16 + 2;
    let width = 38u16;
    let x = area.width.saturating_sub(width) / 2;
    let y = area.height.saturating_sub(height) / 2;
    let popup_area = Rect::new(x, y, width.min(area.width), height.min(area.height));

    // Clear background
    let clear = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));
    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(clear, popup_area);

    let inner = popup_area.inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 });
    let paragraph = Paragraph::new(help_lines);
    f.render_widget(paragraph, inner);
}
