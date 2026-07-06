-- Graph retrieval latency: fetch_graph_candidates filters code_references and
-- code_symbols with leading-wildcard ILIKE ('%term%'), which btree indexes
-- cannot serve — PostgreSQL seq-scans the tables on every graph query
-- (160k+ code_references on a large project). Trigram GIN indexes serve
-- ILIKE '%...%' directly. pg_trgm is a trusted contrib module.

CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE INDEX IF NOT EXISTS idx_code_references_target_text_trgm
    ON code_references USING gin (target_text gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_code_references_source_text_trgm
    ON code_references USING gin (source_text gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_code_references_file_path_trgm
    ON code_references USING gin (file_path gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_code_symbols_name_trgm
    ON code_symbols USING gin (name gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_code_symbols_qualified_name_trgm
    ON code_symbols USING gin (qualified_name gin_trgm_ops);
CREATE INDEX IF NOT EXISTS idx_code_symbols_file_path_trgm
    ON code_symbols USING gin (file_path gin_trgm_ops);
