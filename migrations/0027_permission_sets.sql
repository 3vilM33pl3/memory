-- Permission sets replace the ordinal role ladder as the authorization
-- model. Role names survive as named presets; a membership or principal may
-- additionally carry an explicit permissions_json array of permission names
-- that overrides the preset expansion ('custom' as a role name is reserved
-- for grants that are only defined by their permission list).
ALTER TABLE auth_project_memberships DROP CONSTRAINT IF EXISTS auth_project_memberships_role_check;
ALTER TABLE auth_project_memberships
    ADD CONSTRAINT auth_project_memberships_role_check
    CHECK (role IN ('reader', 'writer', 'operator', 'admin', 'custom'));
ALTER TABLE auth_project_memberships ADD COLUMN IF NOT EXISTS permissions_json JSONB;

ALTER TABLE auth_principals DROP CONSTRAINT IF EXISTS auth_principals_global_role_check;
ALTER TABLE auth_principals
    ADD CONSTRAINT auth_principals_global_role_check
    CHECK (global_role IS NULL OR global_role IN ('reader', 'writer', 'operator', 'admin', 'custom'));
ALTER TABLE auth_principals ADD COLUMN IF NOT EXISTS global_permissions_json JSONB;
