-- No default subscriptions. All existing mutation paths participate only after
-- an explicit reviewed opt-in. Deferred hooks see tags/provenance written later
-- in the SAME transaction and freeze the committed source snapshot, not raw data.
CREATE TABLE federation_local_grants (
    space UUID NOT NULL, instance UUID NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    principal_id UUID NOT NULL REFERENCES auth_principals(id) ON DELETE RESTRICT,
    capabilities JSONB NOT NULL, enabled BOOLEAN NOT NULL DEFAULT TRUE,
    PRIMARY KEY(space,instance,project_id,principal_id)
);
CREATE TABLE federation_publication_reviews (
    id UUID PRIMARY KEY, space UUID NOT NULL, instance UUID NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    canonical_id UUID NOT NULL, source_id UUID NOT NULL,
    reviewer UUID NOT NULL REFERENCES auth_principals(id) ON DELETE RESTRICT,
    reviewed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    evidence JSONB NOT NULL
);
CREATE TABLE federation_source_subscriptions (
    space UUID NOT NULL, instance UUID NOT NULL, canonical_id UUID NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    principal_id UUID NOT NULL REFERENCES auth_principals(id) ON DELETE RESTRICT,
    review_id UUID NOT NULL REFERENCES federation_publication_reviews(id),
    evidence JSONB NOT NULL, enabled BOOLEAN NOT NULL DEFAULT TRUE,
    capture_counter BIGINT NOT NULL DEFAULT 0,
    shared_version BIGINT NOT NULL DEFAULT 0 CHECK(shared_version BETWEEN 0 AND 4294967295),
    state_revision BIGINT NOT NULL DEFAULT 0 CHECK(state_revision BETWEEN 0 AND 4294967295),
    last_content JSONB, last_state JSONB, last_record JSONB,
    PRIMARY KEY(space,instance,canonical_id)
);
CREATE TABLE federation_source_changes (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    space UUID NOT NULL, instance UUID NOT NULL, canonical_id UUID NOT NULL,
    capture_order BIGINT NOT NULL, transaction_id TEXT NOT NULL,
    snapshot JSONB NOT NULL, prepared BOOLEAN NOT NULL DEFAULT FALSE,
    UNIQUE(space,instance,canonical_id,capture_order),
    UNIQUE(space,instance,canonical_id,transaction_id),
    FOREIGN KEY(space,instance,canonical_id) REFERENCES federation_source_subscriptions(space,instance,canonical_id) ON DELETE RESTRICT
);
CREATE INDEX federation_source_changes_pending ON federation_source_changes(space,instance,canonical_id,capture_order) WHERE NOT prepared;
CREATE INDEX federation_source_outbox_pending ON federation_source_outbox(space,publisher,queued_at) WHERE acceptance IS NULL;

CREATE TRIGGER federation_reviews_immutable BEFORE UPDATE OR DELETE ON federation_publication_reviews
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
CREATE FUNCTION federation_source_snapshot_immutable() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    IF (to_jsonb(NEW)-'prepared') IS DISTINCT FROM (to_jsonb(OLD)-'prepared') OR (OLD.prepared AND NOT NEW.prepared) THEN
        RAISE EXCEPTION 'frozen publication intent is immutable';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER federation_source_snapshot_immutable BEFORE UPDATE ON federation_source_changes
FOR EACH ROW EXECUTE FUNCTION federation_source_snapshot_immutable();

