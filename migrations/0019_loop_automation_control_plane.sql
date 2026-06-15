CREATE TABLE IF NOT EXISTS loop_definitions (
    id UUID PRIMARY KEY,
    loop_id TEXT NOT NULL,
    version INT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL,
    risk_level TEXT NOT NULL,
    default_mode TEXT NOT NULL,
    trigger_spec JSONB NOT NULL DEFAULT '{}'::jsonb,
    context_spec JSONB NOT NULL DEFAULT '{}'::jsonb,
    policy_spec JSONB NOT NULL DEFAULT '{}'::jsonb,
    output_spec JSONB NOT NULL DEFAULT '{}'::jsonb,
    is_current BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (loop_id, version)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_loop_definitions_current
    ON loop_definitions (loop_id)
    WHERE is_current;

CREATE TABLE IF NOT EXISTS loop_global_state (
    id BOOLEAN PRIMARY KEY DEFAULT TRUE,
    kill_switch_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    updated_by TEXT NULL,
    reason TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (id = TRUE)
);

INSERT INTO loop_global_state (id, kill_switch_enabled)
VALUES (TRUE, FALSE)
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS loop_settings (
    id UUID PRIMARY KEY,
    loop_id TEXT NOT NULL,
    scope_type TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    project_id UUID NULL REFERENCES projects(id) ON DELETE CASCADE,
    repo_root TEXT NULL,
    enabled BOOLEAN NULL,
    mode TEXT NULL,
    budgets_json JSONB NULL,
    approval_overrides_json JSONB NULL,
    paused_until TIMESTAMPTZ NULL,
    snoozed_until TIMESTAMPTZ NULL,
    updated_by TEXT NULL,
    reason TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (loop_id, scope_type, scope_id)
);

CREATE INDEX IF NOT EXISTS idx_loop_settings_scope
    ON loop_settings (scope_type, scope_id, loop_id);

CREATE INDEX IF NOT EXISTS idx_loop_settings_project
    ON loop_settings (project_id, loop_id)
    WHERE project_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS trigger_events (
    id UUID PRIMARY KEY,
    source TEXT NOT NULL,
    event_type TEXT NOT NULL,
    project_id UUID NULL REFERENCES projects(id) ON DELETE SET NULL,
    repo_root TEXT NULL,
    payload_hash TEXT NOT NULL,
    dedupe_key TEXT NULL,
    trust_level TEXT NOT NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_trigger_events_dedupe
    ON trigger_events (dedupe_key)
    WHERE dedupe_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_trigger_events_project_received
    ON trigger_events (project_id, received_at DESC)
    WHERE project_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS loop_runs (
    id UUID PRIMARY KEY,
    loop_id TEXT NOT NULL,
    definition_id UUID NULL REFERENCES loop_definitions(id) ON DELETE SET NULL,
    definition_version INT NOT NULL,
    project_id UUID NULL REFERENCES projects(id) ON DELETE SET NULL,
    repo_root TEXT NULL,
    scope_type TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    trigger_event_id UUID NULL REFERENCES trigger_events(id) ON DELETE SET NULL,
    mode TEXT NOT NULL,
    status TEXT NOT NULL,
    run_reason TEXT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ NULL,
    cancel_requested_at TIMESTAMPTZ NULL,
    cost_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    output_summary TEXT NULL,
    output_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    effective_settings_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    policy_decisions_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    blocked_reasons_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    trace_count INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_loop_runs_loop_started
    ON loop_runs (loop_id, started_at DESC);

CREATE INDEX IF NOT EXISTS idx_loop_runs_project_started
    ON loop_runs (project_id, started_at DESC)
    WHERE project_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_loop_runs_status
    ON loop_runs (status, started_at DESC);

CREATE TABLE IF NOT EXISTS approval_requests (
    id UUID PRIMARY KEY,
    run_id UUID NULL REFERENCES loop_runs(id) ON DELETE SET NULL,
    project_id UUID NULL REFERENCES projects(id) ON DELETE SET NULL,
    loop_id TEXT NOT NULL,
    action_type TEXT NOT NULL,
    proposed_action_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    risk_reason TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    requester TEXT NULL,
    reviewer TEXT NULL,
    decision_reason TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS idx_approval_requests_project_status
    ON approval_requests (project_id, status, created_at DESC)
    WHERE project_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_approval_requests_loop_status
    ON approval_requests (loop_id, status, created_at DESC);

CREATE TABLE IF NOT EXISTS memory_proposals (
    id UUID PRIMARY KEY,
    run_id UUID NULL REFERENCES loop_runs(id) ON DELETE SET NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    loop_id TEXT NOT NULL,
    proposal_type TEXT NOT NULL,
    target_memory_id UUID NULL REFERENCES memory_entries(id) ON DELETE SET NULL,
    candidate_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    evidence_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    confidence REAL NOT NULL DEFAULT 0,
    risk_notes TEXT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_proposals_project_status
    ON memory_proposals (project_id, status, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_memory_proposals_run
    ON memory_proposals (run_id)
    WHERE run_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS learned_skills (
    id UUID PRIMARY KEY,
    project_id UUID NULL REFERENCES projects(id) ON DELETE CASCADE,
    run_id UUID NULL REFERENCES loop_runs(id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    applicability_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    recipe_markdown TEXT NOT NULL,
    commands_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    validation_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved_at TIMESTAMPTZ NULL
);

CREATE INDEX IF NOT EXISTS idx_learned_skills_project_status
    ON learned_skills (project_id, status, created_at DESC)
    WHERE project_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS run_traces (
    id UUID PRIMARY KEY,
    run_id UUID NOT NULL REFERENCES loop_runs(id) ON DELETE CASCADE,
    sequence INT NOT NULL,
    trace_type TEXT NOT NULL,
    title TEXT NOT NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    redacted BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (run_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_run_traces_run_sequence
    ON run_traces (run_id, sequence);
