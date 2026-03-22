CREATE TABLE IF NOT EXISTS project_commits (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    commit_hash TEXT NOT NULL,
    short_hash TEXT NOT NULL,
    subject TEXT NOT NULL,
    body TEXT NOT NULL,
    author_name TEXT,
    author_email TEXT,
    committed_at TIMESTAMPTZ NOT NULL,
    parent_hashes TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    changed_paths TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    imported_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    search_document tsvector NOT NULL,
    UNIQUE (project_id, commit_hash)
);

CREATE INDEX IF NOT EXISTS idx_project_commits_project_committed_at
    ON project_commits (project_id, committed_at DESC);

CREATE INDEX IF NOT EXISTS idx_project_commits_search_document
    ON project_commits USING GIN (search_document);
