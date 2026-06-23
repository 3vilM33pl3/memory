CREATE INDEX IF NOT EXISTS idx_code_symbols_run_file
    ON code_symbols (extraction_run_id, file_path);

CREATE INDEX IF NOT EXISTS idx_code_symbols_run_name
    ON code_symbols (extraction_run_id, name);

CREATE INDEX IF NOT EXISTS idx_code_symbols_run_qualified_name
    ON code_symbols (extraction_run_id, qualified_name);

CREATE INDEX IF NOT EXISTS idx_graph_edges_run_source
    ON graph_edges (extraction_run_id, source_node_id);

CREATE INDEX IF NOT EXISTS idx_graph_edges_run_target
    ON graph_edges (extraction_run_id, target_node_id);

CREATE INDEX IF NOT EXISTS idx_graph_edges_run_kind
    ON graph_edges (extraction_run_id, edge_kind);
