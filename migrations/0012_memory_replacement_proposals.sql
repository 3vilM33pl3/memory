CREATE TABLE IF NOT EXISTS memory_replacement_proposals (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    target_memory_id UUID NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    raw_capture_id UUID NOT NULL REFERENCES raw_captures(id) ON DELETE CASCADE,
    candidate_json JSONB NOT NULL,
    policy TEXT NOT NULL,
    score INT NOT NULL,
    rationale_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_memory_replacement_proposals_project_status_created
    ON memory_replacement_proposals (project_id, status, created_at DESC);
