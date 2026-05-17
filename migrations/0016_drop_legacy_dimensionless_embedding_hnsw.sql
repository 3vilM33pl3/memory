-- pgvector's HNSW indexes require a fixed vector dimension. The legacy
-- memory_chunks.embedding column is intentionally dimensionless because older
-- installations may have mixed embedding providers. Multi-space embeddings are
-- stored in memory_chunk_embeddings, so remove the legacy dimension-specific
-- index if an older database created it.
DROP INDEX IF EXISTS idx_memory_chunks_embedding_hnsw;
