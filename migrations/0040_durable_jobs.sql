-- Durable local work is independent of the federation gateway/outbox.
CREATE TABLE background_project_slots (
    project_id UUID PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    job_id UUID,
    lease_token UUID,
    lease_until TIMESTAMPTZ,
    CHECK ((job_id IS NULL) = (lease_token IS NULL)),
    CHECK ((job_id IS NULL) = (lease_until IS NULL))
);
CREATE TABLE background_jobs (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    operation_id UUID NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('embedding','semantic_dedup','validation_due','consolidation','consolidation_synthesis')),
    work_key TEXT NOT NULL CHECK (length(work_key) BETWEEN 1 AND 256),
    -- Explicit internal actor or authenticated requester; never a bearer token.
    actor JSONB NOT NULL,
    input JSONB NOT NULL CHECK (octet_length(input::text) <= 65536),
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','running','retry_wait','blocked','failed','succeeded','skipped')),
    generation INTEGER NOT NULL DEFAULT 1 CHECK (generation > 0),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    available_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    lease_token UUID,
    lease_until TIMESTAMPTZ,
    last_failure TEXT,
    UNIQUE(project_id, work_key),
    UNIQUE(project_id, id),
    CHECK ((status = 'running') = (lease_token IS NOT NULL)),
    CHECK ((lease_token IS NULL) = (lease_until IS NULL))
);
CREATE INDEX background_jobs_ready ON background_jobs(project_id, available_at, created_at)
WHERE status IN ('queued','retry_wait','running');
CREATE TABLE background_job_dependencies (
    project_id UUID NOT NULL,
    job_id UUID NOT NULL,
    depends_on UUID NOT NULL,
    PRIMARY KEY(job_id, depends_on),
    CHECK (job_id <> depends_on),
    FOREIGN KEY(project_id,job_id) REFERENCES background_jobs(project_id,id) ON DELETE CASCADE,
    FOREIGN KEY(project_id,depends_on) REFERENCES background_jobs(project_id,id) ON DELETE CASCADE
);
CREATE TABLE background_job_attempts (
    job_id UUID NOT NULL REFERENCES background_jobs(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    attempt INTEGER NOT NULL,
    token UUID NOT NULL UNIQUE,
    started_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    finished_at TIMESTAMPTZ,
    outcome TEXT CHECK (outcome IN ('succeeded','skipped','retry_wait','blocked','failed','lease_lost')),
    failure TEXT,
    uncertain BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY(job_id,generation,attempt)
);
CREATE TABLE background_job_retry_receipts (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    request_id UUID NOT NULL,
    job_id UUID NOT NULL REFERENCES background_jobs(id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    actor JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(project_id,request_id)
);

CREATE FUNCTION immutable_background_job_input() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF (NEW.id,NEW.project_id,NEW.operation_id,NEW.kind,NEW.work_key,NEW.actor,NEW.input,NEW.created_at)
       IS DISTINCT FROM
       (OLD.id,OLD.project_id,OLD.operation_id,OLD.kind,OLD.work_key,OLD.actor,OLD.input,OLD.created_at) THEN
        RAISE EXCEPTION 'background job input is immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER background_job_input_immutable BEFORE UPDATE ON background_jobs
FOR EACH ROW EXECUTE FUNCTION immutable_background_job_input();
