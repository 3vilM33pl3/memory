-- A review may share a transaction with more source mutations. Capture after
-- that whole transaction, just like ordinary source changes, not at API call time.
CREATE CONSTRAINT TRIGGER federation_review_capture
AFTER INSERT OR UPDATE OF review_id ON federation_source_subscriptions
DEFERRABLE INITIALLY DEFERRED FOR EACH ROW
EXECUTE FUNCTION federation_source_changed();

CREATE TRIGGER federation_source_snapshot_no_delete
BEFORE DELETE ON federation_source_changes
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
