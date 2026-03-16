CREATE TABLE IF NOT EXISTS projects (
    id UUID PRIMARY KEY,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    root_path TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS sessions (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    external_session_id TEXT NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    ended_at TIMESTAMPTZ,
    agent_name TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS tasks (
    id UUID PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    user_prompt TEXT NOT NULL,
    task_summary TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    completed_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS raw_captures (
    id UUID PRIMARY KEY,
    task_id UUID NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    capture_type TEXT NOT NULL,
    payload_json JSONB NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL,
    curated_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS memory_entries (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    canonical_text TEXT NOT NULL,
    summary TEXT NOT NULL,
    memory_type TEXT NOT NULL,
    scope TEXT NOT NULL,
    importance INT NOT NULL,
    confidence REAL NOT NULL,
    status TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    archived_at TIMESTAMPTZ,
    search_document tsvector NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_entries_project_status ON memory_entries(project_id, status);
CREATE INDEX IF NOT EXISTS idx_memory_entries_search_document ON memory_entries USING GIN(search_document);

CREATE TABLE IF NOT EXISTS memory_sources (
    id UUID PRIMARY KEY,
    memory_entry_id UUID NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
    task_id UUID REFERENCES tasks(id) ON DELETE SET NULL,
    file_path TEXT,
    git_commit TEXT,
    source_kind TEXT NOT NULL,
    excerpt TEXT,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE IF NOT EXISTS memory_relations (
    id UUID PRIMARY KEY,
    src_memory_id UUID NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL,
    dst_memory_id UUID NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS memory_tags (
    memory_entry_id UUID NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
    tag TEXT NOT NULL,
    PRIMARY KEY (memory_entry_id, tag)
);

CREATE TABLE IF NOT EXISTS memory_chunks (
    id UUID PRIMARY KEY,
    memory_entry_id UUID NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
    chunk_text TEXT NOT NULL,
    search_text TEXT NOT NULL,
    tsv tsvector NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_memory_chunks_tsv ON memory_chunks USING GIN(tsv);

CREATE TABLE IF NOT EXISTS curation_runs (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    trigger_type TEXT NOT NULL,
    input_count BIGINT NOT NULL,
    output_count BIGINT NOT NULL,
    model_name TEXT,
    created_at TIMESTAMPTZ NOT NULL
);
