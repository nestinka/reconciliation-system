# Compliance Audit Chain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a hash-chained, tamper-evident audit log covering every material action in the platform, per-tenant verifiable, with a global anchor chain. Surface it through an admin Audit Log screen and a Controls panel that maps ISO 27001 / SOC 2 / FCA control items to the audit-event kinds that demonstrate them.

**Architecture:** A new pure `recon-audit` crate (chain serialization + SHA-256 hashing + `verify` + closed `AuditKind`/`AuditPayload` enums + a static controls registry). `recon-store` gains `audit_events` and `audit_anchors` tables plus an `append_audit(tx, …)` helper that runs INSIDE the caller's transaction — so a failed audit insert rolls the audited action back. Existing store writes thread their tx through `append_audit`; `routes_auth` handlers (which were sequential, not transactional) get a thin refactor to wrap their DB ops in one tx so the audit emission is same-tx too. A `tokio::time::interval` in `recon-api` runs `anchor_now()` periodically; the same code path is exposed as `POST /api/audit/anchor`. The frontend adds two admin screens (Audit, Controls) and a sidebar entry for each.

**Tech Stack:** Rust (axum 0.7, sqlx 0.8, `sha2`, `serde_json` with sorted-keys default), PostgreSQL (`bytea` for hashes, `jsonb` for payloads), Next.js 16 / React 19, TanStack Query, react-hook-form + zod, vitest, Playwright.

**Source of truth:** `docs/superpowers/specs/2026-05-26-recon-compliance-audit-chain-design.md`.

---

## Shared type contract (authoritative — reuse verbatim across tasks)

**`recon-audit` (Rust):**

```rust
/// Closed enum of every kind the audit log can carry. New kinds require a code change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
pub enum AuditKind {
    AuthLoginSuccess, AuthLoginFailure, AuthLockout, AuthLogout,
    AuthPasswordChanged, AuthPasswordResetRequested, AuthPasswordResetCompleted,
    AuthRefreshReused, AuthTenantSwitched,
    AdminUserCreated, AdminUserRoleChanged, AdminUserDisabled, AdminUserEnabled, AdminUserRemoved,
    DataSourceCreated, DataIngestCompleted, DataRunCreated,
    CaseAssigned, CaseEventAppended,
    SystemAnchorCreated,
}

impl AuditKind {
    /// Stable string identifier used in DB rows and on the wire (e.g. "auth.login.success").
    pub fn as_str(&self) -> &'static str { /* implemented in events.rs */ }
}

/// Typed payload variants. No serde_json::Value escape hatch — sensitive material is
/// impossible to add by accident.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data")]
pub enum AuditPayload {
    AuthLoginSuccess { user_id: String, email: String, ip: Option<String> },
    AuthLoginFailure { email: String, ip: Option<String>, reason: LoginFailureReason },
    AuthLockout { user_id: String, email: String, locked_until: String /* RFC3339 */ },
    AuthLogout { user_id: String, ip: Option<String> },
    AuthPasswordChanged { user_id: String, ip: Option<String> },
    AuthPasswordResetRequested { email: String, ip: Option<String> },
    AuthPasswordResetCompleted { user_id: String, ip: Option<String> },
    AuthRefreshReused { user_id: String, token_id: String, ip: Option<String> },
    AuthTenantSwitched { user_id: String, from_tenant: String, to_tenant: String },
    AdminUserCreated { user_id: String, email: String, role: String },
    AdminUserRoleChanged { user_id: String, from: String, to: String },
    AdminUserDisabled { user_id: String },
    AdminUserEnabled { user_id: String },
    AdminUserRemoved { user_id: String },
    DataSourceCreated { source_id: String, kind: String, currency: String, name: String },
    DataIngestCompleted { source_id: String, format: String, file_sha256: String, bytes: i64, ingested: i64 },
    DataRunCreated { run_id: String, source_a_id: String, source_b_id: String, from: String, to: String, matched: i64, unmatched: i64 },
    CaseAssigned { case_id: String, break_id: String, assignee_id: String },
    CaseEventAppended { case_id: String, break_id: String, event_kind: String },
    SystemAnchorCreated { anchor_seq: i64, tenant_count: i64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginFailureReason { BadCredentials, Locked, RateLimited }

/// One entry as it sits in the DB and on the wire (hashes hex-encoded for JSON transport).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub tenant_id: String,
    pub seq: i64,
    pub at: String,          // RFC3339
    pub actor_id: String,
    pub kind: AuditKind,
    pub payload: AuditPayload,
    pub prev_hash: [u8; 32],
    pub hash: [u8; 32],
}

/// Verification error: which seq broke, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub seq: i64,
    pub reason: VerifyReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyReason {
    /// Hash didn't match recomputed value (payload or metadata tampered).
    Tampered,
    /// prev_hash doesn't equal the previous entry's hash.
    WrongPrev,
    /// seq isn't consecutive (gap).
    Missing,
    /// seq went backwards (reordered).
    Reordered,
    /// Genesis entry has non-zero prev_hash, or expected_prev_hash didn't match.
    WrongGenesis,
}

/// Controls registry entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub id: &'static str,            // e.g. "ISO27001:A.9.2.1"
    pub framework: &'static str,     // "ISO 27001" | "SOC 2" | "FCA"
    pub description: &'static str,
    pub event_kinds: &'static [AuditKind],
}
```

**Frontend (`web/lib/api/client.ts`):**

```ts
export type AuditKind =
  | "auth.login.success" | "auth.login.failure" | "auth.lockout" | "auth.logout"
  | "auth.password.changed" | "auth.password.reset_requested" | "auth.password.reset_completed"
  | "auth.refresh.reused" | "auth.tenant.switched"
  | "admin.user.created" | "admin.user.role_changed" | "admin.user.disabled" | "admin.user.enabled" | "admin.user.removed"
  | "data.source.created" | "data.ingest.completed" | "data.run.created"
  | "case.assigned" | "case.event_appended"
  | "system.anchor.created";

export interface AuditEvent {
  tenantId: string;
  seq: number;
  at: string;
  actorId: string;
  kind: AuditKind;
  payload: Record<string, unknown>;
  prevHash: string;  // hex
  hash: string;      // hex
}

export interface AuditPage { items: AuditEvent[]; nextCursor: number | null; }

export interface AuditQuery {
  from?: string; to?: string;
  kind?: AuditKind[]; actorId?: string;
  limit?: number; before?: number;
}

export interface VerifyRequest { from?: string; to?: string; expectedPrevHash?: string; }
export interface VerifyResult {
  status: "valid" | "invalid";
  checked: number;
  firstBrokenSeq?: number;
  reason?: "tampered" | "wrong_prev" | "missing" | "reordered" | "wrong_genesis";
}

export interface Anchor {
  anchorSeq: number;
  at: string;
  tenantHeads: Record<string, { seq: number; hash: string }>;
  prevHash: string;
  hash: string;
}

export interface Control {
  id: string;
  framework: string;
  description: string;
  eventKinds: AuditKind[];
}
```

**`ApiClient` additions:**

```ts
listAudit(tenantId: string, q?: AuditQuery): Promise<AuditPage>;
verifyAudit(tenantId: string, body: VerifyRequest): Promise<VerifyResult>;
anchorAudit(tenantId: string): Promise<{ anchorSeq: number; hash: string }>;
listAnchors(tenantId: string, limit?: number): Promise<Anchor[]>;
listControls(): Promise<Control[]>;
```

**API wire shape for an audit row:**

```json
{
  "tenantId": "tenant-acme",
  "seq": 42,
  "at": "2026-05-26T10:30:00Z",
  "actorId": "user-mia",
  "kind": "data.ingest.completed",
  "payload": { "sourceId": "src-…", "format": "csv", "fileSha256": "…", "bytes": 1234, "ingested": 20 },
  "prevHash": "0a1b…",
  "hash": "9f3c…"
}
```

The error envelope is the existing `{ "error": { "code", "message", … } }`. **Verification failure is a 200** with `status:"invalid"` and the diagnostics — it's a successful check that the chain is broken.

---

# Phase A — `recon-audit` crate (pure chain primitives)

### Task A1: Scaffold the crate + core types

**Files:**
- Create: `backend/crates/recon-audit/Cargo.toml`
- Create: `backend/crates/recon-audit/src/lib.rs`
- Modify: `backend/Cargo.toml` (add workspace member)

- [ ] **Step 1: Add the workspace member**

In `backend/Cargo.toml`, add to `[workspace] members`:

```toml
  "crates/recon-audit",
```

- [ ] **Step 2: Create the crate manifest**

`backend/crates/recon-audit/Cargo.toml`:

```toml
[package]
name = "recon-audit"
edition.workspace = true
version.workspace = true

[dependencies]
recon-domain = { path = "../recon-domain" }
serde = { workspace = true }
serde_json = { workspace = true }
sha2 = { workspace = true }
thiserror = { workspace = true }
hex = { workspace = true }

[dev-dependencies]
proptest = { workspace = true }
```

- [ ] **Step 3: Write the failing test for the core types**

`backend/crates/recon-audit/src/lib.rs`:

```rust
pub mod chain;
pub mod controls;
pub mod events;

pub use chain::{verify, VerifyError, VerifyReason};
pub use controls::{Control, CONTROLS};
pub use events::{AuditKind, AuditPayload, LoginFailureReason};

/// One entry as it sits in the DB and on the wire (in-memory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub tenant_id: String,
    pub seq: i64,
    pub at: String,
    pub actor_id: String,
    pub kind: AuditKind,
    pub payload: AuditPayload,
    pub prev_hash: [u8; 32],
    pub hash: [u8; 32],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_entry_is_constructible() {
        let e = AuditEntry {
            tenant_id: "tenant-acme".into(),
            seq: 1,
            at: "2026-05-26T10:00:00Z".into(),
            actor_id: "user-mia".into(),
            kind: AuditKind::AuthLogout,
            payload: AuditPayload::AuthLogout { user_id: "user-mia".into(), ip: None },
            prev_hash: [0u8; 32],
            hash: [0u8; 32],
        };
        assert_eq!(e.seq, 1);
        assert_eq!(e.kind.as_str(), "auth.logout");
    }
}
```

- [ ] **Step 4: Create empty module stubs so it compiles**

Create three files containing only the doc lines, fleshed out by later tasks:

`backend/crates/recon-audit/src/chain.rs`:

```rust
//! Canonical serialization + SHA-256 hashing + chain verification. Filled in A3/A4.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub seq: i64,
    pub reason: VerifyReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyReason { Tampered, WrongPrev, Missing, Reordered, WrongGenesis }

pub fn verify(
    _entries: &[crate::AuditEntry],
    _expected_prev_hash: Option<[u8; 32]>,
) -> Result<(), VerifyError> {
    // Stub — real implementation in A4.
    Ok(())
}
```

`backend/crates/recon-audit/src/events.rs`:

```rust
//! AuditKind + AuditPayload enums. Real bodies in A2.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginFailureReason { BadCredentials, Locked, RateLimited }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditKind { AuthLogout }

impl AuditKind {
    pub fn as_str(&self) -> &'static str {
        match self { AuditKind::AuthLogout => "auth.logout" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum AuditPayload {
    AuthLogout { user_id: String, ip: Option<String> },
}
```

(A2 will replace this stub with the full enum.)

`backend/crates/recon-audit/src/controls.rs`:

```rust
//! Static controls registry. Real bodies in A5.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub id: &'static str,
    pub framework: &'static str,
    pub description: &'static str,
    pub event_kinds: &'static [crate::AuditKind],
}

pub static CONTROLS: &[Control] = &[];
```

- [ ] **Step 5: Add `hex` to workspace deps**

