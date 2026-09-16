-- R1 foreground phases may span external work, but no SQL transaction does.
-- A dedicated PostgreSQL session owns each execution's advisory lock. Recovery
-- acquires that same lock only after the owner disconnects; it never guesses
-- based on a run's age or replays an uncertain external action.
CREATE TABLE foreground_executions (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('loop', 'validation')),
    project_id UUID REFERENCES projects(id) ON DELETE CASCADE,
    backend_pid INTEGER NOT NULL,
    lock_key BIGINT NOT NULL UNIQUE,
    status TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running','finished','interrupted')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    finished_at TIMESTAMPTZ,
    CHECK ((status='running') = (finished_at IS NULL))
);
CREATE INDEX foreground_executions_running ON foreground_executions(kind, created_at, id)
    WHERE status='running';
