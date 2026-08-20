-- The project timeline becomes an ordered, durable log - the future data
-- plane's spine. Events gain a monotonic sequence for cursors and gap
-- detection, and stop losing their subject when retention prunes versions.
ALTER TABLE project_timeline_events ADD COLUMN seq BIGINT;
UPDATE project_timeline_events t
   SET seq = ranked.rn
  FROM (
      SELECT id, row_number() OVER (ORDER BY recorded_at, id) AS rn
      FROM project_timeline_events
  ) ranked
 WHERE ranked.id = t.id;
ALTER TABLE project_timeline_events ALTER COLUMN seq SET NOT NULL;

CREATE SEQUENCE project_timeline_events_seq_seq OWNED BY project_timeline_events.seq;
SELECT setval(
    'project_timeline_events_seq_seq',
    COALESCE((SELECT MAX(seq) FROM project_timeline_events), 0) + 1,
    false
);
ALTER TABLE project_timeline_events
    ALTER COLUMN seq SET DEFAULT nextval('project_timeline_events_seq_seq');

CREATE UNIQUE INDEX idx_timeline_events_seq ON project_timeline_events(seq);
CREATE INDEX idx_timeline_events_project_seq ON project_timeline_events(project_id, seq DESC);

-- Subject identity survives retention: plain UUIDs, no FK.
ALTER TABLE project_timeline_events
    DROP CONSTRAINT IF EXISTS project_timeline_events_memory_id_fkey;
ALTER TABLE project_timeline_events
    ADD COLUMN canonical_id UUID,
    ADD COLUMN version_no INTEGER;
UPDATE project_timeline_events t
   SET canonical_id = m.canonical_id, version_no = m.version_no
  FROM memory_entries m
 WHERE m.id = t.memory_id;