In `backend/Cargo.toml` `[workspace.dependencies]` (it's already declared in the auth slice — verify with `grep hex backend/Cargo.toml`):

```toml
hex = "0.4"
```

If already present, skip.

- [ ] **Step 6: Run the test**

Run: `cd backend && cargo test -p recon-audit audit_entry_is_constructible`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add backend/Cargo.toml backend/crates/recon-audit
git commit -m "feat(audit): scaffold recon-audit crate with AuditEntry stub

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A2: Full `AuditKind` + `AuditPayload` enums

**Files:**
- Modify: `backend/crates/recon-audit/src/events.rs` (replace stub)

- [ ] **Step 1: Write the events module**

Replace `backend/crates/recon-audit/src/events.rs` with the full enum:

```rust
//! Closed set of audit event kinds + their typed payloads.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginFailureReason { BadCredentials, Locked, RateLimited }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum AuditKind {
    AuthLoginSuccess, AuthLoginFailure, AuthLockout, AuthLogout,
    AuthPasswordChanged, AuthPasswordResetRequested, AuthPasswordResetCompleted,
    AuthRefreshReused, AuthTenantSwitched,
    AdminUserCreated, AdminUserRoleChanged, AdminUserDisabled, AdminUserEnabled, AdminUserRemoved,
    DataSourceCreated, DataIngestCompleted, DataRunCreated,
    CaseAssigned, CaseEventAppended,
    SystemAnchorCreated,
}

impl AuditKind {
    /// Stable string identifier used in DB rows and on the wire.
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditKind::AuthLoginSuccess => "auth.login.success",
            AuditKind::AuthLoginFailure => "auth.login.failure",
            AuditKind::AuthLockout => "auth.lockout",
            AuditKind::AuthLogout => "auth.logout",
            AuditKind::AuthPasswordChanged => "auth.password.changed",
            AuditKind::AuthPasswordResetRequested => "auth.password.reset_requested",
            AuditKind::AuthPasswordResetCompleted => "auth.password.reset_completed",
            AuditKind::AuthRefreshReused => "auth.refresh.reused",
            AuditKind::AuthTenantSwitched => "auth.tenant.switched",
            AuditKind::AdminUserCreated => "admin.user.created",
            AuditKind::AdminUserRoleChanged => "admin.user.role_changed",
            AuditKind::AdminUserDisabled => "admin.user.disabled",
            AuditKind::AdminUserEnabled => "admin.user.enabled",
            AuditKind::AdminUserRemoved => "admin.user.removed",
            AuditKind::DataSourceCreated => "data.source.created",
            AuditKind::DataIngestCompleted => "data.ingest.completed",
            AuditKind::DataRunCreated => "data.run.created",
            AuditKind::CaseAssigned => "case.assigned",
            AuditKind::CaseEventAppended => "case.event_appended",
            AuditKind::SystemAnchorCreated => "system.anchor.created",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "auth.login.success" => AuditKind::AuthLoginSuccess,
            "auth.login.failure" => AuditKind::AuthLoginFailure,
            "auth.lockout" => AuditKind::AuthLockout,
            "auth.logout" => AuditKind::AuthLogout,
            "auth.password.changed" => AuditKind::AuthPasswordChanged,
            "auth.password.reset_requested" => AuditKind::AuthPasswordResetRequested,
            "auth.password.reset_completed" => AuditKind::AuthPasswordResetCompleted,
            "auth.refresh.reused" => AuditKind::AuthRefreshReused,
            "auth.tenant.switched" => AuditKind::AuthTenantSwitched,
            "admin.user.created" => AuditKind::AdminUserCreated,
            "admin.user.role_changed" => AuditKind::AdminUserRoleChanged,
            "admin.user.disabled" => AuditKind::AdminUserDisabled,
            "admin.user.enabled" => AuditKind::AdminUserEnabled,
            "admin.user.removed" => AuditKind::AdminUserRemoved,
            "data.source.created" => AuditKind::DataSourceCreated,
            "data.ingest.completed" => AuditKind::DataIngestCompleted,
            "data.run.created" => AuditKind::DataRunCreated,
            "case.assigned" => AuditKind::CaseAssigned,
            "case.event_appended" => AuditKind::CaseEventAppended,
            "system.anchor.created" => AuditKind::SystemAnchorCreated,
            _ => return None,
        })
    }
}

/// Typed payload variants. No serde_json::Value escape hatch — sensitive material is
/// impossible to add by accident. Variant names mirror AuditKind exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum AuditPayload {
    AuthLoginSuccess { user_id: String, email: String, ip: Option<String> },
    AuthLoginFailure { email: String, ip: Option<String>, reason: LoginFailureReason },
    AuthLockout { user_id: String, email: String, locked_until: String },
    AuthLogout { user_id: String, ip: Option<String> },
    AuthPasswordChanged { user_id: String, ip: Option<String> },
    AuthPasswordResetRequested { email: String, ip: Option<String> },
    AuthPasswordResetCompleted { user_id: String, ip: Option<String> },
    AuthRefreshReused { user_id: String, token_id: String, ip: Option<String> },
    AuthTenantSwitched { user_id: String, from_tenant: String, to_tenant: String },
    AdminUserCreated { user_id: String, email: String, role: String },
    AdminUserRoleChanged { user_id: String, from: String, to: String },
    AdminUserDisabled { user_id: String },
    AdminUserEnabled { user_id: String },
    AdminUserRemoved { user_id: String },
    DataSourceCreated { source_id: String, kind: String, currency: String, name: String },
    DataIngestCompleted { source_id: String, format: String, file_sha256: String, bytes: i64, ingested: i64 },
    DataRunCreated { run_id: String, source_a_id: String, source_b_id: String, from: String, to: String, matched: i64, unmatched: i64 },
    CaseAssigned { case_id: String, break_id: String, assignee_id: String },
    CaseEventAppended { case_id: String, break_id: String, event_kind: String },
    SystemAnchorCreated { anchor_seq: i64, tenant_count: i64 },
}

impl AuditPayload {
    /// The kind tag that matches this payload (used for asserting consistency).
    pub fn kind(&self) -> AuditKind {
        match self {
            AuditPayload::AuthLoginSuccess { .. } => AuditKind::AuthLoginSuccess,
            AuditPayload::AuthLoginFailure { .. } => AuditKind::AuthLoginFailure,
            AuditPayload::AuthLockout { .. } => AuditKind::AuthLockout,
            AuditPayload::AuthLogout { .. } => AuditKind::AuthLogout,
            AuditPayload::AuthPasswordChanged { .. } => AuditKind::AuthPasswordChanged,
            AuditPayload::AuthPasswordResetRequested { .. } => AuditKind::AuthPasswordResetRequested,
            AuditPayload::AuthPasswordResetCompleted { .. } => AuditKind::AuthPasswordResetCompleted,
            AuditPayload::AuthRefreshReused { .. } => AuditKind::AuthRefreshReused,
            AuditPayload::AuthTenantSwitched { .. } => AuditKind::AuthTenantSwitched,
            AuditPayload::AdminUserCreated { .. } => AuditKind::AdminUserCreated,
            AuditPayload::AdminUserRoleChanged { .. } => AuditKind::AdminUserRoleChanged,
            AuditPayload::AdminUserDisabled { .. } => AuditKind::AdminUserDisabled,
            AuditPayload::AdminUserEnabled { .. } => AuditKind::AdminUserEnabled,
            AuditPayload::AdminUserRemoved { .. } => AuditKind::AdminUserRemoved,
            AuditPayload::DataSourceCreated { .. } => AuditKind::DataSourceCreated,
            AuditPayload::DataIngestCompleted { .. } => AuditKind::DataIngestCompleted,
            AuditPayload::DataRunCreated { .. } => AuditKind::DataRunCreated,
            AuditPayload::CaseAssigned { .. } => AuditKind::CaseAssigned,
            AuditPayload::CaseEventAppended { .. } => AuditKind::CaseEventAppended,
            AuditPayload::SystemAnchorCreated { .. } => AuditKind::SystemAnchorCreated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_strings_are_stable_dot_notation() {
        assert_eq!(AuditKind::AuthLoginSuccess.as_str(), "auth.login.success");
        assert_eq!(AuditKind::DataIngestCompleted.as_str(), "data.ingest.completed");
        assert_eq!(AuditKind::SystemAnchorCreated.as_str(), "system.anchor.created");
    }

    #[test]
    fn kind_string_roundtrip() {
        for k in [
            AuditKind::AuthLoginSuccess, AuditKind::AuthLockout, AuditKind::DataRunCreated,
            AuditKind::CaseAssigned, AuditKind::SystemAnchorCreated,
        ] {
            assert_eq!(AuditKind::from_str(k.as_str()), Some(k));
        }
        assert_eq!(AuditKind::from_str("nope"), None);
    }

    #[test]
    fn payload_kind_matches_variant() {
        let p = AuditPayload::AuthLogout { user_id: "u".into(), ip: None };
        assert_eq!(p.kind(), AuditKind::AuthLogout);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cd backend && cargo test -p recon-audit events::`
Expected: 3 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/recon-audit/src/events.rs
git commit -m "feat(audit): complete AuditKind + AuditPayload taxonomy

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A3: Canonical serialization + SHA-256 hashing

**Files:**
- Modify: `backend/crates/recon-audit/src/chain.rs`

- [ ] **Step 1: Implement canonical hashing**

Replace `backend/crates/recon-audit/src/chain.rs`:

```rust
//! Deterministic canonical serialization + SHA-256 hashing for audit entries.
//!
//! Encoding: a sequence of length-prefixed binary fields. Each field is
//! `<u32-BE byte-length> <utf-8 bytes>`. Fields are appended in a fixed order:
//!     prev_hash (32 bytes, no length prefix),
//!     seq (u64-BE, no length prefix),
//!     tenant_id (length-prefixed UTF-8),
//!     at (length-prefixed UTF-8 RFC3339),
//!     actor_id (length-prefixed UTF-8),
//!     kind (length-prefixed ASCII, e.g. "data.ingest.completed"),
//!     payload (length-prefixed sorted-keys JSON bytes).
//!
//! Sorted-keys JSON is produced by `serde_json::to_value(&payload)` (which uses
//! a BTreeMap-backed Map when the `preserve_order` feature is OFF — the workspace
//! does NOT enable it) followed by `serde_json::to_vec(&value)`.

use crate::AuditEntry;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub seq: i64,
    pub reason: VerifyReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyReason {
    Tampered,
    WrongPrev,
    Missing,
    Reordered,
    WrongGenesis,
}

/// Build the canonical pre-image bytes for an entry. Pure: same inputs → same bytes.
pub fn canonical_bytes(
    prev_hash: &[u8; 32],
    seq: i64,
    tenant_id: &str,
    at: &str,
    actor_id: &str,
    kind_str: &str,
    payload_canonical_json: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        32 + 8 + 4 + tenant_id.len() + 4 + at.len() + 4 + actor_id.len()
            + 4 + kind_str.len() + 4 + payload_canonical_json.len(),
    );
    out.extend_from_slice(prev_hash);
    out.extend_from_slice(&(seq as u64).to_be_bytes());
    push_lp(&mut out, tenant_id.as_bytes());
    push_lp(&mut out, at.as_bytes());
    push_lp(&mut out, actor_id.as_bytes());
    push_lp(&mut out, kind_str.as_bytes());
    push_lp(&mut out, payload_canonical_json);
    out
}

fn push_lp(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// Serialize an `AuditPayload` to sorted-keys, no-whitespace JSON bytes. Determinism
/// relies on serde_json's `preserve_order` feature being OFF in the workspace.
pub fn payload_canonical_json(payload: &crate::AuditPayload) -> Vec<u8> {
    // to_value emits a `Map<String, Value>` which is `BTreeMap`-backed without the
    // `preserve_order` feature; to_vec then yields sorted-keys JSON.
    let v = serde_json::to_value(payload).expect("AuditPayload is always serializable");
    serde_json::to_vec(&v).expect("Value is always serializable")
}

/// Compute the SHA-256 hash of an entry given prev_hash + the entry's fields.
pub fn compute_hash(
    prev_hash: &[u8; 32],
    seq: i64,
    tenant_id: &str,
    at: &str,
    actor_id: &str,
    kind: crate::AuditKind,
    payload: &crate::AuditPayload,
) -> [u8; 32] {
    let pcj = payload_canonical_json(payload);
    let bytes = canonical_bytes(prev_hash, seq, tenant_id, at, actor_id, kind.as_str(), &pcj);
    let mut h = Sha256::new();
    h.update(&bytes);
    h.finalize().into()
}

/// Verify a contiguous slice of entries (in seq order).
///
/// If `entries[0].seq == 1`, the genesis prev_hash must be all-zero. Otherwise the
/// caller can supply `expected_prev_hash` to verify a slice mid-chain.
pub fn verify(
    entries: &[AuditEntry],
    expected_prev_hash: Option<[u8; 32]>,
) -> Result<(), VerifyError> {
    if entries.is_empty() {
        return Ok(());
    }
    // Genesis check.
    if entries[0].seq == 1 {
        if entries[0].prev_hash != [0u8; 32] {
            return Err(VerifyError { seq: entries[0].seq, reason: VerifyReason::WrongGenesis });
        }
    } else if let Some(expected) = expected_prev_hash {
        if entries[0].prev_hash != expected {
            return Err(VerifyError { seq: entries[0].seq, reason: VerifyReason::WrongGenesis });
        }
    }

    let mut prev_seq: Option<i64> = None;
    let mut prev_hash: Option<[u8; 32]> = None;
    for e in entries {
        if let Some(ps) = prev_seq {
            if e.seq < ps + 1 {
                return Err(VerifyError { seq: e.seq, reason: VerifyReason::Reordered });
            }
            if e.seq > ps + 1 {
                return Err(VerifyError { seq: e.seq, reason: VerifyReason::Missing });
            }
        }
        if let Some(ph) = prev_hash {
            if e.prev_hash != ph {
                return Err(VerifyError { seq: e.seq, reason: VerifyReason::WrongPrev });
            }
        }
        let recomputed = compute_hash(&e.prev_hash, e.seq, &e.tenant_id, &e.at, &e.actor_id, e.kind, &e.payload);
        if recomputed != e.hash {
            return Err(VerifyError { seq: e.seq, reason: VerifyReason::Tampered });
        }
        prev_seq = Some(e.seq);
        prev_hash = Some(e.hash);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuditEntry, AuditKind, AuditPayload};

    fn mk(seq: i64, prev: [u8; 32], payload: AuditPayload) -> AuditEntry {
        let kind = payload.kind();
        let at = "2026-05-26T10:00:00Z".to_string();
        let hash = compute_hash(&prev, seq, "tenant-acme", &at, "user-mia", kind, &payload);
        AuditEntry {
            tenant_id: "tenant-acme".into(),
            seq,
            at,
            actor_id: "user-mia".into(),
            kind,
            payload,
            prev_hash: prev,
            hash,
        }
    }

    #[test]
    fn canonical_bytes_is_deterministic() {
        let p = AuditPayload::AuthLogout { user_id: "u".into(), ip: None };
        let pcj1 = payload_canonical_json(&p);
        let pcj2 = payload_canonical_json(&p);
        assert_eq!(pcj1, pcj2);
        let b1 = canonical_bytes(&[0u8; 32], 1, "t", "at", "a", "auth.logout", &pcj1);
        let b2 = canonical_bytes(&[0u8; 32], 1, "t", "at", "a", "auth.logout", &pcj2);
        assert_eq!(b1, b2);
    }

    #[test]
    fn canonical_json_is_sorted_keys() {
        // For a struct variant, serde_json emits keys in BTreeMap order without preserve_order.
        let p = AuditPayload::AuthLoginSuccess {
            user_id: "u".into(), email: "e".into(), ip: Some("1.2.3.4".into()),
        };
        let s = String::from_utf8(payload_canonical_json(&p)).unwrap();
        // External tag + content shape: {"kind":"auth_login_success","data":{...sorted...}}
        // Verify the inner data object keys are sorted alphabetically (email < ip < user_id).
        let i_email = s.find("\"email\"").unwrap();
        let i_ip = s.find("\"ip\"").unwrap();
        let i_user_id = s.find("\"user_id\"").unwrap();
        assert!(i_email < i_ip && i_ip < i_user_id, "keys must be sorted: {s}");
    }

    #[test]
    fn verify_accepts_valid_chain_of_three() {
        let e1 = mk(1, [0u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let e2 = mk(2, e1.hash, AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let e3 = mk(3, e2.hash, AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        assert!(verify(&[e1, e2, e3], None).is_ok());
    }

    #[test]
    fn verify_detects_tamper() {
        let e1 = mk(1, [0u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let mut e2 = mk(2, e1.hash, AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        // Tamper: change payload but keep hash stale.
        e2.payload = AuditPayload::AuthLogout { user_id: "evil".into(), ip: None };
        let err = verify(&[e1, e2], None).unwrap_err();
        assert_eq!(err.seq, 2);
        assert_eq!(err.reason, VerifyReason::Tampered);
    }

    #[test]
    fn verify_detects_missing() {
        let e1 = mk(1, [0u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let e3 = mk(3, e1.hash, AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let err = verify(&[e1, e3], None).unwrap_err();
        assert_eq!(err.seq, 3);
        assert_eq!(err.reason, VerifyReason::Missing);
    }

    #[test]
    fn verify_detects_wrong_prev() {
        let e1 = mk(1, [0u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let e2 = mk(2, [9u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let err = verify(&[e1, e2], None).unwrap_err();
        assert_eq!(err.seq, 2);
        assert_eq!(err.reason, VerifyReason::WrongPrev);
    }

    #[test]
    fn verify_rejects_non_zero_genesis() {
        let e1 = mk(1, [9u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let err = verify(&[e1], None).unwrap_err();
        assert_eq!(err.reason, VerifyReason::WrongGenesis);
    }

    #[test]
    fn verify_partial_range_with_expected_prev() {
        let e1 = mk(1, [0u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let e2 = mk(2, e1.hash, AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        // Verify just e2 with expected_prev_hash = e1.hash.
        assert!(verify(&[e2.clone()], Some(e1.hash)).is_ok());
        // Wrong expected_prev fails.
        let err = verify(&[e2], Some([7u8; 32])).unwrap_err();
        assert_eq!(err.reason, VerifyReason::WrongGenesis);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cd backend && cargo test -p recon-audit chain::`
Expected: 7 tests PASS.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/recon-audit/src/chain.rs
git commit -m "feat(audit): canonical serialization + SHA-256 chain verify

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A4: Golden vector + property test

**Files:**
- Create: `backend/crates/recon-audit/tests/properties.rs`
- Modify: `backend/crates/recon-audit/src/chain.rs` (append golden vector test)

- [ ] **Step 1: Lock the canonical encoding with a golden vector**

Append to `backend/crates/recon-audit/src/chain.rs`'s `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn golden_vector_for_logout_genesis_entry() {
        // A specific entry whose hash is locked in. If this test ever flips, the
        // canonical encoding has changed and existing chains become unverifiable —
        // require a deliberate migration.
        let p = AuditPayload::AuthLogout { user_id: "user-mia".into(), ip: None };
        let h = compute_hash(&[0u8; 32], 1, "tenant-acme", "2026-05-26T10:00:00Z", "user-mia", AuditKind::AuthLogout, &p);
        let actual = hex::encode(h);
        // The expected value is computed once during initial implementation; replace
        // with the value printed by this test on first run.
        let expected = "REPLACE_WITH_INITIAL_HASH";
        if expected == "REPLACE_WITH_INITIAL_HASH" {
            // First-run helper: print the hash so the developer can paste it in.
            // After replacing, this branch is never taken again.
            panic!("first run: replace expected with {actual}");
        }
        assert_eq!(actual, expected, "canonical encoding changed");
    }
