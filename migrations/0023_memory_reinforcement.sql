-- Memory reinforcement: access-driven activation scores, spreading activation,
-- volatility tracking, and threshold-triggered validation runs.
--
-- memory_scores is mutable per-canonical state. Memory rows themselves stay
-- immutable versions (see 0013), so score state is keyed by canonical_id in a
-- separate table. No FK to memory_entries: retention may prune version 1
-- (whose id equals canonical_id); orphaned score rows are removed by the
-- reinforcement scheduler's compaction sweep.
CREATE TABLE IF NOT EXISTS memory_scores (
    canonical_id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    activation DOUBLE PRECISION NOT NULL DEFAULT 0,
    -- when activation was last materialized; decay is computed from this
    -- timestamp inside each UPDATE so concurrent writers never race a
    -- read-modify-write cycle.
    last_decay_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_access_at TIMESTAMPTZ,
    access_count BIGINT NOT NULL DEFAULT 0,
    citation_count BIGINT NOT NULL DEFAULT 0,
    propagated_count BIGINT NOT NULL DEFAULT 0,
    -- EWMA of provenance-file change events per day (update-risk TTL model);
    -- scales how soon a validated memory becomes due again.
    volatility REAL NOT NULL DEFAULT 0,
    volatility_updated_at TIMESTAMPTZ,
    validated_at TIMESTAMPTZ,
    validation_confidence REAL,
    needs_review BOOLEAN NOT NULL DEFAULT FALSE,
    needs_review_reason TEXT,
    last_invalidated_at TIMESTAMPTZ,
    last_validation_id UUID,
    validation_cooldown_until TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_memory_scores_project_activation
    ON memory_scores (project_id, activation DESC);
CREATE INDEX IF NOT EXISTS idx_memory_scores_needs_review
    ON memory_scores (project_id) WHERE needs_review;
CREATE INDEX IF NOT EXISTS idx_memory_scores_validation_scan
    ON memory_scores (project_id, activation DESC) WHERE NOT needs_review;

-- Compact append-only access log for analysis and debugging. The running
-- activation in memory_scores is authoritative (Petrov O(1) incremental
-- form); this log is never replayed for scoring and is retention-pruned.
CREATE TABLE IF NOT EXISTS memory_access_events (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    canonical_id UUID NOT NULL,
    project_id UUID NOT NULL,
    accessed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- retrieval | citation | direct_read | propagated
    kind TEXT NOT NULL,
    boost REAL NOT NULL,
    hop_distance SMALLINT NOT NULL DEFAULT 0,
    operation_id TEXT
);

CREATE INDEX IF NOT EXISTS idx_memory_access_events_canonical
    ON memory_access_events (canonical_id, accessed_at DESC);
CREATE INDEX IF NOT EXISTS idx_memory_access_events_prune
    ON memory_access_events (accessed_at);

-- Audit of significant score/status transitions only, never per-access.
-- Reasons: threshold_crossed | validation_completed | needs_review_set |
-- needs_review_resolved | volatility_shift | decay_compaction | manual_reset
CREATE TABLE IF NOT EXISTS memory_score_audit (
    id UUID PRIMARY KEY,
    canonical_id UUID NOT NULL,
    project_id UUID NOT NULL,
    reason TEXT NOT NULL,
    old_activation DOUBLE PRECISION,
    new_activation DOUBLE PRECISION,
    details_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_memory_score_audit_canonical
    ON memory_score_audit (canonical_id, created_at DESC);

CREATE TABLE IF NOT EXISTS memory_validation_runs (
    id UUID PRIMARY KEY,
    canonical_id UUID NOT NULL,
    -- version that was validated; no FK because retention may prune it
    memory_id UUID NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    -- threshold | curator | manual | scheduled
    trigger_kind TEXT NOT NULL,
    -- running | completed | failed
    status TEXT NOT NULL DEFAULT 'running',
    -- valid | partially_valid | outdated | ambiguous | unsupported
    verdict TEXT,
    confidence REAL,
    dry_run BOOLEAN NOT NULL DEFAULT FALSE,
    -- none | revalidated | reworded | correction_pending |
    -- flagged_needs_review (would_* variants when dry_run)
    action TEXT,
    proposed_candidate_json JSONB,
    -- NULL | pending | applied | rejected
    review_status TEXT,
    reasons_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    model TEXT,
    details_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    error TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_memory_validation_runs_canonical
    ON memory_validation_runs (canonical_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_memory_validation_runs_project_review
    ON memory_validation_runs (project_id, started_at DESC)
    WHERE review_status = 'pending';
CREATE INDEX IF NOT EXISTS idx_memory_validation_runs_started
    ON memory_validation_runs (started_at);

-- Per-run, stance-annotated evidence snapshot. Distinct from memory_sources,
-- which is the memory's own long-lived provenance verified by the provenance
-- scheduler; evidence here records what a specific validation run consulted
-- and whether it supported or contradicted the memory.
CREATE TABLE IF NOT EXISTS memory_validation_evidence (
    id UUID PRIMARY KEY,
    validation_run_id UUID NOT NULL
        REFERENCES memory_validation_runs(id) ON DELETE CASCADE,
    -- file | code_symbol | doc | commit | test | issue | memory | search_hit
    kind TEXT NOT NULL,
    evidence_ref TEXT NOT NULL,
    -- supports | contradicts | neutral
    stance TEXT NOT NULL,
    excerpt TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_memory_validation_evidence_run
    ON memory_validation_evidence (validation_run_id);
