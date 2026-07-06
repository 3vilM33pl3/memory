-- Procedural utility: ACT-R production-utility learning
--   U_n = U_{n-1} + alpha * (R_n - U_{n-1})   (delta rule; Fu & Anderson 2006)
-- Learned, mutable value for automation producers (loops in v1; strategies and
-- skills later via producer_kind), kept OUTSIDE immutable loop/memory content,
-- mirroring ADR-0002 and memory_scores (0023). Advisory only: nothing here may
-- drive LoopMode or permission-gate decisions.

CREATE TABLE IF NOT EXISTS procedural_utility (
    project_id     UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    producer_kind  TEXT NOT NULL,              -- 'loop' (v1); 'strategy' | 'skill' later
    producer_id    TEXT NOT NULL,              -- loop_id, matches memory_proposals.loop_id
    utility        DOUBLE PRECISION NOT NULL DEFAULT 0,
    update_count   BIGINT NOT NULL DEFAULT 0,
    last_reward    DOUBLE PRECISION,
    last_update_at TIMESTAMPTZ,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, producer_kind, producer_id)
);

CREATE INDEX IF NOT EXISTS idx_procedural_utility_project_util
    ON procedural_utility (project_id, producer_kind, utility DESC);

-- Every utility update is audited (reward events are sparse, so per-event
-- rows are affordable, unlike memory_score_audit which logs transitions only).
-- Reasons: proposal_approved | proposal_edited_approved | proposal_rejected
--        | loop_run_error | memory_cited | manual_reset
CREATE TABLE IF NOT EXISTS procedural_utility_audit (
    id             UUID PRIMARY KEY,
    project_id     UUID NOT NULL,
    producer_kind  TEXT NOT NULL,
    producer_id    TEXT NOT NULL,
    reason         TEXT NOT NULL,
    reward         DOUBLE PRECISION,
    alpha          DOUBLE PRECISION,
    old_utility    DOUBLE PRECISION,
    new_utility    DOUBLE PRECISION,
    details_json   JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_procedural_utility_audit_producer
    ON procedural_utility_audit (project_id, producer_kind, producer_id, created_at DESC);

-- Edited proposals return to status='pending', erasing the edit signal by the
-- time they are approved. Persist it so edited-then-approved earns the partial
-- reward exactly once at the terminal decision.
ALTER TABLE memory_proposals
    ADD COLUMN IF NOT EXISTS was_edited BOOLEAN NOT NULL DEFAULT FALSE;

-- Deterministic link from a proposal-created memory back to the loop that
-- produced it, so later citations of that memory can reward the loop without
-- parsing provenance note strings.
CREATE TABLE IF NOT EXISTS loop_produced_memory (
    canonical_id UUID NOT NULL PRIMARY KEY,
    project_id   UUID NOT NULL,
    loop_id      TEXT NOT NULL,
    run_id       UUID,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_loop_produced_memory_project_loop
    ON loop_produced_memory (project_id, loop_id);
