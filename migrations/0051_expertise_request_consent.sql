-- Origin-private consent, never gateway/PDS/timeline or retrieval context.
CREATE TABLE federation_expertise_requests (
    project_id UUID NOT NULL,
    space UUID NOT NULL,
    requester UUID NOT NULL,
    request_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    request_digest JSONB NOT NULL,
    review_digest JSONB NOT NULL,
    advertisement_digest JSONB NOT NULL,
    question JSONB CHECK (octet_length(question::text) <= 32768),
    confirmed_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    expires_at TIMESTAMPTZ NOT NULL,
    payload_until TIMESTAMPTZ NOT NULL,
    metadata_until TIMESTAMPTZ NOT NULL,
    hold BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY(space,requester,request_id)
);
CREATE TABLE federation_expertise_request_audit (
    project_id UUID NOT NULL,
    space UUID NOT NULL,
    requester UUID NOT NULL,
    request_id UUID NOT NULL,
    principal_id UUID NOT NULL,
    review_digest JSONB NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    PRIMARY KEY(space,requester,request_id),
    FOREIGN KEY(space,requester,request_id)
        REFERENCES federation_expertise_requests(space,requester,request_id)
);
