# MinFlow Concepts Worth Considering

Taken from friend's MinFlow app (Electron desktop, deck/card visual task manager).

## 1. Undo/Redo via Mutation Choke Point
- All mutations go through a single `_mutate()` function
- Snapshot-based: full state captured before each mutation
- Max 50 snapshots in stack
- Redo stack cleared on new mutation
- Enables multi-window sync (all windows notified after mutation)

## 2. History / Audit Log
- Tracks all actions with timestamps and human-readable descriptions
- Typed actions: `deck.created`, `card.completed`, etc.
- Verbose/non-verbose filtering
- Limited to 100 entries (oldest dropped)
- Useful for: AI agent context, sync conflict resolution, "what did I do today?"

## 3. Recurrent Tasks with Cycles
- Decks marked "recurrent" use cycle-based completion tracking
- When all cards in current cycle completed, new cycle starts automatically
- New cycle clones all cards as incomplete with new cycle number
- Old cards remain as historical data
- `parentCardId` links cloned cards to source

## 4. Card Types
- task, question, note, milestone
- Type affects display and interpretation
- Could affect how an AI agent interacts with items

## 5. Export/Import
- Full workspace export as JSON
- Import with validation
- Useful for backup, migration, sharing

## 6. Filter System
- Filter by color, shape, size
- Non-destructive (sets visibility flag, doesn't delete)
- Re-applied on every refresh

## 7. Data Model Highlights
- Workspace > Decks > Cards (hierarchical)
- Cards nested within decks in storage
- ID generation: timestamp-based + random (`Date.now().toString(36) + Math.random()...`)
- Allowlisted update fields to prevent overwriting internal fields

## 8. REST API (27 endpoints)
- Full CRUD for workspace, decks, cards
- Position/size operations
- Recurrence operations (start cycle, reset)
- History management
- Undo/redo with status endpoints
- Export/import
