ALTER TABLE sessions
    ADD COLUMN IF NOT EXISTS agent_id TEXT;

UPDATE sessions
SET agent_id = COALESCE(NULLIF(agent_name, ''), external_session_id, 'unknown-agent')
WHERE agent_id IS NULL;

ALTER TABLE sessions
    ALTER COLUMN agent_id SET NOT NULL;
