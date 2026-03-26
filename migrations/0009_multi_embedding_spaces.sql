CREATE TABLE IF NOT EXISTS memory_chunk_embeddings (
    chunk_id UUID NOT NULL REFERENCES memory_chunks(id) ON DELETE CASCADE,
    embedding_space TEXT NOT NULL,
    embedding vector NOT NULL,
    embedding_provider TEXT,
    embedding_base_url TEXT,
    embedding_model TEXT,
    embedding_dimension INTEGER,
    embedding_updated_at TIMESTAMPTZ,
    PRIMARY KEY (chunk_id, embedding_space)
);

CREATE INDEX IF NOT EXISTS idx_memory_chunk_embeddings_space
    ON memory_chunk_embeddings (embedding_space);

INSERT INTO memory_chunk_embeddings (
    chunk_id,
    embedding_space,
    embedding,
    embedding_provider,
    embedding_base_url,
    embedding_model,
    embedding_dimension,
    embedding_updated_at
)
SELECT
    mc.id,
    COALESCE(
        mc.embedding_space,
        CASE
            WHEN mc.embedding_provider IS NOT NULL
             AND mc.embedding_base_url IS NOT NULL
             AND mc.embedding_model IS NOT NULL
            THEN concat(mc.embedding_provider, '|', rtrim(mc.embedding_base_url, '/'), '|', mc.embedding_model)
            ELSE 'legacy|unknown'
        END
    ) AS embedding_space,
    mc.embedding,
    mc.embedding_provider,
    mc.embedding_base_url,
    mc.embedding_model,
    mc.embedding_dimension,
    mc.embedding_updated_at
FROM memory_chunks mc
WHERE mc.embedding IS NOT NULL
ON CONFLICT (chunk_id, embedding_space) DO UPDATE
SET embedding = EXCLUDED.embedding,
    embedding_provider = EXCLUDED.embedding_provider,
    embedding_base_url = EXCLUDED.embedding_base_url,
    embedding_model = EXCLUDED.embedding_model,
    embedding_dimension = EXCLUDED.embedding_dimension,
    embedding_updated_at = EXCLUDED.embedding_updated_at;
