CREATE TABLE IF NOT EXISTS memory_source_verifications (
    source_id UUID PRIMARY KEY REFERENCES memory_sources(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    checked_at TIMESTAMPTZ NOT NULL,
    reason TEXT,
    resolved_path TEXT
);

CREATE INDEX IF NOT EXISTS idx_memory_source_verifications_status
    ON memory_source_verifications(status);
