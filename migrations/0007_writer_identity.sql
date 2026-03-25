ALTER TABLE sessions
    RENAME COLUMN agent_id TO writer_id;

ALTER TABLE sessions
    RENAME COLUMN agent_name TO writer_name;

ALTER TABLE sessions
    ALTER COLUMN writer_id SET NOT NULL;

ALTER TABLE sessions
    ALTER COLUMN writer_name SET NOT NULL;
