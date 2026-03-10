# Flo Spec

## Overview

Flo is a task storage system with hierarchical frame navigation. Tasks are organized as a tree — every frame is a task, every task is a frame. The "current frame" is a cursor into the tree that determines what you see and work on.

The system is designed for a neurodivergent/ADHD user who needs:
- Immediate context on open ("where was I?")
- Focused view (only see children of current frame)
- Low friction task capture and navigation
- Full queryability and editability by AI agents via API

## Architecture ✅

- **Language:** Rust ✅
- **Single crate** with modules: server, cli, tui, client, models, sync ✅
- **Server:** Axum HTTP framework, REST API ✅
- **Database:** SQLite via sqlx (async, compile-time checked queries) ✅
- **CLI:** clap for arg parsing, reqwest for HTTP client ✅
- **TUI:** ratatui for terminal UI, reqwest for HTTP client ✅
- **Shared client:** Common API client library used by both CLI and TUI ✅
- **Web UI:** Vite + React + TypeScript + Tailwind CSS ✅

### Runtime Model ✅

The server runs as a persistent process. CLI and TUI are clients that talk to the server over HTTP.

```
flo server        -- start API server                    ✅
flo <command>     -- CLI commands (talk to server)       ✅
flo tui           -- launch interactive TUI (talks to server)  ✅
```

## Data Model

### Task (the only entity) ✅

| Field       | Type     | Description                              | Status |
|-------------|----------|------------------------------------------|--------|
| id          | TEXT PK  | Unique identifier                        | ✅ |
| parent_id   | TEXT FK  | Parent task ID (NULL for root)           | ✅ |
| title       | TEXT     | Task title                               | ✅ |
| notes       | TEXT     | Free-form notes (markdown)               | ✅ |
| completed   | BOOL     | Whether the task is done                 | ✅ |
| position    | INTEGER  | Sort order among siblings                | ✅ |
| today       | BOOL     | Flagged for today's work                 | ✅ |
| created_at  | TEXT     | ISO 8601 timestamp                       | ✅ |
| updated_at  | TEXT     | ISO 8601 timestamp                       | ✅ |

Tree structure via parent pointer. Root tasks have `parent_id = NULL`.

Multiple root-level tasks are allowed (separate projects/areas of life).

### Cursor ✅

| Field       | Type     | Description                              |
|-------------|----------|------------------------------------------|
| id          | TEXT PK  | Cursor identifier (e.g. "default")       |
| task_id     | TEXT FK  | Currently focused task                   |

Single cursor for now. Multiple cursors (per-agent) can be added later without schema changes.

Note: CLI/TUI use a local file cursor (~/.flo/cursor) instead of the DB cursor table. Web UI manages frame state client-side.

### History ❌

| Field       | Type     | Description                              |
|-------------|----------|------------------------------------------|
| id          | INTEGER PK | Auto-increment                         |
| action      | TEXT     | Action type (e.g. "task.created")        |
| task_id     | TEXT     | Related task ID                          |
| detail      | TEXT     | Human-readable description               |
| snapshot    | TEXT     | JSON snapshot for undo (nullable)        |
| created_at  | TEXT     | ISO 8601 timestamp                       |

## Frame Navigation ✅

The frame concept is the primary view. Your "current frame" determines what you see: the current task's title, notes, and its children.

### Commands ✅

| Command                   | Description                                      | Status |
|---------------------------|--------------------------------------------------|--------|
| `push <title>`            | Create child task under current frame, move into it | ✅ |
| `pop`                     | Move to parent frame                             | ✅ |
| `up`                      | Move to parent frame (alias)                     | ✅ |
| `down <index\|id>`        | Move into a child frame                          | ✅ |
| `top`                     | Move to root (clear cursor)                      | ✅ |
| `switch <id>`             | Jump to any task by ID                           | ✅ (TUI/Web via search) |
| `status`                  | Show current frame: title, notes, children       | ✅ |
| `tree`                    | Show full tree from current frame down            | ✅ |

### Completed Task Visibility ✅

- Completed tasks are hidden from default frame view and tree display ✅
- `--all` flag shows completed tasks ✅ (CLI --all, TUI/Web toggle)
- Completed tasks remain in the tree, not moved or deleted ✅
- History log separately records completion events ❌

## Today's Tasks (partial)

Tasks can be flagged for "today." This is the subset that syncs to Todoist and represents your daily focus.

| Command                   | Description                                      | Status |
|---------------------------|--------------------------------------------------|--------|
| `today`                   | List today's tasks                               | ❌ (no dedicated endpoint/command) |
| `today add <id>`          | Flag a task for today                            | ✅ (via PATCH today flag) |
| `today remove <id>`       | Unflag a task from today                         | ✅ (via PATCH today flag) |
| `today clear`             | Clear all today flags                            | ❌ |

Selection is manual. AI agent integration for auto-suggesting today's tasks is future work.

## Task CRUD ✅

| Command                   | Description                                      | Status |
|---------------------------|--------------------------------------------------|--------|
| `add <title>`             | Create child task under current frame             | ✅ |
| `edit <id> --title <t>`   | Update task title                                | ✅ |
| `edit <id> --notes <n>`   | Update task notes                                | ✅ |
| `complete <id>`           | Mark task as completed                           | ✅ |
| `uncomplete <id>`         | Mark task as not completed                       | ✅ |
| `delete <id>`             | Delete task and all descendants                  | ✅ |
| `move <id> --before <id>` | Reorder task among siblings                      | ✅ (via position swap) |
| `move <id> --into <id>`   | Reparent task                                    | ✅ |

