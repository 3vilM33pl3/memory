-- Coordinated cutover: stop all application writers before migration. Existing
-- positions survive; historical events lost before this migration are not made up.
LOCK TABLE project_timeline_events IN ACCESS EXCLUSIVE MODE;

CREATE TABLE timeline_position (
    singleton BOOLEAN PRIMARY KEY DEFAULT TRUE CHECK (singleton),
    position BIGINT NOT NULL CHECK (position >= 0)
);
INSERT INTO timeline_position (position)
SELECT COALESCE(MAX(seq), 0) FROM project_timeline_events;

ALTER TABLE project_timeline_events ALTER COLUMN seq DROP DEFAULT;

-- The UPDATE lock lives until transaction end. A later committed event can
-- never become visible while an earlier allocated position remains uncommitted.
-- Ignore supplied seq, including old callers: allocation belongs to this log.
CREATE FUNCTION allocate_timeline_position() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    UPDATE timeline_position SET position = position + 1
    WHERE singleton RETURNING position INTO NEW.seq;
    RETURN NEW;
END;
$$;
CREATE TRIGGER timeline_position_before_insert
BEFORE INSERT ON project_timeline_events FOR EACH ROW
EXECUTE FUNCTION allocate_timeline_position();

-- Gaps from retention are distinct from global positions belonging to another
-- project. Keep this small tombstone even when the project itself is removed.
CREATE TABLE timeline_retention_watermarks (
    project_id UUID PRIMARY KEY,
    deleted_through BIGINT NOT NULL CHECK (deleted_through > 0)
);
CREATE FUNCTION retain_timeline_watermark() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO timeline_retention_watermarks VALUES (OLD.project_id, OLD.seq)
    ON CONFLICT (project_id) DO UPDATE SET deleted_through =
        GREATEST(timeline_retention_watermarks.deleted_through, EXCLUDED.deleted_through);
    RETURN OLD;
END;
$$;
CREATE TRIGGER timeline_retention_before_delete
BEFORE DELETE ON project_timeline_events FOR EACH ROW
EXECUTE FUNCTION retain_timeline_watermark();
