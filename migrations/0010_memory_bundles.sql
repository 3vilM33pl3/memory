CREATE TABLE IF NOT EXISTS memory_bundle_imports (
    id UUID PRIMARY KEY,
    target_project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    bundle_id TEXT NOT NULL,
    bundle_hash TEXT NOT NULL,
    source_project_slug TEXT NOT NULL,
    summary TEXT NOT NULL,
    options_json JSONB NOT NULL,
    imported_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_memory_bundle_imports_project
    ON memory_bundle_imports (target_project_id, imported_at DESC);

CREATE TABLE IF NOT EXISTS imported_memory_entries (
    target_project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    bundle_id TEXT NOT NULL,
    exported_entry_key TEXT NOT NULL,
    entry_hash TEXT NOT NULL,
    memory_entry_id UUID NOT NULL REFERENCES memory_entries(id) ON DELETE CASCADE,
    latest_import_id UUID NOT NULL REFERENCES memory_bundle_imports(id) ON DELETE CASCADE,
    imported_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (target_project_id, bundle_id, exported_entry_key)
);

CREATE INDEX IF NOT EXISTS idx_imported_memory_entries_memory
    ON imported_memory_entries (memory_entry_id);
