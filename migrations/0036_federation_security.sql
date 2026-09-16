-- Explicit enrollment only. Existing published data is NOT silently certified.
CREATE TABLE federation_security (
    space UUID PRIMARY KEY REFERENCES federation_gateway_spaces(space) ON DELETE RESTRICT,
    keys JSONB NOT NULL,
    policy JSONB NOT NULL
);
ALTER TABLE federation_accepted ADD COLUMN publication_policy JSONB NOT NULL DEFAULT '{"external_models":[]}';
CREATE FUNCTION federation_freeze_policy() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    NEW.publication_policy := COALESCE((SELECT policy FROM federation_security WHERE space=NEW.space), '{"external_models":[]}'::jsonb);
    RETURN NEW;
END $$;
CREATE TRIGGER federation_freeze_policy BEFORE INSERT ON federation_accepted
FOR EACH ROW EXECUTE FUNCTION federation_freeze_policy();
CREATE TABLE federation_audit (
    space UUID NOT NULL,
    sequence BIGINT NOT NULL,
    receipt JSONB NOT NULL,
    PRIMARY KEY (space, sequence),
    FOREIGN KEY (space, sequence) REFERENCES federation_accepted(space, sequence) ON DELETE RESTRICT
);
CREATE TRIGGER federation_audit_immutable BEFORE UPDATE OR DELETE ON federation_audit
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
CREATE TABLE federation_replica_receipts (
    space UUID NOT NULL,
    receiver UUID NOT NULL,
    sequence BIGINT NOT NULL,
    receipt JSONB NOT NULL,
    PRIMARY KEY (space, receiver, sequence),
    FOREIGN KEY (space, receiver, sequence) REFERENCES federation_inbox(space, receiver, sequence) ON DELETE RESTRICT
);
CREATE TRIGGER federation_replica_receipts_immutable BEFORE UPDATE OR DELETE ON federation_replica_receipts
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
CREATE TRIGGER federation_inbox_immutable BEFORE UPDATE OR DELETE ON federation_inbox
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
CREATE TRIGGER federation_review_immutable BEFORE UPDATE OR DELETE ON federation_publication_reviews
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
CREATE TRIGGER federation_source_intent_retained BEFORE DELETE ON federation_source_outbox
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
CREATE TRIGGER federation_source_change_retained BEFORE DELETE ON federation_source_changes
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
CREATE TRIGGER federation_publish_intent_retained BEFORE DELETE ON federation_publish_outbox
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
CREATE TABLE federation_replica_trust (
    space UUID NOT NULL, receiver UUID NOT NULL, keys JSONB NOT NULL,
    PRIMARY KEY (space, receiver),
    FOREIGN KEY (space, receiver) REFERENCES federation_replicas(space,receiver) ON DELETE RESTRICT
);
-- Decisions are auditable, never a deletion job or a claim of legal compliance.
CREATE TABLE federation_retention_decisions (
    space UUID NOT NULL REFERENCES federation_gateway_spaces(space) ON DELETE RESTRICT,
    decision BIGINT NOT NULL CHECK (decision > 0),
    actor UUID NOT NULL,
    action TEXT NOT NULL CHECK (action IN ('hold','release_hold','request_erasure')),
    reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 1024),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY (space,decision)
);
CREATE TRIGGER federation_retention_immutable BEFORE UPDATE OR DELETE ON federation_retention_decisions
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
