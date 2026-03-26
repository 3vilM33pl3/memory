ALTER TABLE memory_chunks
    ADD COLUMN IF NOT EXISTS embedding_provider TEXT,
    ADD COLUMN IF NOT EXISTS embedding_base_url TEXT,
    ADD COLUMN IF NOT EXISTS embedding_dimension INTEGER,
    ADD COLUMN IF NOT EXISTS embedding_space TEXT;

CREATE INDEX IF NOT EXISTS idx_memory_chunks_embedding_space
    ON memory_chunks (embedding_space)
    WHERE embedding IS NOT NULL;