```

> First implementation run will print the hash; replace `REPLACE_WITH_INITIAL_HASH` with the value the test printed, commit, then the test locks the encoding.

- [ ] **Step 2: Write the property test**

`backend/crates/recon-audit/tests/properties.rs`:

```rust
use proptest::prelude::*;
use recon_audit::{verify, AuditEntry, AuditKind, AuditPayload};
use recon_audit::chain::compute_hash;

fn mk(seq: i64, prev: [u8; 32], user: String) -> AuditEntry {
    let payload = AuditPayload::AuthLogout { user_id: user, ip: None };
    let at = "2026-05-26T10:00:00Z".to_string();
    let hash = compute_hash(&prev, seq, "tenant-acme", &at, "actor", AuditKind::AuthLogout, &payload);
    AuditEntry {
        tenant_id: "tenant-acme".into(),
        seq,
        at,
        actor_id: "actor".into(),
        kind: AuditKind::AuthLogout,
        payload,
        prev_hash: prev,
        hash,
    }
}

proptest! {
    /// A generated chain of N valid entries always verifies.
    #[test]
    fn valid_chains_always_verify(n in 1usize..50) {
        let mut entries = Vec::with_capacity(n);
        let mut prev = [0u8; 32];
        for i in 0..n {
            let e = mk(i as i64 + 1, prev, format!("user-{i}"));
            prev = e.hash;
            entries.push(e);
        }
        prop_assert!(verify(&entries, None).is_ok());
    }

    /// Tampering with a single byte of any entry's payload always breaks verify.
    #[test]
    fn tampering_breaks_verify(n in 2usize..20, tamper_at in 0usize..20) {
        let n = n; // moved
        let mut entries = Vec::with_capacity(n);
        let mut prev = [0u8; 32];
        for i in 0..n {
            let e = mk(i as i64 + 1, prev, format!("user-{i}"));
            prev = e.hash;
            entries.push(e);
        }
        let target = tamper_at % n;
        if let AuditPayload::AuthLogout { user_id, .. } = &mut entries[target].payload {
            user_id.push('!');
        }
        // After tampering, the recomputed hash won't match the stored hash anywhere
        // from the tamper point onward.
        prop_assert!(verify(&entries, None).is_err());
    }
}
```

- [ ] **Step 3: First-run the golden test, capture the hash, paste it back**

Run: `cd backend && cargo test -p recon-audit golden_vector_for_logout_genesis_entry 2>&1 | grep "first run"`
Copy the hex value, paste over `REPLACE_WITH_INITIAL_HASH` in `chain.rs`.

- [ ] **Step 4: Run all audit tests**

Run: `cd backend && cargo test -p recon-audit && cargo clippy -p recon-audit -- -D warnings`
Expected: all tests PASS; clippy clean.

- [ ] **Step 5: Commit**

```bash
git add backend/crates/recon-audit/src/chain.rs backend/crates/recon-audit/tests/properties.rs
git commit -m "test(audit): golden vector + chain property tests

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task A5: Controls registry

**Files:**
- Modify: `backend/crates/recon-audit/src/controls.rs`
- Create: `docs/compliance/controls.md`

- [ ] **Step 1: Populate the controls registry**

Replace `backend/crates/recon-audit/src/controls.rs`:

```rust
//! ISO 27001 / SOC 2 / FCA control → audit event kind mapping.
//!
//! This is the authoritative registry. New controls or new audit kinds require
//! a code change. The frontend reads this through `GET /api/audit/controls`.

use crate::AuditKind;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub id: &'static str,
    pub framework: &'static str,
    pub description: &'static str,
    pub event_kinds: &'static [AuditKind],
}

pub static CONTROLS: &[Control] = &[
    Control {
        id: "ISO27001:A.9.2.1",
        framework: "ISO 27001",
        description: "User registration and de-registration",
        event_kinds: &[
            AuditKind::AdminUserCreated,
            AuditKind::AdminUserDisabled,
            AuditKind::AdminUserEnabled,
            AuditKind::AdminUserRemoved,
        ],
    },
    Control {
        id: "ISO27001:A.9.2.3",
        framework: "ISO 27001",
        description: "Management of privileged access rights",
        event_kinds: &[
            AuditKind::AdminUserRoleChanged,
        ],
    },
    Control {
        id: "ISO27001:A.9.4.2",
        framework: "ISO 27001",
        description: "Secure log-on procedures",
        event_kinds: &[
            AuditKind::AuthLoginSuccess,
            AuditKind::AuthLoginFailure,
            AuditKind::AuthLockout,
        ],
    },
    Control {
        id: "ISO27001:A.9.4.3",
        framework: "ISO 27001",
        description: "Password management system",
        event_kinds: &[
            AuditKind::AuthPasswordChanged,
            AuditKind::AuthPasswordResetRequested,
            AuditKind::AuthPasswordResetCompleted,
        ],
    },
    Control {
        id: "ISO27001:A.12.4.1",
        framework: "ISO 27001",
        description: "Event logging",
        event_kinds: &[
            AuditKind::SystemAnchorCreated,
            AuditKind::DataIngestCompleted,
            AuditKind::DataRunCreated,
            AuditKind::DataSourceCreated,
        ],
    },
    Control {
        id: "SOC2:CC6.1",
        framework: "SOC 2",
        description: "Logical access security software, infrastructure, and architectures",
        event_kinds: &[
            AuditKind::AuthLoginSuccess,
            AuditKind::AuthLoginFailure,
            AuditKind::AuthLockout,
            AuditKind::AuthTenantSwitched,
            AuditKind::AuthRefreshReused,
        ],
    },
    Control {
        id: "SOC2:CC6.2",
        framework: "SOC 2",
        description: "Prior to issuing system credentials and granting access",
        event_kinds: &[
            AuditKind::AdminUserCreated,
            AuditKind::AdminUserRoleChanged,
        ],
    },
    Control {
        id: "SOC2:CC6.3",
        framework: "SOC 2",
        description: "Authorize, modify, or remove access to data, software, functions",
        event_kinds: &[
            AuditKind::AdminUserRoleChanged,
            AuditKind::AdminUserDisabled,
            AuditKind::AdminUserEnabled,
            AuditKind::AdminUserRemoved,
        ],
    },
    Control {
        id: "SOC2:CC7.2",
        framework: "SOC 2",
        description: "Monitors system components and operation",
        event_kinds: &[
            AuditKind::AuthRefreshReused,
            AuditKind::AuthLockout,
            AuditKind::SystemAnchorCreated,
        ],
    },
    Control {
        id: "FCA:SYSC9.1",
        framework: "FCA",
        description: "Record keeping — adequacy of records of business activities",
        event_kinds: &[
            AuditKind::DataIngestCompleted,
            AuditKind::DataRunCreated,
            AuditKind::CaseAssigned,
            AuditKind::CaseEventAppended,
        ],
    },
    Control {
        id: "FCA:SYSC4.1.10",
        framework: "FCA",
        description: "Four-eyes / segregation of duties on resolution decisions",
        event_kinds: &[
            AuditKind::CaseEventAppended,
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_control_has_a_framework_and_kinds() {
        for c in CONTROLS {
            assert!(!c.id.is_empty());
            assert!(!c.framework.is_empty());
            assert!(!c.description.is_empty());
            assert!(!c.event_kinds.is_empty(), "control {} has no event_kinds", c.id);
        }
    }

    #[test]
    fn control_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in CONTROLS {
            assert!(seen.insert(c.id), "duplicate control id {}", c.id);
        }
    }

    #[test]
    fn serde_roundtrip() {
        // Confirm camelCase: `eventKinds` not `event_kinds`.
        let c = &CONTROLS[0];
        let s = serde_json::to_string(c).unwrap();
        assert!(s.contains("\"eventKinds\""), "expected camelCase: {s}");
    }
}
```

