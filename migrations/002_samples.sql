CREATE TABLE IF NOT EXISTS samples (
    id TEXT PRIMARY KEY NOT NULL,
    prompt_type TEXT NOT NULL DEFAULT 'activity',
    response TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_samples_created ON samples(created_at);
