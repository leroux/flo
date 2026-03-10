# Flo Spec

## Overview

Flo is an executive function prosthetic — a hierarchical task manager with frame-based navigation, experience sampling, and spaced repetition review. Every frame is a task, every task is a frame.

Designed for a neurodivergent/ADHD user who needs:
- Immediate context on open ("where was I?")
- Focused view (only see children of current frame)
- Low friction task capture and navigation
- Honest feedback about where attention actually goes
- Full queryability and editability by AI agents via API

## Architecture

- **Language:** Rust
- **Single crate** with modules: server, cli, tui, client, models, db, logging
- **Server:** Axum HTTP framework, REST API
- **Database:** SQLite via sqlx (async)
- **CLI:** clap for arg parsing, reqwest for HTTP client
- **TUI:** ratatui for terminal UI, reqwest for HTTP client
- **Shared client:** Common `FloClient` library used by both CLI and TUI

### Runtime Model

The server runs as a persistent background daemon. CLI and TUI are clients that talk to the server over HTTP. Server auto-starts on first CLI/TUI command if not running, with version checking and auto-restart on binary update.

```
~/.flo/
├── flo.db           # SQLite database
├── cursor           # current frame ID (or empty for root)
└── server.pid       # daemon process ID
```

## Data Model

### tasks

```sql
CREATE TABLE tasks (
    id              TEXT PRIMARY KEY NOT NULL,   -- ULID
    parent_id       TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    notes           TEXT NOT NULL DEFAULT '',
    completed       BOOLEAN NOT NULL DEFAULT FALSE,
    position        INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL,               -- RFC 3339
    updated_at      TEXT NOT NULL,               -- RFC 3339
    deferred        BOOLEAN NOT NULL DEFAULT FALSE,
    review_interval INTEGER NOT NULL DEFAULT 7,  -- days
    next_review_at  TEXT                         -- RFC 3339, nullable
);
```

Tree structure via parent pointer. Root tasks (`parent_id = NULL`) are projects. `ON DELETE CASCADE` removes subtrees. ULID-based IDs are sortable and distributed-friendly.

### samples (experience sampling)

```sql
CREATE TABLE samples (
    id          TEXT PRIMARY KEY NOT NULL,   -- ULID
    prompt_type TEXT NOT NULL,               -- "activity", "ping"
    response    TEXT NOT NULL,
    created_at  TEXT NOT NULL                -- RFC 3339
);
```

### Cursor

Local file `~/.flo/cursor` containing a single task ID (or empty for root). Not in the database — each client manages its own cursor.

## Task Lifecycle

There are two independent axes to a task's state:

**Commitment** (do I intend to do this?)
- **Captured** — brain-dumped, no decision made yet
- **Committed** — deliberate acceptance: "I'm going to do this"
- **Abandoned** — deleted from the system

**Temporal** (when?)
- **Active** — on the workbench right now
- **Queued** — committed but waiting its turn
- **Deferred** — consciously parked, hidden from default views, surfaced on a spaced repetition schedule
- **Done** — completed, hidden from default views

```
capture ──→ focus ──→ touch ──→ touch ──→ done
  │           ↑          │
  │           │          └──→ (dimming: "you're not working on this")
  │           │
  │           └── unfocus (remove from slot, no state change)
  │
  └──→ defer ──→ review ──→ focus / snooze / done / kill
```

**Touch** is an event, not a state — "I worked on this" resets the staleness clock without changing any state. Any task mutation (edit, reorder, add child, toggle done) implicitly touches by updating `updated_at`.

## Staleness Dimming

Tasks in the TUI dim based on time since last `updated_at`:

| Age       | Appearance                    |
|-----------|-------------------------------|
| < 3 days  | Full brightness               |
| 3–7 days  | Slightly dimmed (rgb 160)     |
| 7–14 days | Dimmer (rgb 120)              |
| 14+ days  | Very dim (rgb 80)             |

Completed tasks keep their existing crossed-out style regardless of age.

Dimming measures **evidence of work**, not claims or intent. A task that's been focused but never touched still dims — that's a valuable signal.

## Defer & Spaced Repetition Review

### Defer

Toggle a task between active and deferred. Deferred tasks are hidden from default views (list and tree) in both CLI and TUI.

- CLI: `flo defer [index]`
- TUI: `z` to toggle defer, `Z` to toggle showing deferred tasks
- Deferred tasks show a `[zzz]` indicator when visible

On defer: `next_review_at` is set to `now + review_interval` (default 7 days).
On undefer: `review_interval` resets to 7 days, `next_review_at` clears.

### Review

`flo review` surfaces deferred tasks where `next_review_at <= now`. Interactive prompt for each:

- **keep** — undefer, bring back to active, reset interval to 7d
- **snooze** — double the review interval (7d → 14d → 28d → 56d → 90d cap), push out next review
- **done** — mark complete
- **quit** — stop reviewing

Tasks you keep snoozing fade out of your review cycle. Tasks you engage with stay active.

## Mirror (Experience Sampling)

Self-reported attention tracking. Not automated — works for all of life, not just the computer.

- `flo log <text>` — log what you're doing right now
- `flo ping` — interactive "what are you doing?" prompt
- `flo mirror` — show today's samples as a timeline

The gap between what the task tree says you should be doing and what ESM says you're actually doing is where insight lives.

## Frame Navigation

The "current frame" is a cursor into the task tree that determines what you see and work on.

| Command        | Description                                    |
|----------------|------------------------------------------------|
| `flo`          | Home view: list projects with next-action preview |
| `flo status`   | Current frame: title, notes, children          |
| `flo push <t>` | Create child + enter it                        |
| `flo pop / up` | Move to parent                                 |
| `flo down <N>` | Enter child N (1-based)                        |
| `flo top`      | Go to root                                     |
| `flo tree`     | Full tree from current frame                   |