- [ ] **Step 2: Write the human-readable controls doc**

Create `docs/compliance/controls.md` with one section per control (this is the auditor-facing description; the registry above is the machine-readable mapping). Use this template per entry:

```markdown
# Compliance controls — audit-event mapping

This document maps ISO 27001 / SOC 2 / FCA control items to the audit-event kinds
that demonstrate them. The same mapping is exposed programmatically by
`GET /api/audit/controls` and rendered in the Controls admin screen.

## ISO 27001

### A.9.2.1 — User registration and de-registration
**Evidence:** `admin.user.created`, `admin.user.disabled`, `admin.user.enabled`, `admin.user.removed`.
Filter the audit log to these kinds to enumerate all on/off-boarding events.

### A.9.2.3 — Management of privileged access rights
**Evidence:** `admin.user.role_changed`. Every role transition (operator ↔ approver ↔ admin) is recorded with `from`/`to`.

### A.9.4.2 — Secure log-on procedures
**Evidence:** `auth.login.success`, `auth.login.failure`, `auth.lockout`. Brute-force protection and lockout events are visible per account.

### A.9.4.3 — Password management system
**Evidence:** `auth.password.changed`, `auth.password.reset_requested`, `auth.password.reset_completed`.

### A.12.4.1 — Event logging
**Evidence:** `system.anchor.created`, `data.ingest.completed`, `data.run.created`, `data.source.created`. The audit chain itself is hash-anchored periodically.

## SOC 2

### CC6.1 — Logical access security software, infrastructure, and architectures
**Evidence:** `auth.login.success`, `auth.login.failure`, `auth.lockout`, `auth.tenant.switched`, `auth.refresh.reused`.

### CC6.2 — Prior to issuing system credentials and granting access
**Evidence:** `admin.user.created`, `admin.user.role_changed`.

### CC6.3 — Authorize, modify, or remove access to data, software, functions
**Evidence:** `admin.user.role_changed`, `admin.user.disabled`, `admin.user.enabled`, `admin.user.removed`.

### CC7.2 — Monitors system components and operation
**Evidence:** `auth.refresh.reused` (theft detection), `auth.lockout` (brute-force detection), `system.anchor.created` (audit-chain integrity).

## FCA

### SYSC 9.1 — Record keeping
**Evidence:** `data.ingest.completed`, `data.run.created`, `case.assigned`, `case.event_appended`. Every reconciliation action that creates or modifies records is captured.

### SYSC 4.1.10 — Four-eyes / segregation of duties
**Evidence:** `case.event_appended` (carries the `event_kind` field — auditor filters to `approval_requested` / `approved` to verify maker-checker separation).

## Chain integrity

The audit log is per-tenant hash-chained (SHA-256), with `prev_hash` and `hash`
on every row. A periodic (hourly by default) `system.anchor.created` ties every
tenant's current head into a global anchor chain, providing wholesale-deletion
detection. The admin **Audit Log** screen exposes a **Verify chain** action that
walks any time range and reports the first broken entry (if any).
```

- [ ] **Step 3: Run all audit tests**

Run: `cd backend && cargo test -p recon-audit`
Expected: all tests PASS (including controls roundtrip and uniqueness).

- [ ] **Step 4: Commit**

```bash
git add backend/crates/recon-audit/src/controls.rs docs/compliance/controls.md
git commit -m "feat(audit): ISO27001/SOC2/FCA controls registry + auditor doc

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase B — Store layer

### Task B1: Migration `0004_compliance.sql`

**Files:**
- Create: `backend/migrations/0004_compliance.sql`

- [ ] **Step 1: Write the migration**

`backend/migrations/0004_compliance.sql`:

```sql
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
```

- [ ] **Step 2: Write a smoke test confirming the schema applies**

`backend/crates/recon-store/tests/audit_schema.rs`:

```rust
use recon_store::Store;

#[sqlx::test(migrations = "../../migrations")]
async fn audit_tables_exist(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    // Inserting a no-op tenant + a single row exercises the schema.
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO audit_events(tenant_id,seq,at,actor_id,kind,payload,prev_hash,hash) \
         VALUES ('t',1, now(),'system','auth.logout','{}'::jsonb, $1, $2)",
    )
    .bind(vec![0u8; 32])
    .bind(vec![1u8; 32])
    .execute(&store.pool).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE tenant_id='t'")
        .fetch_one(&store.pool).await.unwrap();
    assert_eq!(n, 1);
    // Composite PK rejects a duplicate seq for the same tenant.
    let err = sqlx::query(
        "INSERT INTO audit_events(tenant_id,seq,at,actor_id,kind,payload,prev_hash,hash) \
         VALUES ('t',1, now(),'system','auth.logout','{}'::jsonb, $1, $2)",
    )
    .bind(vec![0u8; 32])
    .bind(vec![2u8; 32])
    .execute(&store.pool).await;
    assert!(err.is_err(), "duplicate (tenant_id,seq) must violate the PK");
}
```

- [ ] **Step 3: Run the test**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store --test audit_schema`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add backend/migrations/0004_compliance.sql backend/crates/recon-store/tests/audit_schema.rs
git commit -m "feat(store): migration 0004 — audit_events + audit_anchors

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B2: `append_audit` (same-transaction primitive)

**Files:**
- Create: `backend/crates/recon-store/src/audit.rs`
- Modify: `backend/crates/recon-store/src/lib.rs` (`pub mod audit;`)
- Modify: `backend/crates/recon-store/Cargo.toml` (add `recon-audit`)
- Modify: `backend/crates/recon-store/tests/audit_schema.rs` (append tests)

- [ ] **Step 1: Add the crate dependency**

In `backend/crates/recon-store/Cargo.toml` `[dependencies]`:

```toml
recon-audit = { path = "../recon-audit" }
```

- [ ] **Step 2: Register the module**

In `backend/crates/recon-store/src/lib.rs` module list:

```rust
pub mod audit;
```

- [ ] **Step 3: Implement `append_audit`**

`backend/crates/recon-store/src/audit.rs`:

```rust
use crate::{Store, StoreError};
use recon_audit::{chain, AuditEntry, AuditKind, AuditPayload};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

impl Store {
    /// Append an audit event to a tenant's chain INSIDE the caller's transaction.
    /// Fetches the current tail row with `FOR UPDATE`, computes the next hash,
    /// and inserts. If the caller's transaction rolls back (for any reason),
    /// the audit row rolls back with it.
    pub async fn append_audit(
        &self,
        tx: &mut sqlx::PgConnection,
        tenant_id: &str,
        actor_id: &str,
        payload: AuditPayload,
    ) -> Result<AuditEntry, StoreError> {
        // 1. Lock the tail (or genesis).
        let row: Option<(i64, Vec<u8>)> = sqlx::query_as(
            "SELECT seq, hash FROM audit_events WHERE tenant_id=$1 ORDER BY seq DESC LIMIT 1 FOR UPDATE",
        )
        .bind(tenant_id)
        .fetch_optional(&mut *tx)
        .await?;
        let (prev_seq, prev_hash) = match row {
            Some((s, h)) => (s, vec_to_arr32(h)?),
            None => (0, [0u8; 32]),
        };
        let seq = prev_seq + 1;

        // 2. Timestamp + hash.
        let now = OffsetDateTime::now_utc();
        let at = now.format(&Rfc3339).map_err(|_| StoreError::Db(sqlx::Error::Decode("rfc3339".into())))?;
        let kind = payload.kind();
        let hash = chain::compute_hash(&prev_hash, seq, tenant_id, &at, actor_id, kind, &payload);

        // 3. Insert. The composite PK rejects a colliding concurrent insert with 23505;
        //    the caller's transaction will be retried at the action layer.
        let payload_json = serde_json::to_value(&payload)?;
        sqlx::query(
            "INSERT INTO audit_events(tenant_id,seq,at,actor_id,kind,payload,prev_hash,hash) \
             VALUES ($1,$2,$3::timestamptz,$4,$5,$6,$7,$8)",
        )
        .bind(tenant_id)
        .bind(seq)
        .bind(&at)
        .bind(actor_id)
        .bind(kind.as_str())
        .bind(&payload_json)
        .bind(prev_hash.as_slice())
        .bind(hash.as_slice())
        .execute(&mut *tx)
        .await?;

        Ok(AuditEntry {
            tenant_id: tenant_id.into(),
            seq,
            at,
            actor_id: actor_id.into(),
            kind,
            payload,
            prev_hash,
            hash,
        })
    }
}

fn vec_to_arr32(v: Vec<u8>) -> Result<[u8; 32], StoreError> {
    let arr: [u8; 32] = v.try_into().map_err(|_| StoreError::Db(sqlx::Error::Decode("hash len".into())))?;
    Ok(arr)
}
```

- [ ] **Step 4: Append tests**

In `backend/crates/recon-store/tests/audit_schema.rs`:

```rust
use recon_audit::{AuditPayload, AuditKind};

#[sqlx::test(migrations = "../../migrations")]
async fn append_audit_chains_per_tenant(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t'),('u','U','u')")
        .execute(&store.pool).await.unwrap();

    let mut tx = store.pool.begin().await.unwrap();
    let e1 = store.append_audit(&mut tx, "t", "system",
        AuditPayload::AuthLogout { user_id: "user-1".into(), ip: None }).await.unwrap();
    let e2 = store.append_audit(&mut tx, "t", "system",
        AuditPayload::AuthLogout { user_id: "user-2".into(), ip: None }).await.unwrap();
    let f1 = store.append_audit(&mut tx, "u", "system",
        AuditPayload::AuthLogout { user_id: "user-3".into(), ip: None }).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(e1.seq, 1);
    assert_eq!(e2.seq, 2);
    assert_eq!(e2.prev_hash, e1.hash, "chain links inside a tenant");
    assert_eq!(f1.seq, 1, "the other tenant has its own seq=1");
    assert_eq!(f1.prev_hash, [0u8; 32]);
    assert_eq!(e1.kind, AuditKind::AuthLogout);
}
```

- [ ] **Step 5: Run the tests**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store --test audit_schema`
Expected: 2 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add backend/crates/recon-store/Cargo.toml backend/crates/recon-store/src/lib.rs backend/crates/recon-store/src/audit.rs backend/crates/recon-store/tests/audit_schema.rs
git commit -m "feat(store): append_audit primitive (same-tx, per-tenant chain)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B3: `list_audit` + `verify_audit`

**Files:**
- Modify: `backend/crates/recon-store/src/audit.rs`
- Modify: `backend/crates/recon-store/tests/audit_schema.rs`

- [ ] **Step 1: Implement list + verify**

Append to `backend/crates/recon-store/src/audit.rs`:

```rust
#[derive(Default, Debug, Clone)]
pub struct AuditFilter {
    pub from: Option<String>,        // ISO 8601 date or datetime
    pub to: Option<String>,
    pub kinds: Vec<AuditKind>,
    pub actor_id: Option<String>,
    pub limit: i64,                  // <= 500
    pub before: Option<i64>,         // cursor: return seq < before
}

#[derive(Debug, Clone)]
pub struct AuditPage {
    pub items: Vec<AuditEntry>,
    pub next_cursor: Option<i64>,
}

