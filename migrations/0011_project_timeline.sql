CREATE TABLE IF NOT EXISTS project_timeline_events (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    kind TEXT NOT NULL,
    memory_id UUID NULL REFERENCES memory_entries(id) ON DELETE SET NULL,
    summary TEXT NOT NULL,
    details_json JSONB NULL
);

CREATE INDEX IF NOT EXISTS idx_project_timeline_events_project_recorded
    ON project_timeline_events (project_id, recorded_at DESC);
