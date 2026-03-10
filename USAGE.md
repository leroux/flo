# flo usage

Executive function prosthetic. Hierarchical task manager with frame-based navigation.

## Concepts

**Frames** — flo uses a cursor that points at a task. All commands operate relative to this cursor. `push` creates a child and moves into it, `pop`/`up` moves to the parent, `down N` enters child N. Think of it like `cd` for tasks.

**Projects** — root-level tasks (no parent). `flo` with no args shows all projects.

**Staleness dimming** — in the TUI, tasks dim based on how long since they were last touched. Any interaction (edit, reorder, toggle) refreshes the timestamp and undims them.

**Defer** — park a task you don't want to work on right now. Deferred tasks are hidden from default views but scheduled for periodic review via spaced repetition.

## Data

Everything lives in `~/.flo/`:
- `flo.db` — SQLite database
- `cursor` — current frame ID
- `server.pid` — background server PID

## CLI commands

### Navigation

```
flo                  # home view: list all projects
flo status           # show current frame with children
flo push <title>     # create child + enter it
flo pop / flo up     # move to parent
flo down <N>         # enter child N (1-based)
flo top              # go to root
```

### Task management

```
flo add <title>      # add child without entering it
flo done [N]         # toggle done (current frame or child N)
flo edit <N> <title> # rename child N
flo delete <N>       # delete child N (cascades)
flo note [text]      # view notes (no args) or set notes
flo indent <N>       # make child N a subtask of the sibling above
flo tree             # show full tree from current frame
```

### Defer & review

```
flo defer [N]        # toggle defer on current frame or child N
flo review           # interactive review of deferred tasks due for check-in
```

Review actions:
- **k** keep — undefer, bring back to active
- **s** snooze — double the review interval (7d -> 14d -> 28d -> 56d -> 90d cap)
- **d** done — mark complete
- **q** quit — stop reviewing

### Experience sampling (mirror)

```
flo log <text>       # log what you're doing
flo ping             # interactive "what are you doing?" prompt
flo mirror           # show today's samples
```

### Server

```
flo server           # start API server (auto-starts when needed)
```

The server runs on port 4242 by default. Override with `--port`.

## TUI

```
flo tui
```

### Navigation

| Key           | Action                    |
|---------------|---------------------------|
| j/k, arrows   | move selection (wraps)    |
| Enter, l, ->  | enter task                |
| h, <-, Bksp   | go to parent              |
| H, ~          | go to root                |
| gg            | jump to first             |
| G             | jump to last              |
| Ctrl-d/u      | half page down/up         |

### Actions

| Key    | Action                          |
|--------|---------------------------------|
| a      | insert below selected           |
| A      | append to end                   |
| o      | push (insert + enter)           |
| e      | edit title                      |
| x      | toggle done (bulk if selected)  |
| z      | defer/undefer task              |
| dd     | cut task                        |
| yy     | yank (copy) task                |
| p      | paste                           |
| J/K    | move task down/up               |
| >>     | indent (child of above)         |
| <<     | outdent (up a level)            |
| D      | delete task                     |
| n      | edit notes ($EDITOR)            |
| u      | undo                            |
| Ctrl-r | redo                            |

### Selection

| Key    | Action                |
|--------|-----------------------|
| Space  | toggle select         |
| V      | select/deselect all   |
| Esc    | clear selection       |

### Views

| Key    | Action              |
|--------|----------------------|
| Tab    | toggle notes panel   |
| [ / ]  | scroll notes         |
| t      | toggle tree/list     |
| c      | toggle completed     |
| Z      | toggle deferred      |
| /      | filter (local)       |
| Ctrl-f | search (global)      |
| r      | refresh              |
| ?      | help                 |

### Text input

| Key         | Action              |
|-------------|----------------------|
| arrows      | move cursor          |
| Home/End    | start/end of line    |
| Ctrl-a/e    | start/end of line    |
| Ctrl-w      | delete word          |
| Ctrl-u/k    | kill to start/end    |

### Mouse

Click to select, scroll to navigate or scroll notes.

### Quit

`qq` or `Ctrl-c`.

## Staleness dimming

Tasks in the TUI dim based on age since last update:
- **< 3 days** — full brightness
- **3-7 days** — slightly dimmed
- **7-14 days** — dimmer
- **14+ days** — very dim

Any mutation (edit, reorder, toggle done, add child) refreshes `updated_at` and undims the task automatically.

## Spaced repetition review

When you defer a task, it gets a review interval (default 7 days) and a `next_review_at` timestamp. Running `flo review` surfaces deferred tasks whose review date has passed.

- **Keep** — undefers the task, resets interval to 7 days
- **Snooze** — doubles the interval (capped at 90 days), pushes out next review
- **Done** — marks complete

Tasks you keep snoozing fade out of your review cycle. Tasks you keep engage with stay active.
