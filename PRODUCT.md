# Flo — Product Document

## Problem Statement

ADHD and executive dysfunction create a full-spectrum failure pattern:

1. **Can't start** — Know what to do but can't begin. Task feels too big, too vague, or freeze.
2. **Lose the thread** — Get deep into something, get pulled away, come back with no idea where you were.
3. **Wrong thing** — Spend hours on something that felt productive but wasn't important. Busy but not effective.
4. **Drift / no anchor** — No structure at all. Float between things, nothing sticks, the day evaporates.

### The Biggest Lever

Single-tasking is the answer, but there are 4-6 concurrent projects. The tool must help **start** and then **sustain focus across days**, while managing project concurrency through single-tasking.

### What Works

**Momentum/flow** is the superpower. Once rolling, momentum carries. The problem is getting the ball moving, not keeping it moving.

### The Cold Start Problem

The failure mode is "wrong door" — energy is there, willingness to work is there, but the path of least resistance leads to the wrong project. The tool must be **the door** — the first thing you see should make the right next step obvious.

## Product Framework: Map + Mirror + Ritual

Three interlocking systems with AI woven through all three.

### 1. The Map (where should my attention go?)

- 4-6 **projects** as root-level nodes in a task tree
- Hierarchical tasks with notes under each project (Frame concept)
- At-a-glance: **next-action preview** (top 1-2 pending tasks per project)
- A **profile document** (TELOS-style goals/values) — background context for AI alignment, not actively managed in the UI
- The home view takes ~5 seconds from open to "I know what to do"

### 2. The Mirror (where is my attention actually going?)

- **Random/variable experience sampling** pings throughout the day
- Response: **quick freeform text** (~10 seconds, e.g. "debugging auth flow", "scrolling twitter", "lunch")
- Builds a real picture of time and attention allocation over days/weeks
- The gap between Map and Mirror is where insight lives
- Not automated tracking (like RescueTime) — self-reported, works for all of life not just computer

### 3. The Ritual (daily bookends)

- **Morning intention**: Pick 1-3 things to focus on today. Sets direction.
- **Evening reflection**: Review what happened vs what you intended. AI reflects patterns back.
- The daily cycle creates accountability and learning.

### AI Co-pilot (woven through all three)

| Capability | Description |
|---|---|
| **Suggest what's next** | Analyzes map + mirror + goals profile to propose focus |
| **Break things down** | Decomposes vague/big tasks into small startable steps |
| **Accountability mirror** | Reflects patterns without judgment ("you haven't touched project B in 5 days") |
| **Pattern analysis** | "You never work on deep tasks after 2pm" — surfaces non-obvious insights from ESM data |

Trusted as a full co-pilot: suggestions, decomposition, and reflection.

### Platforms

- **Computer**: Full experience — tree, notes, AI, everything (CLI + TUI, then Web UI)
- **Phone**: Pings (experience sampling), top-3 list, quick AI chat, reminders
- **Discord (Axi bot)**: Integrated as a conversational interface — experience sampling pings, quick status checks, AI reachouts, and push notifications via a channel you already live in

## Design Principles

Drawn from the influences (Frame, Intend, Nowify, Timers):

1. **"Don't make me think"** — The system should calculate what to do next, not require planning (Nowify)
2. **Push over pull** — Active nudges beat passive dashboards (Nowify)
3. **Goals as context, not management** — Goals inform AI alignment but aren't another thing to track (Intend, simplified)
4. **Aliveness over exhaustiveness** — Tasks that aren't relevant should fade, not accumulate (Intend)
5. **Context preservation** — Notes at each level solve "where was I?" across sessions (Frame)
6. **Single-tasking with concurrency** — Show one thing at a time but manage many projects

## Influences

| System | Key Takeaway |
|---|---|
| **Frame** | Hierarchical task tree with push/pop navigation. Notes as context breadcrumbs. "Where was I?" |
| **Intend** | Goals-first philosophy. Tasks must serve a purpose. Aliveness over exhaustiveness. |
| **Nowify** | Push-based "what now?" with routines. Annoy feature. Don't make me think. |
| **Timers** | Time-boxing creates urgency and boundaries. Visual countdowns. |

## Build Plan

### Phase 1: The Map (MVP)
- Fresh Rust project, CLI + TUI
- Task tree with Frame navigation
- Home view with project next-action previews
- Profile document
- SQLite storage

### Phase 2: The Mirror
- Experience sampling system (random pings)
- Freeform text logging
- Basic attention data storage and retrieval
- Phone notification layer

### Phase 3: The Ritual
- Morning intention picker
- Evening reflection prompt
- Gap analysis (intended vs actual, using ESM data)

### Phase 4: The AI
- Claude integration for task suggestions
- Pattern analysis from ESM data
- Task decomposition
- Accountability reflection
- Phone-accessible AI chat