impl Store {
    pub async fn list_audit(&self, tenant_id: &str, f: &AuditFilter) -> Result<AuditPage, StoreError> {
        let limit = f.limit.clamp(1, 500);
        let kinds_strs: Vec<&str> = f.kinds.iter().map(|k| k.as_str()).collect();
        // sqlx doesn't support optional ANY() bindings as a single expression; use COALESCE pattern.
        let rows: Vec<(String, i64, time::OffsetDateTime, String, String, serde_json::Value, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT tenant_id, seq, at, actor_id, kind, payload, prev_hash, hash \
             FROM audit_events \
             WHERE tenant_id = $1 \
               AND ($2::timestamptz IS NULL OR at >= $2::timestamptz) \
               AND ($3::timestamptz IS NULL OR at <= $3::timestamptz) \
               AND (cardinality($4::text[]) = 0 OR kind = ANY($4::text[])) \
               AND ($5::text IS NULL OR actor_id = $5) \
               AND ($6::bigint IS NULL OR seq < $6) \
             ORDER BY seq DESC \
             LIMIT $7",
        )
        .bind(tenant_id)
        .bind(f.from.as_deref())
        .bind(f.to.as_deref())
        .bind(&kinds_strs)
        .bind(f.actor_id.as_deref())
        .bind(f.before)
        .bind(limit + 1) // fetch one extra to detect a next page
        .fetch_all(&self.pool)
        .await?;

        let has_more = rows.len() as i64 > limit;
        let items: Vec<AuditEntry> = rows.into_iter().take(limit as usize).map(row_to_entry).collect::<Result<_,_>>()?;
        let next_cursor = if has_more { items.last().map(|e| e.seq) } else { None };
        Ok(AuditPage { items, next_cursor })
    }

    /// Load a range in seq order (ascending) and run chain::verify on it.
    pub async fn verify_audit(
        &self,
        tenant_id: &str,
        from_seq: Option<i64>,
        to_seq: Option<i64>,
        expected_prev_hash: Option<[u8; 32]>,
    ) -> Result<recon_audit::chain::VerifyOutcome, StoreError> {
        let rows: Vec<(String, i64, time::OffsetDateTime, String, String, serde_json::Value, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT tenant_id, seq, at, actor_id, kind, payload, prev_hash, hash \
             FROM audit_events \
             WHERE tenant_id = $1 \
               AND ($2::bigint IS NULL OR seq >= $2) \
               AND ($3::bigint IS NULL OR seq <= $3) \
             ORDER BY seq ASC",
        )
        .bind(tenant_id)
        .bind(from_seq)
        .bind(to_seq)
        .fetch_all(&self.pool)
        .await?;
        let entries: Vec<AuditEntry> = rows.into_iter().map(row_to_entry).collect::<Result<_,_>>()?;
        let checked = entries.len() as i64;
        match recon_audit::chain::verify(&entries, expected_prev_hash) {
            Ok(()) => Ok(recon_audit::chain::VerifyOutcome::valid(checked)),
            Err(e) => Ok(recon_audit::chain::VerifyOutcome::invalid(checked, e)),
        }
    }
}

fn row_to_entry(
    r: (String, i64, time::OffsetDateTime, String, String, serde_json::Value, Vec<u8>, Vec<u8>),
) -> Result<AuditEntry, StoreError> {
    let (tenant_id, seq, at, actor_id, kind_str, payload, prev_hash, hash) = r;
    let kind = recon_audit::AuditKind::from_str(&kind_str)
        .ok_or_else(|| StoreError::Db(sqlx::Error::Decode(format!("unknown audit kind {kind_str}").into())))?;
    let payload: recon_audit::AuditPayload = serde_json::from_value(payload)?;
    let prev_hash = vec_to_arr32(prev_hash)?;
    let hash = vec_to_arr32(hash)?;
    let at = at.format(&Rfc3339).map_err(|_| StoreError::Db(sqlx::Error::Decode("rfc3339".into())))?;
    Ok(AuditEntry { tenant_id, seq, at, actor_id, kind, payload, prev_hash, hash })
}
```

- [ ] **Step 2: Add `VerifyOutcome` to `recon-audit::chain`**

In `backend/crates/recon-audit/src/chain.rs`, append:

```rust
/// API-shape outcome of running `verify` on a stored range.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyOutcome {
    pub status: VerifyStatus,
    pub checked: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_broken_seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<VerifyReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyStatus { Valid, Invalid }

impl VerifyOutcome {
    pub fn valid(checked: i64) -> Self {
        Self { status: VerifyStatus::Valid, checked, first_broken_seq: None, reason: None }
    }
    pub fn invalid(checked: i64, e: VerifyError) -> Self {
        Self { status: VerifyStatus::Invalid, checked, first_broken_seq: Some(e.seq), reason: Some(e.reason) }
    }
}
```

- [ ] **Step 3: Append store tests**

Append to `backend/crates/recon-store/tests/audit_schema.rs`:

```rust
use recon_audit::chain::VerifyStatus;
use recon_store::audit::AuditFilter;

#[sqlx::test(migrations = "../../migrations")]
async fn list_and_verify_round_trip(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    for i in 0..5 {
        store.append_audit(&mut tx, "t", "system",
            AuditPayload::AuthLogout { user_id: format!("user-{i}"), ip: None }).await.unwrap();
    }
    tx.commit().await.unwrap();

    let page = store.list_audit("t", &AuditFilter { limit: 100, ..Default::default() }).await.unwrap();
    assert_eq!(page.items.len(), 5);
    assert_eq!(page.items.first().unwrap().seq, 5, "descending");
    assert_eq!(page.items.last().unwrap().seq, 1);
    assert!(page.next_cursor.is_none());

    let outcome = store.verify_audit("t", None, None, None).await.unwrap();
    assert_eq!(outcome.status, VerifyStatus::Valid);
    assert_eq!(outcome.checked, 5);
}

#[sqlx::test(migrations = "../../migrations")]
async fn verify_detects_payload_tamper(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    store.append_audit(&mut tx, "t", "system",
        AuditPayload::AuthLogout { user_id: "u1".into(), ip: None }).await.unwrap();
    store.append_audit(&mut tx, "t", "system",
        AuditPayload::AuthLogout { user_id: "u2".into(), ip: None }).await.unwrap();
    tx.commit().await.unwrap();

    // Manually tamper with row seq=2.
    sqlx::query(
        "UPDATE audit_events SET payload = jsonb_set(payload, '{data,user_id}', '\"evil\"') \
         WHERE tenant_id='t' AND seq=2",
    )
    .execute(&store.pool).await.unwrap();

    let outcome = store.verify_audit("t", None, None, None).await.unwrap();
    assert_eq!(outcome.status, VerifyStatus::Invalid);
    assert_eq!(outcome.first_broken_seq, Some(2));
    assert_eq!(outcome.reason, Some(recon_audit::chain::VerifyReason::Tampered));
}
```

- [ ] **Step 4: Run the tests**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store`
Expected: all store tests PASS (existing + new).

- [ ] **Step 5: Commit**

```bash
git add backend/crates/recon-store/src/audit.rs backend/crates/recon-audit/src/chain.rs backend/crates/recon-store/tests/audit_schema.rs
git commit -m "feat(store): list_audit + verify_audit; VerifyOutcome wire type

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task B4: `anchor_now` + `list_anchors`

**Files:**
- Modify: `backend/crates/recon-store/src/audit.rs`
- Modify: `backend/crates/recon-store/tests/audit_schema.rs`

- [ ] **Step 1: Implement anchor**

Append to `backend/crates/recon-store/src/audit.rs`:

```rust
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    pub anchor_seq: i64,
    pub at: String,
    pub tenant_heads: BTreeMap<String, TenantHead>,
    pub prev_hash: Vec<u8>,  // serialized as hex in API layer
    pub hash: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantHead { pub seq: i64, pub hash: Vec<u8> }

impl Store {
    /// Snapshot every tenant's current head and append a new global anchor row.
    /// Emits one `system.anchor.created` event per affected tenant inside the
    /// same transaction (so each tenant's chain self-describes the anchor).
    pub async fn anchor_now(&self) -> Result<Anchor, StoreError> {
        let mut tx = self.pool.begin().await?;

        // 1. Each tenant's current head.
        let head_rows: Vec<(String, i64, Vec<u8>)> = sqlx::query_as(
            "SELECT ae.tenant_id, ae.seq, ae.hash FROM audit_events ae \
             WHERE ae.seq = (SELECT max(seq) FROM audit_events WHERE tenant_id = ae.tenant_id) \
             ORDER BY ae.tenant_id",
        )
        .fetch_all(&mut *tx)
        .await?;

        let mut tenant_heads = BTreeMap::new();
        for (tid, seq, hash) in &head_rows {
            tenant_heads.insert(tid.clone(), TenantHead { seq: *seq, hash: hash.clone() });
        }

        // 2. Previous anchor.
        let prev: Option<(i64, Vec<u8>)> = sqlx::query_as(
            "SELECT anchor_seq, hash FROM audit_anchors ORDER BY anchor_seq DESC LIMIT 1 FOR UPDATE",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let (prev_seq, prev_hash_vec) = prev.unwrap_or((0, vec![0u8; 32]));
        let anchor_seq = prev_seq + 1;

        let now = time::OffsetDateTime::now_utc();
        let at = now.format(&Rfc3339).map_err(|_| StoreError::Db(sqlx::Error::Decode("rfc3339".into())))?;

        // 3. Compute the anchor hash: prev_hash || u64-BE(anchor_seq) || at || sorted-keys-JSON(tenant_heads).
        let tenant_heads_json = serde_json::to_value(&tenant_heads)?;
        let tenant_heads_bytes = serde_json::to_vec(&tenant_heads_json)?;
        let mut hasher = Sha256::new();
        hasher.update(&prev_hash_vec);
        hasher.update((anchor_seq as u64).to_be_bytes());
        hasher.update(at.as_bytes());
        hasher.update(&tenant_heads_bytes);
        let hash: [u8; 32] = hasher.finalize().into();

        // 4. Insert the anchor row.
        sqlx::query(
            "INSERT INTO audit_anchors(anchor_seq, at, tenant_heads, prev_hash, hash) \
             VALUES ($1, $2::timestamptz, $3, $4, $5)",
        )
        .bind(anchor_seq)
        .bind(&at)
        .bind(&tenant_heads_json)
        .bind(prev_hash_vec.as_slice())
        .bind(hash.as_slice())
        .execute(&mut *tx)
        .await?;

        // 5. Emit a per-tenant system.anchor.created so each tenant chain self-describes.
        let tenant_count = head_rows.len() as i64;
        for (tid, _seq, _h) in &head_rows {
            self.append_audit(
                &mut tx,
                tid,
                "system",
                AuditPayload::SystemAnchorCreated { anchor_seq, tenant_count },
            )
            .await?;
        }

        tx.commit().await?;

        Ok(Anchor {
            anchor_seq,
            at,
            tenant_heads,
            prev_hash: prev_hash_vec,
            hash: hash.to_vec(),
        })
    }

    pub async fn list_anchors(&self, limit: i64) -> Result<Vec<Anchor>, StoreError> {
        let limit = limit.clamp(1, 200);
        let rows: Vec<(i64, time::OffsetDateTime, serde_json::Value, Vec<u8>, Vec<u8>)> = sqlx::query_as(
            "SELECT anchor_seq, at, tenant_heads, prev_hash, hash FROM audit_anchors ORDER BY anchor_seq DESC LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut out = Vec::with_capacity(rows.len());
        for (anchor_seq, at, tenant_heads, prev_hash, hash) in rows {
            let at = at.format(&Rfc3339).map_err(|_| StoreError::Db(sqlx::Error::Decode("rfc3339".into())))?;
            let tenant_heads: BTreeMap<String, TenantHead> = serde_json::from_value(tenant_heads)?;
            out.push(Anchor { anchor_seq, at, tenant_heads, prev_hash, hash });
        }
        Ok(out)
    }
}
```

- [ ] **Step 2: Append test**

Append to `backend/crates/recon-store/tests/audit_schema.rs`:

```rust
#[sqlx::test(migrations = "../../migrations")]
async fn anchor_now_writes_anchor_and_per_tenant_event(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t'),('u','U','u')")
        .execute(&store.pool).await.unwrap();
    // Seed an event per tenant so each has a head.
    let mut tx = store.pool.begin().await.unwrap();
    store.append_audit(&mut tx, "t", "system",
        AuditPayload::AuthLogout { user_id: "u1".into(), ip: None }).await.unwrap();
    store.append_audit(&mut tx, "u", "system",
        AuditPayload::AuthLogout { user_id: "u2".into(), ip: None }).await.unwrap();
    tx.commit().await.unwrap();

    let anchor = store.anchor_now().await.unwrap();
    assert_eq!(anchor.anchor_seq, 1);
    assert_eq!(anchor.tenant_heads.len(), 2);
    assert!(anchor.tenant_heads.contains_key("t"));
    assert!(anchor.tenant_heads.contains_key("u"));

    // Each tenant gained a `system.anchor.created` row.
    let n_t: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE tenant_id='t' AND kind='system.anchor.created'",
    )
    .fetch_one(&store.pool).await.unwrap();
    assert_eq!(n_t, 1);

    // List anchors returns the row.
    let anchors = store.list_anchors(10).await.unwrap();
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].anchor_seq, 1);
}
```

- [ ] **Step 3: Run the test + full backend regression**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store && cargo clippy -p recon-store -- -D warnings`
Expected: all pass; clippy clean.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/recon-store/src/audit.rs backend/crates/recon-store/tests/audit_schema.rs
git commit -m "feat(store): anchor_now + list_anchors with per-tenant chain emission

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase C — Emission wiring (refactor existing handlers to thread tx + emit)

