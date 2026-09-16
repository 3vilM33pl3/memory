-- Local HTTP idempotency, not a gateway table or publication ordering source.
CREATE TABLE federation_review_requests (
    space UUID NOT NULL,
    instance UUID NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    principal_id UUID NOT NULL REFERENCES auth_principals(id) ON DELETE RESTRICT,
    request_id UUID NOT NULL CHECK(request_id <> '00000000-0000-0000-0000-000000000000'),
    request_digest JSONB NOT NULL,
    review_id UUID NOT NULL REFERENCES federation_publication_reviews(id) ON DELETE RESTRICT,
    PRIMARY KEY(space, instance, project_id, principal_id, request_id)
);
CREATE TRIGGER federation_review_requests_immutable
BEFORE UPDATE OR DELETE ON federation_review_requests
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();
