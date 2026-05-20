ALTER TABLE memory_sources
    ADD COLUMN IF NOT EXISTS symbol_name TEXT,
    ADD COLUMN IF NOT EXISTS symbol_kind TEXT;

CREATE INDEX IF NOT EXISTS idx_memory_sources_symbol
    ON memory_sources (memory_entry_id, symbol_name)
    WHERE symbol_name IS NOT NULL;
