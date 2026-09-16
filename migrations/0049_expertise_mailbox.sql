-- Private operational Q&A; never an AT repository publication.
CREATE TABLE federation_expertise_mailbox (
    space UUID NOT NULL REFERENCES federation_gateway_spaces(space) ON DELETE RESTRICT,
    requester UUID NOT NULL, responder UUID NOT NULL, request_id UUID NOT NULL,
    request_digest JSONB NOT NULL, binding JSONB NOT NULL,
    question JSONB, answer JSONB, policy_digest JSONB NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('queued','running','answered','refused','denied','expired','failed','uncertain')),
    created_at TIMESTAMPTZ NOT NULL, expires_at TIMESTAMPTZ NOT NULL,
    payload_until TIMESTAMPTZ NOT NULL, metadata_until TIMESTAMPTZ NOT NULL,
    lease_token UUID, lease_until TIMESTAMPTZ, provider_started BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY(space,requester,request_id),
    CHECK(expires_at > created_at AND expires_at <= created_at + interval '120 seconds'),
    CHECK((lease_token IS NULL) = (lease_until IS NULL)),
    CHECK(status='running' OR lease_token IS NULL),
    CHECK(question IS NULL OR octet_length(question::text)<=16384),
    CHECK(answer IS NULL OR octet_length(answer::text)<=131072)
);
CREATE UNIQUE INDEX expertise_one_active_answer ON federation_expertise_mailbox(space,responder) WHERE status='running';
CREATE INDEX expertise_mailbox_ready ON federation_expertise_mailbox(space,responder,created_at,request_id) WHERE status='queued';
CREATE TABLE federation_expertise_attempts (
    id UUID PRIMARY KEY, space UUID NOT NULL, requester UUID NOT NULL,
    responder UUID NOT NULL, request_id UUID NOT NULL,
    action TEXT NOT NULL CHECK(action IN ('queued','claimed','renewed','provider_started','answered','refused','denied','expired','failed','uncertain','recovered')),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(), metadata_until TIMESTAMPTZ NOT NULL
);
CREATE TABLE federation_expertise_daily_budget (
    space UUID NOT NULL, responder UUID NOT NULL, day DATE NOT NULL,
    calls BIGINT NOT NULL DEFAULT 0, reserved_tokens BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY(space,responder,day)
);
