CREATE EXTENSION IF NOT EXISTS vector;

DROP INDEX IF EXISTS idx_memory_chunks_embedding_hnsw;

ALTER TABLE memory_chunks
    DROP COLUMN IF EXISTS embedding;

ALTER TABLE memory_chunks
    ADD COLUMN IF NOT EXISTS embedding vector;

CREATE INDEX IF NOT EXISTS idx_memory_chunks_embedding_hnsw
    ON memory_chunks USING hnsw (embedding vector_cosine_ops);
