CREATE TABLE IF NOT EXISTS graph_extraction_runs (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    repo_root TEXT NOT NULL,
    git_head TEXT,
    since_marker TEXT,
    analyzer_version TEXT NOT NULL,
    strategy_version TEXT NOT NULL,
    status TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    summary_json JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_graph_extraction_runs_project_completed
    ON graph_extraction_runs (project_id, completed_at DESC NULLS LAST, started_at DESC);

CREATE INDEX IF NOT EXISTS idx_graph_extraction_runs_identity
    ON graph_extraction_runs (
        project_id,
        repo_root,
        COALESCE(git_head, ''),
        COALESCE(since_marker, ''),
        analyzer_version,
        strategy_version,
        status
    );

CREATE TABLE IF NOT EXISTS graph_nodes (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    extraction_run_id UUID NOT NULL REFERENCES graph_extraction_runs(id) ON DELETE CASCADE,
    node_kind TEXT NOT NULL,
    stable_identity TEXT NOT NULL,
    display_name TEXT NOT NULL,
    metadata_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (extraction_run_id, stable_identity)
);

CREATE INDEX IF NOT EXISTS idx_graph_nodes_project_kind
    ON graph_nodes (project_id, node_kind);

CREATE TABLE IF NOT EXISTS graph_edges (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    extraction_run_id UUID NOT NULL REFERENCES graph_extraction_runs(id) ON DELETE CASCADE,
    source_node_id UUID NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    target_node_id UUID NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    edge_kind TEXT NOT NULL,
    confidence REAL NOT NULL,
    metadata_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_graph_edges_project_kind
    ON graph_edges (project_id, edge_kind);

CREATE INDEX IF NOT EXISTS idx_graph_edges_source
    ON graph_edges (source_node_id);

CREATE INDEX IF NOT EXISTS idx_graph_edges_target
    ON graph_edges (target_node_id);

CREATE TABLE IF NOT EXISTS graph_evidence (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    extraction_run_id UUID NOT NULL REFERENCES graph_extraction_runs(id) ON DELETE CASCADE,
    node_id UUID REFERENCES graph_nodes(id) ON DELETE CASCADE,
    edge_id UUID REFERENCES graph_edges(id) ON DELETE CASCADE,
    evidence_kind TEXT NOT NULL,
    file_path TEXT NOT NULL,
    start_line BIGINT NOT NULL,
    end_line BIGINT NOT NULL,
    confidence REAL NOT NULL,
    strategy_version TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (node_id IS NOT NULL AND edge_id IS NULL)
        OR (node_id IS NULL AND edge_id IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_graph_evidence_run
    ON graph_evidence (extraction_run_id);

CREATE TABLE IF NOT EXISTS code_symbols (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    extraction_run_id UUID NOT NULL REFERENCES graph_extraction_runs(id) ON DELETE CASCADE,
    graph_node_id UUID NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
    fact_id TEXT NOT NULL,
    stable_identity TEXT NOT NULL,
    language TEXT NOT NULL,
    file_path TEXT NOT NULL,
    symbol_kind TEXT NOT NULL,
    name TEXT NOT NULL,
    qualified_name TEXT,
    start_byte BIGINT NOT NULL,
    end_byte BIGINT NOT NULL,
    start_line BIGINT NOT NULL,
    end_line BIGINT NOT NULL,
    display_name TEXT NOT NULL,
    source_hash TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (extraction_run_id, stable_identity)
);

CREATE INDEX IF NOT EXISTS idx_code_symbols_project_identity
    ON code_symbols (project_id, stable_identity);

CREATE INDEX IF NOT EXISTS idx_code_symbols_project_file
    ON code_symbols (project_id, file_path);

CREATE TABLE IF NOT EXISTS code_references (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    extraction_run_id UUID NOT NULL REFERENCES graph_extraction_runs(id) ON DELETE CASCADE,
    graph_edge_id UUID REFERENCES graph_edges(id) ON DELETE SET NULL,
    fact_id TEXT NOT NULL,
    reference_kind TEXT NOT NULL,
    language TEXT NOT NULL,
    file_path TEXT NOT NULL,
    source_symbol_identity TEXT,
    target_symbol_identity TEXT,
    source_text TEXT,
    target_text TEXT NOT NULL,
    resolution_status TEXT NOT NULL,
    confidence REAL NOT NULL,
    start_byte BIGINT NOT NULL,
    end_byte BIGINT NOT NULL,
    start_line BIGINT NOT NULL,
    end_line BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_code_references_run_status
    ON code_references (extraction_run_id, resolution_status);

CREATE INDEX IF NOT EXISTS idx_code_references_project_file
    ON code_references (project_id, file_path);
