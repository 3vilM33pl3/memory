-- R1: private operational intent/outcome journal. Never expose these resource
-- locators through project timelines or background-job listings.
CREATE TABLE external_effects (
    id UUID PRIMARY KEY,
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    kind TEXT NOT NULL CHECK (kind IN ('file_replace','provider_request','watcher_restart','workspace_create','export_prepare')),
    host_id TEXT NOT NULL CHECK (length(host_id) BETWEEN 1 AND 256),
    resource TEXT NOT NULL CHECK (length(resource) BETWEEN 1 AND 4096),
    input_fingerprint TEXT NOT NULL CHECK (input_fingerprint ~ '^[0-9a-f]{64}$'),
    actor_id TEXT NOT NULL CHECK (length(actor_id) BETWEEN 1 AND 256),
    source TEXT NOT NULL CHECK (length(source) BETWEEN 1 AND 80),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','confirmed','failed','uncertain')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    reconcile_after TIMESTAMPTZ NOT NULL DEFAULT now() + INTERVAL '10 minutes',
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE external_effect_outcomes (
    id UUID PRIMARY KEY,
    effect_id UUID NOT NULL REFERENCES external_effects(id),
    status TEXT NOT NULL CHECK (status IN ('confirmed','failed','uncertain')),
    reason TEXT NOT NULL CHECK (reason IN ('observed','local_io','remote_io','interrupted','mismatch','denied')),
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(effect_id,id)
);
CREATE INDEX external_effect_recovery ON external_effects(host_id,reconcile_after,id)
    WHERE status='pending';
CREATE FUNCTION preserve_external_effect_intent() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF ROW(NEW.id,NEW.kind,NEW.host_id,NEW.resource,NEW.input_fingerprint,NEW.actor_id,NEW.source,NEW.created_at)
       IS DISTINCT FROM ROW(OLD.id,OLD.kind,OLD.host_id,OLD.resource,OLD.input_fingerprint,OLD.actor_id,OLD.source,OLD.created_at) THEN
        RAISE EXCEPTION 'external effect intent is immutable';
    END IF;
    RETURN NEW;
END $$;
CREATE TRIGGER immutable_external_effect_intent BEFORE UPDATE ON external_effects
    FOR EACH ROW EXECUTE FUNCTION preserve_external_effect_intent();
