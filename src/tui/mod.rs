use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEventKind,
    },
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
use std::collections::HashSet;
use std::io;
use std::time::Duration;

use crate::client::FloClient;
use crate::models::{CreateTask, SearchResult, Task, UpdateTask};

// ── Text Input with cursor ──

struct TextInput {
    content: String,
    cursor: usize, // character position
}

impl TextInput {
    fn empty() -> Self {
        Self {
            content: String::new(),
            cursor: 0,
        }
    }

    fn new(s: String) -> Self {
        let cursor = s.chars().count();
        Self { content: s, cursor }
    }

    fn byte_pos(&self) -> usize {
        self.content
            .char_indices()
            .nth(self.cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.content.len())
    }

    fn char_count(&self) -> usize {
        self.content.chars().count()
    }

    fn insert(&mut self, c: char) {
        let bp = self.byte_pos();
        self.content.insert(bp, c);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let bp = self.byte_pos();
            self.content.remove(bp);
        }
    }

    fn delete(&mut self) {
        let bp = self.byte_pos();
        if bp < self.content.len() {
            self.content.remove(bp);
        }
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        if self.cursor < self.char_count() {
            self.cursor += 1;
        }
    }

    fn home(&mut self) {
        self.cursor = 0;
    }

    fn end(&mut self) {
        self.cursor = self.char_count();
    }

    fn delete_word(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let before: String = self.content.chars().take(self.cursor).collect();
        let after: String = self.content.chars().skip(self.cursor).collect();
        let trimmed = before.trim_end();
        let new_before = match trimmed.rfind(|c: char| c.is_whitespace()) {
            Some(pos) => &trimmed[..=pos],
            None => "",
        };
        self.cursor = new_before.chars().count();
        self.content = format!("{}{}", new_before, after);
    }

    fn kill_to_start(&mut self) {
        let after: String = self.content.chars().skip(self.cursor).collect();
        self.content = after;
        self.cursor = 0;
    }

    fn kill_to_end(&mut self) {
        self.content = self.content.chars().take(self.cursor).collect();
    }

    fn as_str(&self) -> &str {
        &self.content
    }

    fn into_string(self) -> String {
        self.content
    }

    fn before_cursor(&self) -> &str {
        &self.content[..self.byte_pos()]
    }

    fn at_cursor(&self) -> &str {
        let bp = self.byte_pos();
        if bp < self.content.len() {
            let next = self.content[bp..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| bp + i)
                .unwrap_or(self.content.len());
            &self.content[bp..next]
        } else {
            ""
        }
    }

    fn after_cursor(&self) -> &str {
        let bp = self.byte_pos();
        if bp < self.content.len() {
            let next = self.content[bp..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| bp + i)
                .unwrap_or(self.content.len());
            &self.content[next..]
        } else {
            ""
        }
    }
}

// ── Types ──

enum InputMode {
    Normal,
    Adding(TextInput),
    Editing(TextInput),
    Filtering(TextInput),
    Searching(TextInput),
    ConfirmDelete(String, String), // (task_id, title)
    WantEditorForNotes,
}

enum Clipboard {
    Empty,
    Cut(Task),
    Yank(String, String), // (task_id, title)
}

/// Pending two-key sequence
#[derive(Clone, Copy, PartialEq)]
enum PendingKey {
    None,
    G,  // gg
    D,  // dd
    Y,  // yy
    Lt, // <<
    Gt, // >>
}

impl PendingKey {
    fn label(&self) -> Option<&'static str> {
        match self {
            PendingKey::None => None,
            PendingKey::G => Some("g"),
            PendingKey::D => Some("d"),
            PendingKey::Y => Some("y"),
            PendingKey::Lt => Some("<"),
            PendingKey::Gt => Some(">"),
        }
    }
}

// ── Undo/Redo ──

#[derive(Clone)]
enum UndoAction {
    Delete(String),
    Create {
        parent_id: Option<String>,
        title: String,
        notes: String,
        completed: bool,
        position: i64,
    },
    Update {
        id: String,
        title: String,
        notes: String,
        completed: bool,
        position: i64,
        parent_id: Option<String>,
        deferred: bool,
    },
}

#[derive(Clone)]
struct UndoEntry {
    description: String,
    actions: Vec<UndoAction>,
}

// ── App ──

struct App {
    client: FloClient,
    nav_stack: Vec<(String, String)>,
    children: Vec<Task>,
    current_task: Option<Task>,
    list_state: ListState,
    input_mode: InputMode,
    show_notes: bool,
    pending_counts: std::collections::HashMap<String, i64>,
    show_completed: bool,
    show_deferred: bool,
    show_tree: bool,
    tree_items: Vec<(Task, usize)>,
    pending_key: PendingKey,
    clipboard: Clipboard,
    filter: String,
    status_msg: Option<String>,
    term_height: u16,
    show_help: bool,
    adding_is_push: bool,
    adding_is_append: bool,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    search_results: Vec<SearchResult>,
    search_list_state: ListState,
    selected_id: Option<String>,
    // QoL additions
    notes_scroll: u16,
    selected_ids: HashSet<String>,
    quit_pending: bool,
    list_area: Rect,
    notes_area: Rect,
    help_scroll: u16,
    // Focus
    focused_tasks: Vec<Task>,
}

impl App {
    fn new(port: u16) -> Self {
        Self {
            client: FloClient::new(port),
            nav_stack: Vec::new(),
            children: Vec::new(),
            current_task: None,
            list_state: ListState::default(),
            input_mode: InputMode::Normal,
            show_notes: true,
            pending_counts: std::collections::HashMap::new(),
            show_completed: false,
            show_deferred: false,
            show_tree: false,
            tree_items: Vec::new(),
            pending_key: PendingKey::None,
            clipboard: Clipboard::Empty,
            filter: String::new(),
            status_msg: None,
            term_height: 24,
            show_help: false,
            adding_is_push: false,
            adding_is_append: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            search_results: Vec::new(),
            search_list_state: ListState::default(),
            selected_id: None,
            notes_scroll: 0,
            selected_ids: HashSet::new(),
            quit_pending: false,
            list_area: Rect::default(),
            notes_area: Rect::default(),
            help_scroll: 0,
            focused_tasks: Vec::new(),
        }
    }

    fn current_id(&self) -> Option<&str> {
        self.nav_stack.last().map(|(id, _)| id.as_str())
    }

    // ── Selection ──

    /// Save the currently selected task's ID
    fn save_selection(&mut self) {
        self.selected_id = self.selected_task().map(|t| t.id.clone());
    }

    /// Restore selection by task ID, falling back to same index or clamping
    fn restore_selection(&mut self) {
        let len = self.visible_count();
        if len == 0 {
            self.list_state.select(None);
            return;
        }

        // Try to find the saved task
        if let Some(ref id) = self.selected_id {
            if let Some(idx) = self.find_index_by_id(id) {
                self.list_state.select(Some(idx));
                return;
            }
        }

        // Fall back: clamp current index
        match self.list_state.selected() {
            Some(sel) if sel >= len => self.list_state.select(Some(len - 1)),
            None => self.list_state.select(Some(0)),
            _ => {} // keep current index
        }
    }

