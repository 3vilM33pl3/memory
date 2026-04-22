-- Memory versioning.
--
-- Each row in memory_entries used to be a mutable, deletable record. This
-- migration turns every row into an immutable version of a "canonical" memory:
--
--   * canonical_id groups together every version of the same logical memory.
--   * version_no orders them (1, 2, 3, ...) per canonical_id.
--   * is_tombstone marks the "deleted" sentinel version whose content is empty.
--
-- Existing rows become version 1 of themselves (canonical_id = id).

ALTER TABLE memory_entries
    ADD COLUMN IF NOT EXISTS canonical_id UUID,
    ADD COLUMN IF NOT EXISTS version_no INTEGER,
    ADD COLUMN IF NOT EXISTS is_tombstone BOOLEAN;

-- Back-fill existing rows: every row is version 1 of itself.
UPDATE memory_entries
   SET canonical_id = id
 WHERE canonical_id IS NULL;

UPDATE memory_entries
   SET version_no = 1
 WHERE version_no IS NULL;

UPDATE memory_entries
   SET is_tombstone = FALSE
 WHERE is_tombstone IS NULL;

ALTER TABLE memory_entries
    ALTER COLUMN canonical_id SET NOT NULL,
    ALTER COLUMN version_no SET NOT NULL,
    ALTER COLUMN is_tombstone SET NOT NULL,
    ALTER COLUMN version_no SET DEFAULT 1,
    ALTER COLUMN is_tombstone SET DEFAULT FALSE;

-- Enforce one row per (canonical_id, version_no) so concurrent curators can't
-- produce conflicting versions. The per-row `id` remains the PK that chunks,
-- embeddings, sources, and relations FK to.
CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_entries_canonical_version
    ON memory_entries(canonical_id, version_no);

-- Fast lookup of the latest version per canonical_id — used by every default
-- query and by the curator when deciding the next version_no.
CREATE INDEX IF NOT EXISTS idx_memory_entries_canonical_latest
    ON memory_entries(canonical_id, version_no DESC);
