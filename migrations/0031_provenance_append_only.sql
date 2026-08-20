-- Verification history becomes append-only: memory_source_verifications kept
-- one row per source (ON CONFLICT DO UPDATE), destroying the transitions an
-- evidence record stream needs. Sources also gain the anchors real evidence
-- requires: commit, line range, content hash, and a typed memory reference
-- instead of consolidation's UUID-in-excerpt hack.
CREATE TABLE memory_source_checks (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source_id UUID NOT NULL REFERENCES memory_sources(id) ON DELETE CASCADE,
    status TEXT NOT NULL,
    checked_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    reason TEXT,
    resolved_path TEXT,
    observed_git_commit TEXT,
    observed_content_hash TEXT
);
CREATE INDEX idx_memory_source_checks_latest
    ON memory_source_checks(source_id, checked_at DESC);

INSERT INTO memory_source_checks (source_id, status, checked_at, reason, resolved_path)
SELECT source_id, status, checked_at, reason, resolved_path
FROM memory_source_verifications;

DROP TABLE memory_source_verifications;

CREATE VIEW memory_source_verifications AS
SELECT DISTINCT ON (source_id)
    source_id, status, checked_at, reason, resolved_path
FROM memory_source_checks
ORDER BY source_id, checked_at DESC;

ALTER TABLE memory_sources
    ADD COLUMN line_start INTEGER,
    ADD COLUMN line_end INTEGER,
    ADD COLUMN content_hash TEXT,
    ADD COLUMN target_memory_id UUID;

UPDATE memory_sources
   SET target_memory_id = excerpt::uuid
 WHERE source_kind = 'memory'
   AND excerpt ~ '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$';
