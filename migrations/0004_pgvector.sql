CREATE EXTENSION IF NOT EXISTS vector;

DROP INDEX IF EXISTS idx_memory_chunks_embedding_hnsw;

ALTER TABLE memory_chunks
    DROP COLUMN IF EXISTS embedding;

ALTER TABLE memory_chunks
    ADD COLUMN IF NOT EXISTS embedding vector;

-- pgvector's HNSW indexes require a fixed vector dimension. The legacy
-- memory_chunks.embedding column is intentionally dimensionless because older
-- installations may have mixed embedding providers. Multi-space embeddings are
-- stored in memory_chunk_embeddings by later migrations, so fresh databases
-- must not create a dimension-specific index here.