    /// Select a specific task by ID, with fallback
    fn select_by_id(&mut self, id: &str) {
        let len = self.visible_count();
        if len == 0 {
            self.list_state.select(None);
            return;
        }
        match self.find_index_by_id(id) {
            Some(idx) => self.list_state.select(Some(idx)),
            None => {
                // Clamp
                if let Some(sel) = self.list_state.selected() {
                    if sel >= len {
                        self.list_state.select(Some(len - 1));
                    }
                } else {
                    self.list_state.select(Some(0));
                }
            }
        }
    }

    fn find_index_by_id(&self, id: &str) -> Option<usize> {
        if self.show_tree {
            self.tree_items.iter().position(|(t, _)| t.id == id)
        } else {
            self.children.iter().position(|t| t.id == id)
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

    // ── Undo ──

    fn push_undo(&mut self, entry: UndoEntry) {
        self.undo_stack.push(entry);
        self.redo_stack.clear();
    }

    fn snapshot_task(task: &Task) -> UndoAction {
        UndoAction::Update {
            id: task.id.clone(),
            title: task.title.clone(),
            notes: task.notes.clone(),
            completed: task.completed,
            position: task.position,
            parent_id: task.parent_id.clone(),
            deferred: task.deferred,
        }
    }

    async fn execute_undo_entry(&self, entry: &UndoEntry) -> Result<UndoEntry> {
        let mut reverse_actions = Vec::new();
        for action in entry.actions.iter().rev() {
            match action {
                UndoAction::Delete(id) => {
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
                    if *completed || *position != new_task.position {
                        self.client.update_task(&new_task.id, &UpdateTask {
                            completed: Some(*completed),
                            position: Some(*position),
                            ..Default::default()
                        }).await?;
                    }
                    reverse_actions.push(UndoAction::Delete(new_task.id));
                }
                UndoAction::Update { id, title, notes, completed, position, parent_id, deferred } => {
                    if let Ok(tw) = self.client.get_task(id).await {
                        reverse_actions.push(Self::snapshot_task(&tw.task));
                    }
                    self.client.update_task(id, &UpdateTask {
                        title: Some(title.clone()),
                        notes: Some(notes.clone()),
                        completed: Some(*completed),
                        position: Some(*position),
                        parent_id: Some(parent_id.clone().unwrap_or_default()),
                        deferred: Some(*deferred),
                        ..Default::default()
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
        self.save_selection();
        self.refresh().await?;
        self.restore_selection();
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
        self.save_selection();
        self.refresh().await?;
        self.restore_selection();
        Ok(())
    }

    // ── Navigation ──

    fn breadcrumb(&self) -> String {
        if self.nav_stack.is_empty() {
            "root".to_string()
        } else {
            let mut parts = vec!["root"];
            parts.extend(self.nav_stack.iter().map(|(_, t)| t.as_str()));
            parts.join(" > ")
        }
    }

    async fn refresh(&mut self) -> Result<()> {
        let parent_id = self.current_id().map(|s| s.to_string());

        self.current_task = match &parent_id {
            Some(id) => {
                let tw = self.client.get_task(id).await?;
                Some(tw.task)
            }
            None => None,
        };

        let all_children = self.client.list_tasks(parent_id.as_deref()).await?;
        self.children = all_children
            .into_iter()
            .filter(|t| self.show_completed || !t.completed)
            .filter(|t| self.show_deferred || !t.deferred)
            .filter(|t| {
                self.filter.is_empty()
                    || t.title.to_lowercase().contains(&self.filter.to_lowercase())
            })
            .collect();

        // Populate pending counts for all levels
        self.pending_counts.clear();
        if parent_id.is_none() {
            let home = self.client.home().await?;
            for p in home {
                self.pending_counts.insert(p.id, p.pending_count);
            }
        } else if !self.show_tree {
            // For non-root levels, fetch child counts per task
            for child in &self.children {
                let grandchildren = self.client.list_tasks(Some(&child.id)).await?;
                let pending = grandchildren.iter().filter(|t| !t.completed).count() as i64;
                if pending > 0 {
                    self.pending_counts.insert(child.id.clone(), pending);
                }
            }
        }

        if self.show_tree {
            self.tree_items.clear();
            for child in &self.children {
                let subtree = self.client.get_subtree(&child.id).await?;
                let mut depth_map: std::collections::HashMap<String, usize> =
                    std::collections::HashMap::new();
                for task in &subtree {
                    let depth = match &task.parent_id {
                        None => 0,
                        Some(pid) => depth_map.get(pid).map(|d| d + 1).unwrap_or(0),
                    };
                    depth_map.insert(task.id.clone(), depth);
                    if (!self.show_completed && task.completed)
                        || (!self.show_deferred && task.deferred)
                        || (!self.filter.is_empty()
                            && !task.title.to_lowercase().contains(&self.filter.to_lowercase()))
                    {
                        continue;
                    }
                    self.tree_items.push((task.clone(), depth));
                }
            }
        }

        // Refresh focused tasks
        self.focused_tasks = self.client.get_focused_tasks().await.unwrap_or_default();

        Ok(())
    }

    /// Refresh and restore selection to the same task
    async fn refresh_keep_selection(&mut self) -> Result<()> {
        self.save_selection();
        self.refresh().await?;
        self.restore_selection();
        Ok(())
    }

    async fn enter_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task().cloned() {
            // Auto-acknowledge on enter
            if !task.acknowledged {
                let _ = self.client.acknowledge_task(&task.id).await;
            }
            if self.show_tree {
                let ancestors = self.client.get_ancestors(&task.id).await?;
                self.nav_stack.clear();
                for ancestor in &ancestors {
                    self.nav_stack
                        .push((ancestor.id.clone(), ancestor.title.clone()));
                }
            } else {
                self.nav_stack.push((task.id.clone(), task.title.clone()));
            }
            self.selected_ids.clear();
            self.notes_scroll = 0;
            self.refresh().await?;
            self.list_state.select(if self.visible_count() > 0 { Some(0) } else { None });
        }
        Ok(())
    }

    async fn pop(&mut self) -> Result<()> {
        if let Some((child_id, _)) = self.nav_stack.pop() {
            self.selected_ids.clear();
            self.notes_scroll = 0;
            self.refresh().await?;
            self.select_by_id(&child_id);
        }
        Ok(())
    }

    // ── Mutations ──

    /// Insert a task below the currently selected task (same level).
    /// Bumps positions of subsequent siblings to make room.
    async fn insert_task(&mut self, title: &str) -> Result<()> {
        let parent_id = self.current_id().map(|s| s.to_string());

        // Determine insertion position: right after selected task
        let insert_pos = self.selected_task().map(|t| t.position + 1);

        let task = self.client
            .create_task(&CreateTask {
                parent_id: parent_id.clone(),
                title: title.to_string(),
                notes: String::new(),
            })
            .await?;
        let new_id = task.id.clone();

        // If we have an insertion point, reposition
        if let Some(pos) = insert_pos {
            // Bump siblings at or after the target position (excluding the new task)
            let siblings = self.client.list_tasks(parent_id.as_deref()).await?;
            let mut undo_actions: Vec<UndoAction> = vec![UndoAction::Delete(task.id.clone())];
            for sib in &siblings {
                if sib.id != new_id && sib.position >= pos {
                    undo_actions.push(Self::snapshot_task(sib));
                    self.client.update_task(&sib.id, &UpdateTask {
                        position: Some(sib.position + 1),
                        ..Default::default()
                    }).await?;
                }
            }
            self.client.update_task(&new_id, &UpdateTask {
                position: Some(pos),
                ..Default::default()
            }).await?;
            self.push_undo(UndoEntry {
                description: format!("insert \"{}\"", title),
                actions: undo_actions,
            });
        } else {
            // No selection — just append
            self.push_undo(UndoEntry {
                description: format!("add \"{}\"", title),
                actions: vec![UndoAction::Delete(task.id)],
            });
        }

        self.refresh().await?;
        self.select_by_id(&new_id);
        self.status_msg = Some(format!("Added: {}", title));
        Ok(())
    }

    /// Append a task to the end of the current level.
    async fn append_task(&mut self, title: &str) -> Result<()> {
        let parent_id = self.current_id().map(|s| s.to_string());
        let task = self.client
            .create_task(&CreateTask {
                parent_id,
                title: title.to_string(),
                notes: String::new(),
            })
            .await?;
        let new_id = task.id.clone();
        self.push_undo(UndoEntry {
            description: format!("append \"{}\"", title),
            actions: vec![UndoAction::Delete(task.id)],
        });
        self.refresh().await?;
        self.select_by_id(&new_id);
        self.status_msg = Some(format!("Appended: {}", title));
        Ok(())
    }

    /// Insert below selected + enter into it.
    async fn push_task(&mut self, title: &str) -> Result<()> {
        let parent_id = self.current_id().map(|s| s.to_string());
        let insert_pos = self.selected_task().map(|t| t.position + 1);

        let task = self.client
            .create_task(&CreateTask {
                parent_id: parent_id.clone(),
                title: title.to_string(),
                notes: String::new(),
            })
            .await?;
        let new_id = task.id.clone();

        if let Some(pos) = insert_pos {
            let siblings = self.client.list_tasks(parent_id.as_deref()).await?;
            for sib in &siblings {
                if sib.id != new_id && sib.position >= pos {
                    self.client.update_task(&sib.id, &UpdateTask {
                        position: Some(sib.position + 1),
                        ..Default::default()
                    }).await?;
                }
            }
            self.client.update_task(&new_id, &UpdateTask {
                position: Some(pos),
                ..Default::default()
            }).await?;
        }

        self.push_undo(UndoEntry {
            description: format!("push \"{}\"", title),
            actions: vec![UndoAction::Delete(task.id.clone())],
        });
        self.nav_stack.push((task.id.clone(), task.title.clone()));
        self.refresh().await?;
        self.list_state.select(if self.visible_count() > 0 { Some(0) } else { None });
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

            if !self.show_completed && new_completed {
                let old_idx = self.list_state.selected();
                self.refresh().await?;
                let len = self.visible_count();
                if len == 0 {
                    self.list_state.select(None);
                } else if let Some(idx) = old_idx {
                    self.list_state.select(Some(idx.min(len - 1)));
                }
            } else {
                self.refresh_keep_selection().await?;
            }
        }
        Ok(())
    }

    async fn toggle_done_bulk(&mut self) -> Result<()> {
        let ids: Vec<String> = self.selected_ids.drain().collect();
        let mut undo_actions = Vec::new();
        let mut count = 0usize;
        for id in &ids {
            if let Ok(tw) = self.client.get_task(id).await {
                let task = &tw.task;
                undo_actions.push(Self::snapshot_task(task));
                self.client.update_task(id, &UpdateTask {
                    completed: Some(!task.completed),
                    ..Default::default()
                }).await?;
                count += 1;
            }
        }
        self.push_undo(UndoEntry {
            description: format!("toggle {} tasks", count),
            actions: undo_actions,
        });
        self.status_msg = Some(format!("Toggled {} tasks", count));
        self.refresh_keep_selection().await?;
        Ok(())
    }

    async fn defer_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task().cloned() {
            let updated = self.client.defer_task(&task.id).await?;
            if updated.deferred {
                self.status_msg = Some(format!("Deferred: {} (review in {}d)", updated.title, updated.review_interval));
                if !self.show_deferred {
                    let old_idx = self.list_state.selected();
                    self.refresh().await?;
                    let len = self.visible_count();
                    if len == 0 {
                        self.list_state.select(None);
                    } else if let Some(idx) = old_idx {
                        self.list_state.select(Some(idx.min(len - 1)));
                    }
                } else {
                    self.refresh_keep_selection().await?;
                }
            } else {
                self.status_msg = Some(format!("Undeferred: {}", updated.title));
                self.refresh_keep_selection().await?;
            }
        }
        Ok(())
    }

    async fn touch_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task().cloned() {
            let _ = self.client.touch_task(&task.id, None).await?;
            self.status_msg = Some(format!("Touched: {}", task.title));
            self.refresh_keep_selection().await?;
        }
        Ok(())
    }

    async fn focus_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task().cloned() {
            match self.client.focus_task(&task.id, None).await {
                Ok(updated) => {
                    if updated.focused {
                        self.status_msg = Some(format!("Focused: {}", updated.title));
                    } else {
                        self.status_msg = Some(format!("Unfocused: {}", updated.title));
                    }
                    self.refresh_keep_selection().await?;
                }
                Err(e) => {
                    self.status_msg = Some(format!("{}", e));
                }
            }
        }
        Ok(())
    }

    async fn delete_selected(&mut self) -> Result<()> {
        if let Some(task) = self.selected_task().cloned() {
            let children = self.client.list_tasks(Some(&task.id)).await?;
            if children.is_empty() {
                let old_idx = self.list_state.selected();
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
                let len = self.visible_count();
                if len == 0 {
                    self.list_state.select(None);
                } else if let Some(idx) = old_idx {
                    self.list_state.select(Some(idx.min(len - 1)));
                }
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
            self.refresh_keep_selection().await?;
        }
        Ok(())
    }

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
        let tid = task.id.clone();
        self.refresh().await?;
        self.select_by_id(&tid);
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
        let tid = task.id.clone();
        self.refresh().await?;
        self.select_by_id(&tid);
        Ok(())
    }

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
        let tid = task.id.clone();
        self.refresh().await?;
        if self.show_tree {
            self.select_by_id(&tid);
        } else {
            let len = self.visible_count();
            if len == 0 {
                self.list_state.select(None);
            } else {
                let clamped = idx.min(len - 1);
                self.list_state.select(Some(clamped));
            }
        }
        Ok(())
    }

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
        let tid = task.id.clone();
        self.refresh().await?;
        self.select_by_id(&tid);
        Ok(())
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
                let new_id = new_task.id.clone();
                self.push_undo(UndoEntry {
                    description: format!("paste \"{}\"", title),
                    actions: vec![UndoAction::Delete(new_task.id)],
                });
                self.status_msg = Some(format!("Pasted: {}", title));
                self.refresh().await?;
                self.select_by_id(&new_id);
            }
            Clipboard::Yank(id, title) => {
                if let Ok(source) = self.client.get_task(&id).await {
                    let new_task = self.client.create_task(&CreateTask {
                        parent_id,
                        title: source.task.title.clone(),
                        notes: source.task.notes.clone(),
                    }).await?;
                    let new_id = new_task.id.clone();
                    self.push_undo(UndoEntry {
                        description: format!("paste \"{}\"", title),
                        actions: vec![UndoAction::Delete(new_task.id)],
                    });
                    self.status_msg = Some(format!("Copied: {}", title));
                    self.clipboard = Clipboard::Yank(id, title);
                    self.refresh().await?;
                    self.select_by_id(&new_id);
                } else {
                    self.status_msg = Some("Source task no longer exists".to_string());
                    self.clipboard = Clipboard::Yank(id, title);
                }
            }
        }
        Ok(())
    }

    async fn edit_notes_external(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        let Some(task) = self.selected_task().cloned() else {
            self.status_msg = Some("No task selected".to_string());
            return Ok(());
        };

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

        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

        let status = std::process::Command::new(&editor)
            .arg("+4")
            .arg(&tmp_path)
            .status();

        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
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
                    self.refresh_keep_selection().await?;
                }
            }
            _ => {
                self.status_msg = Some("Editor failed or cancelled".to_string());
            }
        }

        std::fs::remove_file(&tmp_path).ok();
        Ok(())
    }

    // ── Cursor movement (wrap-around) ──

    fn move_up(&mut self) {
        self.notes_scroll = 0;
        let len = self.visible_count();
        if len == 0 { return; }
        match self.list_state.selected() {
            Some(0) => self.list_state.select(Some(len - 1)), // wrap to bottom
            Some(sel) => self.list_state.select(Some(sel - 1)),
            None => self.list_state.select(Some(0)),
        }
    }

    fn move_down(&mut self) {
        self.notes_scroll = 0;
        let len = self.visible_count();
        if len == 0 { return; }
        match self.list_state.selected() {
            Some(sel) if sel + 1 >= len => self.list_state.select(Some(0)), // wrap to top
            Some(sel) => self.list_state.select(Some(sel + 1)),
            None => self.list_state.select(Some(0)),
        }
    }
}

