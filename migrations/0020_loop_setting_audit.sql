CREATE TABLE IF NOT EXISTS loop_setting_audit (
    id UUID PRIMARY KEY,
    loop_id TEXT NULL,
    setting_id UUID NULL REFERENCES loop_settings(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    scope_type TEXT NULL,
    scope_id TEXT NULL,
    repo_root TEXT NULL,
    actor TEXT NULL,
    reason TEXT NULL,
    payload_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_loop_setting_audit_loop_created
    ON loop_setting_audit (loop_id, created_at DESC)
    WHERE loop_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_loop_setting_audit_setting_created
    ON loop_setting_audit (setting_id, created_at DESC)
    WHERE setting_id IS NOT NULL;
