CREATE EXTENSION IF NOT EXISTS vector;

DROP INDEX IF EXISTS idx_memory_chunks_embedding_hnsw;

ALTER TABLE memory_chunks
    DROP COLUMN IF EXISTS embedding;

ALTER TABLE memory_chunks
    ADD COLUMN IF NOT EXISTS embedding vector;

-- Fresh installs no longer rely on the legacy memory_chunks embedding index.
-- Later migrations move embeddings into memory_chunk_embeddings, and creating
-- an HNSW index on a dimensionless vector column fails on current pgvector.