> **Pattern for this phase:** each store write method that already runs its
> own `pool.begin()` gets an inline `self.append_audit(&mut tx, …)` call before
> commit. Auth-flow API handlers (`routes_auth`) that previously did sequential
> store calls are wrapped in `state.store.pool.begin()` plus a final
> `append_audit`. The audit emission is part of the SAME transaction as the
> action; if it errors, the action rolls back.

### Task C1: `ViewAudit` permission

**Files:**
- Modify: `backend/crates/recon-auth/src/rbac.rs`

- [ ] **Step 1: Add the permission**

In `backend/crates/recon-auth/src/rbac.rs`, extend the enum:

```rust
pub enum Permission { ViewRecon, AssignBreak, ProposeResolution, ApproveResolution, ManageUsers, ManageData, ViewAudit }
```

In `permitted`, add `ViewAudit` to the admin-only arm:

```rust
    match perm {
        ViewRecon | AssignBreak | ProposeResolution | ManageData => true,
        ApproveResolution => matches!(role, Approver | Admin),
        ManageUsers | ViewAudit => matches!(role, Admin),
    }
```

Add a test in `mod tests`:

```rust
    #[test]
    fn view_audit_admin_only() {
        assert!(!permitted(Operator, Permission::ViewAudit));
        assert!(!permitted(Approver, Permission::ViewAudit));
        assert!(permitted(Admin, Permission::ViewAudit));
    }
```

- [ ] **Step 2: Run + commit**

Run: `cd backend && cargo test -p recon-auth rbac`
Expected: PASS (including existing matrix tests).

```bash
git add backend/crates/recon-auth/src/rbac.rs
git commit -m "feat(auth): ViewAudit permission (admin-only)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task C2: Case-event emission (`assign_break`, `append_case_event`)

**Files:**
- Modify: `backend/crates/recon-store/src/write.rs`

- [ ] **Step 1: Emit on `assign_break`**

In `backend/crates/recon-store/src/write.rs::assign_break`, BEFORE `tx.commit()`, add (the `tx` is already in scope; `actor_id`/`tenant_id`/`assignee_id`/`brk.case_id`/`break_id` are all available):

```rust
        self.append_audit(
            &mut tx,
            tenant_id,
            actor_id,
            recon_audit::AuditPayload::CaseAssigned {
                case_id: brk.case_id.clone(),
                break_id: break_id.to_string(),
                assignee_id: assignee_id.to_string(),
            },
        ).await?;
```

(Add `use recon_audit;` to imports at the top if not already imported via the crate root.)

- [ ] **Step 2: Emit on `append_case_event`**

In `backend/crates/recon-store/src/write.rs::append_case_event`, BEFORE `tx.commit()` and AFTER the case-event INSERT, add:

```rust
        // The `kind` string was already extracted above from `ev.body` (e.g. "approved",
        // "approval_requested"). Mirror it into the audit chain.
        self.append_audit(
            &mut tx,
            tenant_id,
            &ev.actor_id,
            recon_audit::AuditPayload::CaseEventAppended {
                case_id: case_id.to_string(),
                break_id: case_snapshot.break_id.clone(),
                event_kind: kind.clone(),
            },
        ).await?;
```

- [ ] **Step 3: Update tests**

The existing tests of `assign_break` and `append_case_event` should still pass (the new emission is invisible to them) — they're tested through their public Store methods. Run:

`cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store`

Expected: all existing PASS. Add a new integration assertion in `backend/crates/recon-store/tests/audit_schema.rs`:

```rust
#[sqlx::test(migrations = "../../migrations")]
async fn assign_break_emits_audit(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    // Seed minimal tenant/user/membership/case/break.
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('u1','U','u@x',false),('u2','V','v@x',false)").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('u1','t','operator'),('u2','t','approver')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ('s','t','bank','S','GBP')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO reconciliation_runs(id,tenant_id,name,source_a_id,source_b_id,status,started_at,completed_at,config_version,stats) VALUES ('r','t','R','s','s','completed', now(), now(), 'v1', '{}'::jsonb)").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO cases(id,tenant_id,break_id,status) VALUES ('c','t','b','open')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO breaks(id,tenant_id,run_id,case_id,type,status,value_minor,currency,txn_ids,opened_at) VALUES ('b','t','r','c','unmatched','open',0,'GBP','{}', now())").execute(&store.pool).await.unwrap();

    store.assign_break("t", "b", "u1", "u2").await.unwrap();

    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE tenant_id='t' AND kind='case.assigned'",
    )
    .fetch_one(&store.pool).await.unwrap();
    assert_eq!(n, 1, "case.assigned audit emitted");
}
```

- [ ] **Step 4: Run + commit**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store`
Expected: all PASS.

```bash
git add backend/crates/recon-store/src/write.rs backend/crates/recon-store/tests/audit_schema.rs
git commit -m "feat(audit): emit case.assigned and case.event_appended in same tx

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task C3: Data-plane emission (`create_source`, `ingest_transactions`, `create_run`)

**Files:**
- Modify: `backend/crates/recon-store/src/sources.rs` (`create_source`, `ingest_transactions`)
- Modify: `backend/crates/recon-store/src/runs.rs` (`create_run`)

- [ ] **Step 1: Refactor `create_source` to use a transaction + emit**

`create_source` currently uses `&self.pool` directly. Convert to a transaction so the audit emission rides along:

```rust
    pub async fn create_source(
        &self,
        tenant_id: &str,
        kind: SourceKind,
        name: &str,
        currency: &str,
        actor_id: &str,    // <-- new parameter
    ) -> Result<Source, StoreError> {
        let id = format!("src-{}", Uuid::new_v4());
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ($1,$2,$3,$4,$5)")
            .bind(&id).bind(tenant_id).bind(kind_str(kind)).bind(name).bind(currency)
            .execute(&mut *tx).await?;
        self.append_audit(
            &mut tx,
            tenant_id,
            actor_id,
            recon_audit::AuditPayload::DataSourceCreated {
                source_id: id.clone(), kind: kind_str(kind).to_string(),
                currency: currency.to_string(), name: name.to_string(),
            },
        ).await?;
        tx.commit().await?;
        Ok(Source { id, tenant_id: tenant_id.to_string(), kind, name: name.to_string(), currency: currency.to_string() })
    }
```

Update the existing call site in `routes.rs::create_source` to pass `&ctx.user_id`.

- [ ] **Step 2: Emit on `ingest_transactions`**

`ingest_transactions` already runs a transaction (`self.pool.begin()`). Add the audit emission BEFORE `tx.commit()` and adjust the signature to take the file metadata + actor for the audit payload:

```rust
    pub async fn ingest_transactions(
        &self,
        tenant_id: &str,
        source_id: &str,
        txns: &[CanonicalTransaction],
        actor_id: &str,           // <-- new
        file_sha256: &str,        // <-- new (the API layer hashes the multipart bytes)
        file_format: &str,        // <-- new ("csv" | "camt053")
        file_bytes: i64,          // <-- new
    ) -> Result<usize, StoreError> {
        // ... existing dup-check + insert loop ...
        self.append_audit(
            &mut tx,
            tenant_id,
            actor_id,
            recon_audit::AuditPayload::DataIngestCompleted {
                source_id: source_id.to_string(),
                format: file_format.to_string(),
                file_sha256: file_sha256.to_string(),
                bytes: file_bytes,
                ingested: txns.len() as i64,
            },
        ).await?;
        tx.commit().await?;
        Ok(txns.len())
    }
```

Update the call site in `routes.rs::ingest_source` to compute `Sha256::digest(&bytes)` from the uploaded file's raw bytes (already available in scope) and pass through.

- [ ] **Step 3: Emit on `create_run`**

`create_run` already runs a transaction. Add audit emission BEFORE `tx.commit()`:

```rust
        self.persist_run(&mut tx, &run_id, tenant_id, name, source_a_id, source_b_id, &started, &result, &cfg).await?;
        // Audit row inside the same tx.
        self.append_audit(
            &mut tx,
            tenant_id,
            actor_id, // <-- new param on create_run
            recon_audit::AuditPayload::DataRunCreated {
                run_id: run_id.clone(),
                source_a_id: source_a_id.to_string(),
                source_b_id: source_b_id.to_string(),
                from: from.to_string(),
                to: to.to_string(),
                matched: result.stats.matched,
                unmatched: result.stats.unmatched,
            },
        ).await?;
        tx.commit().await?;
```

Add `actor_id: &str` to `create_run`'s signature; update its call site in `routes.rs::create_run`.

- [ ] **Step 4: Update existing store tests for the new signatures**

The store tests in `backend/crates/recon-store/tests/ingest.rs` call `create_source`, `ingest_transactions`, `create_run` — pass an `"actor"` string and dummy file metadata where needed. Pattern:

```rust
store.create_source("t", SourceKind::Bank, "Acme Bank", "GBP", "actor").await.unwrap();
// ...
store.ingest_transactions("t", &bank.id, &txns, "actor", "00", "csv", 0).await.unwrap();
// ...
store.create_run("t", "Test run", &bank.id, &ledger.id, "2026-05-01", "2026-05-31", "actor").await.unwrap();
```

- [ ] **Step 5: Run + commit**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store`
Expected: all PASS (existing tests with new actor args + the audit emission is invisible to them; assertions below cover the emission).

Add an assertion to `audit_schema.rs`:

```rust
#[sqlx::test(migrations = "../../migrations")]
async fn create_source_emits_audit(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')").execute(&store.pool).await.unwrap();
    store.create_source("t", recon_domain::SourceKind::Bank, "S", "GBP", "actor").await.unwrap();
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE tenant_id='t' AND kind='data.source.created'",
    ).fetch_one(&store.pool).await.unwrap();
    assert_eq!(n, 1);
}
```

```bash
git add backend/crates/recon-store backend/crates/recon-api/src/routes.rs
git commit -m "feat(audit): emit data.source.created/ingest.completed/run.created same-tx

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task C4: Admin user management emission

**Files:**
- Modify: `backend/crates/recon-store/src/auth.rs` (`create_user_with_membership`, `update_membership_role`, `set_user_disabled`, `remove_membership`)

- [ ] **Step 1: Add audit emission to each user-management method**

Each of these methods either already runs a transaction or can be wrapped in one. For each, add the matching `append_audit` call inside the same tx before commit. New parameter `actor_id: &str` on each method, and (for role changes) `previous_role: &str`:

For `create_user_with_membership`, emit `AdminUserCreated`:

```rust
        self.append_audit(
            &mut tx,
            tenant_id,
            actor_id,
            recon_audit::AuditPayload::AdminUserCreated {
                user_id: user_id.clone(),
                email: email.to_string(),
                role: role.as_str().to_string(),
            },
        ).await?;
```

For `update_membership_role`, fetch the current role first, then emit `AdminUserRoleChanged { from, to }`.

For `set_user_disabled` (when disabling), emit `AdminUserDisabled`; when enabling, emit `AdminUserEnabled`.

For `remove_membership`, emit `AdminUserRemoved`.

Update call sites in `backend/crates/recon-api/src/routes_users.rs` to pass `&ctx.user_id` as `actor_id`.

- [ ] **Step 2: Test one path end-to-end**

Append to `audit_schema.rs`:

```rust
#[sqlx::test(migrations = "../../migrations")]
async fn admin_create_user_emits_audit(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('admin','Admin','a@x',false)").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('admin','t','admin')").execute(&store.pool).await.unwrap();
    let hash = "$argon2id$v=19$m=19456,t=2,p=1$abc$def".to_string(); // dummy hash; not verified here
    store.create_user_with_membership(
        "t", "Bob", "bob@x", recon_domain::UserRole::Operator, &hash, "admin",
    ).await.unwrap();
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE tenant_id='t' AND kind='admin.user.created'",
    ).fetch_one(&store.pool).await.unwrap();
    assert_eq!(n, 1);
}
```

- [ ] **Step 3: Run + commit**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store -p recon-api`
Expected: all PASS.

