-- Mutable memory state moves off the version rows. memory_entries rows are
-- immutable content versions (ADR: content addressing must not drift);
-- status/archive and the curate-boosted confidence/importance live here,
-- keyed by canonical_id. No FK to memory_entries: retention may prune any
-- version row (memory_scores precedent, migration 0023).
CREATE TABLE memory_canonical_state (
    canonical_id UUID PRIMARY KEY,
    project_id   UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    status       TEXT NOT NULL DEFAULT 'active',
    archived_at  TIMESTAMPTZ,
    confidence   REAL NOT NULL,
    importance   INTEGER NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_memory_canonical_state_project_status
    ON memory_canonical_state(project_id, status);

-- Seed from the latest version of each canonical.
INSERT INTO memory_canonical_state
       (canonical_id, project_id, status, archived_at, confidence, importance)
SELECT DISTINCT ON (canonical_id)
       canonical_id, project_id, status, archived_at, confidence, importance
FROM memory_entries
ORDER BY canonical_id, version_no DESC;

-- Single-statement STABLE SQL functions: the planner inlines them, so the
-- read paths can reference canonical state without restructuring joins.
CREATE FUNCTION memory_state_status(cid UUID) RETURNS TEXT
LANGUAGE sql STABLE AS
$$ SELECT status FROM memory_canonical_state WHERE canonical_id = cid $$;

CREATE FUNCTION memory_state_archived_at(cid UUID) RETURNS TIMESTAMPTZ
LANGUAGE sql STABLE AS
$$ SELECT archived_at FROM memory_canonical_state WHERE canonical_id = cid $$;

CREATE FUNCTION memory_state_confidence(cid UUID) RETURNS REAL
LANGUAGE sql STABLE AS
$$ SELECT confidence FROM memory_canonical_state WHERE canonical_id = cid $$;

CREATE FUNCTION memory_state_importance(cid UUID) RETURNS INTEGER
LANGUAGE sql STABLE AS
$$ SELECT importance FROM memory_canonical_state WHERE canonical_id = cid $$;

-- Every new (non-tombstone) version reasserts the memory: the canonical
-- state row follows automatically, so no insert path can forget it.
CREATE FUNCTION sync_memory_canonical_state() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO memory_canonical_state
        (canonical_id, project_id, status, archived_at, confidence, importance)
    VALUES (NEW.canonical_id, NEW.project_id, 'active', NULL, NEW.confidence, NEW.importance)
    ON CONFLICT (canonical_id) DO UPDATE
        SET status = 'active',
            archived_at = NULL,
            confidence = EXCLUDED.confidence,
            importance = EXCLUDED.importance,
            updated_at = now();
    RETURN NEW;
END $$;

CREATE TRIGGER memory_entries_sync_state
AFTER INSERT ON memory_entries
FOR EACH ROW
WHEN (NOT NEW.is_tombstone)
EXECUTE FUNCTION sync_memory_canonical_state();
