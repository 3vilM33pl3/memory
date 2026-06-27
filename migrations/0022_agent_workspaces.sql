CREATE TABLE IF NOT EXISTS agent_workspaces (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    repo_root TEXT NOT NULL,
    worktree_path TEXT NOT NULL,
    branch TEXT NOT NULL,
    task TEXT,
    base_commit TEXT,
    head_commit TEXT,
    dirty_files TEXT[] NOT NULL DEFAULT '{}',
    agent_cli TEXT NOT NULL,
    agent_session_id TEXT,
    agent_session_key TEXT NOT NULL DEFAULT '',
    hostname TEXT,
    writer_id TEXT,
    profile TEXT,
    service_endpoint TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'active',
    finish_summary TEXT,
    pushed_branch BOOLEAN,
    merged_commit TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, repo_root, branch, agent_session_key)
);

CREATE INDEX IF NOT EXISTS idx_agent_workspaces_project_status
    ON agent_workspaces (project_id, status, last_heartbeat_at DESC);

CREATE INDEX IF NOT EXISTS idx_agent_workspaces_project_branch
    ON agent_workspaces (project_id, branch);

CREATE INDEX IF NOT EXISTS idx_agent_workspaces_worktree
    ON agent_workspaces (worktree_path);
