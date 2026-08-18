CREATE TABLE IF NOT EXISTS auth_principals (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('human_oidc', 'service_token', 'legacy_service_token', 'internal')),
    issuer TEXT,
    subject TEXT,
    display_name TEXT NOT NULL,
    email TEXT,
    groups_json JSONB NOT NULL DEFAULT '[]'::jsonb,
    global_role TEXT CHECK (global_role IS NULL OR global_role IN ('reader', 'writer', 'operator', 'admin')),
    disabled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((issuer IS NULL AND subject IS NULL) OR (issuer IS NOT NULL AND subject IS NOT NULL))
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_auth_principals_oidc_subject
    ON auth_principals(issuer, subject)
    WHERE issuer IS NOT NULL AND subject IS NOT NULL;

CREATE TABLE IF NOT EXISTS auth_service_tokens (
    id UUID PRIMARY KEY,
    principal_id UUID NOT NULL REFERENCES auth_principals(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_hash BYTEA NOT NULL UNIQUE,
    token_prefix TEXT NOT NULL,
    created_by UUID REFERENCES auth_principals(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_auth_service_tokens_principal
    ON auth_service_tokens(principal_id);
CREATE INDEX IF NOT EXISTS idx_auth_service_tokens_active
    ON auth_service_tokens(token_hash)
    WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS auth_project_memberships (
    id UUID PRIMARY KEY,
    principal_id UUID NOT NULL REFERENCES auth_principals(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('reader', 'writer', 'operator', 'admin')),
    source TEXT NOT NULL DEFAULT 'direct' CHECK (source IN ('direct', 'bootstrap')),
    created_by UUID REFERENCES auth_principals(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(principal_id, project_id)
);

CREATE INDEX IF NOT EXISTS idx_auth_project_memberships_project
    ON auth_project_memberships(project_id, role);

CREATE TABLE IF NOT EXISTS auth_web_sessions (
    id UUID PRIMARY KEY,
    principal_id UUID NOT NULL REFERENCES auth_principals(id) ON DELETE CASCADE,
    session_hash BYTEA NOT NULL UNIQUE,
    csrf_hash BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_auth_web_sessions_principal
    ON auth_web_sessions(principal_id);
CREATE INDEX IF NOT EXISTS idx_auth_web_sessions_active
    ON auth_web_sessions(session_hash, expires_at)
    WHERE revoked_at IS NULL;

CREATE TABLE IF NOT EXISTS auth_oidc_flows (
    id UUID PRIMARY KEY,
    state_hash BYTEA NOT NULL UNIQUE,
    nonce TEXT NOT NULL,
    pkce_verifier TEXT NOT NULL,
    return_to TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_auth_oidc_flows_expiry
    ON auth_oidc_flows(expires_at);

CREATE TABLE IF NOT EXISTS auth_audit_events (
    id UUID PRIMARY KEY,
    actor_principal_id UUID REFERENCES auth_principals(id) ON DELETE SET NULL,
    event_type TEXT NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('success', 'denied', 'error')),
    project_id UUID REFERENCES projects(id) ON DELETE SET NULL,
    target_principal_id UUID REFERENCES auth_principals(id) ON DELETE SET NULL,
    target_token_id UUID REFERENCES auth_service_tokens(id) ON DELETE SET NULL,
    request_id TEXT,
    metadata_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_auth_audit_events_recorded
    ON auth_audit_events(recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_auth_audit_events_actor
    ON auth_audit_events(actor_principal_id, recorded_at DESC);
