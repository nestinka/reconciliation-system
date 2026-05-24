CREATE EXTENSION IF NOT EXISTS citext;

-- memberships: per-tenant role for a global user
CREATE TABLE memberships (
    user_id   TEXT NOT NULL REFERENCES users(id),
    tenant_id TEXT NOT NULL REFERENCES tenants(id),
    role      TEXT NOT NULL CHECK (role IN ('operator','approver','admin')),
    PRIMARY KEY (user_id, tenant_id)
);

-- Backfill memberships from existing users.tenant_id/role, then drop those columns.
INSERT INTO memberships (user_id, tenant_id, role)
SELECT id, tenant_id, role FROM users;

-- users becomes a global identity
ALTER TABLE users ADD COLUMN email CITEXT;
ALTER TABLE users ADD COLUMN disabled BOOLEAN NOT NULL DEFAULT FALSE;
UPDATE users SET email = lower(replace(id,'user-','')) || '@example.com' WHERE email IS NULL;
ALTER TABLE users ALTER COLUMN email SET NOT NULL;
ALTER TABLE users ADD CONSTRAINT users_email_unique UNIQUE (email);
ALTER TABLE users DROP COLUMN tenant_id;
ALTER TABLE users DROP COLUMN role;

CREATE TABLE user_credentials (
    user_id            TEXT PRIMARY KEY REFERENCES users(id),
    password_hash      TEXT NOT NULL,
    password_updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    failed_attempts    INT NOT NULL DEFAULT 0,
    locked_until       TIMESTAMPTZ
);

CREATE TABLE refresh_tokens (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users(id),
    tenant_id    TEXT NOT NULL REFERENCES tenants(id),
    token_hash   TEXT NOT NULL UNIQUE,
    expires_at   TIMESTAMPTZ NOT NULL,
    revoked_at   TIMESTAMPTZ,
    rotated_from TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_refresh_user ON refresh_tokens(user_id);

CREATE TABLE password_reset_tokens (
    id         TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id),
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at    TIMESTAMPTZ
);
