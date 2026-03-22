ALTER TABLE memory_chunks
    ADD COLUMN IF NOT EXISTS embedding REAL[],
    ADD COLUMN IF NOT EXISTS embedding_model TEXT,
    ADD COLUMN IF NOT EXISTS embedding_updated_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_memory_relations_src ON memory_relations(src_memory_id);
CREATE INDEX IF NOT EXISTS idx_memory_relations_dst ON memory_relations(dst_memory_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_memory_relations_unique
    ON memory_relations(src_memory_id, relation_type, dst_memory_id);
