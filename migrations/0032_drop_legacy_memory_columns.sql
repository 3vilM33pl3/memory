-- Version rows are immutable content records. Mutable state (status,
-- archive) lives in memory_canonical_state since migration 0028; the legacy
-- columns and their write paths are gone, so drop them. confidence and
-- importance REMAIN on memory_entries redefined as the values asserted when
-- this version was captured (the live values are canonical state).
DROP INDEX IF EXISTS idx_memory_entries_project_status;
ALTER TABLE memory_entries DROP COLUMN status;
ALTER TABLE memory_entries DROP COLUMN archived_at;

COMMENT ON COLUMN memory_entries.confidence IS
    'Confidence as asserted at capture of this version; live value is memory_canonical_state.confidence.';
COMMENT ON COLUMN memory_entries.importance IS
    'Importance as asserted at capture of this version; live value is memory_canonical_state.importance.';