```bash
git add backend/crates/recon-store/src/auth.rs backend/crates/recon-api/src/routes_users.rs backend/crates/recon-store/tests/audit_schema.rs
git commit -m "feat(audit): emit admin.user.* events for user-management actions

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task C5: Auth-flow emission (wrap each handler in a single tx)

**Files:**
- Modify: `backend/crates/recon-api/src/routes_auth.rs`
- Modify: `backend/crates/recon-store/src/auth.rs` (add tx-taking variants where the existing methods use `&self.pool`)

> The auth-flow handlers (`login`, `logout`, `refresh`, `change_password`,
> `forgot`, `reset`, `switch_tenant`) currently perform sequential, non-transactional
> store calls. To make audit emission same-tx we wrap each handler's DB ops in a
> single tx and have it call existing store methods through tx-accepting wrappers.

- [ ] **Step 1: Add tx-accepting variants in `recon-store/src/auth.rs`**

For each existing method called from `routes_auth.rs` (e.g. `reset_login_failures`,
`record_login_failure`, `revoke_refresh_by_hash`, etc.), add a corresponding
`*_tx` variant that takes `&mut sqlx::PgConnection`. Example for `reset_login_failures`:

```rust
    pub async fn reset_login_failures_tx(
        &self, tx: &mut sqlx::PgConnection, user_id: &str,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE user_credentials SET failed_attempts = 0, locked_until = NULL WHERE user_id = $1")
            .bind(user_id).execute(&mut *tx).await?;
        Ok(())
    }
```

Apply the same pattern to the other methods routes_auth uses (each is a single SQL statement — mechanical).

- [ ] **Step 2: Refactor each handler in `routes_auth.rs` to use one tx + emit**

For `login`, the new shape is:

```rust
    let mut tx = state.store.pool.begin().await?;
    // ... (existing reads can stay on the pool; only mutations + the audit go through tx)
    state.store.reset_login_failures_tx(&mut tx, &user.id).await?;
    // refresh-token insert via the tx variant
    state.store.insert_refresh_tx(&mut tx, &user.id, &active_tenant.id, &hashed_refresh, expires_at).await?;
    state.store.append_audit(
        &mut tx,
        &active_tenant.id,
        &user.id,
        recon_audit::AuditPayload::AuthLoginSuccess {
            user_id: user.id.clone(),
            email: user.email.clone(),
            ip: extract_ip(&parts),
        },
    ).await?;
    tx.commit().await?;
```

Similar treatment for `logout`, `refresh`, `change_password`, `forgot`, `reset`, `switch_tenant`. The audit payload per kind is taken from the contract block above.

Add a helper `fn extract_ip(parts: &http::request::Parts) -> Option<String>` that reads `X-Forwarded-For` (first value) or returns `None`.

- [ ] **Step 3: Test that each emits**

Append to `backend/crates/recon-api/tests/api.rs` (the existing api integration test file):

```rust
#[sqlx::test(migrations = "../../migrations")]
async fn login_success_emits_audit(pool: sqlx::PgPool) {
    // ... reuse the existing fixture-setup pattern from this file ...
    let (app, cfg) = recon_api::test_app(pool.clone());
    let body = serde_json::json!({ "email": "mia@acme.test", "password": "Password123!" });
    let req = Request::builder().method("POST").uri("/auth/login")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string())).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE kind='auth.login.success'",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(n, 1);
}
```

Mirror tests for `auth.login.failure`, `auth.logout`, `auth.password.changed`, `auth.password.reset_requested` / `_completed`, `auth.refresh.reused`, `auth.tenant.switched`.

- [ ] **Step 4: Run + commit**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api`
Expected: all PASS.

```bash
git add backend/crates/recon-api/src/routes_auth.rs backend/crates/recon-store/src/auth.rs backend/crates/recon-api/tests/api.rs
git commit -m "feat(audit): wrap auth flows in one tx and emit auth.* events same-tx

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase D — API surface + scheduler

### Task D1: `routes_audit.rs` + register routes

**Files:**
- Create: `backend/crates/recon-api/src/routes_audit.rs`
- Modify: `backend/crates/recon-api/src/routes.rs` (router registration)
- Modify: `backend/crates/recon-api/Cargo.toml` (add `recon-audit`)

- [ ] **Step 1: Add the crate dep**

In `backend/crates/recon-api/Cargo.toml`:

```toml
recon-audit = { path = "../recon-audit" }
```

- [ ] **Step 2: Write the audit routes**

`backend/crates/recon-api/src/routes_audit.rs`:

```rust
use crate::auth::AuthContext;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Query, State};
use axum::Json;
use recon_audit::CONTROLS;
use recon_store::audit::AuditFilter;
use serde::Deserialize;
use serde_json::{json, Value};

