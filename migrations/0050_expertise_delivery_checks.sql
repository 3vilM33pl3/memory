-- One fresh, addressed origin check for each retained-answer release. No
-- reusable authorization cache and no additional private payload copies.
CREATE TABLE federation_expertise_deliveries (
    space UUID NOT NULL,
    nonce UUID NOT NULL,
    requester UUID NOT NULL,
    responder UUID NOT NULL,
    request_id UUID NOT NULL,
    request_digest JSONB NOT NULL,
    answer_digest JSONB NOT NULL,
    policy_digest JSONB NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending','claimed','released','denied','expired')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    expires_at TIMESTAMPTZ NOT NULL,
    metadata_until TIMESTAMPTZ NOT NULL,
    lease_token UUID,
    PRIMARY KEY(space,nonce),
    FOREIGN KEY(space,requester,request_id) REFERENCES federation_expertise_mailbox(space,requester,request_id) ON DELETE RESTRICT
);
CREATE INDEX expertise_delivery_pending ON federation_expertise_deliveries(space,responder,created_at,nonce) WHERE status='pending';
CREATE UNIQUE INDEX expertise_delivery_single_claim ON federation_expertise_deliveries(space,responder) WHERE status='claimed';
ALTER TABLE federation_expertise_attempts DROP CONSTRAINT federation_expertise_attempts_action_check;
ALTER TABLE federation_expertise_attempts ADD CONSTRAINT federation_expertise_attempts_action_check CHECK(action IN (
    'queued','claimed','renewed','provider_started','answered','refused','denied','expired','failed','uncertain','recovered',
    'delivery_requested','delivery_claimed','delivery_released','delivery_denied'
));