CREATE FUNCTION federation_capture_source(cid UUID) RETURNS VOID LANGUAGE plpgsql AS $$
DECLARE s RECORD; content_row RECORD; latest RECORD; state_row RECORD; frozen JSONB; tags JSONB; author TEXT;
BEGIN
    FOR s IN SELECT * FROM federation_source_subscriptions WHERE canonical_id=cid AND enabled ORDER BY space,instance FOR UPDATE LOOP
        IF EXISTS(SELECT 1 FROM federation_source_changes WHERE space=s.space AND instance=s.instance AND canonical_id=cid AND transaction_id=txid_current()::text) THEN CONTINUE; END IF;
        SELECT * INTO content_row FROM memory_entries WHERE canonical_id=cid AND project_id=s.project_id AND NOT is_tombstone ORDER BY version_no DESC LIMIT 1;
        IF NOT FOUND THEN CONTINUE; END IF;
        SELECT * INTO latest FROM memory_entries WHERE canonical_id=cid AND project_id=s.project_id ORDER BY version_no DESC LIMIT 1;
        SELECT * INTO state_row FROM memory_canonical_state WHERE canonical_id=cid AND project_id=s.project_id;
        IF NOT FOUND THEN CONTINUE; END IF;
        SELECT COALESCE(jsonb_agg(tag ORDER BY tag),'[]'::jsonb) INTO tags FROM memory_tags WHERE memory_entry_id=content_row.id;
        SELECT se.writer_id INTO author FROM memory_sources ms JOIN tasks t ON t.id=ms.task_id JOIN sessions se ON se.id=t.session_id WHERE ms.memory_entry_id=content_row.id ORDER BY ms.created_at,ms.id LIMIT 1;
        frozen := jsonb_build_object(
            'source',jsonb_build_object('record_id',content_row.id,'version',content_row.version_no),
            'content',jsonb_build_object('text',content_row.canonical_text,'summary',content_row.summary,'memory_type',content_row.memory_type,'confidence',floor(content_row.confidence::float8*1000000+0.5)::bigint,'importance',content_row.importance,'tags',tags),
            'status',state_row.status,'confidence',floor(state_row.confidence::float8*1000000+0.5)::bigint,
            'tombstone',latest.is_tombstone,'author',COALESCE(NULLIF(author,''),'unknown:local-author'),
            'authored_at',content_row.created_at,'state_at',GREATEST(state_row.updated_at,latest.created_at),
            'evidence',s.evidence);
        UPDATE federation_source_subscriptions SET capture_counter=capture_counter+1 WHERE space=s.space AND instance=s.instance AND canonical_id=cid;
        INSERT INTO federation_source_changes(space,instance,canonical_id,capture_order,transaction_id,snapshot)
        VALUES(s.space,s.instance,cid,s.capture_counter+1,txid_current()::text,frozen);
    END LOOP;
END $$;

CREATE FUNCTION federation_source_changed() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM federation_capture_source(NEW.canonical_id);
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER federation_memory_changed AFTER INSERT ON memory_entries
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION federation_source_changed();
CREATE CONSTRAINT TRIGGER federation_state_changed AFTER INSERT OR UPDATE ON memory_canonical_state
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION federation_source_changed();
CREATE FUNCTION federation_tags_changed() RETURNS TRIGGER LANGUAGE plpgsql AS $$
DECLARE cid UUID;
BEGIN
    SELECT canonical_id INTO cid FROM memory_entries WHERE id=COALESCE(NEW.memory_entry_id,OLD.memory_entry_id);
    IF cid IS NOT NULL THEN PERFORM federation_capture_source(cid); END IF;
    RETURN NULL;
END $$;
CREATE CONSTRAINT TRIGGER federation_tags_changed AFTER INSERT OR UPDATE OR DELETE ON memory_tags
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION federation_tags_changed();

-- Delivery state persists across disconnected processes. Quarantine waits for
-- dependency/resync, whereas authorization/conflict failures require intervention.
ALTER TABLE federation_source_outbox ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE federation_source_outbox ADD COLUMN available_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp();
ALTER TABLE federation_source_outbox ADD COLUMN blocked BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE federation_source_outbox ADD COLUMN last_error TEXT;
ALTER TABLE federation_source_outbox ADD COLUMN principal_id UUID;
ALTER TABLE federation_replicas ADD COLUMN sync_failures INTEGER NOT NULL DEFAULT 0;
ALTER TABLE federation_replicas ADD COLUMN retry_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp();
ALTER TABLE federation_replicas ADD COLUMN sync_error TEXT;