// ── Event loop ──

pub async fn run(port: u16) -> Result<()> {
    let mut app = App::new(port);
    app.refresh().await?;
    app.list_state.select(if app.visible_count() > 0 { Some(0) } else { None });

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    result
}

async fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        if matches!(app.input_mode, InputMode::WantEditorForNotes) {
            app.input_mode = InputMode::Normal;
            app.edit_notes_external(terminal).await?;
            continue;
        }

        terminal.draw(|f| {
            app.term_height = f.area().height;
            ui(f, app);
        })?;

        // Poll with 1-second timeout so budget countdowns update
        if event::poll(Duration::from_secs(1))? {
            match event::read()? {
                Event::Key(key) => {
                    if app.show_help {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('?') => {
                                app.show_help = false;
                                app.help_scroll = 0;
                            }
                            KeyCode::Char('j') | KeyCode::Down => {
                                app.help_scroll += 1;
                            }
                            KeyCode::Char('k') | KeyCode::Up => {
                                app.help_scroll = app.help_scroll.saturating_sub(1);
                            }
                            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.help_scroll += 10;
                            }
                            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                app.help_scroll = app.help_scroll.saturating_sub(10);
                            }
                            KeyCode::Char('g') => app.help_scroll = 0,
                            KeyCode::Char('G') => app.help_scroll = 100, // will be clamped
                            _ => {}
                        }
                        continue;
                    }

                    match &mut app.input_mode {
                        InputMode::Normal => {
                            if handle_normal_key(app, key).await? {
                                return Ok(());
                            }
                        }
                        InputMode::Adding(ref mut input) => {
                            match key.code {
                                KeyCode::Enter => {
                                    let is_push = app.adding_is_push;
                                    let is_append = app.adding_is_append;
                                    if let InputMode::Adding(input) =
                                        std::mem::replace(&mut app.input_mode, InputMode::Normal)
                                    {
                                        let text = input.into_string();
                                        if !text.trim().is_empty() {
                                            if is_push {
                                                app.push_task(text.trim()).await?;
                                            } else if is_append {
                                                app.append_task(text.trim()).await?;
                                            } else {
                                                app.insert_task(text.trim()).await?;
                                            }
                                        }
                                    }
                                    app.adding_is_push = false;
                                    app.adding_is_append = false;
                                }
                                KeyCode::Esc => {
                                    app.input_mode = InputMode::Normal;
                                    app.adding_is_push = false;
                                    app.adding_is_append = false;
                                }
                                _ => handle_text_input(key, input),
                            }
                        }
                        InputMode::Editing(ref mut input) => {
                            match key.code {
                                KeyCode::Enter => {
                                    if let InputMode::Editing(input) =
                                        std::mem::replace(&mut app.input_mode, InputMode::Normal)
                                    {
                                        let text = input.into_string();
                                        if !text.trim().is_empty() {
                                            app.rename_selected(text.trim()).await?;
                                        }
                                    }
                                }
                                KeyCode::Esc => {
                                    app.input_mode = InputMode::Normal;
                                }
                                _ => handle_text_input(key, input),
                            }
                        }
                        InputMode::Filtering(ref mut input) => {
                            match key.code {
                                KeyCode::Esc => {
                                    app.filter.clear();
                                    app.input_mode = InputMode::Normal;
                                    app.refresh_keep_selection().await?;
                                }
                                KeyCode::Enter => {
                                    if let InputMode::Filtering(input) =
                                        std::mem::replace(&mut app.input_mode, InputMode::Normal)
                                    {
                                        app.filter = input.into_string();
                                        app.refresh().await?;
                                        app.list_state.select(
                                            if app.visible_count() > 0 { Some(0) } else { None }
                                        );
                                    }
                                }
                                _ => {
                                    let old = input.as_str().to_string();
                                    handle_text_input(key, input);
                                    if old != input.as_str() {
                                        app.filter = input.as_str().to_string();
                                        app.refresh().await?;
                                        app.list_state.select(
                                            if app.visible_count() > 0 { Some(0) } else { None }
                                        );
                                    }
                                }
                            }
                        }
                        InputMode::Searching(ref mut input) => {
                            match key.code {
                                KeyCode::Esc => {
                                    app.search_results.clear();
                                    app.input_mode = InputMode::Normal;
                                }
                                KeyCode::Enter => {
                                    if let Some(sel) = app.search_list_state.selected() {
                                        if let Some(result) = app.search_results.get(sel) {
                                            let task_id = result.task.id.clone();
                                            let ancestors = app.client.get_ancestors(&task_id).await?;
                                            app.nav_stack.clear();
                                            for ancestor in &ancestors {
                                                app.nav_stack.push((
                                                    ancestor.id.clone(),
                                                    ancestor.title.clone(),
                                                ));
                                            }
                                            app.search_results.clear();
                                            app.input_mode = InputMode::Normal;
                                            app.refresh().await?;
                                            // Land on the actual found task
                                            app.select_by_id(&task_id);
                                        }
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
                                _ => {
                                    let old = input.as_str().to_string();
                                    handle_text_input(key, input);
                                    if old != input.as_str() {
                                        let q = input.as_str().to_string();
                                        do_search(app, &q).await?;
                                    }
                                }
                            }
                        }
                        InputMode::ConfirmDelete(ref id, ref title) => {
                            match key.code {
                                KeyCode::Char('y') | KeyCode::Char('Y') => {
                                    let id = id.clone();
                                    let title = title.clone();
                                    let old_idx = app.list_state.selected();
                                    app.client.delete_task(&id).await?;
                                    app.status_msg = Some(format!("Deleted: {}", title));
                                    app.input_mode = InputMode::Normal;
                                    app.refresh().await?;
                                    let len = app.visible_count();
                                    if len == 0 {
                                        app.list_state.select(None);
                                    } else if let Some(idx) = old_idx {
                                        app.list_state.select(Some(idx.min(len - 1)));
                                    }
                                }
                                _ => {
                                    app.status_msg = None;
                                    app.input_mode = InputMode::Normal;
                                }
                            }
                        }
                        InputMode::WantEditorForNotes => {}
                    }
                }
                Event::Mouse(mouse) => {
                    if app.show_help {
                        match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                app.help_scroll = app.help_scroll.saturating_sub(3);
                            }
                            MouseEventKind::ScrollDown => {
                                app.help_scroll += 3;
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if !matches!(app.input_mode, InputMode::Normal) { continue; }

                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            let row = mouse.row;
                            let col = mouse.column;
                            // Click in task list area
                            if row >= app.list_area.y
                                && row < app.list_area.y + app.list_area.height
                                && col >= app.list_area.x
                                && col < app.list_area.x + app.list_area.width
                            {
                                let clicked_row = (row - app.list_area.y) as usize;
                                let offset = app.list_state.offset();
                                let clicked_index = offset + clicked_row;
                                if clicked_index < app.visible_count() {
                                    app.notes_scroll = 0;
                                    app.list_state.select(Some(clicked_index));
                                }
                            }
                        }
                        MouseEventKind::ScrollUp => {
                            if mouse.column >= app.notes_area.x
                                && app.notes_area.width > 0
                                && app.show_notes
                            {
                                app.notes_scroll = app.notes_scroll.saturating_sub(3);
                            } else {
                                app.move_up();
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            if mouse.column >= app.notes_area.x
                                && app.notes_area.width > 0
                                && app.show_notes
                            {
                                app.notes_scroll += 3;
                            } else {
                                app.move_down();
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        } else {
            // Timeout — check for budget expiry alerts
            check_budget_alerts(app);
        }
    }
}

fn check_budget_alerts(app: &mut App) {
    for task in &app.focused_tasks {
        if let (Some(ref focused_at), Some(budget)) = (&task.focused_at, task.budget_minutes) {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(focused_at) {
                let elapsed = chrono::Utc::now().signed_duration_since(dt).num_minutes();
                let remaining = budget - elapsed;
                // Alert when budget just expired (within 1 minute window)
                if (-1..=0).contains(&remaining) {
                    app.status_msg = Some(format!("BUDGET EXPIRED: {}", task.title));
                }
            }
        }
    }
}

async fn do_search(app: &mut App, query: &str) -> Result<()> {
    if query.len() >= 2 {
        app.search_results = app.client.search(query).await?;
        app.search_list_state.select(
            if !app.search_results.is_empty() { Some(0) } else { None }
        );
    } else {
        app.search_results.clear();
        app.search_list_state.select(None);
    }
    Ok(())
}

fn handle_text_input(key: KeyEvent, input: &mut TextInput) {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('a') => input.home(),
            KeyCode::Char('e') => input.end(),
            KeyCode::Char('w') => input.delete_word(),
            KeyCode::Char('u') => input.kill_to_start(),
            KeyCode::Char('k') => input.kill_to_end(),
            _ => {} // ignore other ctrl combos
        }
        return;
    }
    match key.code {
        KeyCode::Char(c) => input.insert(c),
        KeyCode::Backspace => input.backspace(),
        KeyCode::Delete => input.delete(),
        KeyCode::Left => input.move_left(),
        KeyCode::Right => input.move_right(),
        KeyCode::Home => input.home(),
        KeyCode::End => input.end(),
        _ => {}
    }
}

