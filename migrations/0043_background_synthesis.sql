CREATE TABLE background_synthesis_inputs (
    job_id UUID PRIMARY KEY REFERENCES background_jobs(id) ON DELETE CASCADE,
    cluster_snapshot JSONB NOT NULL CHECK(pg_column_size(cluster_snapshot) <= 1048576)
);
CREATE TABLE background_daily_budget (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    day DATE NOT NULL,
    attempts INTEGER NOT NULL CHECK(attempts >= 0),
    PRIMARY KEY(project_id, day)
);
ALTER TABLE background_jobs ADD COLUMN reason TEXT
    CHECK(reason IN ('disabled','no_inputs','dependency_skipped','dry_run','utility_floor','coalesced'));
