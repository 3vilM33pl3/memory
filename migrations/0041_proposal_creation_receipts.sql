-- R1: keep original creation identity separate from mutable reviewed proposals.
CREATE TABLE proposal_creation_receipts (
    operation_id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    proposal_id UUID NOT NULL UNIQUE REFERENCES memory_proposals(id) ON DELETE CASCADE,
    input_digest TEXT NOT NULL CHECK (input_digest ~ '^[0-9a-f]{64}$'),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
