-- F1 origin-private reviewed snapshots. Contents never enter PDS publication.
CREATE TABLE federation_expertise_collections (
    space UUID NOT NULL,
    owner UUID NOT NULL,
    project_id UUID NOT NULL,
    collection UUID NOT NULL,
    revision BIGINT NOT NULL CHECK (revision BETWEEN 1 AND 4294967295),
    PRIMARY KEY(space,owner,project_id,collection)
);
CREATE TABLE federation_expertise_reviews (
    space UUID NOT NULL,
    owner UUID NOT NULL,
    project_id UUID NOT NULL,
    collection UUID NOT NULL,
    revision BIGINT NOT NULL CHECK (revision BETWEEN 1 AND 4294967295),
    principal_id UUID NOT NULL,
    request_id UUID NOT NULL,
    request_digest JSONB NOT NULL,
    snapshot JSONB NOT NULL CHECK (octet_length(snapshot::text) <= 2097152),
    advertisement JSONB NOT NULL CHECK (octet_length(advertisement::text) <= 65536),
    intent JSONB NOT NULL,
    active BOOLEAN NOT NULL,
    confirmed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(space,owner,project_id,collection,revision),
    UNIQUE(space,owner,project_id,principal_id,request_id)
);
CREATE TRIGGER expertise_reviews_immutable BEFORE UPDATE OR DELETE ON federation_expertise_reviews
    FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
CREATE TABLE federation_expertise_collection_grants (
    space UUID NOT NULL,
    owner UUID NOT NULL,
    project_id UUID NOT NULL,
    collection UUID NOT NULL,
    principal_id UUID NOT NULL,
    enabled BOOLEAN NOT NULL,
    PRIMARY KEY(space,owner,project_id,collection,principal_id)
);
-- Sanitized operational journal: never snapshot/question/provider content.
CREATE TABLE federation_expertise_collection_audit (
    id UUID PRIMARY KEY,
    space UUID NOT NULL,
    owner UUID NOT NULL,
    project_id UUID NOT NULL,
    collection UUID NOT NULL,
    actor UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN ('activate','withdraw','grant','revoke_grant')),
    revision BIGINT,
    subject UUID,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()
);
CREATE TRIGGER expertise_collection_audit_immutable BEFORE UPDATE OR DELETE ON federation_expertise_collection_audit
    FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