## Task CRUD

| Command              | Description                        |
|----------------------|------------------------------------|
| `flo add <title>`    | Create child without entering it   |
| `flo done [N]`       | Toggle done (current or child N)   |
| `flo edit <N> <t>`   | Rename child N                     |
| `flo delete <N>`     | Delete child N (cascades)          |
| `flo note [text]`    | View or set notes on current frame |
| `flo indent <N>`     | Make child N a subtask of sibling above |

## REST API

All endpoints return JSON. Server on `127.0.0.1:4242` (configurable via `--port`).

### Tasks

```
GET    /api/tasks                  -- list children (root or ?parent_id=X)
GET    /api/tasks/:id              -- get task with children
GET    /api/tasks/:id/subtree      -- get full subtree
GET    /api/tasks/:id/ancestors    -- get ancestor chain
POST   /api/tasks                  -- create {parent_id?, title, notes?}
PATCH  /api/tasks/:id              -- update {title?, notes?, completed?, position?, parent_id?, deferred?}
DELETE /api/tasks/:id              -- delete + cascade
```

### Defer & Review

```
POST   /api/tasks/:id/defer       -- toggle defer (returns updated task)
POST   /api/tasks/:id/snooze      -- double review interval, push next_review_at
GET    /api/review                 -- list deferred tasks due for review
```

### Search

```
GET    /api/search?q=<query>       -- search titles and notes (LIKE, top 30)
```

### Home & Health

```
GET    /api/home                   -- project previews with pending counts
GET    /api/health                 -- {status, version}
```

### Mirror

```
POST   /api/samples               -- create sample {response, prompt_type?}
GET    /api/samples                -- list today's samples
```

## TUI

Interactive terminal UI. Vim-style keybindings. See USAGE.md for full keymap.

Key features:
- Tree and list view toggle
- Notes panel (side-by-side with task list)
- Staleness dimming
- Defer toggle with `[zzz]` indicator
- Multi-select with bulk operations
- Cut/yank/paste with undo/redo
- Inline search (global) and filter (local)
- Mouse support (click, scroll)

## Planned: Touch

Explicit "I worked on this" action. Resets `updated_at` (undims) without changing any other state.

- CLI: `flo touch [N]`
- TUI: `.` keystroke
- Creates a sample linked to the task (bridges ESM — see below)

Touch is the evidence counterpart to focus (which is a claim). The system measures evidence, not claims.

## Planned: Focus Slots

N persistent cursors (N=3) representing what you're actively working on. Distinct from the navigational cursor — focus persists while you navigate freely.

- Pinned in a TUI header area, always visible
- WIP limit enforced (can't focus more than N things)
- Focus does NOT auto-refresh `updated_at` — a focused task still dims if you don't touch it
- Staleness color varies by context: unfocused tasks dim gray (benign decay), focused tasks go warm toward red/amber ("you said this matters and you're not doing it")

### ESM-Task Linking

Touch and focus connect the task tree to ESM:

- `samples` table gets a nullable `task_id` foreign key
- Touch = create sample with `task_id` set + refresh `updated_at`
- `flo log` = create sample with free text, no task_id
- `flo ping` could show focus slots as context ("you're focused on X, Y — is that what you're doing?")
- `flo mirror` becomes a unified timeline of task-linked and free-text entries

### Data Model Changes

```sql
ALTER TABLE tasks ADD COLUMN focused BOOLEAN NOT NULL DEFAULT FALSE;
-- CHECK constraint: max N rows with focused = TRUE

ALTER TABLE samples ADD COLUMN task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL;
```

## Planned: Inbox / Commit Distinction

Tasks currently enter the system as implicitly committed. A future distinction:

- **Inbox** — captured but not yet decided on. Lower visual weight.
- **Committed** — deliberately accepted. Full visual weight.

An ack action (entering the task, or a deliberate keystroke) moves inbox → committed. Unacked tasks could have a subtle visual marker.

## Planned: Time Budgets

Focus slots get an optional time budget. When you focus on something, you can say how long: "1h", "2h", "this morning." Flo nudges you when the budget runs out — not a hard stop, just a check-in: "you've been on X for 2h, you planned 1h. Keep going or switch?"

- Budget is per focus-slot session, not per task globally
- Notifications via text, Discord, or phone call (see Notifications below)
- Over-budget is a signal, not a punishment — you can acknowledge and continue
- ESM pings during a focused session include budget context ("45min left on X")

This addresses the "wrong thing" failure mode: spending hours on something that felt productive but wasn't important. The intervention happens at the right moment — when you're deep in flow and have lost track of time.

## Planned: Notifications & Reachout

Push-based system for nudges, ESM pings, and budget alerts. Multiple channels:

- **Text (SMS)** — ESM pings, budget alerts, review reminders
- **Discord (Axi bot)** — integrated as a conversational interface in a channel you already live in
- **Phone call** — escalation for critical nudges (e.g. 2x over budget, haven't responded to pings)

Notifications are bidirectional — you can respond inline (text back, reply in Discord) to log ESM samples, snooze, or switch focus without opening the app.

## Future Work

- **AI co-pilot:** Task suggestions, decomposition, pattern analysis from ESM data, accountability reflection
- **Morning/evening ritual:** Daily intention setting and reflection
- **History/audit log:** Server-side action log with snapshots for undo
- **Multi-agent access:** Scoped API access for different AI agents
- **Additional views:** Kanban, calendar, flat list
- **Notifications:** Push-based reminders
- **Sync:** Bidirectional sync with external todo apps
- **Phone client:** ESM pings, quick status, AI chat