async fn handle_normal_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Reset quit pending on any key except q
    if key.code != KeyCode::Char('q') && app.quit_pending {
        app.quit_pending = false;
        app.status_msg = None;
    }

    // Handle pending two-key sequences
    if app.pending_key != PendingKey::None {
        let pending = app.pending_key;
        app.pending_key = PendingKey::None;

        match (pending, key.code) {
            (PendingKey::G, KeyCode::Char('g')) => {
                if app.visible_count() > 0 {
                    app.notes_scroll = 0;
                    app.list_state.select(Some(0));
                }
                return Ok(false);
            }
            (PendingKey::D, KeyCode::Char('d')) => {
                if let Some(task) = app.selected_task().cloned() {
                    let old_idx = app.list_state.selected();
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
                    let len = app.visible_count();
                    if len == 0 {
                        app.list_state.select(None);
                    } else if let Some(idx) = old_idx {
                        app.list_state.select(Some(idx.min(len - 1)));
                    }
                }
                return Ok(false);
            }
            (PendingKey::Y, KeyCode::Char('y')) => {
                if let Some(task) = app.selected_task().cloned() {
                    app.clipboard = Clipboard::Yank(task.id.clone(), task.title.clone());
                    app.status_msg = Some(format!("Yanked: {}", task.title));
                }
                return Ok(false);
            }
            (PendingKey::Lt, KeyCode::Char('<')) => {
                app.outdent().await?;
                return Ok(false);
            }
            (PendingKey::Gt, KeyCode::Char('>')) => {
                app.indent().await?;
                return Ok(false);
            }
            _ => {
                // Sequence broken — fall through to handle this key normally
            }
        }
    }

    match key.code {
        // Quit: double-tap q to confirm
        KeyCode::Char('q') => {
            if app.quit_pending {
                return Ok(true);
            }
            app.quit_pending = true;
            app.status_msg = Some("Press q again to quit".to_string());
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(true),

        // Navigation
        KeyCode::Char('j') | KeyCode::Down => {
            app.status_msg = None;
            app.move_down();
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.status_msg = None;
            app.move_up();
        }
        KeyCode::Char('g') => {
            app.pending_key = PendingKey::G;
        }
        KeyCode::Char('G') => {
            let len = app.visible_count();
            if len > 0 {
                app.notes_scroll = 0;
                app.list_state.select(Some(len - 1));
            }
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let half = (app.term_height / 2) as usize;
            if let Some(sel) = app.list_state.selected() {
                let target = (sel + half).min(app.visible_count().saturating_sub(1));
                app.notes_scroll = 0;
                app.list_state.select(Some(target));
            }
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            let half = (app.term_height / 2) as usize;
            if let Some(sel) = app.list_state.selected() {
                let target = sel.saturating_sub(half);
                app.notes_scroll = 0;
                app.list_state.select(Some(target));
            }
        }

        // Frame navigation
        KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => app.enter_selected().await?,
        KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => app.pop().await?,
        KeyCode::Char('H') | KeyCode::Char('~') => {
            app.nav_stack.clear();
            app.filter.clear();
            app.selected_ids.clear();
            app.notes_scroll = 0;
            app.refresh().await?;
            app.list_state.select(if app.visible_count() > 0 { Some(0) } else { None });
        }

        // Actions
        KeyCode::Char('a') => {
            app.input_mode = InputMode::Adding(TextInput::empty());
        }
        KeyCode::Char('A') => {
            app.input_mode = InputMode::Adding(TextInput::empty());
            app.adding_is_append = true;
        }
        KeyCode::Char('o') => {
            app.input_mode = InputMode::Adding(TextInput::empty());
            app.adding_is_push = true;
        }
        KeyCode::Char('e') => {
            if let Some(task) = app.selected_task() {
                let title = task.title.clone();
                app.input_mode = InputMode::Editing(TextInput::new(title));
            }
        }
        KeyCode::Char('x') => {
            if !app.selected_ids.is_empty() {
                app.toggle_done_bulk().await?;
            } else {
                app.toggle_done().await?;
            }
        }
        KeyCode::Char('z') => app.defer_selected().await?,
        KeyCode::Char('.') => app.touch_selected().await?,
        KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.search_results.clear();
            app.search_list_state.select(None);
            app.input_mode = InputMode::Searching(TextInput::empty());
        }
        KeyCode::Char('f') => app.focus_selected().await?,
        KeyCode::Char('d') => {
            app.pending_key = PendingKey::D;
        }
        KeyCode::Char('y') => {
            app.pending_key = PendingKey::Y;
        }
        KeyCode::Char('p') => app.paste().await?,
        KeyCode::Char('D') => app.delete_selected().await?,
        KeyCode::Char('J') => app.move_selected_down().await?,
        KeyCode::Char('K') => app.move_selected_up().await?,
        KeyCode::Char('<') => { app.pending_key = PendingKey::Lt; }
        KeyCode::Char('>') => { app.pending_key = PendingKey::Gt; }

        // Multi-select
        KeyCode::Char(' ') => {
            if let Some(task) = app.selected_task() {
                let id = task.id.clone();
                if app.selected_ids.contains(&id) {
                    app.selected_ids.remove(&id);
                } else {
                    app.selected_ids.insert(id);
                }
                app.move_down();
            }
        }
        KeyCode::Char('V') => {
            if app.selected_ids.is_empty() {
                // Select all visible
                if app.show_tree {
                    for (task, _) in &app.tree_items {
                        app.selected_ids.insert(task.id.clone());
                    }
                } else {
                    for task in &app.children {
                        app.selected_ids.insert(task.id.clone());
                    }
                }
                app.status_msg = Some(format!("{} selected", app.selected_ids.len()));
            } else {
                let count = app.selected_ids.len();
                app.selected_ids.clear();
                app.status_msg = Some(format!("{} deselected", count));
            }
        }

        // Notes scroll
        KeyCode::Char('[') => {
            app.notes_scroll = app.notes_scroll.saturating_sub(3);
        }
        KeyCode::Char(']') => {
            app.notes_scroll += 3;
        }

        // Undo/redo
        KeyCode::Char('u') => app.undo().await?,
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => app.redo().await?,

        // Notes
        KeyCode::Tab => {
            app.show_notes = !app.show_notes;
        }
        KeyCode::Char('n') => {
            if app.selected_task().is_some() {
                app.show_notes = true;
                app.input_mode = InputMode::WantEditorForNotes;
            }
        }

        // Search/filter
        KeyCode::Char('/') => {
            app.input_mode = InputMode::Filtering(TextInput::new(app.filter.clone()));
        }
        KeyCode::Esc => {
            if !app.selected_ids.is_empty() {
                let count = app.selected_ids.len();
                app.selected_ids.clear();
                app.status_msg = Some(format!("{} deselected", count));
            } else if !app.filter.is_empty() {
                app.filter.clear();
                app.refresh_keep_selection().await?;
            }
        }

        // Toggles
        KeyCode::Char('t') => {
            app.show_tree = !app.show_tree;
            app.save_selection();
            app.refresh().await?;
            app.restore_selection();
        }
        KeyCode::Char('c') => {
            app.show_completed = !app.show_completed;
            app.save_selection();
            app.refresh().await?;
            app.restore_selection();
        }
        KeyCode::Char('Z') => {
            app.show_deferred = !app.show_deferred;
            app.save_selection();
            app.refresh().await?;
            app.restore_selection();
        }

        // Refresh / Help
        KeyCode::Char('r') => app.refresh_keep_selection().await?,
        KeyCode::Char('?') => {
            app.show_help = true;
        }

        _ => {}
    }
    Ok(false)
}