fn require_view_audit(ctx: &AuthContext) -> Result<(), ApiError> {
    recon_auth::rbac::require(ctx.role, recon_auth::rbac::Permission::ViewAudit)
        .map_err(|_| ApiError::Forbidden())
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ListAuditQ {
    pub from: Option<String>,
    pub to: Option<String>,
    #[serde(default)]
    pub kind: Vec<String>,
    pub actor_id: Option<String>,
    pub limit: Option<i64>,
    pub before: Option<i64>,
}

pub async fn list_audit(
    State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListAuditQ>,
) -> Result<Json<Value>, ApiError> {
    require_view_audit(&ctx)?;
    let kinds = q.kind.iter()
        .filter_map(|k| recon_audit::AuditKind::from_str(k))
        .collect::<Vec<_>>();
    let f = AuditFilter {
        from: q.from, to: q.to, kinds, actor_id: q.actor_id,
        limit: q.limit.unwrap_or(100),
        before: q.before,
    };
    let page = s.store.list_audit(&ctx.tenant_id, &f).await?;
    Ok(Json(json!({
        "items": page.items.iter().map(audit_entry_json).collect::<Vec<_>>(),
        "nextCursor": page.next_cursor,
    })))
}

fn audit_entry_json(e: &recon_audit::AuditEntry) -> Value {
    json!({
        "tenantId": e.tenant_id,
        "seq": e.seq,
        "at": e.at,
        "actorId": e.actor_id,
        "kind": e.kind.as_str(),
        "payload": serde_json::to_value(&e.payload).unwrap().get("data").cloned().unwrap_or(Value::Null),
        "prevHash": hex::encode(e.prev_hash),
        "hash": hex::encode(e.hash),
    })
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReq {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub expected_prev_hash: Option<String>, // hex
}

pub async fn verify_audit(
    State(s): State<AppState>, ctx: AuthContext, Json(body): Json<VerifyReq>,
) -> Result<Json<Value>, ApiError> {
    require_view_audit(&ctx)?;
    let expected = match body.expected_prev_hash {
        Some(h) => Some(hex_to_arr32(&h)?),
        None => None,
    };
    let outcome = s.store.verify_audit(&ctx.tenant_id, body.from, body.to, expected).await?;
    Ok(Json(serde_json::to_value(&outcome).unwrap()))
}

fn hex_to_arr32(s: &str) -> Result<[u8; 32], ApiError> {
    let v = hex::decode(s).map_err(|_| ApiError::BadRequest())?;
    if v.len() != 32 { return Err(ApiError::BadRequest()); }
    let mut a = [0u8; 32];
    a.copy_from_slice(&v);
    Ok(a)
}

pub async fn anchor_audit(
    State(s): State<AppState>, ctx: AuthContext,
) -> Result<Json<Value>, ApiError> {
    require_view_audit(&ctx)?;
    let a = s.store.anchor_now().await?;
    Ok(Json(json!({ "anchorSeq": a.anchor_seq, "hash": hex::encode(&a.hash) })))
}

#[derive(Deserialize, Default)]
pub struct ListAnchorsQ { pub limit: Option<i64> }

pub async fn list_anchors(
    State(s): State<AppState>, ctx: AuthContext, Query(q): Query<ListAnchorsQ>,
) -> Result<Json<Value>, ApiError> {
    require_view_audit(&ctx)?;
    let limit = q.limit.unwrap_or(50);
    let anchors = s.store.list_anchors(limit).await?;
    let items: Vec<Value> = anchors.into_iter().map(|a| json!({
        "anchorSeq": a.anchor_seq,
        "at": a.at,
        "tenantHeads": serde_json::to_value(&a.tenant_heads).unwrap(),
        "prevHash": hex::encode(&a.prev_hash),
        "hash": hex::encode(&a.hash),
    })).collect();
    Ok(Json(serde_json::to_value(&items).unwrap()))
}

/// Open to any authenticated member — controls metadata is non-sensitive.
pub async fn list_controls(_ctx: AuthContext) -> Result<Json<Value>, ApiError> {
    Ok(Json(serde_json::to_value(CONTROLS).unwrap()))
}
```

- [ ] **Step 3: Register routes**

In `backend/crates/recon-api/src/routes.rs::router(...)`, before `.with_state(state)`:

```rust
        .route("/api/audit", get(crate::routes_audit::list_audit))
        .route("/api/audit/verify", post(crate::routes_audit::verify_audit))
        .route("/api/audit/anchor", post(crate::routes_audit::anchor_audit))
        .route("/api/audit/anchors", get(crate::routes_audit::list_anchors))
        .route("/api/audit/controls", get(crate::routes_audit::list_controls))
```

Add `pub mod routes_audit;` to `lib.rs`.

- [ ] **Step 4: Test routes**

Append to `backend/crates/recon-api/tests/api.rs` integration suite an admin-vs-non-admin RBAC test for `/api/audit`, a verify-on-clean-chain test (status:"valid"), and a verify-after-tamper test (status:"invalid"). The fixture pattern is already in the file — mirror it.

- [ ] **Step 5: Run + commit**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api`
Expected: all PASS.

```bash
git add backend/crates/recon-api/Cargo.toml backend/crates/recon-api/src/lib.rs backend/crates/recon-api/src/routes_audit.rs backend/crates/recon-api/src/routes.rs backend/crates/recon-api/tests/api.rs
git commit -m "feat(api): audit routes (list/verify/anchor/anchors/controls) + RBAC

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task D2: Anchor scheduler

**Files:**
- Create: `backend/crates/recon-api/src/scheduler.rs`
- Modify: `backend/crates/recon-api/src/main.rs`
- Modify: `backend/crates/recon-api/src/lib.rs` (`pub mod scheduler;`)

- [ ] **Step 1: Write the scheduler**

`backend/crates/recon-api/src/scheduler.rs`:

```rust
use recon_store::Store;
use std::time::Duration;

/// Spawn the audit-anchor background loop. Reads `AUDIT_ANCHOR_INTERVAL_SECS`
/// (default 3600). Runs forever; errors are logged but do not stop the loop.
pub fn spawn_anchor_loop(store: Store) {
    let secs: u64 = std::env::var("AUDIT_ANCHOR_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);
    let interval = Duration::from_secs(secs);
    tokio::spawn(async move {
        // Wait one interval before the first anchor so app startup isn't blocked.
        tokio::time::sleep(interval).await;
        loop {
            match store.anchor_now().await {
                Ok(a) => tracing::info!(anchor_seq = a.anchor_seq, "audit anchor written"),
                Err(e) => tracing::error!(error = %e, "audit anchor failed"),
            }
            tokio::time::sleep(interval).await;
        }
    });
}
```

- [ ] **Step 2: Start the loop in `main.rs`**

In `backend/crates/recon-api/src/main.rs`, after `let store = Store::connect(...)?;` and before `axum::serve`:

```rust
    recon_api::scheduler::spawn_anchor_loop(store.clone());
```

- [ ] **Step 3: Register the module**

In `backend/crates/recon-api/src/lib.rs`:

```rust
pub mod scheduler;
```

- [ ] **Step 4: Commit**

(No new test — the loop is exercised via the `POST /api/audit/anchor` endpoint already covered, and via the E2E in Phase F.)

```bash
git add backend/crates/recon-api/src/scheduler.rs backend/crates/recon-api/src/main.rs backend/crates/recon-api/src/lib.rs
git commit -m "feat(api): tokio interval scheduler for audit anchors

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase E — Frontend

### Task E1: `ApiClient` types + Http + Mock additions

**Files:**
- Modify: `web/lib/api/client.ts`
- Modify: `web/lib/api/http.ts`
- Modify: `web/lib/api/mock.ts`
- Modify: `web/lib/api/http.test.ts` + `web/lib/api/mock.test.ts`

- [ ] **Step 1: Add types + interface methods**

In `web/lib/api/client.ts`, append the types and `ApiClient` method signatures from the **Shared type contract** block at the top of this plan (`AuditKind`, `AuditEvent`, `AuditPage`, `AuditQuery`, `VerifyRequest`, `VerifyResult`, `Anchor`, `Control`, and the five `listAudit`/`verifyAudit`/`anchorAudit`/`listAnchors`/`listControls` methods).

- [ ] **Step 2: Implement on `HttpApiClient`**

In `web/lib/api/http.ts`:

```ts
  listAudit(tenantId: string, q?: AuditQuery): Promise<AuditPage> {
    const params = new URLSearchParams();
    if (q?.from) params.append("from", q.from);
    if (q?.to) params.append("to", q.to);
    for (const k of q?.kind ?? []) params.append("kind", k);
    if (q?.actorId) params.append("actorId", q.actorId);
    if (q?.limit) params.append("limit", String(q.limit));
    if (q?.before) params.append("before", String(q.before));
    const qs = params.toString();
    return this.req(`/api/audit${qs ? `?${qs}` : ""}`, tenantId);
  }
  verifyAudit(tenantId: string, body: VerifyRequest): Promise<VerifyResult> {
    return this.req("/api/audit/verify", tenantId, { method: "POST", body: JSON.stringify(body) });
  }
  anchorAudit(tenantId: string): Promise<{ anchorSeq: number; hash: string }> {
    return this.req("/api/audit/anchor", tenantId, { method: "POST" });
  }
  listAnchors(tenantId: string, limit?: number): Promise<Anchor[]> {
    return this.req(`/api/audit/anchors${limit ? `?limit=${limit}` : ""}`, tenantId);
  }
  listControls(): Promise<Control[]> { return this.req("/api/audit/controls", null); }
```

- [ ] **Step 3: Implement on `MockApiClient`**

In `web/lib/api/mock.ts`, add an in-memory `auditEvents` array to `Fixtures` (or local class field). Implement each method against it. `verifyAudit` always returns `valid` in the mock (no real chain). `listControls` returns a deterministic small list (one ISO27001 entry, one SOC2 entry, one FCA entry) mirroring the shape of the backend response.

Add tests in `mock.test.ts` for each method.

- [ ] **Step 4: Run typecheck + tests**

Run: `pnpm -C web tsc --noEmit && pnpm -C web test`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add web/lib/api
git commit -m "feat(web): ApiClient audit/anchors/controls methods + types

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task E2: `useAudit`/`useAnchors`/`useControls` hooks

**Files:**
- Create: `web/lib/hooks/use-audit.ts`

- [ ] **Step 1: Write the hooks**

`web/lib/hooks/use-audit.ts`:

```ts
import { useQuery } from "@tanstack/react-query";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import type { AuditQuery } from "@/lib/api/client";

export function useAudit(q?: AuditQuery) {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["audit", tenantId, q],
    queryFn: () => api.listAudit(tenantId, q),
  });
}

export function useAnchors(limit = 50) {
  const api = useApi();
  const { tenantId } = useTenant();
  return useQuery({
    queryKey: ["audit", "anchors", tenantId, limit],
    queryFn: () => api.listAnchors(tenantId, limit),
  });
}

export function useControls() {
  const api = useApi();
  return useQuery({
    queryKey: ["audit", "controls"],
    queryFn: () => api.listControls(),
  });
}
```

- [ ] **Step 2: Commit (no test yet — hooks tested through the screen tests)**

```bash
git add web/lib/hooks/use-audit.ts
git commit -m "feat(web): useAudit/useAnchors/useControls hooks

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task E3: Audit Log screen

**Files:**
- Create: `web/app/(app)/audit/page.tsx`
- Create: `web/tests/audit-page.test.tsx`

- [ ] **Step 1: Build the screen**

`web/app/(app)/audit/page.tsx` — a client component admin-gated via `useAuth()` (redirect non-admins to /dashboard like Users page). Reuse the same filter/table pattern as Runs page. Include:

- Filter bar: `kind` multi-select (from `AuditKind` enum), `actorId` (from `useMembers()`), date range, all URL-persisted via nuqs.
- Table columns: time, actor (resolved name), kind (badge), payload summary (`JSON.stringify` truncated), prev/hash short-form (first 8 hex chars with a click-to-copy icon).
- Toolbar buttons: **Verify chain** opens a dialog with `from`/`to` seq fields → calls `verifyAudit` → renders `status` (green/red), `checked`, `firstBrokenSeq` and `reason` if invalid. **Anchor now** calls `anchorAudit` → toasts `anchorSeq`.
- Collapsible Anchor history (uses `useAnchors`).
- Pagination via the `nextCursor` returned by `useAudit`.

Mirror `web/app/(app)/runs/page.tsx` for the page skeleton; mirror the dialog pattern from `web/app/(app)/users/page.tsx`.

- [ ] **Step 2: Write a vitest screen test**

`web/tests/audit-page.test.tsx` — render the page with a stubbed `ApiClient` that returns a 3-item audit page; assert the rows render; click **Verify chain** → assert the dialog shows; have the stub return `status:"invalid"` and assert the red message; click **Anchor now** and assert the toast.

Read `web/tests/upload-dialog.test.tsx` for the existing provider-wrapping pattern.

- [ ] **Step 3: Run typecheck + tests + commit**

Run: `pnpm -C web tsc --noEmit && pnpm -C web test -- audit-page`
Expected: clean.

```bash
git add "web/app/(app)/audit/page.tsx" web/tests/audit-page.test.tsx
git commit -m "feat(web): Audit Log admin screen with verify + anchor actions

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task E4: Controls screen

**Files:**
- Create: `web/app/(app)/controls/page.tsx`
- Create: `web/tests/controls-page.test.tsx`

- [ ] **Step 1: Build the screen**

`web/app/(app)/controls/page.tsx` — admin-only client component. Uses `useControls()` to fetch the registry. Renders a table: framework, id, description, event kinds (chip list). Clicking a row navigates to `/audit?kind=<k1>&kind=<k2>…` via `router.push` (uses `next/navigation`).

- [ ] **Step 2: Vitest screen test**

`web/tests/controls-page.test.tsx` — stub `listControls` to return two entries; assert rows render; simulate click on the first row → assert `router.push` called with the right URL (mock `useRouter`).

- [ ] **Step 3: Run typecheck + tests + commit**

Run: `pnpm -C web tsc --noEmit && pnpm -C web test -- controls-page`
Expected: clean.

```bash
git add "web/app/(app)/controls/page.tsx" web/tests/controls-page.test.tsx
git commit -m "feat(web): Controls admin screen with audit-log click-through

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task E5: Sidebar nav

**Files:**
- Modify: `web/components/app/app-sidebar.tsx`

- [ ] **Step 1: Add the two nav items**

In `web/components/app/app-sidebar.tsx`:

```tsx
import { LayoutDashboard, ListChecks, TriangleAlert, Scale, Users, Database, ShieldCheck, ClipboardCheck, type LucideIcon } from "lucide-react";
```

Append to `NAV_ITEMS`:

```tsx
  { href: "/audit", label: "Audit", icon: ShieldCheck, adminOnly: true },
  { href: "/controls", label: "Controls", icon: ClipboardCheck, adminOnly: true },
```

- [ ] **Step 2: Run full frontend suite**

Run: `pnpm -C web tsc --noEmit && pnpm -C web lint && pnpm -C web test`
Expected: all green.

- [ ] **Step 3: Commit**

```bash
git add web/components/app/app-sidebar.tsx
git commit -m "feat(web): add Audit + Controls to the sidebar (admin only)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

# Phase F — E2E + final verification

### Task F1: Playwright E2E

**Files:**
- Create: `web/tests/e2e/compliance.spec.ts`

- [ ] **Step 1: Write the E2E**

`web/tests/e2e/compliance.spec.ts`:

```ts
import { test, expect } from "@playwright/test";
import { loginViaUI, reseed } from "./helpers";

test.beforeEach(async () => {
  await reseed();
});

test("admin verifies the audit chain and anchors", async ({ page }) => {
  await loginViaUI(page, "ada@acme.test", "Password123!");

  // The act of logging in produced an auth.login.success audit row.
  await page.goto("/audit");
  await expect(page.getByText(/auth\.login\.success/)).toBeVisible();

  // Verify chain → expect valid.
  await page.getByRole("button", { name: /verify chain/i }).click();
  await page.getByRole("button", { name: /^verify$/i }).click(); // dialog's submit
  await expect(page.getByText(/chain valid/i)).toBeVisible();

  // Anchor now → toast with anchorSeq.
  await page.getByRole("button", { name: /anchor now/i }).click();
  await expect(page.getByText(/anchor.* (created|written|#1)/i)).toBeVisible();

  // Controls → click a row → audit filtered to its kinds.
  await page.goto("/controls");
  await page.getByRole("row", { name: /A\.9\.2\.1/ }).click();
  await expect(page).toHaveURL(/\/audit\?.*kind=admin\.user\.created/);
});
```

- [ ] **Step 2: Run the E2E (requires the live backend on the new code)**

Backend must be running with `AUDIT_ANCHOR_INTERVAL_SECS` defaulted (anchor on demand only) and `RECON_DEV=1`.

Run: `pnpm -C web exec playwright test compliance --reporter=line`
Expected: PASS.

Then run the FULL E2E suite:

Run: `pnpm -C web exec playwright test --reporter=line`
Expected: all PASS.

- [ ] **Step 3: Commit**

```bash
git add web/tests/e2e/compliance.spec.ts
git commit -m "test(e2e): admin verifies audit chain, anchors, and controls click-through

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task F2: README update

**Files:**
- Modify: `web/README.md`

- [ ] **Step 1: Document the compliance flow**

Append to `web/README.md` after the ingestion section:

```markdown
### Compliance audit log

Sign in as an admin (`ada@acme.test`) and visit:

- **Audit** — every material action recorded in a per-tenant hash-chained log. Filter by kind/actor/date; click **Verify chain** to walk a range and confirm integrity; click **Anchor now** to write a global anchor that ties every tenant's current head into the anchor chain. The backend also runs an internal anchor scheduler every `AUDIT_ANCHOR_INTERVAL_SECS` (default 3600).
- **Controls** — ISO 27001 / SOC 2 / FCA control items mapped to the audit-event kinds that demonstrate them. Clicking a row jumps to the audit log filtered to that control's events.

The auditor-facing description of each control lives in `docs/compliance/controls.md`.
```

- [ ] **Step 2: Commit**

```bash
git add web/README.md
git commit -m "docs: document the compliance audit log and controls screens

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Final verification (after all tasks)

- [ ] **Backend:** `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test && cargo clippy --workspace -- -D warnings` → all green.
- [ ] **Frontend:** `pnpm -C web tsc --noEmit && pnpm -C web lint && pnpm -C web test` → all green.
- [ ] **E2E:** with the new backend up (`RECON_DEV=1` + `AUDIT_ANCHOR_INTERVAL_SECS` default), `pnpm -C web e2e` → all specs green (existing + compliance).
- [ ] Dispatch a final code review across `master..HEAD`, then use **superpowers:finishing-a-development-branch**.

---

## Notes for the implementer

- **Read-before-edit:** Phase C touches many existing files (`write.rs`, `sources.rs`, `runs.rs`, `auth.rs`, `routes_auth.rs`, `routes_users.rs`, `routes.rs`). Read each file first; the audit emission is additive, but the new `actor_id`/`file_*` parameters threading into the store methods cascade through call sites.
- **`tx` threading invariant:** any store method that emits audit MUST run in a single transaction. If you're adding emission to a method that uses `&self.pool` directly, wrap it in `let mut tx = self.pool.begin().await?; … tx.commit().await?;` first.
- **Auth flows are special:** they were sequential before; the new `*_tx` variants on store methods exist specifically so `routes_auth` handlers can run a single tx end-to-end. Don't keep the old `&self.pool` calls AND the new tx — pick the tx path consistently within a handler.
- **Canonical JSON depends on serde_json's default `Map<String, Value>` being `BTreeMap`-backed.** Do NOT enable the `preserve_order` feature on `serde_json` in any crate; it would break the chain. (Verify with `grep -r preserve_order backend/`.)
- **Hex on the wire:** `prev_hash` / `hash` / `expected_prev_hash` are hex strings in JSON. The store uses raw `[u8; 32]` / `BYTEA`. The API layer handles encode/decode at the boundary.
- **Migration `0004` on a populated DB:** unlike `0003`, this migration creates new tables only — no constraint over existing data. It's safe to apply to any DB. The dev DB still needs a wipe before applying `0003`'s edited form (see prior slice's commit notes) plus this `0004`.
- **`AUDIT_ANCHOR_INTERVAL_SECS=0` or very low values in tests** would flood the DB; keep the default in dev and tests, and don't override.
- **Don't change the seed's fixed ids** (`case-pending`/`break-pending`/`txn-brk001`); the existing test suites pin to them.
