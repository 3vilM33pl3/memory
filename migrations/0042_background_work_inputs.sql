-- Immutable version identities are retained even if retention removes source
-- bytes. A worker must then report stale input, never re-select another version.
CREATE TABLE background_work_versions (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    operation_id UUID NOT NULL,
    memory_id UUID NOT NULL,
    PRIMARY KEY(project_id, operation_id, memory_id)
);

-- Private, reusable successful provider outputs. Never exposed by jobs APIs.
CREATE TABLE background_job_outputs (
    job_id UUID NOT NULL REFERENCES background_jobs(id) ON DELETE CASCADE,
    output_key TEXT NOT NULL CHECK(length(output_key) BETWEEN 1 AND 256),
    config_fingerprint TEXT NOT NULL CHECK(length(config_fingerprint)=64),
    output JSONB NOT NULL CHECK(pg_column_size(output) <= 16777216),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(job_id, output_key, config_fingerprint)
);