// ── Helpers ──

fn format_time(iso: &str) -> &str {
    if iso.len() >= 16 {
        &iso[..16]
    } else {
        iso
    }
}

fn render_input_line(label: &str, input: &TextInput, color: Color) -> Line<'static> {
    let before = input.before_cursor().to_string();
    let cursor_ch = input.at_cursor().to_string();
    let after = input.after_cursor().to_string();

    if cursor_ch.is_empty() {
        Line::from(vec![
            Span::styled(format!(" {}", label), Style::default().fg(color)),
            Span::raw(before),
            Span::styled(" ", Style::default().bg(color)),
        ])
    } else {
        Line::from(vec![
            Span::styled(format!(" {}", label), Style::default().fg(color)),
            Span::raw(before),
            Span::styled(cursor_ch, Style::default().bg(color).fg(Color::Black)),
            Span::raw(after),
        ])
    }
}

// ── Rendering ──

fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();

    let focus_height = if app.focused_tasks.is_empty() { 0 } else { app.focused_tasks.len() as u16 + 1 };

    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),             // breadcrumb
            Constraint::Length(focus_height),   // focus header (0 if none)
            Constraint::Min(3),                // content
            Constraint::Length(2),              // status
        ])
        .split(area);

    // Breadcrumb + position indicator
    let position_text = match app.list_state.selected() {
        Some(sel) => format!("{}/{}", sel + 1, app.visible_count()),
        None if app.visible_count() == 0 => "empty".to_string(),
        None => format!("-/{}", app.visible_count()),
    };

    let mut bc_spans = vec![
        Span::styled(" ", Style::default()),
        Span::styled(
            app.breadcrumb(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    // Toggle indicators
    if app.show_tree {
        bc_spans.push(Span::styled("  [tree]", Style::default().fg(Color::Yellow)));
    }
    if app.show_completed {
        bc_spans.push(Span::styled("  [+done]", Style::default().fg(Color::Yellow)));
    }
    if app.show_deferred {
        bc_spans.push(Span::styled("  [+deferred]", Style::default().fg(Color::Yellow)));
    }
    if !app.selected_ids.is_empty() {
        bc_spans.push(Span::styled(
            format!("  [{} sel]", app.selected_ids.len()),
            Style::default().fg(Color::Magenta),
        ));
    }

    // Right-align the position indicator
    let bc_text_len: usize = bc_spans.iter().map(|s| s.content.len()).sum();
    let pos_len = position_text.len() + 1; // +1 for trailing space
    let padding = (area.width as usize).saturating_sub(bc_text_len + pos_len);
    if padding > 0 {
        bc_spans.push(Span::raw(" ".repeat(padding)));
    }
    bc_spans.push(Span::styled(
        format!("{} ", position_text),
        Style::default().fg(Color::DarkGray),
    ));

    f.render_widget(Paragraph::new(Line::from(bc_spans)), outer[0]);

    // Focus header
    if !app.focused_tasks.is_empty() {
        render_focus_header(f, app, outer[1]);
    }

    // Content
    let content_area = outer[2];
    if app.show_notes {
        let content = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(content_area);

        app.list_area = content[0];
        app.notes_area = content[1];
        render_task_list(f, app, content[0]);
        render_notes_panel(f, app, content[1]);
    } else {
        app.list_area = content_area;
        app.notes_area = Rect::default();
        render_task_list(f, app, content_area);
    }

    // Status bar
    render_status_bar(f, app, outer[3]);

    // Overlays
    if matches!(app.input_mode, InputMode::Searching(_)) {
        render_search(f, app, area);
    }
    if app.show_help {
        render_help(f, area, app.help_scroll);
    }
}

fn render_focus_header(f: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        " Focus",
        Style::default().fg(Color::Rgb(200, 150, 50)).add_modifier(Modifier::BOLD),
    )));
    for task in &app.focused_tasks {
        let mut spans = vec![Span::raw("  ")];
        spans.push(Span::styled(
            &task.title,
            Style::default().fg(Color::Rgb(200, 150, 50)),
        ));
        // Show elapsed / budget countdown
        if let Some(ref focused_at) = task.focused_at {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(focused_at) {
                let elapsed_mins = chrono::Utc::now().signed_duration_since(dt).num_minutes();
                if let Some(budget) = task.budget_minutes {
                    let remaining = budget - elapsed_mins;
                    let (text, color) = if remaining > 10 {
                        (format!("  ({}m left)", remaining), Color::Green)
                    } else if remaining > 0 {
                        (format!("  ({}m left)", remaining), Color::Yellow)
                    } else {
                        (format!("  (OVER {}m)", -remaining), Color::Red)
                    };
                    spans.push(Span::styled(text, Style::default().fg(color)));
                } else {
                    spans.push(Span::styled(
                        format!("  ({}m)", elapsed_mins),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
            }
        }
        lines.push(Line::from(spans));
    }
    let p = Paragraph::new(lines);
    f.render_widget(p, area);
}

fn render_task_list(f: &mut Frame, app: &mut App, area: Rect) {
    if app.visible_count() == 0 {
        let msg = if !app.filter.is_empty() {
            "No matches. Press Esc to clear filter."
        } else if app.nav_stack.is_empty() {
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
            .map(|(task, depth)| {
                build_task_line(task, *depth, &app.pending_counts, app.selected_ids.contains(&task.id))
            })
            .collect()
    } else {
        app.children
            .iter()
            .map(|task| {
                build_task_line(task, 0, &app.pending_counts, app.selected_ids.contains(&task.id))
            })
            .collect()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut app.list_state);
}

/// Compute staleness color from updated_at age.
/// For focused tasks: warm amber gradient. For unfocused: gray gradient.
fn staleness_color(updated_at: &str, focused: bool) -> Option<Color> {
    let Ok(updated) = chrono::DateTime::parse_from_rfc3339(updated_at) else {
        return None;
    };
    let age = chrono::Utc::now().signed_duration_since(updated);
    let days = age.num_days();

    if focused {
        // Focused tasks use warm amber gradient
        if days < 1 {
            Some(Color::Rgb(200, 150, 50))  // warm amber
        } else if days < 3 {
            Some(Color::Rgb(220, 100, 30))  // warm orange
        } else {
            Some(Color::Rgb(200, 60, 20))   // warm red
        }
    } else {
        // Unfocused tasks use gray dimming
        if days < 3 {
            None // fresh
        } else if days < 7 {
            Some(Color::Rgb(160, 160, 160)) // slightly dim
        } else if days < 14 {
            Some(Color::Rgb(120, 120, 120)) // dimmer
        } else {
            Some(Color::Rgb(80, 80, 80)) // very dim
        }
    }
}

fn build_task_line(
    task: &Task,
    depth: usize,
    pending_counts: &std::collections::HashMap<String, i64>,
    is_selected: bool,
) -> ListItem<'static> {
    let indent = "  ".repeat(depth);

    // Checkbox: [x] done, [?] unacked, [F] focused, [ ] normal
    let check = if task.completed {
        "x"
    } else if !task.acknowledged {
        "?"
    } else {
        " "
    };

    let select_marker = if is_selected { "*" } else { " " };

    // Staleness dimming for incomplete tasks
    let stale_color = if !task.completed {
        staleness_color(&task.updated_at, task.focused)
    } else {
        None
    };

    let mut spans = vec![
        Span::styled(
            select_marker.to_string(),
            if is_selected {
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            },
        ),
        Span::raw(indent),
    ];

    // Checkbox styling
    let check_style = if task.completed {
        Style::default().fg(Color::DarkGray)
    } else if !task.acknowledged {
        // Unacked: dimmer style
        Style::default().fg(Color::Rgb(120, 120, 120))
    } else if let Some(color) = stale_color {
        Style::default().fg(color)
    } else {
        Style::default().fg(Color::Green)
    };
    spans.push(Span::styled(format!("[{}] ", check), check_style));

    // Title styling
    let title_style = if task.completed {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::CROSSED_OUT)
    } else if !task.acknowledged {
        Style::default().fg(Color::Rgb(120, 120, 120))
    } else if let Some(color) = stale_color {
        Style::default().fg(color)
    } else {
        Style::default()
    };
    spans.push(Span::styled(task.title.clone(), title_style));

    // Show child counts at any depth level (not just root)
    if depth == 0 {
        if let Some(count) = pending_counts.get(&task.id) {
            if *count > 0 {
                spans.push(Span::styled(
                    format!("  ({})", count),
                    Style::default().fg(Color::DarkGray),
                ));
            }
        }
    }

    // Indicators
    if task.focused {
        spans.push(Span::styled(" [F]", Style::default().fg(Color::Rgb(200, 150, 50))));
    }

    if task.deferred {
        spans.push(Span::styled(" [zzz]", Style::default().fg(Color::DarkGray)));
    }

    if !task.notes.is_empty() {
        spans.push(Span::styled(" [n]", Style::default().fg(Color::Yellow)));
    }

    ListItem::new(Line::from(spans))
}

fn render_notes_panel(f: &mut Frame, app: &App, area: Rect) {
    let (title_text, notes_text, timestamps) = match app.selected_task() {
        Some(task) => {
            let notes = if task.notes.is_empty() {
                "No notes — press 'n' to edit".to_string()
            } else {
                task.notes.clone()
            };
            let ts = format!(
                "created {}  ·  updated {}",
                format_time(&task.created_at),
                format_time(&task.updated_at),
            );
            (task.title.clone(), notes, ts)
        }
        None => ("No task selected".to_string(), String::new(), String::new()),
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
    ];

    if !timestamps.is_empty() {
        lines.push(Line::from(Span::styled(
            timestamps,
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(""));

    if notes_text.is_empty() && app.selected_task().is_none() {
        lines.push(Line::from(Span::styled(
            "No task selected",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for line in notes_text.lines() {
            lines.push(Line::from(Span::raw(line)));
        }
    }

    // Add scroll indicator if scrolled
    if app.notes_scroll > 0 {
        lines.insert(0, Line::from(Span::styled(
            format!("  [{}/] scroll", app.notes_scroll),
            Style::default().fg(Color::DarkGray),
        )));
    }

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.notes_scroll, 0));
    f.render_widget(paragraph, area);
}

fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let content = match &app.input_mode {
        InputMode::Adding(input) => {
            let label = if app.adding_is_push {
                "push: "
            } else if app.adding_is_append {
                "append: "
            } else {
                "add: "
            };
            render_input_line(label, input, Color::Yellow)
        }
        InputMode::Editing(input) => {
            render_input_line("edit: ", input, Color::Yellow)
        }
        InputMode::Filtering(input) => {
            render_input_line("/", input, Color::Yellow)
        }
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
            // Show pending key sequence
            if let Some(label) = app.pending_key.label() {
                Line::from(Span::styled(
                    format!(" {}-", label),
                    Style::default().fg(Color::Yellow),
                ))
            } else if !app.filter.is_empty() {
                Line::from(Span::styled(
                    format!(" filter: \"{}\" (Esc to clear)", app.filter),
                    Style::default().fg(Color::Yellow),
                ))
            } else if let Some(msg) = &app.status_msg {
                let color = if app.quit_pending || msg.starts_with("BUDGET EXPIRED") {
                    Color::Red
                } else {
                    Color::Green
                };
                Line::from(Span::styled(
                    format!(" {}", msg),
                    Style::default().fg(color),
                ))
            } else {
                let dim = Style::default().fg(Color::DarkGray);
                let key_style = Style::default().fg(Color::Yellow);

                let mut row1: Vec<Span> = vec![Span::raw(" ")];
                for (i, (k, label)) in [("a", "ins"), ("A", "append"), ("o", "push"), ("x", "done"), ("dd", "cut"), ("yy", "yank"), ("p", "paste")].iter().enumerate() {
                    if i > 0 { row1.push(Span::styled("  ", dim)); }
                    row1.push(Span::styled(*k, key_style));
                    row1.push(Span::styled(format!(":{}", label), dim));
                }
                match &app.clipboard {
                    Clipboard::Cut(t) => {
                        row1.push(Span::styled(
                            format!("  [cut: {}]", t.title),
                            Style::default().fg(Color::Yellow),
                        ));
                    }
                    Clipboard::Yank(_, t) => {
                        row1.push(Span::styled(
                            format!("  [yank: {}]", t),
                            Style::default().fg(Color::Cyan),
                        ));
                    }
                    Clipboard::Empty => {}
                }

                let mut row2: Vec<Span> = vec![Span::raw(" ")];
                for (i, (k, label)) in [(".", "touch"), ("f", "focus"), ("z", "defer"), ("J/K", "move"), ("Spc", "sel"), ("u", "undo"), ("?", "help")].iter().enumerate() {
                    if i > 0 { row2.push(Span::styled("  ", dim)); }
                    row2.push(Span::styled(*k, key_style));
                    row2.push(Span::styled(format!(":{}", label), dim));
                }

                return f.render_widget(
                    Paragraph::new(vec![Line::from(row1), Line::from(row2)]),
                    area,
                );
            }
        }
    };

    f.render_widget(Paragraph::new(content), area);
}

fn render_search(f: &mut Frame, app: &mut App, area: Rect) {
    let query = match &app.input_mode {
        InputMode::Searching(input) => input.as_str(),
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

    // Render search input with cursor
    let search_input_line = match &app.input_mode {
        InputMode::Searching(input) => {
            let before = input.before_cursor().to_string();
            let cursor_ch = input.at_cursor().to_string();
            let after = input.after_cursor().to_string();
            if cursor_ch.is_empty() {
                Line::from(vec![
                    Span::styled(" > ", Style::default().fg(Color::Cyan)),
                    Span::raw(before),
                    Span::styled(" ", Style::default().bg(Color::Cyan)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(" > ", Style::default().fg(Color::Cyan)),
                    Span::raw(before),
                    Span::styled(cursor_ch, Style::default().bg(Color::Cyan).fg(Color::Black)),
                    Span::raw(after),
                ])
            }
        }
        _ => Line::from(vec![
            Span::styled(" > ", Style::default().fg(Color::Cyan)),
        ]),
    };
    f.render_widget(Paragraph::new(search_input_line), chunks[0]);

    if app.search_results.is_empty() {
        let msg = if query.len() < 2 { "type to search..." } else { "no results" };
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
        f.render_stateful_widget(list, chunks[1], &mut app.search_list_state);
    }
}

fn render_help(f: &mut Frame, area: Rect, scroll: u16) {
    let help_lines = vec![
        Line::from(Span::styled(
            " Keybindings",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Navigation", Style::default().fg(Color::Yellow)),
        ]),
        Line::from("   j/k ↑/↓     move selection (wraps)"),
        Line::from("   Enter/l/→   enter task (auto-acks)"),
        Line::from("   h/←/Bksp    go to parent"),
        Line::from("   H/~         go to root"),
        Line::from("   gg          jump to first"),
        Line::from("   G           jump to last"),
        Line::from("   Ctrl-d/u    half page down/up"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Actions", Style::default().fg(Color::Yellow)),
        ]),
        Line::from("   a           insert below selected"),
        Line::from("   A           append to end"),
        Line::from("   o           push (insert + enter)"),
        Line::from("   e           edit title"),
        Line::from("   x           toggle done (bulk)"),
        Line::from("   .           touch (update timestamp)"),
        Line::from("   f           toggle focus (max 3)"),
        Line::from("   dd          cut task"),
        Line::from("   yy          yank (copy) task"),
        Line::from("   p           paste"),
        Line::from("   J/K         move task down/up"),
        Line::from("   >>          indent (child of above)"),
        Line::from("   <<          outdent (up a level)"),
        Line::from("   z           defer/undefer task"),
        Line::from("   D           delete task"),
        Line::from("   n           edit notes ($EDITOR)"),
        Line::from("   u           undo"),
        Line::from("   Ctrl-r      redo"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Selection", Style::default().fg(Color::Yellow)),
        ]),
        Line::from("   Space       toggle select"),
        Line::from("   V           select/deselect all"),
        Line::from("   Esc         clear selection"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Views", Style::default().fg(Color::Yellow)),
        ]),
        Line::from("   Tab         toggle notes panel"),
        Line::from("   [/]         scroll notes up/down"),
        Line::from("   t           toggle tree/list"),
        Line::from("   c           toggle completed"),
        Line::from("   Z           toggle deferred"),
        Line::from("   /           filter (local)"),
        Line::from("   Ctrl-f      search (global)"),
        Line::from("   r           refresh"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Text Input", Style::default().fg(Color::Yellow)),
        ]),
        Line::from("   ←/→         move cursor"),
        Line::from("   Home/End    start/end of line"),
        Line::from("   Ctrl-a/e    start/end of line"),
        Line::from("   Ctrl-w      delete word"),
        Line::from("   Ctrl-u/k    kill to start/end"),
        Line::from(""),
        Line::from(vec![
            Span::styled(" Mouse", Style::default().fg(Color::Yellow)),
        ]),
        Line::from("   click       select task"),
        Line::from("   scroll      navigate / scroll notes"),
        Line::from(""),
        Line::from("   qq/Ctrl-c   quit"),
        Line::from(""),
        Line::from(Span::styled(
            "   j/k to scroll, q/Esc to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let total_lines = help_lines.len() as u16;
    let width = 42u16.min(area.width.saturating_sub(2));
    // Clamp popup height to terminal height
    let max_height = area.height.saturating_sub(2);
    let inner_height = total_lines.min(max_height.saturating_sub(2)); // -2 for border
    let height = inner_height + 2;
    let x = area.width.saturating_sub(width) / 2;
    let y = area.height.saturating_sub(height) / 2;
    let popup_area = Rect::new(x, y, width, height);

    // Clamp scroll so we don't scroll past content
    let max_scroll = total_lines.saturating_sub(inner_height);
    let clamped_scroll = scroll.min(max_scroll);

    let mut title = " Help ".to_string();
    if max_scroll > 0 {
        title = format!(" Help [{}/{}] ", clamped_scroll + 1, max_scroll + 1);
    }

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black));
    f.render_widget(ratatui::widgets::Clear, popup_area);

    let inner = popup_area.inner(ratatui::layout::Margin { horizontal: 0, vertical: 0 });
    let paragraph = Paragraph::new(help_lines)
        .block(block)
        .scroll((clamped_scroll, 0));
    f.render_widget(paragraph, inner);
}
