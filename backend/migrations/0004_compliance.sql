-- Per-tenant hash-chained audit log.
CREATE TABLE audit_events (
  tenant_id  TEXT     NOT NULL REFERENCES tenants(id),
  seq        BIGINT   NOT NULL,
  at         TIMESTAMPTZ NOT NULL,
  actor_id   TEXT     NOT NULL,
  kind       TEXT     NOT NULL,
  payload    JSONB    NOT NULL,
  prev_hash  BYTEA    NOT NULL,
  hash       BYTEA    NOT NULL,
  PRIMARY KEY (tenant_id, seq)
);
CREATE INDEX idx_audit_tenant_at   ON audit_events(tenant_id, at);
CREATE INDEX idx_audit_tenant_kind ON audit_events(tenant_id, kind);

-- Global anchor chain: each anchor row hashes the current per-tenant heads + the
-- previous anchor's hash, providing wholesale-deletion detection.
CREATE TABLE audit_anchors (
  anchor_seq    BIGINT      NOT NULL PRIMARY KEY,
  at            TIMESTAMPTZ NOT NULL,
  tenant_heads  JSONB       NOT NULL,
  prev_hash     BYTEA       NOT NULL,
  hash          BYTEA       NOT NULL
);
