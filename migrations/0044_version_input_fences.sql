-- New versions (including tombstones) respect a prepared synthesis's state
-- lock, preventing a newer version committing before its result commit.
CREATE FUNCTION lock_memory_version_state() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM canonical_id FROM memory_canonical_state
        WHERE canonical_id=NEW.canonical_id FOR UPDATE;
    RETURN NEW;
END $$;
CREATE TRIGGER memory_entries_version_input_fence BEFORE INSERT ON memory_entries
    FOR EACH ROW EXECUTE FUNCTION lock_memory_version_state();