## REST API

All endpoints return JSON.

### Tasks ✅

```
GET    /api/tasks                  -- list root tasks (or children of ?parent_id=)     ✅
GET    /api/tasks/:id              -- get task with children                            ✅
GET    /api/tasks/:id/subtree      -- get full subtree (optional ?depth=N)              ✅
POST   /api/tasks                  -- create task {parent_id, title, notes?, position?} ✅
PATCH  /api/tasks/:id              -- update task {title?, notes?, completed?, position?, parent_id?, today?}  ✅
DELETE /api/tasks/:id              -- delete task and descendants                       ✅
```

### Frame Navigation (Cursor) ❌

```
GET    /api/cursor                 -- get current frame
PUT    /api/cursor                 -- set current frame {task_id}
POST   /api/cursor/push            -- create child + move cursor {title}
POST   /api/cursor/pop             -- move cursor to parent
POST   /api/cursor/up              -- move cursor to parent
POST   /api/cursor/down            -- move cursor to child {task_id}
POST   /api/cursor/top             -- clear cursor (root view)
```

Note: CLI/TUI use local file cursor instead. Web UI manages frame client-side. These server-side cursor endpoints are not implemented.

### Today ❌

```
GET    /api/today                  -- list today's tasks
POST   /api/today/:id              -- flag task for today
DELETE /api/today/:id              -- unflag task from today
DELETE /api/today                  -- clear all today flags
```

Note: Today flag is toggled via `PATCH /api/tasks/:id {today: true/false}` instead. Dedicated endpoints not implemented.

### Search ❌

```
GET    /api/search?q=<query>       -- full-text search across titles and notes
```

Note: Client-side search implemented in TUI and Web UI (loads all tasks, filters locally). No server-side FTS5.

### History ❌

```
GET    /api/history                -- list recent actions (optional ?limit=N)
```

### Undo/Redo ❌

```
POST   /api/undo                   -- undo last action
POST   /api/redo                   -- redo last undone action
GET    /api/undo/status            -- {can_undo: bool, can_redo: bool}
```

## Undo/Redo ❌

All mutations go through a single choke point. Before each mutation, a snapshot is captured for undo.

- Snapshot-based (store previous state as JSON)
- Max 50 undo steps
- Redo stack cleared on new mutation

## Search ❌ (server-side)

Full-text search across task titles and notes. SQLite FTS5 extension for efficient searching.

Note: Client-side fuzzy search works in TUI and Web UI.

## TUI ✅

Interactive terminal UI built with ratatui. Keyboard-first design.

Key features:
- Tree view of tasks from current frame ✅
- Navigate with arrow keys / vim bindings ✅
- Inline task creation and editing ✅
- Toggle completed task visibility ✅
- Today's tasks view ✅ (flag toggle, no dedicated view)
- Command palette for quick actions ✅ (search/move modals)
- Status bar showing current frame breadcrumb ✅

## Web UI ✅ (not in original spec)

React SPA with Vite dev server, full feature parity with TUI:
- Tree view with DFS-ordered nesting ✅
- All keyboard shortcuts matching TUI ✅
- Search modal, move/reparent modal ✅
- Detail panel with notes autosave ✅
- Breadcrumb navigation ✅

## Sync (Stubbed)

Bidirectional sync with Todoist (or other todo apps).

Scope: push tasks flagged as "today" to a Todoist project. Sync completions back.

Design:
- Sync trait/interface defined now, implementation deferred
- Mapping table: flo task ID <-> external task ID
- Last-write-wins conflict resolution (with timestamps)
- Sync triggered manually or on a timer

### Sync Mapping Table (future)

| Field         | Type     | Description                            |
|---------------|----------|----------------------------------------|
| task_id       | TEXT FK  | Flo task ID                        |
| provider      | TEXT     | e.g. "todoist"                         |
| external_id   | TEXT     | ID in external system                  |
| synced_at     | TEXT     | Last sync timestamp                    |

## AI Agent Integration (Future)

Flo will be drivable by AI agents via the REST API. For deeper integration (autonomous planning, task breakdown, daily suggestions), a minimal Rust port of [claudewire](reference/claudewire) will be needed.

Claudewire wraps the Claude Code CLI's `--output-format stream-json` protocol into a programmable interface. A Rust subset would provide:
- Stream-JSON protocol types (serde models for all message types)
- ProcessConnection trait (async trait for managing CLI subprocess)
- BridgeTransport (connects process to typed event stream)
- Permission policies (composable allow/deny rules for tool use)

This enables Flo's server to spawn and manage Claude Code sessions directly — e.g. an agent that reviews your task tree each morning and suggests today's focus, or an autonomous agent that works through its own task subtree.

Full claudewire reference: `reference/claudewire/`

## Future Work

- **Multi-agent access:** Namespacing or permissioning so different AI agents get scoped access to different trees. API token-based auth.
- **Additional views:** Flat list, kanban board, calendar — different lenses over the same task tree.
- **Task statuses:** Beyond completed/not-completed (backlog, in-progress, blocked, cancelled).
- **Repeating tasks:** Interval-based, completion-based, or cycle-based recurrence. Open question on how repeats interact with the tree.
- **AI integration via claudewire:** Rust port of claudewire for driving Claude Code sessions. Claude Code SDK for auto-suggesting today's tasks, breaking down tasks, autonomous agent workflows.
- **Notifications:** Push-based reminders (offloaded to AI agent for now).
- **Due dates, priority, labels/tags:** Additional task fields as needed.
- **Export/import:** Full data export as JSON for backup and migration.
