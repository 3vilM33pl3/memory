-- Trigger intake is an operational journal, not evidence of completed dispatch.
-- Historical rows retain NULL attribution: do not invent an authenticated actor.
ALTER TABLE trigger_events ADD COLUMN actor_id TEXT NULL;
ALTER TABLE trigger_events ADD COLUMN actor_source TEXT NULL;
