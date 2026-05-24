CREATE TABLE tenants (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  slug TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE users (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  name TEXT NOT NULL,
  role TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE sources (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  currency TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE canonical_transactions (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  source_id TEXT NOT NULL REFERENCES sources(id),
  external_ref TEXT NOT NULL,
  value_date DATE NOT NULL,
  posted_at TIMESTAMPTZ NOT NULL,
  amount_minor BIGINT NOT NULL,
  currency TEXT NOT NULL,
  direction TEXT NOT NULL,
  counterparty TEXT,
  description TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE reconciliation_runs (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  name TEXT NOT NULL,
  source_a_id TEXT NOT NULL REFERENCES sources(id),
  source_b_id TEXT NOT NULL REFERENCES sources(id),
  status TEXT NOT NULL,
  started_at TIMESTAMPTZ NOT NULL,
  completed_at TIMESTAMPTZ,
  config_version TEXT NOT NULL,
  stats JSONB NOT NULL
);

CREATE TABLE match_decisions (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  run_id TEXT NOT NULL REFERENCES reconciliation_runs(id),
  type TEXT NOT NULL,
  txn_ids TEXT[] NOT NULL,
  score DOUBLE PRECISION NOT NULL,
  config_version TEXT NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE cases (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  break_id TEXT NOT NULL,
  assignee_id TEXT REFERENCES users(id),
  status TEXT NOT NULL
);

CREATE TABLE breaks (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  run_id TEXT NOT NULL REFERENCES reconciliation_runs(id),
  case_id TEXT NOT NULL REFERENCES cases(id),
  type TEXT NOT NULL,
  status TEXT NOT NULL,
  value_minor BIGINT NOT NULL,
  currency TEXT NOT NULL,
  assignee_id TEXT REFERENCES users(id),
  txn_ids TEXT[] NOT NULL,
  opened_at TIMESTAMPTZ NOT NULL
);

CREATE TABLE case_events (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL REFERENCES tenants(id),
  case_id TEXT NOT NULL REFERENCES cases(id),
  seq INT NOT NULL,
  kind TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  at TIMESTAMPTZ NOT NULL,
  payload JSONB NOT NULL,
  UNIQUE (case_id, seq)
);

CREATE INDEX idx_txn_tenant_source ON canonical_transactions(tenant_id, source_id);
CREATE INDEX idx_runs_tenant ON reconciliation_runs(tenant_id, started_at DESC);
CREATE INDEX idx_breaks_tenant ON breaks(tenant_id);
CREATE INDEX idx_decisions_run ON match_decisions(run_id);
CREATE INDEX idx_events_case ON case_events(case_id, seq);
