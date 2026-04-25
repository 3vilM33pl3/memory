ALTER TABLE project_timeline_events
    ADD COLUMN IF NOT EXISTS actor_id TEXT NULL,
    ADD COLUMN IF NOT EXISTS actor_name TEXT NULL,
    ADD COLUMN IF NOT EXISTS source TEXT NULL,
    ADD COLUMN IF NOT EXISTS operation_id TEXT NULL,
    ADD COLUMN IF NOT EXISTS duration_ms BIGINT NULL,
    ADD COLUMN IF NOT EXISTS provider TEXT NULL,
    ADD COLUMN IF NOT EXISTS model TEXT NULL,
    ADD COLUMN IF NOT EXISTS input_tokens BIGINT NULL,
    ADD COLUMN IF NOT EXISTS output_tokens BIGINT NULL,
    ADD COLUMN IF NOT EXISTS cache_read_tokens BIGINT NULL,
    ADD COLUMN IF NOT EXISTS cache_write_tokens BIGINT NULL,
    ADD COLUMN IF NOT EXISTS total_tokens BIGINT NULL;

CREATE INDEX IF NOT EXISTS idx_project_timeline_events_project_kind_recorded
    ON project_timeline_events (project_id, kind, recorded_at DESC);

CREATE INDEX IF NOT EXISTS idx_project_timeline_events_operation_id
    ON project_timeline_events (operation_id)
    WHERE operation_id IS NOT NULL;
