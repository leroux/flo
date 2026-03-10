# Map MVP — Technical Spec

The Map is Phase 1 of Flo v2. A fresh Rust project.

## Architecture

- **Language:** Rust
- **Single crate** with modules: server, cli, tui, client, models, db
- **Storage:** SQLite via rusqlite (single file at `~/.flo/flo.db`)
- **Server:** Axum HTTP (for TUI and future web/phone clients)
- **CLI:** clap, reqwest (talks to server)
- **TUI:** ratatui, reqwest (talks to server)
- **Profile:** `~/.flo/profile.md` (plain markdown, read by AI later)
- **Cursor:** `~/.flo/cursor` (plain text file, just a task ID)

### Runtime Model

```
flo server       -- start API server (background daemon)
flo <command>    -- CLI commands (talk to server over HTTP)
flo tui          -- interactive TUI (talks to server over HTTP)
```

## Data Model

### Task (the only entity)

```sql
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    parent_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    notes TEXT NOT NULL DEFAULT '',
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX idx_tasks_parent ON tasks(parent_id);
```

- Tree structure via parent pointer
- Root tasks (parent_id = NULL) are projects
- Position determines sibling order
- ON DELETE CASCADE removes subtrees

### Cursor

Local file `~/.flo/cursor` containing a single task ID (or empty for root view).

Not stored in DB — each client manages its own cursor.

### Profile

`~/.flo/profile.md` — User's goals, values, priorities in markdown. Not read by the Map MVP directly; exists as groundwork for AI integration in Phase 4.

## CLI Commands

| Command | Description |
|---|---|
| `flo` | Home view: list root tasks with top 1-2 pending children each |
| `flo status` | Current frame: title, notes, pending children |
| `flo push <title>` | Create child under current frame + move cursor into it |
| `flo pop` | Move cursor to parent |
| `flo add <title>` | Create child under current frame (don't move cursor) |
| `flo done [index]` | Mark task complete (current frame if no index, or child by index) |
| `flo note [text]` | Show notes (no args) or set notes on current frame |
| `flo edit [index] <title>` | Edit task title |
| `flo tree` | Show full subtree from current frame |
| `flo up` | Alias for pop |
| `flo down <index>` | Move cursor into child by index |
| `flo top` | Move cursor to root (clear cursor file) |
| `flo delete <index>` | Delete child task and its subtree |
| `flo server` | Start the API server |
| `flo tui` | Launch interactive TUI |

### Home View Output (no args)

```
Projects
────────
1. flo v2          → "write MAP_MVP spec"
2. client project      → "deploy staging env"
3. health              → "schedule dentist"
4. music               → "finish track 3 mix"

4 projects, 23 total pending tasks
```

Shows each root task + its first pending child as a preview. Quick scan, pick the one with energy, `flo down 1`.

### Status Output

```
flo v2 > backend > api routes
──────────────────────────────────
Notes: Working on the PATCH endpoint for task updates.
       Need to handle position recalculation on reparent.

Children:
  1. [ ] validate input fields
  2. [ ] handle position swap
  3. [x] basic CRUD routes
```

Breadcrumb path, notes, pending children with indices.

## REST API

All endpoints return JSON. Server listens on `127.0.0.1:4242`.

### Tasks

```
GET    /api/tasks              -- list root tasks (children of NULL)
GET    /api/tasks?parent_id=X  -- list children of task X
GET    /api/tasks/:id          -- get single task
GET    /api/tasks/:id/subtree  -- get full subtree as nested JSON
POST   /api/tasks              -- create task {parent_id?, title, notes?, position?}
PATCH  /api/tasks/:id          -- update {title?, notes?, completed?, position?, parent_id?}
DELETE /api/tasks/:id          -- delete task + descendants (CASCADE)
```

### Home View

```
GET    /api/home               -- list root tasks with first pending child preview
```

Returns:
```json
[
  {
    "id": "abc123",
    "title": "flo v2",
    "pending_count": 12,
    "next_actions": [
      {"id": "def456", "title": "write MAP_MVP spec"}
    ]
  }
]
```

## TUI

Built with ratatui. Keyboard-first.

### Views

1. **Home view** — All projects with next-action previews. Arrow keys to select, Enter to dive in.
2. **Frame view** — Current task's children as a list. Title + notes visible. Navigate, add, complete, enter subtasks.

### Key Bindings

| Key | Action |
|---|---|
| `j/k` or `↑/↓` | Move selection |
| `Enter` or `l` | Enter selected task (push) |
| `Backspace` or `h` | Go to parent (pop) |
| `a` | Add new child task |
| `e` | Edit selected task title |
| `n` | Edit current frame's notes |
| `d` | Mark selected task done |
| `D` | Delete selected task |
| `H` | Go to home view |
| `q` | Quit |

## File Structure

```
flo/
├── Cargo.toml
├── src/
│   ├── main.rs          -- entry point, dispatches to server/cli/tui
│   ├── cli/
│   │   └── mod.rs       -- clap command definitions + handlers
│   ├── server/
│   │   ├── mod.rs       -- axum server setup
│   │   └── routes.rs    -- API route handlers
│   ├── tui/
│   │   └── mod.rs       -- ratatui app
│   ├── client/
│   │   └── mod.rs       -- HTTP client (shared by CLI and TUI)
│   ├── db/
│   │   └── mod.rs       -- rusqlite operations
│   └── models/
│       └── mod.rs       -- Task struct, serialization
├── migrations/
│   └── 001_init.sql     -- schema
└── tests/
    └── api_test.rs      -- integration tests
```

## What's NOT in MVP

- Goals as a managed entity (profile.md is enough)
- Dependencies between tasks
- Today's tasks / daily focus
- Experience sampling (Phase 2)
- Morning/evening ritual (Phase 3)
- AI integration (Phase 4)
- Web UI (after CLI + TUI are solid)
- Phone app (after web)
- Search
- Undo/redo
- History/audit log
- Sync with external services
- Task types, labels, due dates, priority
