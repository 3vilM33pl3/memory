-- Federation is opt-in: no existing project is enrolled or queued here.
-- Explicit counters under row locks establish commit order; never BIGSERIAL.
CREATE TABLE federation_gateway_spaces (
    space UUID PRIMARY KEY,
    authority UUID NOT NULL,
    writer_epoch UUID NOT NULL,
    accepted_sequence BIGINT NOT NULL DEFAULT 0 CHECK (accepted_sequence BETWEEN 0 AND 4294967295),
    published_sequence BIGINT NOT NULL DEFAULT 0 CHECK (published_sequence BETWEEN 0 AND accepted_sequence),
    projection JSONB NOT NULL
);

CREATE TABLE federation_source_outbox (
    space UUID NOT NULL,
    publisher UUID NOT NULL,
    record_id UUID NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
    locator TEXT NOT NULL,
    envelope JSONB NOT NULL,
    queued_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    acceptance JSONB,
    PRIMARY KEY (space, publisher, record_id),
    UNIQUE (space, publisher, locator)
);

CREATE TABLE federation_accepted (
    space UUID NOT NULL REFERENCES federation_gateway_spaces(space) ON DELETE RESTRICT,
    sequence BIGINT NOT NULL CHECK (sequence BETWEEN 1 AND 4294967295),
    publisher UUID NOT NULL,
    record_id UUID NOT NULL,
    locator TEXT NOT NULL,
    delivery JSONB NOT NULL,
    PRIMARY KEY (space, sequence),
    UNIQUE (space, publisher, record_id),
    UNIQUE (space, publisher, locator)
);

CREATE FUNCTION federation_accepted_immutable() RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'accepted federation history is immutable';
END;
$$;
CREATE TRIGGER federation_accepted_immutable BEFORE UPDATE OR DELETE ON federation_accepted
FOR EACH ROW EXECUTE FUNCTION federation_accepted_immutable();

CREATE TABLE federation_publish_outbox (
    space UUID NOT NULL,
    sequence BIGINT NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    lease_token UUID,
    lease_until TIMESTAMPTZ,
    available_at TIMESTAMPTZ NOT NULL DEFAULT clock_timestamp(),
    last_error TEXT,
    blocked BOOLEAN NOT NULL DEFAULT FALSE,
    published BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (space, sequence),
    FOREIGN KEY (space, sequence) REFERENCES federation_accepted(space, sequence) ON DELETE RESTRICT,
    CHECK ((lease_token IS NULL) = (lease_until IS NULL))
);

CREATE TABLE federation_replicas (
    space UUID NOT NULL,
    receiver UUID NOT NULL,
    authority UUID NOT NULL,
    cursor BIGINT NOT NULL DEFAULT 0 CHECK (cursor BETWEEN 0 AND 4294967295),
    projection JSONB NOT NULL,
    PRIMARY KEY (space, receiver)
);
CREATE TABLE federation_inbox (
    space UUID NOT NULL,
    receiver UUID NOT NULL,
    sequence BIGINT NOT NULL CHECK (sequence BETWEEN 1 AND 4294967295),
    delivery JSONB NOT NULL,
    PRIMARY KEY (space, receiver, sequence),
    FOREIGN KEY (space, receiver) REFERENCES federation_replicas(space, receiver) ON DELETE RESTRICT
);
