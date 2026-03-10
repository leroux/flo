-- ESM-Task Linking
ALTER TABLE samples ADD COLUMN task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL;

-- Inbox/Commit
ALTER TABLE tasks ADD COLUMN acknowledged BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE tasks SET acknowledged = TRUE;

-- Focus Slots + Time Budgets
ALTER TABLE tasks ADD COLUMN focused BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE tasks ADD COLUMN focused_at TEXT;
ALTER TABLE tasks ADD COLUMN budget_minutes INTEGER;
