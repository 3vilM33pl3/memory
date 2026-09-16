-- Private recoverable deterministic results, never retrieval context.
CREATE TABLE federation_expertise_origin_results (
    project_id UUID NOT NULL,
    space UUID NOT NULL,
    responder UUID NOT NULL,
    requester UUID NOT NULL,
    request_id UUID NOT NULL,
    request_digest JSONB NOT NULL,
    answer_digest JSONB NOT NULL,
    answer JSONB CHECK (octet_length(answer::text) <= 131072),
    lease_token UUID NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    payload_until TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()+interval '24 hours',
    metadata_until TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp()+interval '30 days',
    hold BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY(space,responder,requester,request_id)
);
