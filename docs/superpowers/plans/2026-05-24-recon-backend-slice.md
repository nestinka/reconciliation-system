# Reconciliation Backend Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the existing frontend `ApiClient` contract real over HTTP with a Rust (Axum + sqlx/PostgreSQL) backend and a deterministic matching engine, then hard-switch the frontend to it so the deployed UI runs on real Rust.

**Architecture:** A Cargo workspace with four crates — `recon-domain` (pure types + the wire contract + four-eyes logic), `recon-matching` (the pure, replayable matching engine), `recon-store` (sqlx/Postgres persistence with shared-schema multi-tenancy and append-only audit), and `recon-api` (the Axum binary). The frontend gains an `HttpApiClient implements ApiClient` and swaps its runtime provider default to it; `MockApiClient` stays only as a vitest test double.

**Tech Stack:** Rust (stable), Axum, Tokio, tower-http, sqlx (Postgres, runtime-checked), serde, thiserror, tracing, proptest, uuid, time. PostgreSQL 16 via Docker Compose. Frontend unchanged except `web/lib/api/http.ts` (new) and `web/lib/api/provider.tsx`.

**Spec:** `docs/superpowers/specs/2026-05-24-recon-backend-slice-design.md`

---

## Conventions for every task

- **Working directory:** the backend workspace root is `backend/` (sibling of `web/` and `docs/`). All `cargo` commands run from `backend/`.
- **Test DB:** integration tests use `#[sqlx::test]`, which creates an isolated database per test from `DATABASE_URL`. Postgres must be running (Task 1).
- **No compile-time DB:** use `sqlx::query`, `sqlx::query_as`, and `sqlx::query_scalar` (runtime-checked), never the `query!`/`query_as!` macros — the build must not require a live database.
- **Commit** at the end of each task with the message shown. Branch is `backend-slice` (already created).
- **JSON casing:** every wire struct derives `serde::{Serialize, Deserialize}` and carries `#[serde(rename_all = "camelCase")]`.

---

## File structure

```
backend/
  Cargo.toml                          # [workspace]
  rust-toolchain.toml                 # pin stable
  docker-compose.yml                  # postgres:16
  .env.example                        # DATABASE_URL, RUST_LOG, etc.
  migrations/
    0001_init.sql                     # all tables
  crates/
    recon-domain/
      Cargo.toml
      src/lib.rs                      # re-exports
      src/types.rs                    # scalar enums + entity structs
      src/events.rs                   # CaseEvent / CaseEventBody / NewCaseEvent / Resolution
      src/ageing.rs                   # ageing_bucket(days)
      src/approval.rs                 # can_approve + ApprovalError
    recon-matching/
      Cargo.toml
      src/lib.rs
      src/config.rs                   # MatchConfig
      src/score.rs                    # score_pair, similarity helpers
      src/engine.rs                   # reconcile -> RunResult, DecisionDraft, BreakDraft
      src/suggest.rs                  # suggestions_for (case screen near-misses)
      tests/properties.rs            # proptest
    recon-store/
      Cargo.toml
      src/lib.rs                      # Store struct, connect, migrate
      src/error.rs                    # StoreError
      src/rows.rs                     # FromRow row structs + -> domain mappers
      src/read.rs                     # all read repository methods
      src/write.rs                    # assign_break, append_case_event
      src/dashboard.rs                # get_dashboard aggregation
      src/seed.rs                     # deterministic seed (raw txns -> engine -> overlay)
      tests/isolation.rs             # tenant isolation + immutability
      tests/write.rs                 # four-eyes + transitions at the store layer
    recon-api/
      Cargo.toml
      src/main.rs                     # CLI: serve | seed
      src/state.rs                    # AppState
      src/error.rs                    # ApiError + IntoResponse
      src/auth.rs                     # AuthContext extractor
      src/dto.rs                      # request bodies (AssignBody)
      src/routes.rs                   # router() + all handlers
      tests/api.rs                   # HTTP integration tests
web/
  lib/api/http.ts                     # NEW: HttpApiClient
  lib/api/provider.tsx                # MODIFY: runtime default -> HttpApiClient
  .env.local                          # NEXT_PUBLIC_API_BASE_URL (gitignored)
  tests/e2e/operator-loop.spec.ts     # MODIFY: run against live backend
  README.md                           # MODIFY: full-stack run recipe
```

---

# Phase 1 — Workspace, domain types, four-eyes

### Task 1: Toolchain, workspace skeleton, Postgres

**Files:**
- Create: `backend/Cargo.toml`, `backend/rust-toolchain.toml`, `backend/docker-compose.yml`, `backend/.env.example`, `backend/.gitignore`
- Modify: `.gitignore` (repo root) — add `backend/target/`, `backend/.env`

- [ ] **Step 1: Install the Rust toolchain (not present on this machine)**

Run:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
. "$HOME/.cargo/env"
rustc --version && cargo --version
```
Expected: prints `rustc 1.x` and `cargo 1.x`.

- [ ] **Step 2: Create the workspace manifest**

`backend/Cargo.toml`:
```toml
[workspace]
resolver = "2"
members = [
  "crates/recon-domain",
  "crates/recon-matching",
  "crates/recon-store",
  "crates/recon-api",
]

[workspace.package]
edition = "2021"
version = "0.1.0"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
uuid = { version = "1", features = ["v4"] }
time = { version = "0.3", features = ["serde", "formatting", "parsing", "macros"] }
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "tls-rustls", "postgres", "time", "uuid", "json", "migrate"] }
axum = "0.7"
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "cors"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
proptest = "1"
```

`backend/rust-toolchain.toml`:
```toml
[toolchain]
channel = "stable"
components = ["clippy", "rustfmt"]
```

- [ ] **Step 3: Docker Compose for Postgres**

`backend/docker-compose.yml`:
```yaml
services:
  postgres:
    image: postgres:16
    environment:
      POSTGRES_USER: recon
      POSTGRES_PASSWORD: recon
      POSTGRES_DB: recon
    ports:
      - "5432:5432"
    healthcheck:
      test: ["CMD-SHELL", "pg_isready -U recon"]
      interval: 2s
      timeout: 3s
      retries: 20
    volumes:
      - recon_pg:/var/lib/postgresql/data
volumes:
  recon_pg:
```

`backend/.env.example`:
```
DATABASE_URL=postgres://recon:recon@localhost:5432/recon
RUST_LOG=recon_api=debug,tower_http=debug,info
API_BIND=0.0.0.0:8080
WEB_ORIGIN=http://localhost:3000
```

`backend/.gitignore`:
```
/target
.env
```

- [ ] **Step 4: Bring up Postgres and verify**

Run:
```bash
cd backend && cp .env.example .env
docker compose up -d postgres
docker compose exec -T postgres pg_isready -U recon
```
Expected: `... accepting connections`.

- [ ] **Step 5: Create empty crates so the workspace builds**

Create minimal `crates/<name>/Cargo.toml` + `src/lib.rs` (`recon-api` gets `src/main.rs`). Each lib `Cargo.toml`:
```toml
[package]
name = "recon-domain"   # (and recon-matching / recon-store)
edition.workspace = true
version.workspace = true
```
`recon-api/Cargo.toml`:
```toml
[package]
name = "recon-api"
edition.workspace = true
version.workspace = true

[[bin]]
name = "recon-api"
path = "src/main.rs"
```
`recon-api/src/main.rs`:
```rust
fn main() {
    println!("recon-api placeholder");
}
```
Empty `src/lib.rs` for the three libs.

- [ ] **Step 6: Verify the workspace builds**

Run: `cargo build`
Expected: `Finished` with no errors.

- [ ] **Step 7: Commit**

```bash
git add backend .gitignore
git commit -m "chore: scaffold Rust workspace + Postgres compose"
```

---

### Task 2: Domain scalar enums and entity structs

**Files:**
- Create: `backend/crates/recon-domain/src/types.rs`, `backend/crates/recon-domain/src/ageing.rs`
- Modify: `backend/crates/recon-domain/src/lib.rs`, `backend/crates/recon-domain/Cargo.toml`
- Test: in `types.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Add deps**

`recon-domain/Cargo.toml` add:
```toml
[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 2: Write the failing serialization test**

In `src/types.rs` bottom:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_serializes_camel_case_with_renamed_enums() {
        let s = Source {
            id: "src-acme-cross".into(),
            tenant_id: "tenant-acme".into(),
            kind: SourceKind::CrossSystem,
            name: "Acme Cross".into(),
            currency: "USD".into(),
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["tenantId"], "tenant-acme");
        assert_eq!(v["kind"], "cross_system");
    }

    #[test]
    fn break_status_and_ageing_bucket_wire_values() {
        assert_eq!(serde_json::to_value(BreakStatus::PendingApproval).unwrap(), "pending_approval");
        assert_eq!(serde_json::to_value(AgeingBucket::EightToThirty).unwrap(), "8-30d");
        assert_eq!(serde_json::to_value(BreakType::Break).unwrap(), "break");
        assert_eq!(serde_json::to_value(MatchType::Duplicate).unwrap(), "duplicate");
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p recon-domain`
Expected: FAIL to compile (`Source` not defined).

- [ ] **Step 4: Implement the enums and structs**

`src/types.rs` (top):
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind { Bank, Ledger, CrossSystem }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction { Debit, Credit }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus { Running, Completed, Failed }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchType { Matched, Partial, Duplicate }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakType { Unmatched, Partial, Duplicate, Break }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakStatus { Open, Investigating, PendingApproval, Resolved, WrittenOff }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgeingBucket {
    #[serde(rename = "0-1d")] ZeroToOne,
    #[serde(rename = "2-7d")] TwoToSeven,
    #[serde(rename = "8-30d")] EightToThirty,
    #[serde(rename = "30d+")] ThirtyPlus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole { Operator, Approver, Admin }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tenant { pub id: String, pub name: String, pub slug: String }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User { pub id: String, pub name: String, pub role: UserRole }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    pub tenant_id: String,
    pub kind: SourceKind,
    pub name: String,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalTransaction {
    pub id: String,
    pub tenant_id: String,
    pub source_id: String,
    pub external_ref: String,
    pub value_date: String,   // "YYYY-MM-DD"
    pub posted_at: String,    // RFC3339
    pub amount_minor: i64,
    pub currency: String,
    pub direction: Direction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty: Option<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStats {
    pub matched: i64,
    pub unmatched: i64,
    pub partial: i64,
    pub duplicate: i64,
    pub break_count: i64,
    pub match_rate_pct: f64,
    pub value_at_risk_minor: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconciliationRun {
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    pub source_a_id: String,
    pub source_b_id: String,
    pub status: RunStatus,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
    pub config_version: String,
    pub stats: RunStats,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchDecision {
    pub id: String,
    pub run_id: String,
    #[serde(rename = "type")]
    pub match_type: MatchType,
    pub txn_ids: Vec<String>,
    pub score: f64,
    pub config_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Break {
    pub id: String,
    pub tenant_id: String,
    pub run_id: String,
    pub case_id: String,
    #[serde(rename = "type")]
    pub break_type: BreakType,
    pub status: BreakStatus,
    pub ageing_days: i64,
    pub ageing_bucket: AgeingBucket,
    pub value_minor: i64,
    pub currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    pub txn_ids: Vec<String>,
    pub opened_at: String,
}
```

`src/ageing.rs`:
```rust
use crate::types::AgeingBucket;

/// Maps a non-negative age in days to the canonical bucket used by the UI.
pub fn ageing_bucket(days: i64) -> AgeingBucket {
    match days {
        d if d <= 1 => AgeingBucket::ZeroToOne,
        d if d <= 7 => AgeingBucket::TwoToSeven,
        d if d <= 30 => AgeingBucket::EightToThirty,
        _ => AgeingBucket::ThirtyPlus,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn buckets() {
        assert_eq!(ageing_bucket(0), AgeingBucket::ZeroToOne);
        assert_eq!(ageing_bucket(1), AgeingBucket::ZeroToOne);
        assert_eq!(ageing_bucket(2), AgeingBucket::TwoToSeven);
        assert_eq!(ageing_bucket(7), AgeingBucket::TwoToSeven);
        assert_eq!(ageing_bucket(8), AgeingBucket::EightToThirty);
        assert_eq!(ageing_bucket(30), AgeingBucket::EightToThirty);
        assert_eq!(ageing_bucket(31), AgeingBucket::ThirtyPlus);
    }
}
```

`src/lib.rs`:
```rust
pub mod ageing;
pub mod types;
pub use types::*;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p recon-domain`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add backend/crates/recon-domain
git commit -m "feat(domain): scalar enums + entity structs with camelCase wire shapes"
```

---

### Task 3: CaseEvent union and Case

**Files:**
- Create: `backend/crates/recon-domain/src/events.rs`
- Modify: `backend/crates/recon-domain/src/lib.rs`
- Test: in `events.rs`

- [ ] **Step 1: Write the failing round-trip test**

In `src/events.rs` bottom:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_requested_wire_shape() {
        let e = CaseEvent {
            id: "evt-1".into(),
            actor_id: "user-mia".into(),
            at: "2026-05-16T09:35:00Z".into(),
            body: CaseEventBody::ApprovalRequested { resolution: Resolution::WriteOff },
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["id"], "evt-1");
        assert_eq!(v["actorId"], "user-mia");
        assert_eq!(v["kind"], "approval_requested");
        assert_eq!(v["payload"]["resolution"], "write_off");
    }

    #[test]
    fn assignment_payload_is_camel_case() {
        let e = CaseEvent {
            id: "e".into(), actor_id: "user-ada".into(), at: "t".into(),
            body: CaseEventBody::Assignment { assignee_id: "user-mia".into() },
        };
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["kind"], "assignment");
        assert_eq!(v["payload"]["assigneeId"], "user-mia");
    }

    #[test]
    fn round_trips_through_json() {
        let json = serde_json::json!({
            "id": "x", "actorId": "user-mia", "at": "t",
            "kind": "comment", "payload": { "text": "hi" }
        });
        let e: CaseEvent = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(serde_json::to_value(&e).unwrap(), json);
    }

    #[test]
    fn new_case_event_omits_id_and_at() {
        let json = serde_json::json!({
            "actorId": "user-mia", "kind": "approved", "payload": {}
        });
        let n: NewCaseEvent = serde_json::from_value(json).unwrap();
        assert_eq!(n.actor_id, "user-mia");
        matches!(n.body, CaseEventBody::Approved {});
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p recon-domain events`
Expected: FAIL to compile.

- [ ] **Step 3: Implement events**

`src/events.rs` (top):
```rust
use serde::{Deserialize, Serialize};
use crate::types::BreakStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution { WriteOff, ManualMatch }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum CaseEventBody {
    Comment { text: String },
    Assignment {
        #[serde(rename = "assigneeId")]
        assignee_id: String,
    },
    ManualMatchProposed {
        #[serde(rename = "txnIds")]
        txn_ids: Vec<String>,
    },
    WriteOffProposed { reason: String },
    ApprovalRequested { resolution: Resolution },
    Approved {},
    Rejected { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseEvent {
    pub id: String,
    pub actor_id: String,
    pub at: String,
    #[serde(flatten)]
    pub body: CaseEventBody,
}

/// POST body: the client supplies actor + kind + payload; the server assigns id/at.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCaseEvent {
    pub actor_id: String,
    #[serde(flatten)]
    pub body: CaseEventBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Case {
    pub id: String,
    pub break_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee_id: Option<String>,
    pub status: BreakStatus,
    pub events: Vec<CaseEvent>,
}
```

Add to `src/lib.rs`:
```rust
pub mod events;
pub use events::*;
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p recon-domain`
Expected: PASS. **If the flatten + adjacently-tagged round-trip test fails to deserialize**, replace the `#[serde(flatten)]` on `CaseEvent`/`NewCaseEvent` with a manual `Deserialize` that reads `id`/`actorId`/`at` then builds the body from the remaining `{kind, payload}` via `serde_json::from_value`. (Serialization with flatten is reliable; this is the documented fallback for deserialization only.)

- [ ] **Step 5: Commit**

```bash
git add backend/crates/recon-domain
git commit -m "feat(domain): CaseEvent union + Case with exact wire representation"
```

---

### Task 4: Four-eyes approval logic

**Files:**
- Create: `backend/crates/recon-domain/src/approval.rs`
- Modify: `backend/crates/recon-domain/src/lib.rs`
- Test: in `approval.rs`

- [ ] **Step 1: Write the failing tests** (port of `web/lib/case/approval.ts`)

`src/approval.rs` bottom:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn case_with(status: BreakStatus, events: Vec<CaseEvent>) -> Case {
        Case { id: "c".into(), break_id: "b".into(), assignee_id: None, status, events }
    }
    fn req(actor: &str) -> CaseEvent {
        CaseEvent { id: "r".into(), actor_id: actor.into(), at: "t".into(),
            body: CaseEventBody::ApprovalRequested { resolution: Resolution::WriteOff } }
    }
    fn user(id: &str, role: UserRole) -> User { User { id: id.into(), name: id.into(), role } }

    #[test]
    fn maker_cannot_approve_own_proposal() {
        let c = case_with(BreakStatus::PendingApproval, vec![req("user-mia")]);
        let r = can_approve(&c, &user("user-mia", UserRole::Approver));
        assert!(matches!(r, Err(ApprovalError::Maker)));
    }
    #[test]
    fn operator_cannot_approve() {
        let c = case_with(BreakStatus::PendingApproval, vec![req("user-mia")]);
        let r = can_approve(&c, &user("user-sam", UserRole::Operator));
        assert!(matches!(r, Err(ApprovalError::Role)));
    }
    #[test]
    fn different_approver_can_approve() {
        let c = case_with(BreakStatus::PendingApproval, vec![req("user-mia")]);
        assert!(can_approve(&c, &user("user-theo", UserRole::Approver)).is_ok());
    }
    #[test]
    fn not_pending_is_rejected() {
        let c = case_with(BreakStatus::Open, vec![req("user-mia")]);
        assert!(matches!(can_approve(&c, &user("user-theo", UserRole::Approver)), Err(ApprovalError::NotPending)));
    }
    #[test]
    fn missing_request_fails_closed() {
        let c = case_with(BreakStatus::PendingApproval, vec![]);
        assert!(matches!(can_approve(&c, &user("user-theo", UserRole::Approver)), Err(ApprovalError::NoRequest)));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p recon-domain approval`
Expected: FAIL to compile.

- [ ] **Step 3: Implement**

`src/approval.rs` (top):
```rust
use thiserror::Error;
use crate::events::{CaseEvent, CaseEventBody};
use crate::types::{BreakStatus, User, UserRole};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ApprovalError {
    #[error("Case is not pending approval.")]
    NotPending,
    #[error("User does not have approver or admin role.")]
    Role,
    #[error("No approval request found in case history.")]
    NoRequest,
    #[error("Maker cannot approve their own proposal (four-eyes principle).")]
    Maker,
}

/// Four-eyes gate, ported from web/lib/case/approval.ts. Fails closed.
pub fn can_approve(c: &crate::events::Case, user: &User) -> Result<(), ApprovalError> {
    if c.status != BreakStatus::PendingApproval {
        return Err(ApprovalError::NotPending);
    }
    if !matches!(user.role, UserRole::Approver | UserRole::Admin) {
        return Err(ApprovalError::Role);
    }
    let last_request = c.events.iter().rev().find(|e| {
        matches!(e.body, CaseEventBody::ApprovalRequested { .. })
    });
    let Some(req) = last_request else { return Err(ApprovalError::NoRequest) };
    if req.actor_id == user.id {
        return Err(ApprovalError::Maker);
    }
    Ok(())
}
```
Note: `Case` lives in `events.rs`; this references `crate::events::Case`. Add `pub mod approval; pub use approval::*;` to `lib.rs`.

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p recon-domain`
Expected: PASS (all domain tests).

- [ ] **Step 5: Lint + commit**

```bash
cargo clippy -p recon-domain -- -D warnings
git add backend/crates/recon-domain
git commit -m "feat(domain): server-side four-eyes approval gate"
```

---

# Phase 2 — Matching engine

### Task 5: MatchConfig and pair scoring

**Files:**
- Create: `backend/crates/recon-matching/src/config.rs`, `backend/crates/recon-matching/src/score.rs`
- Modify: `backend/crates/recon-matching/src/lib.rs`, `backend/crates/recon-matching/Cargo.toml`
- Test: in `score.rs`

- [ ] **Step 1: Deps**

`recon-matching/Cargo.toml`:
```toml
[dependencies]
recon-domain = { path = "../recon-domain" }

[dev-dependencies]
proptest = { workspace = true }
```

- [ ] **Step 2: Failing test**

`src/score.rs` bottom:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use recon_domain::{CanonicalTransaction, Direction};

    fn txn(id: &str, amt: i64, date: &str, dir: Direction, cur: &str) -> CanonicalTransaction {
        CanonicalTransaction {
            id: id.into(), tenant_id: "t".into(), source_id: "s".into(),
            external_ref: id.into(), value_date: date.into(),
            posted_at: format!("{date}T00:00:00Z"), amount_minor: amt,
            currency: cur.into(), direction: dir, counterparty: None,
            description: "d".into(),
        }
    }

    #[test]
    fn identical_amount_and_date_scores_one() {
        let a = txn("a", 1000, "2026-05-01", Direction::Debit, "GBP");
        let b = txn("b", 1000, "2026-05-01", Direction::Debit, "GBP");
        assert!((score_pair(&a, &b) - 1.0).abs() < 1e-9);
    }
    #[test]
    fn opposite_direction_or_currency_scores_zero() {
        let a = txn("a", 1000, "2026-05-01", Direction::Debit, "GBP");
        let b = txn("b", 1000, "2026-05-01", Direction::Credit, "GBP");
        assert_eq!(score_pair(&a, &b), 0.0);
        let c = txn("c", 1000, "2026-05-01", Direction::Debit, "USD");
        assert_eq!(score_pair(&a, &c), 0.0);
    }
    #[test]
    fn score_is_always_in_unit_interval() {
        let a = txn("a", 1000, "2026-05-01", Direction::Debit, "GBP");
        let b = txn("b", 950, "2026-05-09", Direction::Debit, "GBP");
        let s = score_pair(&a, &b);
        assert!((0.0..=1.0).contains(&s), "score {s} out of range");
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p recon-matching`
Expected: FAIL to compile.

- [ ] **Step 4: Implement config + score**

`src/config.rs`:
```rust
#[derive(Debug, Clone)]
pub struct MatchConfig {
    pub version: String,
    pub amount_tolerance_minor: i64,
    pub date_tolerance_days: i64,
    pub fuzzy_threshold: f64,
}

impl MatchConfig {
    /// The pinned default configuration used by the seed and tests.
    pub fn v1() -> Self {
        Self { version: "v1.0".into(), amount_tolerance_minor: 500, date_tolerance_days: 2, fuzzy_threshold: 0.6 }
    }
}
```

`src/score.rs` (top):
```rust
use recon_domain::CanonicalTransaction;

/// Parse "YYYY-MM-DD" to a day number (proleptic) for stable, timezone-free diffs.
fn day_number(value_date: &str) -> i64 {
    // value_date is always "YYYY-MM-DD" in canonical transactions.
    let mut parts = value_date.split('-');
    let y: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let m: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    let d: i64 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(1);
    // Days from a fixed epoch; month-length approximation is fine because we only
    // ever take absolute differences of nearby dates (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Similarity in [0,1] for two transactions. Hard gate on direction + currency.
pub fn score_pair(a: &CanonicalTransaction, b: &CanonicalTransaction) -> f64 {
    if a.direction != b.direction || a.currency != b.currency {
        return 0.0;
    }
    let amt_a = a.amount_minor.max(1) as f64;
    let amt_diff = (a.amount_minor - b.amount_minor).abs() as f64;
    let amount_score = (1.0 - amt_diff / amt_a).clamp(0.0, 1.0);

    let date_diff = (day_number(&a.value_date) - day_number(&b.value_date)).abs() as f64;
    let date_score = (1.0 - date_diff / 30.0).clamp(0.0, 1.0);

    let ref_score = if a.external_ref == b.external_ref { 1.0 } else { 0.0 };

    // Weighted blend; amount dominates, date next, exact-ref nudges up.
    let raw = 0.6 * amount_score + 0.3 * date_score + 0.1 * ref_score;
    raw.clamp(0.0, 1.0)
}
```

`src/lib.rs`:
```rust
pub mod config;
pub mod score;
pub use config::MatchConfig;
pub use score::score_pair;
```

- [ ] **Step 5: Run to verify pass**

Run: `cargo test -p recon-matching`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add backend/crates/recon-matching
git commit -m "feat(matching): MatchConfig + deterministic pair scoring"
```

---

### Task 6: The reconcile engine

**Files:**
- Create: `backend/crates/recon-matching/src/engine.rs`
- Modify: `backend/crates/recon-matching/src/lib.rs`
- Test: in `engine.rs`

- [ ] **Step 1: Write failing tests**

`src/engine.rs` bottom:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use recon_domain::{CanonicalTransaction, Direction, MatchType, BreakType};
    use crate::MatchConfig;

    fn txn(id: &str, src: &str, amt: i64, date: &str, eref: &str) -> CanonicalTransaction {
        CanonicalTransaction {
            id: id.into(), tenant_id: "t".into(), source_id: src.into(),
            external_ref: eref.into(), value_date: date.into(),
            posted_at: format!("{date}T00:00:00Z"), amount_minor: amt,
            currency: "GBP".into(), direction: Direction::Debit, counterparty: None,
            description: "d".into(),
        }
    }

    #[test]
    fn exact_pair_matches_unmatched_breaks() {
        let a = vec![ txn("a1","A",1000,"2026-05-01","R1"), txn("a2","A",2000,"2026-05-02","R2") ];
        let b = vec![ txn("b1","B",1000,"2026-05-01","R1") ];
        let r = reconcile(&a, &b, &MatchConfig::v1());
        assert_eq!(r.decisions.iter().filter(|d| d.match_type == MatchType::Matched).count(), 1);
        // a2 has no counterpart -> one break
        assert_eq!(r.breaks.len(), 1);
        assert_eq!(r.breaks[0].txn_ids, vec!["a2".to_string()]);
        assert_eq!(r.breaks[0].break_type, BreakType::Unmatched);
    }

    #[test]
    fn within_tolerance_is_partial() {
        let a = vec![ txn("a1","A",1000,"2026-05-01","R1") ];
        let b = vec![ txn("b1","B",1300,"2026-05-02","R9") ]; // 300 minor diff <= 500 tol, 1 day
        let r = reconcile(&a, &b, &MatchConfig::v1());
        assert_eq!(r.decisions.len(), 1);
        assert_eq!(r.decisions[0].match_type, MatchType::Partial);
    }

    #[test]
    fn duplicate_within_source_detected() {
        let a = vec![ txn("a1","A",950,"2026-05-10","D1"), txn("a2","A",950,"2026-05-10","D1") ];
        let b: Vec<CanonicalTransaction> = vec![];
        let r = reconcile(&a, &b, &MatchConfig::v1());
        assert!(r.decisions.iter().any(|d| d.match_type == MatchType::Duplicate));
    }

    #[test]
    fn stats_are_consistent() {
        let a = vec![ txn("a1","A",1000,"2026-05-01","R1"), txn("a2","A",5,"2026-05-01","X") ];
        let b = vec![ txn("b1","B",1000,"2026-05-01","R1") ];
        let r = reconcile(&a, &b, &MatchConfig::v1());
        assert_eq!(r.stats.matched, 1);
        assert_eq!(r.stats.break_count, r.breaks.len() as i64);
        assert!(r.stats.match_rate_pct >= 0.0 && r.stats.match_rate_pct <= 100.0);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p recon-matching engine`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the engine**

`src/engine.rs`:
```rust
use recon_domain::{BreakType, CanonicalTransaction, MatchType, RunStats};
use crate::config::MatchConfig;
use crate::score::score_pair;

/// A match decision produced by the engine (no DB identity yet).
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionDraft {
    pub match_type: MatchType,
    pub txn_ids: Vec<String>,
    pub score: f64,
}

/// An unmatched transaction that becomes a break (no DB identity yet).
#[derive(Debug, Clone, PartialEq)]
pub struct BreakDraft {
    pub break_type: BreakType,
    pub txn_ids: Vec<String>,
    pub value_minor: i64,
    pub currency: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunResult {
    pub decisions: Vec<DecisionDraft>,
    pub breaks: Vec<BreakDraft>,
    pub stats: RunStats,
}

/// Deterministic, replayable reconciliation of source A against source B.
///
/// Order of operations (all on id-sorted inputs so the result never depends on
/// caller ordering or hashmap iteration):
///   1. detect intra-source duplicates in A, then B,
///   2. greedily pair remaining A with best-scoring remaining B (exact, then
///      tolerant/fuzzy above threshold),
///   3. everything still unpaired becomes an unmatched break.
pub fn reconcile(a: &[CanonicalTransaction], b: &[CanonicalTransaction], cfg: &MatchConfig) -> RunResult {
    let mut a: Vec<&CanonicalTransaction> = a.iter().collect();
    let mut b: Vec<&CanonicalTransaction> = b.iter().collect();
    a.sort_by(|x, y| x.id.cmp(&y.id));
    b.sort_by(|x, y| x.id.cmp(&y.id));

    let mut decisions: Vec<DecisionDraft> = Vec::new();
    let mut consumed_a = vec![false; a.len()];
    let mut consumed_b = vec![false; b.len()];

    // 1. Duplicates within each source: same (amount, external_ref, value_date).
    detect_duplicates(&a, &mut consumed_a, &mut decisions);
    detect_duplicates(&b, &mut consumed_b, &mut decisions);

    // 2. Greedy best-match A -> B.
    for (i, ta) in a.iter().enumerate() {
        if consumed_a[i] { continue; }
        let mut best: Option<(usize, f64)> = None;
        for (j, tb) in b.iter().enumerate() {
            if consumed_b[j] { continue; }
            let s = score_pair(ta, tb);
            if s >= cfg.fuzzy_threshold && best.map_or(true, |(_, bs)| s > bs) {
                best = Some((j, s));
            }
        }
        if let Some((j, s)) = best {
            consumed_a[i] = true;
            consumed_b[j] = true;
            let exact = (ta.amount_minor - b[j].amount_minor).abs() == 0
                && ta.value_date == b[j].value_date;
            let match_type = if exact && s >= 0.999 { MatchType::Matched } else { MatchType::Partial };
            decisions.push(DecisionDraft {
                match_type,
                txn_ids: vec![ta.id.clone(), b[j].id.clone()],
                score: s,
            });
        }
    }

    // 3. Remaining unmatched -> breaks (stable order by id).
    let mut breaks: Vec<BreakDraft> = Vec::new();
    for (i, ta) in a.iter().enumerate() {
        if !consumed_a[i] {
            breaks.push(BreakDraft {
                break_type: BreakType::Unmatched,
                txn_ids: vec![ta.id.clone()],
                value_minor: ta.amount_minor,
                currency: ta.currency.clone(),
            });
        }
    }
    for (j, tb) in b.iter().enumerate() {
        if !consumed_b[j] {
            breaks.push(BreakDraft {
                break_type: BreakType::Unmatched,
                txn_ids: vec![tb.id.clone()],
                value_minor: tb.amount_minor,
                currency: tb.currency.clone(),
            });
        }
    }
    breaks.sort_by(|x, y| x.txn_ids.cmp(&y.txn_ids));

    let stats = compute_stats(&decisions, &breaks);
    RunResult { decisions, breaks, stats }
}

fn detect_duplicates(txns: &[&CanonicalTransaction], consumed: &mut [bool], out: &mut Vec<DecisionDraft>) {
    for i in 0..txns.len() {
        if consumed[i] { continue; }
        for j in (i + 1)..txns.len() {
            if consumed[j] { continue; }
            let (x, y) = (txns[i], txns[j]);
            if x.amount_minor == y.amount_minor
                && x.external_ref.split('-').take(2).eq(y.external_ref.split('-').take(2))
                && x.value_date == y.value_date
            {
                consumed[i] = true;
                consumed[j] = true;
                out.push(DecisionDraft {
                    match_type: MatchType::Duplicate,
                    txn_ids: vec![x.id.clone(), y.id.clone()],
                    score: 0.99,
                });
                break;
            }
        }
    }
}

fn compute_stats(decisions: &[DecisionDraft], breaks: &[BreakDraft]) -> RunStats {
    let count = |t: MatchType| decisions.iter().filter(|d| d.match_type == t).count() as i64;
    let matched = count(MatchType::Matched);
    let partial = count(MatchType::Partial);
    let duplicate = count(MatchType::Duplicate);
    let unmatched = breaks.len() as i64;
    let denom = (matched + partial + duplicate + unmatched).max(1);
    let value_at_risk_minor = breaks.iter().map(|b| b.value_minor).sum();
    RunStats {
        matched,
        unmatched,
        partial,
        duplicate,
        break_count: breaks.len() as i64,
        match_rate_pct: (matched as f64 / denom as f64 * 1000.0).round() / 10.0,
        value_at_risk_minor,
    }
}
```

Add to `src/lib.rs`:
```rust
pub mod engine;
pub use engine::{reconcile, BreakDraft, DecisionDraft, RunResult};
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test -p recon-matching`
Expected: PASS.

- [ ] **Step 5: Lint + commit**

```bash
cargo clippy -p recon-matching -- -D warnings
git add backend/crates/recon-matching
git commit -m "feat(matching): deterministic reconcile engine with stats"
```

---

### Task 7: Property tests + suggestions

**Files:**
- Create: `backend/crates/recon-matching/tests/properties.rs`, `backend/crates/recon-matching/src/suggest.rs`
- Modify: `backend/crates/recon-matching/src/lib.rs`

- [ ] **Step 1: Write the suggestion failing test**

`src/suggest.rs`:
```rust
use recon_domain::CanonicalTransaction;
use crate::score::score_pair;

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub txn_ids: Vec<String>,
    pub score: f64,
    pub rationale: String,
}

/// Near-miss candidates for a break's transactions, sorted by descending score.
/// Used by the case screen. Deterministic: ties broken by candidate id.
pub fn suggestions_for(
    break_txns: &[CanonicalTransaction],
    candidates: &[CanonicalTransaction],
    min_score: f64,
) -> Vec<Suggestion> {
    let mut out: Vec<Suggestion> = Vec::new();
    for bt in break_txns {
        for c in candidates {
            if c.id == bt.id { continue; }
            let s = score_pair(bt, c);
            if s >= min_score {
                out.push(Suggestion {
                    txn_ids: vec![bt.id.clone(), c.id.clone()],
                    score: (s * 100.0).round() / 100.0,
                    rationale: format!(
                        "Amount/date similarity {:.0}% under config tolerance.",
                        s * 100.0
                    ),
                });
            }
        }
    }
    out.sort_by(|x, y| y.score.partial_cmp(&x.score).unwrap().then(x.txn_ids.cmp(&y.txn_ids)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use recon_domain::{CanonicalTransaction, Direction};
    fn txn(id: &str, amt: i64) -> CanonicalTransaction {
        CanonicalTransaction { id: id.into(), tenant_id: "t".into(), source_id: "s".into(),
            external_ref: id.into(), value_date: "2026-05-01".into(),
            posted_at: "2026-05-01T00:00:00Z".into(), amount_minor: amt, currency: "GBP".into(),
            direction: Direction::Debit, counterparty: None, description: "d".into() }
    }
    #[test]
    fn returns_sorted_candidates() {
        let brk = vec![txn("brk", 1000)];
        let cands = vec![txn("c1", 990), txn("c2", 500)];
        let s = suggestions_for(&brk, &cands, 0.5);
        assert!(s[0].score >= s.last().unwrap().score);
        assert_eq!(s[0].txn_ids[1], "c1"); // closest amount first
    }
}
```
Add `pub mod suggest; pub use suggest::{suggestions_for, Suggestion};` to `lib.rs`.

- [ ] **Step 2: Write property tests**

`tests/properties.rs`:
```rust
use proptest::prelude::*;
use recon_domain::{CanonicalTransaction, Direction};
use recon_matching::{reconcile, MatchConfig};

fn arb_txn(prefix: &'static str) -> impl Strategy<Value = CanonicalTransaction> {
    (0u32..50, 1i64..1_000_000, 1u32..28).prop_map(move |(id, amt, day)| CanonicalTransaction {
        id: format!("{prefix}-{id}"),
        tenant_id: "t".into(),
        source_id: prefix.into(),
        external_ref: format!("R{id}"),
        value_date: format!("2026-05-{day:02}"),
        posted_at: format!("2026-05-{day:02}T00:00:00Z"),
        amount_minor: amt,
        currency: "GBP".into(),
        direction: if id % 2 == 0 { Direction::Debit } else { Direction::Credit },
        counterparty: None,
        description: "d".into(),
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn deterministic_and_replayable(
        a in prop::collection::vec(arb_txn("a"), 0..12),
        b in prop::collection::vec(arb_txn("b"), 0..12),
    ) {
        let cfg = MatchConfig::v1();
        let r1 = reconcile(&a, &b, &cfg);
        let r2 = reconcile(&a, &b, &cfg);
        prop_assert_eq!(r1, r2);
    }

    #[test]
    fn no_txn_used_twice_and_conservation(
        a in prop::collection::vec(arb_txn("a"), 0..12),
        b in prop::collection::vec(arb_txn("b"), 0..12),
    ) {
        let r = reconcile(&a, &b, &MatchConfig::v1());
        let mut seen = std::collections::HashSet::new();
        for d in &r.decisions { for id in &d.txn_ids { prop_assert!(seen.insert(id.clone()), "double-used {id}"); } }
        for bk in &r.breaks { for id in &bk.txn_ids { prop_assert!(seen.insert(id.clone()), "double-used {id}"); } }
        // unique ids across both inputs (prefixes differ so no collision) are all classified
        let total_ids: std::collections::HashSet<String> =
            a.iter().chain(b.iter()).map(|t| t.id.clone()).collect();
        prop_assert_eq!(seen, total_ids);
    }

    #[test]
    fn scores_in_unit_interval(
        a in prop::collection::vec(arb_txn("a"), 0..12),
        b in prop::collection::vec(arb_txn("b"), 0..12),
    ) {
        let r = reconcile(&a, &b, &MatchConfig::v1());
        for d in &r.decisions { prop_assert!((0.0..=1.0).contains(&d.score)); }
    }
}
```

- [ ] **Step 3: Run to verify pass**

Run: `cargo test -p recon-matching`
Expected: PASS (unit + property tests).

- [ ] **Step 4: Lint + commit**

```bash
cargo clippy -p recon-matching --all-targets -- -D warnings
git add backend/crates/recon-matching
git commit -m "test(matching): determinism/conservation properties + suggestions"
```

---

# Phase 3 — Persistence

### Task 8: Store connection, migrations, schema

**Files:**
- Create: `backend/migrations/0001_init.sql`, `backend/crates/recon-store/src/lib.rs`, `backend/crates/recon-store/src/error.rs`
- Modify: `backend/crates/recon-store/Cargo.toml`
- Test: `backend/crates/recon-store/tests/isolation.rs` (migrate smoke only this task)

- [ ] **Step 1: Deps**

`recon-store/Cargo.toml`:
```toml
[dependencies]
recon-domain = { path = "../recon-domain" }
recon-matching = { path = "../recon-matching" }
sqlx = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
time = { workspace = true }
uuid = { workspace = true }
tokio = { workspace = true }

[dev-dependencies]
sqlx = { workspace = true }
```

- [ ] **Step 2: Write the migration**

`backend/migrations/0001_init.sql`:
```sql
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
```

- [ ] **Step 3: Implement Store + error**

`src/error.rs`:
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("not found")]
    NotFound,
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error(transparent)]
    Db(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
```

`src/lib.rs`:
```rust
pub mod error;
pub mod rows;
pub mod read;
pub mod write;
pub mod dashboard;
pub mod seed;

pub use error::StoreError;

use sqlx::postgres::{PgPool, PgPoolOptions};

#[derive(Clone)]
pub struct Store {
    pub pool: PgPool,
}

impl Store {
    pub async fn connect(database_url: &str) -> Result<Self, StoreError> {
        let pool = PgPoolOptions::new().max_connections(10).connect(database_url).await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self { Self { pool } }

    pub async fn migrate(&self) -> Result<(), StoreError> {
        sqlx::migrate!("../../migrations").run(&self.pool).await
            .map_err(|e| StoreError::Db(sqlx::Error::Migrate(Box::new(e))))?;
        Ok(())
    }
}
```
(Create empty `src/rows.rs`, `src/read.rs`, `src/write.rs`, `src/dashboard.rs`, `src/seed.rs` for now — filled in later tasks. Add a no-op `pub fn _placeholder() {}` if needed so modules compile.)

- [ ] **Step 4: Write the migrate smoke test**

`tests/isolation.rs`:
```rust
use recon_store::Store;

#[sqlx::test]
async fn migrations_apply(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    store.migrate().await.expect("migrations apply");
    // tables exist
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM tenants").fetch_one(&store.pool).await.unwrap();
    assert_eq!(n, 0);
}
```

- [ ] **Step 5: Run** (Postgres must be up from Task 1)

Run: `DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store migrations_apply`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add backend/migrations backend/crates/recon-store
git commit -m "feat(store): schema, migrations, Store connection"
```

---

### Task 9: Row structs + mappers

**Files:**
- Create/replace: `backend/crates/recon-store/src/rows.rs`
- Test: in `rows.rs`

- [ ] **Step 1: Implement row structs and mappers** (these convert DB rows to domain wire types; dates → strings)

`src/rows.rs`:
```rust
use recon_domain::*;
use sqlx::FromRow;
use time::{format_description::well_known::Rfc3339, Date, OffsetDateTime};
use time::format_description::FormatItem;
use time::macros::format_description;

const YMD: &[FormatItem<'static>] = format_description!("[year]-[month]-[day]");

pub fn date_to_string(d: Date) -> String { d.format(YMD).unwrap_or_default() }
pub fn ts_to_string(t: OffsetDateTime) -> String { t.format(&Rfc3339).unwrap_or_default() }

fn parse_role(s: &str) -> UserRole {
    match s { "approver" => UserRole::Approver, "admin" => UserRole::Admin, _ => UserRole::Operator }
}
fn parse_source_kind(s: &str) -> SourceKind {
    match s { "bank" => SourceKind::Bank, "ledger" => SourceKind::Ledger, _ => SourceKind::CrossSystem }
}
fn parse_direction(s: &str) -> Direction { if s == "credit" { Direction::Credit } else { Direction::Debit } }
fn parse_run_status(s: &str) -> RunStatus {
    match s { "completed" => RunStatus::Completed, "failed" => RunStatus::Failed, _ => RunStatus::Running }
}
fn parse_match_type(s: &str) -> MatchType {
    match s { "matched" => MatchType::Matched, "duplicate" => MatchType::Duplicate, _ => MatchType::Partial }
}
fn parse_break_type(s: &str) -> BreakType {
    match s { "unmatched" => BreakType::Unmatched, "partial" => BreakType::Partial, "duplicate" => BreakType::Duplicate, _ => BreakType::Break }
}
pub fn parse_break_status(s: &str) -> BreakStatus {
    match s {
        "investigating" => BreakStatus::Investigating,
        "pending_approval" => BreakStatus::PendingApproval,
        "resolved" => BreakStatus::Resolved,
        "written_off" => BreakStatus::WrittenOff,
        _ => BreakStatus::Open,
    }
}

#[derive(FromRow)]
pub struct TenantRow { pub id: String, pub name: String, pub slug: String }
impl From<TenantRow> for Tenant { fn from(r: TenantRow) -> Self { Tenant { id: r.id, name: r.name, slug: r.slug } } }

#[derive(FromRow)]
pub struct UserRow { pub id: String, pub name: String, pub role: String }
impl From<UserRow> for User { fn from(r: UserRow) -> Self { User { id: r.id, name: r.name, role: parse_role(&r.role) } } }

#[derive(FromRow)]
pub struct SourceRow { pub id: String, pub tenant_id: String, pub kind: String, pub name: String, pub currency: String }
impl From<SourceRow> for Source {
    fn from(r: SourceRow) -> Self {
        Source { id: r.id, tenant_id: r.tenant_id, kind: parse_source_kind(&r.kind), name: r.name, currency: r.currency }
    }
}

#[derive(FromRow)]
pub struct TxnRow {
    pub id: String, pub tenant_id: String, pub source_id: String, pub external_ref: String,
    pub value_date: Date, pub posted_at: OffsetDateTime, pub amount_minor: i64,
    pub currency: String, pub direction: String, pub counterparty: Option<String>, pub description: String,
}
impl From<TxnRow> for CanonicalTransaction {
    fn from(r: TxnRow) -> Self {
        CanonicalTransaction {
            id: r.id, tenant_id: r.tenant_id, source_id: r.source_id, external_ref: r.external_ref,
            value_date: date_to_string(r.value_date), posted_at: ts_to_string(r.posted_at),
            amount_minor: r.amount_minor, currency: r.currency, direction: parse_direction(&r.direction),
            counterparty: r.counterparty, description: r.description,
        }
    }
}

#[derive(FromRow)]
pub struct RunRow {
    pub id: String, pub tenant_id: String, pub name: String, pub source_a_id: String, pub source_b_id: String,
    pub status: String, pub started_at: OffsetDateTime, pub completed_at: Option<OffsetDateTime>,
    pub config_version: String, pub stats: serde_json::Value,
}
impl TryFrom<RunRow> for ReconciliationRun {
    type Error = serde_json::Error;
    fn try_from(r: RunRow) -> Result<Self, Self::Error> {
        Ok(ReconciliationRun {
            id: r.id, tenant_id: r.tenant_id, name: r.name, source_a_id: r.source_a_id, source_b_id: r.source_b_id,
            status: parse_run_status(&r.status), started_at: ts_to_string(r.started_at),
            completed_at: r.completed_at.map(ts_to_string), config_version: r.config_version,
            stats: serde_json::from_value(r.stats)?,
        })
    }
}

#[derive(FromRow)]
pub struct DecisionRow {
    pub id: String, pub run_id: String, #[sqlx(rename = "type")] pub type_: String,
    pub txn_ids: Vec<String>, pub score: f64, pub config_version: String,
}
impl From<DecisionRow> for MatchDecision {
    fn from(r: DecisionRow) -> Self {
        MatchDecision { id: r.id, run_id: r.run_id, match_type: parse_match_type(&r.type_), txn_ids: r.txn_ids, score: r.score, config_version: r.config_version }
    }
}

#[derive(FromRow)]
pub struct BreakRow {
    pub id: String, pub tenant_id: String, pub run_id: String, pub case_id: String,
    #[sqlx(rename = "type")] pub type_: String, pub status: String,
    pub value_minor: i64, pub currency: String, pub assignee_id: Option<String>,
    pub txn_ids: Vec<String>, pub opened_at: OffsetDateTime,
}
impl BreakRow {
    /// Ageing is computed at read time relative to `now`.
    pub fn into_break(self, now: OffsetDateTime) -> Break {
        let days = ((now - self.opened_at).whole_days()).max(0);
        Break {
            id: self.id, tenant_id: self.tenant_id, run_id: self.run_id, case_id: self.case_id,
            break_type: parse_break_type(&self.type_), status: parse_break_status(&self.status),
            ageing_days: days, ageing_bucket: recon_domain::ageing::ageing_bucket(days),
            value_minor: self.value_minor, currency: self.currency, assignee_id: self.assignee_id,
            txn_ids: self.txn_ids, opened_at: ts_to_string(self.opened_at),
        }
    }
}

#[derive(FromRow)]
pub struct CaseRow { pub id: String, pub break_id: String, pub assignee_id: Option<String>, pub status: String }

#[derive(FromRow)]
pub struct EventRow { pub id: String, pub actor_id: String, pub at: OffsetDateTime, pub kind: String, pub payload: serde_json::Value }
impl TryFrom<EventRow> for CaseEvent {
    type Error = serde_json::Error;
    fn try_from(r: EventRow) -> Result<Self, Self::Error> {
        // Reconstruct the body from {kind, payload} via the adjacently-tagged enum.
        let body: CaseEventBody = serde_json::from_value(serde_json::json!({ "kind": r.kind, "payload": r.payload }))?;
        Ok(CaseEvent { id: r.id, actor_id: r.actor_id, at: ts_to_string(r.at), body })
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p recon-store`
Expected: `Finished`.

- [ ] **Step 3: Commit**

```bash
git add backend/crates/recon-store/src/rows.rs
git commit -m "feat(store): FromRow row structs + domain mappers"
```

---

### Task 10: Read repository — tenants, users, runs, breaks, case + isolation tests

**Files:**
- Replace: `backend/crates/recon-store/src/read.rs`
- Test: `backend/crates/recon-store/tests/isolation.rs`

- [ ] **Step 1: Implement reads**

`src/read.rs`:
```rust
use recon_domain::*;
use time::OffsetDateTime;
use crate::rows::*;
use crate::{Store, StoreError};

#[derive(Default)]
pub struct RunFilter { pub status: Option<String>, pub source_id: Option<String>, pub from: Option<String>, pub to: Option<String> }
#[derive(Default)]
pub struct BreakFilter { pub status: Option<String>, pub kind: Option<String>, pub ageing_bucket: Option<String>, pub assignee_id: Option<String> }

pub struct RunDetail {
    pub run: ReconciliationRun,
    pub transactions: Vec<CanonicalTransaction>,
    pub matched: Vec<MatchDecision>,
    pub partial: Vec<MatchDecision>,
    pub duplicates: Vec<MatchDecision>,
    pub unmatched: Vec<Break>,
}
pub struct CaseBundle {
    pub case: Case,
    pub brk: Break,
    pub suggestions: Vec<(Vec<String>, f64, String)>, // txn_ids, score, rationale
    pub transactions: Vec<CanonicalTransaction>,
}

impl Store {
    pub async fn list_tenants(&self) -> Result<Vec<Tenant>, StoreError> {
        let rows: Vec<TenantRow> = sqlx::query_as("SELECT id, name, slug FROM tenants ORDER BY name").fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn list_users(&self, tenant_id: &str) -> Result<Vec<User>, StoreError> {
        let rows: Vec<UserRow> = sqlx::query_as("SELECT id, name, role FROM users WHERE tenant_id = $1 ORDER BY name")
            .bind(tenant_id).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn list_runs(&self, tenant_id: &str, f: &RunFilter) -> Result<Vec<ReconciliationRun>, StoreError> {
        let rows: Vec<RunRow> = sqlx::query_as(
            "SELECT * FROM reconciliation_runs
             WHERE tenant_id = $1
               AND ($2::text IS NULL OR status = $2)
               AND ($3::text IS NULL OR source_a_id = $3 OR source_b_id = $3)
               AND ($4::text IS NULL OR started_at >= $4::timestamptz)
               AND ($5::text IS NULL OR started_at <= $5::timestamptz)
             ORDER BY started_at DESC")
            .bind(tenant_id).bind(&f.status).bind(&f.source_id).bind(&f.from).bind(&f.to)
            .fetch_all(&self.pool).await?;
        rows.into_iter().map(|r| ReconciliationRun::try_from(r).map_err(StoreError::from)).collect()
    }

    pub async fn get_run(&self, tenant_id: &str, run_id: &str) -> Result<RunDetail, StoreError> {
        let now = OffsetDateTime::now_utc();
        let run_row: Option<RunRow> = sqlx::query_as("SELECT * FROM reconciliation_runs WHERE id = $1 AND tenant_id = $2")
            .bind(run_id).bind(tenant_id).fetch_optional(&self.pool).await?;
        let run = ReconciliationRun::try_from(run_row.ok_or(StoreError::NotFound)?)?;

        let drows: Vec<DecisionRow> = sqlx::query_as("SELECT id, run_id, type, txn_ids, score, config_version FROM match_decisions WHERE run_id = $1 AND tenant_id = $2 ORDER BY id")
            .bind(run_id).bind(tenant_id).fetch_all(&self.pool).await?;
        let decisions: Vec<MatchDecision> = drows.into_iter().map(Into::into).collect();

        let brows: Vec<BreakRow> = sqlx::query_as("SELECT * FROM breaks WHERE run_id = $1 AND tenant_id = $2 ORDER BY id")
            .bind(run_id).bind(tenant_id).fetch_all(&self.pool).await?;
        let unmatched: Vec<Break> = brows.into_iter().map(|b| b.into_break(now)).collect();

        let mut ids: Vec<String> = decisions.iter().flat_map(|d| d.txn_ids.clone())
            .chain(unmatched.iter().flat_map(|b| b.txn_ids.clone())).collect();
        ids.sort(); ids.dedup();
        let transactions = self.txns_by_ids(tenant_id, &ids).await?;

        let by = |t: MatchType| decisions.iter().filter(|d| d.match_type == t).cloned().collect::<Vec<_>>();
        Ok(RunDetail { run, transactions, matched: by(MatchType::Matched), partial: by(MatchType::Partial), duplicates: by(MatchType::Duplicate), unmatched })
    }

    pub async fn list_breaks(&self, tenant_id: &str, f: &BreakFilter) -> Result<Vec<Break>, StoreError> {
        let now = OffsetDateTime::now_utc();
        let rows: Vec<BreakRow> = sqlx::query_as(
            "SELECT * FROM breaks
             WHERE tenant_id = $1
               AND ($2::text IS NULL OR status = $2)
               AND ($3::text IS NULL OR type = $3)
               AND ($4::text IS NULL OR assignee_id = $4)
             ORDER BY opened_at DESC")
            .bind(tenant_id).bind(&f.status).bind(&f.kind).bind(&f.assignee_id)
            .fetch_all(&self.pool).await?;
        let mut breaks: Vec<Break> = rows.into_iter().map(|b| b.into_break(now)).collect();
        // ageing_bucket is computed, so filter it in Rust to match the wire value.
        if let Some(bucket) = &f.ageing_bucket {
            breaks.retain(|b| serde_json::to_value(b.ageing_bucket).ok().and_then(|v| v.as_str().map(|s| s == bucket)).unwrap_or(false));
        }
        Ok(breaks)
    }

    pub async fn txns_by_ids(&self, tenant_id: &str, ids: &[String]) -> Result<Vec<CanonicalTransaction>, StoreError> {
        if ids.is_empty() { return Ok(vec![]); }
        let rows: Vec<TxnRow> = sqlx::query_as("SELECT * FROM canonical_transactions WHERE tenant_id = $1 AND id = ANY($2) ORDER BY id")
            .bind(tenant_id).bind(ids).fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    pub async fn load_case(&self, tenant_id: &str, case_id: &str) -> Result<Case, StoreError> {
        let crow: Option<CaseRow> = sqlx::query_as("SELECT id, break_id, assignee_id, status FROM cases WHERE id = $1 AND tenant_id = $2")
            .bind(case_id).bind(tenant_id).fetch_optional(&self.pool).await?;
        let crow = crow.ok_or(StoreError::NotFound)?;
        let erows: Vec<EventRow> = sqlx::query_as("SELECT id, actor_id, at, kind, payload FROM case_events WHERE case_id = $1 AND tenant_id = $2 ORDER BY seq")
            .bind(case_id).bind(tenant_id).fetch_all(&self.pool).await?;
        let events: Vec<CaseEvent> = erows.into_iter().map(CaseEvent::try_from).collect::<Result<_, _>>()?;
        Ok(Case { id: crow.id, break_id: crow.break_id, assignee_id: crow.assignee_id, status: crate::rows::parse_break_status(&crow.status), events })
    }

    pub async fn get_case(&self, tenant_id: &str, case_id: &str) -> Result<CaseBundle, StoreError> {
        let now = OffsetDateTime::now_utc();
        let case = self.load_case(tenant_id, case_id).await?;
        let brow: Option<BreakRow> = sqlx::query_as("SELECT * FROM breaks WHERE case_id = $1 AND tenant_id = $2")
            .bind(case_id).bind(tenant_id).fetch_optional(&self.pool).await?;
        let brk = brow.ok_or(StoreError::NotFound)?.into_break(now);

        // Suggestions: score the break's transactions against other tenant transactions.
        let brk_txns = self.txns_by_ids(tenant_id, &brk.txn_ids).await?;
        let all: Vec<TxnRow> = sqlx::query_as("SELECT * FROM canonical_transactions WHERE tenant_id = $1 ORDER BY id")
            .bind(tenant_id).fetch_all(&self.pool).await?;
        let candidates: Vec<CanonicalTransaction> = all.into_iter().map(Into::into).collect();
        let sugg = recon_matching::suggestions_for(&brk_txns, &candidates, 0.55);
        let suggestions: Vec<(Vec<String>, f64, String)> = sugg.into_iter().take(3).map(|s| (s.txn_ids, s.score, s.rationale)).collect();

        let mut ids: Vec<String> = brk.txn_ids.clone();
        for (tids, _, _) in &suggestions { ids.extend(tids.clone()); }
        ids.sort(); ids.dedup();
        let transactions = self.txns_by_ids(tenant_id, &ids).await?;
        Ok(CaseBundle { case, brk, suggestions, transactions })
    }
}
```

- [ ] **Step 2: Write isolation + read tests** (extend `tests/isolation.rs`)

```rust
use recon_store::Store;
use recon_store::read::{BreakFilter, RunFilter};

async fn seed_two_tenants(store: &Store) {
    store.migrate().await.unwrap();
    for (t, name) in [("tenant-a", "Alpha"), ("tenant-b", "Beta")] {
        sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,$2,$1)").bind(t).bind(name).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO users(id,tenant_id,name,role) VALUES ($1,$2,'U','operator')").bind(format!("u-{t}")).bind(t).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ($1,$2,'bank','S','GBP')").bind(format!("s-{t}")).bind(t).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO reconciliation_runs(id,tenant_id,name,source_a_id,source_b_id,status,started_at,config_version,stats) VALUES ($1,$2,'R',$3,$3,'completed', now(), 'v1', '{\"matched\":1,\"unmatched\":0,\"partial\":0,\"duplicate\":0,\"breakCount\":0,\"matchRatePct\":100.0,\"valueAtRiskMinor\":0}'::jsonb)")
            .bind(format!("run-{t}")).bind(t).bind(format!("s-{t}")).execute(&store.pool).await.unwrap();
    }
}

#[sqlx::test]
async fn tenant_isolation_on_runs(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_two_tenants(&store).await;
    let a = store.list_runs("tenant-a", &RunFilter::default()).await.unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].tenant_id, "tenant-a");
    // tenant-a cannot fetch tenant-b's run
    let cross = store.get_run("tenant-a", "run-tenant-b").await;
    assert!(matches!(cross, Err(recon_store::StoreError::NotFound)));
}

#[sqlx::test]
async fn tenant_isolation_on_users(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_two_tenants(&store).await;
    let users = store.list_users("tenant-a").await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(store.list_breaks("tenant-a", &BreakFilter::default()).await.unwrap().len(), 0);
}
```

- [ ] **Step 3: Run**

Run: `DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add backend/crates/recon-store
git commit -m "feat(store): read repository + tenant-isolation tests"
```

---

### Task 11: Dashboard aggregation

**Files:**
- Replace: `backend/crates/recon-store/src/dashboard.rs`
- Test: in `tests/isolation.rs`

- [ ] **Step 1: Implement aggregation** (mirrors `mock.ts:getDashboard`)

`src/dashboard.rs`:
```rust
use recon_domain::*;
use time::OffsetDateTime;
use crate::rows::{BreakRow, RunRow};
use crate::{Store, StoreError};

pub struct DashboardSummary {
    pub match_rate_pct: f64,
    pub open_breaks: i64,
    pub value_at_risk_minor: i64,
    pub currency: String,
    pub sla_adherence_pct: f64,
    pub breaks_by_type: Vec<(BreakType, i64)>,
    pub breaks_by_ageing: Vec<(AgeingBucket, i64)>,
    pub recent_runs: Vec<ReconciliationRun>,
}

fn is_open(s: BreakStatus) -> bool { matches!(s, BreakStatus::Open | BreakStatus::Investigating | BreakStatus::PendingApproval) }

impl Store {
    pub async fn get_dashboard(&self, tenant_id: &str) -> Result<DashboardSummary, StoreError> {
        let now = OffsetDateTime::now_utc();
        let brows: Vec<BreakRow> = sqlx::query_as("SELECT * FROM breaks WHERE tenant_id = $1").bind(tenant_id).fetch_all(&self.pool).await?;
        let breaks: Vec<Break> = brows.into_iter().map(|b| b.into_break(now)).collect();
        let rrows: Vec<RunRow> = sqlx::query_as("SELECT * FROM reconciliation_runs WHERE tenant_id = $1 ORDER BY started_at DESC").bind(tenant_id).fetch_all(&self.pool).await?;
        let runs: Vec<ReconciliationRun> = rrows.into_iter().map(ReconciliationRun::try_from).collect::<Result<_, _>>()?;

        let open: Vec<&Break> = breaks.iter().filter(|b| is_open(b.status)).collect();
        let value_at_risk_minor = open.iter().map(|b| b.value_minor).sum();
        let currency = breaks.first().map(|b| b.currency.clone()).unwrap_or_else(|| "GBP".into());

        let completed: Vec<&ReconciliationRun> = runs.iter().filter(|r| r.status == RunStatus::Completed).collect();
        let match_rate_pct = if completed.is_empty() { 0.0 } else {
            let avg = completed.iter().map(|r| r.stats.match_rate_pct).sum::<f64>() / completed.len() as f64;
            (avg * 10.0).round() / 10.0
        };

        let resolved: Vec<&Break> = breaks.iter().filter(|b| matches!(b.status, BreakStatus::Resolved | BreakStatus::WrittenOff)).collect();
        let sla_adherence_pct = if resolved.is_empty() { 100.0 } else {
            let ok = resolved.iter().filter(|b| b.ageing_days <= 7).count();
            ((ok as f64 / resolved.len() as f64) * 1000.0).round() / 10.0
        };

        let breaks_by_type = [BreakType::Unmatched, BreakType::Partial, BreakType::Duplicate, BreakType::Break]
            .into_iter().map(|t| (t, breaks.iter().filter(|b| b.break_type == t).count() as i64)).collect();
        let breaks_by_ageing = [AgeingBucket::ZeroToOne, AgeingBucket::TwoToSeven, AgeingBucket::EightToThirty, AgeingBucket::ThirtyPlus]
            .into_iter().map(|bk| (bk, open.iter().filter(|b| b.ageing_bucket == bk).count() as i64)).collect();

        let recent_runs = completed.into_iter().take(5).cloned().collect();

        Ok(DashboardSummary { match_rate_pct, open_breaks: open.len() as i64, value_at_risk_minor, currency, sla_adherence_pct, breaks_by_type, breaks_by_ageing, recent_runs })
    }
}
```

- [ ] **Step 2: Test** (append to `tests/isolation.rs`)

```rust
#[sqlx::test]
async fn dashboard_counts_open_breaks(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_two_tenants(&store).await;
    // add a case + open break for tenant-a
    sqlx::query("INSERT INTO cases(id,tenant_id,break_id,status) VALUES ('c1','tenant-a','b1','open')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO breaks(id,tenant_id,run_id,case_id,type,status,value_minor,currency,txn_ids,opened_at) VALUES ('b1','tenant-a','run-tenant-a','c1','unmatched','open',1000,'GBP','{}', now())").execute(&store.pool).await.unwrap();
    let d = store.get_dashboard("tenant-a").await.unwrap();
    assert_eq!(d.open_breaks, 1);
    assert_eq!(d.value_at_risk_minor, 1000);
    assert_eq!(d.match_rate_pct, 100.0);
}
```

- [ ] **Step 3: Run + commit**

Run: `DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store`
Expected: PASS.
```bash
git add backend/crates/recon-store
git commit -m "feat(store): dashboard aggregation"
```

---

### Task 12: Write repository — assign + append event (four-eyes, transitions, append-only)

**Files:**
- Replace: `backend/crates/recon-store/src/write.rs`
- Test: `backend/crates/recon-store/tests/write.rs`

- [ ] **Step 1: Implement writes** (mirrors `mock.ts` + adds server-side four-eyes)

`src/write.rs`:
```rust
use recon_domain::*;
use time::OffsetDateTime;
use uuid::Uuid;
use crate::rows::{ts_to_string, BreakRow};
use crate::{Store, StoreError};

impl Store {
    async fn next_seq(&self, tx: &mut sqlx::PgConnection, case_id: &str) -> Result<i32, StoreError> {
        let max: Option<i32> = sqlx::query_scalar("SELECT max(seq) FROM case_events WHERE case_id = $1").bind(case_id).fetch_one(&mut *tx).await?;
        Ok(max.unwrap_or(0) + 1)
    }

    pub async fn assign_break(&self, tenant_id: &str, break_id: &str, user_id: &str) -> Result<Break, StoreError> {
        let now = OffsetDateTime::now_utc();
        let mut tx = self.pool.begin().await?;
        let brow: Option<BreakRow> = sqlx::query_as("SELECT * FROM breaks WHERE id = $1 AND tenant_id = $2 FOR UPDATE")
            .bind(break_id).bind(tenant_id).fetch_optional(&mut *tx).await?;
        let brk = brow.ok_or(StoreError::NotFound)?;
        let new_status = if brk.status == "open" { "investigating" } else { brk.status.as_str() };

        sqlx::query("UPDATE breaks SET assignee_id = $1, status = $2 WHERE id = $3")
            .bind(user_id).bind(new_status).bind(break_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE cases SET assignee_id = $1, status = CASE WHEN status = 'open' THEN 'investigating' ELSE status END WHERE id = $2")
            .bind(user_id).bind(&brk.case_id).execute(&mut *tx).await?;

        // Append an assignment event (actor = assignee, matching mock.ts:313).
        let seq = self.next_seq(&mut tx, &brk.case_id).await?;
        sqlx::query("INSERT INTO case_events(id,tenant_id,case_id,seq,kind,actor_id,at,payload) VALUES ($1,$2,$3,$4,'assignment',$5,$6,$7)")
            .bind(Uuid::new_v4().to_string()).bind(tenant_id).bind(&brk.case_id).bind(seq)
            .bind(user_id).bind(now)
            .bind(serde_json::json!({ "assigneeId": user_id }))
            .execute(&mut *tx).await?;

        let updated: BreakRow = sqlx::query_as("SELECT * FROM breaks WHERE id = $1").bind(break_id).fetch_one(&mut *tx).await?;
        tx.commit().await?;
        Ok(updated.into_break(now))
    }

    pub async fn append_case_event(&self, tenant_id: &str, case_id: &str, ev: NewCaseEvent) -> Result<Case, StoreError> {
        let now = OffsetDateTime::now_utc();
        let mut tx = self.pool.begin().await?;

        // Load current case for status + four-eyes checks.
        let case = self.load_case(tenant_id, case_id).await?;

        // Compute the resulting status transition.
        let new_status: Option<BreakStatus> = match &ev.body {
            CaseEventBody::ApprovalRequested { .. } => Some(BreakStatus::PendingApproval),
            CaseEventBody::Approved {} => {
                // Server-side four-eyes enforcement.
                let actor: Option<crate::rows::UserRow> = sqlx::query_as("SELECT id, name, role FROM users WHERE id = $1 AND tenant_id = $2")
                    .bind(&ev.actor_id).bind(tenant_id).fetch_optional(&mut *tx).await?;
                let actor: User = actor.ok_or(StoreError::NotFound)?.into();
                recon_domain::can_approve(&case, &actor).map_err(|e| StoreError::Forbidden(e.to_string()))?;
                Some(BreakStatus::Resolved)
            }
            CaseEventBody::Rejected { .. } => {
                if case.status != BreakStatus::PendingApproval {
                    return Err(StoreError::Conflict("case is not pending approval".into()));
                }
                Some(BreakStatus::Investigating)
            }
            CaseEventBody::Assignment { .. } => {
                if case.status == BreakStatus::Open { Some(BreakStatus::Investigating) } else { None }
            }
            _ => None,
        };

        // Persist the event (append-only).
        let kind_val = serde_json::to_value(&ev.body)?; // {"kind":..,"payload":..}
        let kind = kind_val["kind"].as_str().unwrap().to_string();
        let payload = kind_val["payload"].clone();
        let seq = self.next_seq(&mut tx, case_id).await?;
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO case_events(id,tenant_id,case_id,seq,kind,actor_id,at,payload) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(&id).bind(tenant_id).bind(case_id).bind(seq).bind(&kind).bind(&ev.actor_id).bind(now).bind(&payload)
            .execute(&mut *tx).await?;

        // Apply transition to case + linked break.
        if let Some(status) = new_status {
            let status_str = serde_json::to_value(status)?.as_str().unwrap().to_string();
            let assignee = if let CaseEventBody::Assignment { assignee_id } = &ev.body { Some(assignee_id.clone()) } else { None };
            sqlx::query("UPDATE cases SET status = $1, assignee_id = COALESCE($2, assignee_id) WHERE id = $3 AND tenant_id = $4")
                .bind(&status_str).bind(&assignee).bind(case_id).bind(tenant_id).execute(&mut *tx).await?;
            sqlx::query("UPDATE breaks SET status = $1, assignee_id = COALESCE($2, assignee_id) WHERE case_id = $3 AND tenant_id = $4")
                .bind(&status_str).bind(&assignee).bind(case_id).bind(tenant_id).execute(&mut *tx).await?;
        }

        tx.commit().await?;
        let _ = ts_to_string(now);
        self.load_case(tenant_id, case_id).await
    }
}
```
(Make `UserRow` fields `pub` in `rows.rs` if not already, and `pub use` it.)

- [ ] **Step 2: Write four-eyes + append-only tests**

`tests/write.rs`:
```rust
use recon_store::Store;
use recon_domain::{NewCaseEvent, CaseEventBody, Resolution, BreakStatus};

async fn seed_pending(store: &Store) {
    store.migrate().await.unwrap();
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')").execute(&store.pool).await.unwrap();
    for (id, role) in [("user-mia","operator"),("user-theo","approver")] {
        sqlx::query("INSERT INTO users(id,tenant_id,name,role) VALUES ($1,'t',$1,$2)").bind(id).bind(role).execute(&store.pool).await.unwrap();
    }
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ('s','t','bank','S','GBP')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO reconciliation_runs(id,tenant_id,name,source_a_id,source_b_id,status,started_at,config_version,stats) VALUES ('r','t','R','s','s','completed',now(),'v1','{}'::jsonb)").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO cases(id,tenant_id,break_id,assignee_id,status) VALUES ('case-pending','t','break-pending','user-mia','pending_approval')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO breaks(id,tenant_id,run_id,case_id,type,status,value_minor,currency,assignee_id,txn_ids,opened_at) VALUES ('break-pending','t','r','case-pending','unmatched','pending_approval',125000,'GBP','user-mia','{}', now())").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO case_events(id,tenant_id,case_id,seq,kind,actor_id,at,payload) VALUES ('e1','t','case-pending',1,'approval_requested','user-mia',now(),'{\"resolution\":\"write_off\"}'::jsonb)").execute(&store.pool).await.unwrap();
}

#[sqlx::test]
async fn maker_approval_is_forbidden(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_pending(&store).await;
    let ev = NewCaseEvent { actor_id: "user-mia".into(), body: CaseEventBody::Approved {} };
    let r = store.append_case_event("t", "case-pending", ev).await;
    assert!(matches!(r, Err(recon_store::StoreError::Forbidden(_))));
    // case still pending
    let c = store.load_case("t", "case-pending").await.unwrap();
    assert_eq!(c.status, BreakStatus::PendingApproval);
}

#[sqlx::test]
async fn different_approver_resolves(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_pending(&store).await;
    let ev = NewCaseEvent { actor_id: "user-theo".into(), body: CaseEventBody::Approved {} };
    let c = store.append_case_event("t", "case-pending", ev).await.unwrap();
    assert_eq!(c.status, BreakStatus::Resolved);
    assert!(c.events.iter().any(|e| matches!(e.body, CaseEventBody::Approved {})));
}

#[sqlx::test]
async fn comment_is_append_only_and_keeps_status(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_pending(&store).await;
    let before = store.load_case("t", "case-pending").await.unwrap().events.len();
    let ev = NewCaseEvent { actor_id: "user-mia".into(), body: CaseEventBody::Comment { text: "hi".into() } };
    let c = store.append_case_event("t", "case-pending", ev).await.unwrap();
    assert_eq!(c.events.len(), before + 1);
    assert_eq!(c.status, BreakStatus::PendingApproval);
}
```

- [ ] **Step 3: Run**

Run: `DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store`
Expected: PASS.

- [ ] **Step 4: Lint + commit**

```bash
cargo clippy -p recon-store --all-targets -- -D warnings
git add backend/crates/recon-store
git commit -m "feat(store): writes with server-side four-eyes + transitions"
```

---

### Task 13: Deterministic seed

**Files:**
- Replace: `backend/crates/recon-store/src/seed.rs`
- Test: in `tests/write.rs`

**Seed contract:** insert the exact fixture tenants/users/sources/transactions (same IDs as `web/lib/api/fixtures.ts`), then for a fixed set of run definitions call `reconcile()` to compute decisions/breaks/stats and insert them with deterministic IDs, then overlay the `case-pending` four-eyes scenario and a couple of other narrative cases. KPI numbers are engine-derived (not the fixtures' fictional values); IDs and the four-eyes scenario match.

- [ ] **Step 1: Implement seed**

`src/seed.rs`:
```rust
use recon_matching::{reconcile, MatchConfig};
use recon_domain::*;
use time::OffsetDateTime;
use crate::{Store, StoreError};

/// Reset (idempotent) and load the deterministic demo dataset.
impl Store {
    pub async fn seed(&self) -> Result<(), StoreError> {
        self.migrate().await?;
        let mut tx = self.pool.begin().await?;
        // Idempotent reset (children first).
        for t in ["case_events","breaks","match_decisions","cases","reconciliation_runs","canonical_transactions","sources","users","tenants"] {
            sqlx::query(&format!("DELETE FROM {t}")).execute(&mut *tx).await?;
        }

        // --- Reference data (IDs identical to web/lib/api/fixtures.ts) ---
        for (id, name, slug) in [("tenant-acme","Acme Capital","acme-capital"), ("tenant-globex","Globex Markets","globex-markets")] {
            sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,$2,$3)").bind(id).bind(name).bind(slug).execute(&mut *tx).await?;
        }
        for (id, name, role) in [("user-mia","Mia","operator"),("user-sam","Sam","operator"),("user-theo","Theo","approver"),("user-ada","Ada","admin")] {
            sqlx::query("INSERT INTO users(id,tenant_id,name,role) VALUES ($1,'tenant-acme',$2,$3)").bind(id).bind(name).bind(role).execute(&mut *tx).await?;
        }
        // Globex gets its own approver so its UI is usable.
        for (id, name, role) in [("user-glo-op","Nia","operator"),("user-glo-ap","Omar","approver")] {
            sqlx::query("INSERT INTO users(id,tenant_id,name,role) VALUES ($1,'tenant-globex',$2,$3)").bind(id).bind(name).bind(role).execute(&mut *tx).await?;
        }
        for (id, tid, kind, name, cur) in [
            ("src-acme-bank","tenant-acme","bank","Acme Bank Statement","GBP"),
            ("src-acme-ledger","tenant-acme","ledger","Acme General Ledger","GBP"),
            ("src-globex-bank","tenant-globex","bank","Globex Bank Statement","USD"),
            ("src-globex-ledger","tenant-globex","ledger","Globex General Ledger","USD"),
        ] {
            sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ($1,$2,$3,$4,$5)").bind(id).bind(tid).bind(kind).bind(name).bind(cur).execute(&mut *tx).await?;
        }

        // --- Raw transactions: (id, source, ref, date, amount, dir) all tenant-acme/GBP unless noted ---
        // Designed so the engine yields exact matches, one partial, one duplicate, and unmatched breaks.
        let txns: &[(&str,&str,&str,&str,i64,&str,&str)] = &[
            // matched pairs
            ("txn-a001","src-acme-bank","BANK-1","2026-05-01",1000000,"debit","tenant-acme"),
            ("txn-b001","src-acme-ledger","BANK-1","2026-05-01",1000000,"debit","tenant-acme"),
            ("txn-a002","src-acme-bank","BANK-2","2026-05-01",500000,"credit","tenant-acme"),
            ("txn-b002","src-acme-ledger","BANK-2","2026-05-01",500000,"credit","tenant-acme"),
            ("txn-a003","src-acme-bank","BANK-3","2026-05-02",250000,"debit","tenant-acme"),
            ("txn-b003","src-acme-ledger","BANK-3","2026-05-02",250000,"debit","tenant-acme"),
            // partial (amount differs by 500, within tolerance)
            ("txn-a005","src-acme-bank","BANK-5","2026-05-04",320000,"debit","tenant-acme"),
            ("txn-b005","src-acme-ledger","BANK-5","2026-05-04",319500,"debit","tenant-acme"),
            // duplicate within ledger
            ("txn-c004","src-acme-ledger","DUP-9","2026-05-10",95000,"debit","tenant-acme"),
            ("txn-c005","src-acme-ledger","DUP-9","2026-05-10",95000,"debit","tenant-acme"),
            // unmatched (the write-off candidate -> case-pending)
            ("txn-brk001","src-acme-bank","BANK-99","2026-05-15",125000,"debit","tenant-acme"),
            // a few more unmatched to populate exceptions across types/ageing
            ("txn-brk002","src-acme-bank","BANK-10","2026-05-16",67500,"credit","tenant-acme"),
            ("txn-brk005","src-acme-bank","BANK-18","2026-05-18",210000,"debit","tenant-acme"),
            ("txn-brk006","src-acme-bank","BANK-19","2026-05-23",88000,"credit","tenant-acme"),
            // globex matched pair + one unmatched
            ("txn-g001","src-globex-bank","GB-1","2026-05-01",2000000,"debit","tenant-globex"),
            ("txn-g002","src-globex-ledger","GB-1","2026-05-01",2000000,"debit","tenant-globex"),
            ("txn-g005","src-globex-bank","GB-9","2026-05-10",390000,"debit","tenant-globex"),
        ];
        for (id, src, eref, date, amt, dir, tid) in txns {
            let cur = if *tid == "tenant-globex" { "USD" } else { "GBP" };
            sqlx::query("INSERT INTO canonical_transactions(id,tenant_id,source_id,external_ref,value_date,posted_at,amount_minor,currency,direction,description) VALUES ($1,$2,$3,$4,$5::date,($5||'T09:00:00Z')::timestamptz,$6,$7,$8,$9)")
                .bind(id).bind(tid).bind(src).bind(eref).bind(date).bind(amt).bind(cur).bind(dir).bind(format!("Txn {id}"))
                .execute(&mut *tx).await?;
        }

        // --- Run definitions: (run_id, tenant, name, src_a, src_b, started_at) ---
        let runs: &[(&str,&str,&str,&str,&str,&str)] = &[
            ("run-acme-001","tenant-acme","Daily Bank-GL 2026-05-02","src-acme-bank","src-acme-ledger","2026-05-02T18:00:00Z"),
            ("run-acme-006","tenant-acme","Daily Bank-GL 2026-05-15","src-acme-bank","src-acme-ledger","2026-05-15T18:00:00Z"),
            ("run-globex-001","tenant-globex","Globex Daily 2026-05-10","src-globex-bank","src-globex-ledger","2026-05-10T19:00:00Z"),
        ];
        let cfg = MatchConfig::v1();
        for (run_id, tid, name, sa, sb, started) in runs {
            let a = self.load_source_txns(&mut tx, tid, sa).await?;
            let b = self.load_source_txns(&mut tx, tid, sb).await?;
            let result = reconcile(&a, &b, &cfg);
            let stats = serde_json::to_value(&result.stats)?;
            sqlx::query("INSERT INTO reconciliation_runs(id,tenant_id,name,source_a_id,source_b_id,status,started_at,completed_at,config_version,stats) VALUES ($1,$2,$3,$4,$5,'completed',$6::timestamptz,$6::timestamptz,$7,$8)")
                .bind(run_id).bind(tid).bind(name).bind(sa).bind(sb).bind(started).bind(&cfg.version).bind(&stats).execute(&mut *tx).await?;
            for (i, d) in result.decisions.iter().enumerate() {
                let type_str = serde_json::to_value(d.match_type)?.as_str().unwrap().to_string();
                sqlx::query("INSERT INTO match_decisions(id,tenant_id,run_id,type,txn_ids,score,config_version) VALUES ($1,$2,$3,$4,$5,$6,$7)")
                    .bind(format!("md-{run_id}-{i}")).bind(tid).bind(run_id).bind(type_str).bind(&d.txn_ids).bind(d.score).bind(&cfg.version).execute(&mut *tx).await?;
            }
            for (i, bd) in result.breaks.iter().enumerate() {
                // The write-off candidate becomes the stable case-pending scenario.
                let is_pending = bd.txn_ids.iter().any(|t| t == "txn-brk001");
                let case_id = if is_pending { "case-pending".to_string() } else { format!("case-{run_id}-{i}") };
                let break_id = if is_pending { "break-pending".to_string() } else { format!("break-{run_id}-{i}") };
                let type_str = serde_json::to_value(bd.break_type)?.as_str().unwrap().to_string();
                let opened = if is_pending { "2026-05-15T10:30:00Z" } else { started };
                let (status, assignee) = if is_pending { ("pending_approval", Some("user-mia")) } else { ("open", None) };
                sqlx::query("INSERT INTO cases(id,tenant_id,break_id,assignee_id,status) VALUES ($1,$2,$3,$4,$5)")
                    .bind(&case_id).bind(tid).bind(&break_id).bind(assignee).bind(status).execute(&mut *tx).await?;
                sqlx::query("INSERT INTO breaks(id,tenant_id,run_id,case_id,type,status,value_minor,currency,assignee_id,txn_ids,opened_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11::timestamptz)")
                    .bind(&break_id).bind(tid).bind(run_id).bind(&case_id).bind(type_str).bind(status).bind(bd.value_minor).bind(&bd.currency).bind(assignee).bind(&bd.txn_ids).bind(opened).execute(&mut *tx).await?;
                if is_pending {
                    let evs: &[(i32,&str,&str,&str,serde_json::Value)] = &[
                        (1,"assignment","user-ada","2026-05-15T11:00:00Z", serde_json::json!({"assigneeId":"user-mia"})),
                        (2,"comment","user-mia","2026-05-16T09:00:00Z", serde_json::json!({"text":"Reviewed; looks like a write-off candidate."})),
                        (3,"write_off_proposed","user-mia","2026-05-16T09:30:00Z", serde_json::json!({"reason":"Counterparty confirmed unmatched; below materiality."})),
                        (4,"approval_requested","user-mia","2026-05-16T09:35:00Z", serde_json::json!({"resolution":"write_off"})),
                    ];
                    for (seq,kind,actor,at,payload) in evs {
                        sqlx::query("INSERT INTO case_events(id,tenant_id,case_id,seq,kind,actor_id,at,payload) VALUES ($1,$2,$3,$4,$5,$6,$7::timestamptz,$8)")
                            .bind(format!("evt-pending-{seq}")).bind(tid).bind(&case_id).bind(seq).bind(kind).bind(actor).bind(at).bind(payload).execute(&mut *tx).await?;
                    }
                }
            }
        }
        tx.commit().await?;
        Ok(())
    }

    async fn load_source_txns(&self, tx: &mut sqlx::PgConnection, tenant_id: &str, source_id: &str) -> Result<Vec<CanonicalTransaction>, StoreError> {
        let rows: Vec<crate::rows::TxnRow> = sqlx::query_as("SELECT * FROM canonical_transactions WHERE tenant_id = $1 AND source_id = $2 ORDER BY id")
            .bind(tenant_id).bind(source_id).fetch_all(&mut *tx).await?;
        Ok(rows.into_iter().map(Into::into).collect())
    }
}

#[allow(unused_imports)]
use OffsetDateTime as _OffsetDateTimeUnused;
```
(Remove the unused-import shim if `OffsetDateTime` ends up used; it's here only to keep the example self-contained.)

- [ ] **Step 2: Test the seed**

Append to `tests/write.rs`:
```rust
#[sqlx::test]
async fn seed_creates_case_pending_with_four_eyes(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    store.seed().await.unwrap();
    let c = store.load_case("tenant-acme", "case-pending").await.unwrap();
    assert_eq!(c.status, recon_domain::BreakStatus::PendingApproval);
    assert!(c.events.iter().any(|e| matches!(e.body, recon_domain::CaseEventBody::ApprovalRequested { .. })));
    // engine produced at least one matched decision for the early run
    let det = store.get_run("tenant-acme", "run-acme-001").await.unwrap();
    assert!(!det.matched.is_empty());
    // seeding is idempotent
    store.seed().await.unwrap();
    let tenants = store.list_tenants().await.unwrap();
    assert_eq!(tenants.len(), 2);
}
```

- [ ] **Step 3: Run + commit**

Run: `DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store`
Expected: PASS.
```bash
git add backend/crates/recon-store
git commit -m "feat(store): deterministic engine-driven seed with case-pending"
```

---

# Phase 4 — HTTP API

### Task 14: AppState, ApiError, AuthContext, healthz

**Files:**
- Create: `backend/crates/recon-api/src/state.rs`, `src/error.rs`, `src/auth.rs`
- Modify: `backend/crates/recon-api/Cargo.toml`, `src/main.rs`
- Test: in `src/auth.rs`

- [ ] **Step 1: Deps**

`recon-api/Cargo.toml`:
```toml
[dependencies]
recon-domain = { path = "../recon-domain" }
recon-store = { path = "../recon-store" }
recon-matching = { path = "../recon-matching" }
axum = { workspace = true }
tokio = { workspace = true }
tower = { workspace = true }
tower-http = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }

[dev-dependencies]
sqlx = { workspace = true }
tower = { workspace = true }
http-body-util = "0.1"
```

- [ ] **Step 2: Implement state + error + auth**

`src/state.rs`:
```rust
use recon_store::Store;
#[derive(Clone)]
pub struct AppState { pub store: Store }
```

`src/error.rs`:
```rust
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use recon_store::StoreError;
use serde_json::json;

pub struct ApiError { pub status: StatusCode, pub code: &'static str, pub message: String }

impl ApiError {
    pub fn unauthorized(m: impl Into<String>) -> Self { Self { status: StatusCode::UNAUTHORIZED, code: "unauthorized", message: m.into() } }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound => ApiError { status: StatusCode::NOT_FOUND, code: "not_found", message: "not found".into() },
            StoreError::Conflict(m) => ApiError { status: StatusCode::CONFLICT, code: "conflict", message: m },
            StoreError::Forbidden(m) => ApiError { status: StatusCode::FORBIDDEN, code: "forbidden", message: m },
            StoreError::Db(_) | StoreError::Json(_) => ApiError { status: StatusCode::INTERNAL_SERVER_ERROR, code: "internal", message: "internal error".into() },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": { "code": self.code, "message": self.message } }))).into_response()
    }
}
```

`src/auth.rs`:
```rust
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use crate::error::ApiError;

/// Establishes the caller's tenant from the X-Tenant-Id header.
/// This is the auth seam: a JWT validator will later populate the same struct.
pub struct AuthContext { pub tenant_id: String }

impl<S: Send + Sync> FromRequestParts<S> for AuthContext {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let tenant_id = parts.headers.get("x-tenant-id").and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ApiError::unauthorized("missing X-Tenant-Id"))?
            .to_string();
        Ok(AuthContext { tenant_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    #[tokio::test]
    async fn extracts_tenant() {
        let req = Request::builder().header("x-tenant-id", "tenant-acme").body(()).unwrap();
        let (mut parts, _) = req.into_parts();
        let ctx = AuthContext::from_request_parts(&mut parts, &()).await.unwrap();
        assert_eq!(ctx.tenant_id, "tenant-acme");
    }
    #[tokio::test]
    async fn missing_header_is_unauthorized() {
        let req = Request::builder().body(()).unwrap();
        let (mut parts, _) = req.into_parts();
        assert!(AuthContext::from_request_parts(&mut parts, &()).await.is_err());
    }
}
```

`src/main.rs` (temporary, replaced in Task 17):
```rust
mod state; mod error; mod auth;
fn main() { println!("recon-api"); }
```

- [ ] **Step 3: Run** `cargo test -p recon-api`
Expected: PASS (auth tests).

- [ ] **Step 4: Commit**
```bash
git add backend/crates/recon-api
git commit -m "feat(api): AppState, ApiError, AuthContext extractor"
```

---

### Task 15: Read routes + integration tests

**Files:**
- Create: `backend/crates/recon-api/src/routes.rs`, `src/dto.rs`
- Modify: `src/main.rs` (declare `mod routes; mod dto;`)
- Test: `backend/crates/recon-api/tests/api.rs`

- [ ] **Step 1: Implement read handlers + router** (writes added in Task 16)

`src/dto.rs`:
```rust
use serde::Deserialize;
#[derive(Deserialize)] #[serde(rename_all = "camelCase")]
pub struct AssignBody { pub user_id: String }
#[derive(Deserialize, Default)] #[serde(rename_all = "camelCase")]
pub struct RunQ { pub status: Option<String>, pub source_id: Option<String>, pub from: Option<String>, pub to: Option<String> }
#[derive(Deserialize, Default)] #[serde(rename_all = "camelCase")]
pub struct BreakQ { pub status: Option<String>, #[serde(rename = "type")] pub kind: Option<String>, pub ageing_bucket: Option<String>, pub assignee_id: Option<String> }
```

`src/routes.rs`:
```rust
use axum::{extract::{Path, Query, State}, routing::{get, post}, Json, Router};
use serde_json::{json, Value};
use recon_store::read::{BreakFilter, RunFilter};
use crate::auth::AuthContext;
use crate::dto::*;
use crate::error::ApiError;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/tenants", get(list_tenants))
        .route("/api/users", get(list_users))
        .route("/api/dashboard", get(dashboard))
        .route("/api/runs", get(list_runs))
        .route("/api/runs/:run_id", get(get_run))
        .route("/api/breaks", get(list_breaks))
        .route("/api/breaks/:break_id/assign", post(assign_break))
        .route("/api/cases/:case_id", get(get_case))
        .route("/api/cases/:case_id/events", post(append_event))
        .with_state(state)
}

async fn list_tenants(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.store.list_tenants().await?)))
}
async fn list_users(State(s): State<AppState>, ctx: AuthContext) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.store.list_users(&ctx.tenant_id).await?)))
}
async fn dashboard(State(s): State<AppState>, ctx: AuthContext) -> Result<Json<Value>, ApiError> {
    let d = s.store.get_dashboard(&ctx.tenant_id).await?;
    Ok(Json(json!({
        "matchRatePct": d.match_rate_pct,
        "openBreaks": d.open_breaks,
        "valueAtRiskMinor": d.value_at_risk_minor,
        "currency": d.currency,
        "slaAdherencePct": d.sla_adherence_pct,
        "breaksByType": d.breaks_by_type.iter().map(|(t,c)| json!({"type": t, "count": c})).collect::<Vec<_>>(),
        "breaksByAgeing": d.breaks_by_ageing.iter().map(|(b,c)| json!({"bucket": b, "count": c})).collect::<Vec<_>>(),
        "recentRuns": d.recent_runs,
    })))
}
async fn list_runs(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<RunQ>) -> Result<Json<Value>, ApiError> {
    let f = RunFilter { status: q.status, source_id: q.source_id, from: q.from, to: q.to };
    Ok(Json(json!(s.store.list_runs(&ctx.tenant_id, &f).await?)))
}
async fn get_run(State(s): State<AppState>, ctx: AuthContext, Path(run_id): Path<String>) -> Result<Json<Value>, ApiError> {
    let d = s.store.get_run(&ctx.tenant_id, &run_id).await?;
    let txn_map: serde_json::Map<String, Value> = d.transactions.iter().map(|t| (t.id.clone(), json!(t))).collect();
    Ok(Json(json!({
        "run": d.run, "transactionsById": txn_map,
        "matched": d.matched, "partial": d.partial, "duplicates": d.duplicates, "unmatched": d.unmatched,
    })))
}
async fn list_breaks(State(s): State<AppState>, ctx: AuthContext, Query(q): Query<BreakQ>) -> Result<Json<Value>, ApiError> {
    let f = BreakFilter { status: q.status, kind: q.kind, ageing_bucket: q.ageing_bucket, assignee_id: q.assignee_id };
    Ok(Json(json!(s.store.list_breaks(&ctx.tenant_id, &f).await?)))
}
async fn get_case(State(s): State<AppState>, ctx: AuthContext, Path(case_id): Path<String>) -> Result<Json<Value>, ApiError> {
    let b = s.store.get_case(&ctx.tenant_id, &case_id).await?;
    let txn_map: serde_json::Map<String, Value> = b.transactions.iter().map(|t| (t.id.clone(), json!(t))).collect();
    let suggestions: Vec<Value> = b.suggestions.iter().enumerate().map(|(i,(ids,score,rat))| json!({
        "id": format!("sug-{}-{}", case_id, i), "txnIds": ids, "score": score, "rationale": rat
    })).collect();
    Ok(Json(json!({ "case": b.case, "brk": b.brk, "suggestions": suggestions, "transactionsById": txn_map })))
}
async fn assign_break(State(s): State<AppState>, ctx: AuthContext, Path(break_id): Path<String>, Json(body): Json<AssignBody>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.store.assign_break(&ctx.tenant_id, &break_id, &body.user_id).await?)))
}
async fn append_event(State(s): State<AppState>, ctx: AuthContext, Path(case_id): Path<String>, Json(ev): Json<recon_domain::NewCaseEvent>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.store.append_case_event(&ctx.tenant_id, &case_id, ev).await?)))
}
```
(Task 15 implements all handlers; Task 16 only adds tests for the write paths to keep commits focused. If you prefer, split the file — but it is small enough to land whole here.)

`src/main.rs`:
```rust
mod state; mod error; mod auth; mod routes; mod dto;
fn main() { println!("recon-api"); }
```

- [ ] **Step 2: Write read integration tests**

`tests/api.rs`:
```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use recon_api::routes::router; // see note below
use recon_api::state::AppState;
use recon_store::Store;
use tower::ServiceExt;

async fn app(pool: sqlx::PgPool) -> axum::Router {
    let store = Store::from_pool(pool);
    store.seed().await.unwrap();
    router(AppState { store })
}

async fn get_json(app: &axum::Router, uri: &str, tenant: Option<&str>) -> (StatusCode, serde_json::Value) {
    let mut b = Request::builder().uri(uri);
    if let Some(t) = tenant { b = b.header("x-tenant-id", t); }
    let res = app.clone().oneshot(b.body(Body::empty()).unwrap()).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
    (status, v)
}

#[sqlx::test]
async fn dashboard_shape(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (st, v) = get_json(&app, "/api/dashboard", Some("tenant-acme")).await;
    assert_eq!(st, StatusCode::OK);
    assert!(v["matchRatePct"].is_number());
    assert!(v["breaksByType"].is_array());
    assert!(v["openBreaks"].is_number());
}

#[sqlx::test]
async fn dashboard_requires_tenant_header(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (st, _) = get_json(&app, "/api/dashboard", None).await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}

#[sqlx::test]
async fn case_pending_shape(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (st, v) = get_json(&app, "/api/cases/case-pending", Some("tenant-acme")).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["case"]["status"], "pending_approval");
    assert_eq!(v["brk"]["caseId"], "case-pending");
    assert!(v["case"]["events"].as_array().unwrap().iter().any(|e| e["kind"] == "approval_requested"));
}

#[sqlx::test]
async fn cross_tenant_case_is_not_found(pool: sqlx::PgPool) {
    let app = app(pool).await;
    let (st, _) = get_json(&app, "/api/cases/case-pending", Some("tenant-globex")).await;
    assert_eq!(st, StatusCode::NOT_FOUND);
}
```
**Note:** to import `router`/`AppState`/`routes`/`state` from an integration test, expose them via a library target. Add to `recon-api/Cargo.toml`:
```toml
[lib]
name = "recon_api"
path = "src/lib.rs"
```
and create `src/lib.rs`:
```rust
pub mod state; pub mod error; pub mod auth; pub mod routes; pub mod dto;
```
Then `src/main.rs` uses `use recon_api::...` instead of `mod` declarations (updated in Task 17).

- [ ] **Step 3: Run + commit**

Run: `DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api`
Expected: PASS.
```bash
git add backend/crates/recon-api
git commit -m "feat(api): read routes + integration tests"
```

---

### Task 16: Write-route integration tests (four-eyes over HTTP)

**Files:**
- Test: append to `backend/crates/recon-api/tests/api.rs`

- [ ] **Step 1: Add a POST helper + four-eyes tests**

```rust
async fn post_json(app: &axum::Router, uri: &str, tenant: &str, body: serde_json::Value) -> (StatusCode, serde_json::Value) {
    let res = app.clone().oneshot(
        Request::builder().method("POST").uri(uri)
            .header("x-tenant-id", tenant).header("content-type", "application/json")
            .body(Body::from(body.to_string())).unwrap()
    ).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v = if bytes.is_empty() { serde_json::Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
    (status, v)
}

#[sqlx::test]
async fn maker_approve_forbidden_then_approver_resolves(pool: sqlx::PgPool) {
    let app = app(pool).await;
    // Mia (maker) is forbidden
    let (st, _) = post_json(&app, "/api/cases/case-pending/events", "tenant-acme",
        serde_json::json!({ "actorId": "user-mia", "kind": "approved", "payload": {} })).await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // Theo (approver) succeeds -> resolved
    let (st, v) = post_json(&app, "/api/cases/case-pending/events", "tenant-acme",
        serde_json::json!({ "actorId": "user-theo", "kind": "approved", "payload": {} })).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["status"], "resolved");
}

#[sqlx::test]
async fn assign_break_sets_assignee(pool: sqlx::PgPool) {
    let app = app(pool).await;
    // find an open break id via the breaks list
    let (_, breaks) = get_json(&app, "/api/breaks?status=open", Some("tenant-acme")).await;
    let break_id = breaks.as_array().unwrap()[0]["id"].as_str().unwrap().to_string();
    let (st, v) = post_json(&app, &format!("/api/breaks/{break_id}/assign"), "tenant-acme",
        serde_json::json!({ "userId": "user-sam" })).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(v["assigneeId"], "user-sam");
    assert_eq!(v["status"], "investigating");
}
```

- [ ] **Step 2: Run + lint + commit**

Run: `DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api`
Expected: PASS.
```bash
cargo clippy -p recon-api --all-targets -- -D warnings
git add backend/crates/recon-api
git commit -m "test(api): four-eyes + assignment over HTTP"
```

---

### Task 17: Binary entrypoint — serve | seed, CORS, tracing

**Files:**
- Replace: `backend/crates/recon-api/src/main.rs`

- [ ] **Step 1: Implement main**

`src/main.rs`:
```rust
use recon_api::routes::router;
use recon_api::state::AppState;
use recon_store::Store;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "recon_api=debug,info".into()))
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let store = Store::connect(&database_url).await?;

    match std::env::args().nth(1).as_deref() {
        Some("seed") => {
            store.seed().await?;
            tracing::info!("seed complete");
            return Ok(());
        }
        Some("serve") | None => {}
        Some(other) => { eprintln!("unknown command: {other}; use serve|seed"); std::process::exit(2); }
    }

    store.migrate().await?;

    let web_origin = std::env::var("WEB_ORIGIN").unwrap_or_else(|_| "http://localhost:3000".into());
    let cors = CorsLayer::new()
        .allow_origin(web_origin.parse::<axum::http::HeaderValue>().unwrap())
        .allow_methods(Any)
        .allow_headers(Any);

    let app = router(AppState { store })
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let bind = std::env::var("API_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "recon-api listening");
    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 2: Build + manual smoke**

Run:
```bash
cargo build -p recon-api
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo run -p recon-api -- seed
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo run -p recon-api &
sleep 2
curl -s localhost:8080/healthz; echo
curl -s -H "x-tenant-id: tenant-acme" localhost:8080/api/dashboard | head -c 200; echo
curl -s -H "x-tenant-id: tenant-acme" localhost:8080/api/cases/case-pending | head -c 200; echo
kill %1
```
Expected: `ok`; dashboard JSON with `matchRatePct`; case JSON with `"status":"pending_approval"`.

- [ ] **Step 3: Commit**
```bash
git add backend/crates/recon-api
git commit -m "feat(api): serve|seed binary with CORS + JSON tracing"
```

---

# Phase 5 — Frontend wiring + end-to-end

### Task 18: HttpApiClient

**Files:**
- Create: `web/lib/api/http.ts`
- Test: `web/lib/api/http.test.ts`

- [ ] **Step 1: Write the failing test** (uses a stubbed `fetch`)

`web/lib/api/http.test.ts`:
```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { HttpApiClient } from "./http";

const okJson = (body: unknown) =>
  Promise.resolve({ ok: true, status: 200, json: () => Promise.resolve(body) } as Response);

describe("HttpApiClient", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("sends X-Tenant-Id and parses dashboard", async () => {
    const fetchMock = vi.fn(() => okJson({ matchRatePct: 91.2, openBreaks: 3 }));
    vi.stubGlobal("fetch", fetchMock);
    const c = new HttpApiClient("http://api.test");
    const d = await c.getDashboard("tenant-acme");
    expect(d.openBreaks).toBe(3);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("http://api.test/api/dashboard");
    expect((init as RequestInit).headers).toMatchObject({ "X-Tenant-Id": "tenant-acme" });
  });

  it("encodes break query params", async () => {
    const fetchMock = vi.fn(() => okJson([]));
    vi.stubGlobal("fetch", fetchMock);
    const c = new HttpApiClient("http://api.test");
    await c.listBreaks("tenant-acme", { status: "open", type: "duplicate" });
    expect(fetchMock.mock.calls[0][0]).toBe("http://api.test/api/breaks?status=open&type=duplicate");
  });

  it("throws on non-2xx", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve({ ok: false, status: 403,
      json: () => Promise.resolve({ error: { code: "forbidden", message: "no" } }) } as Response)));
    const c = new HttpApiClient("http://api.test");
    await expect(c.appendCaseEvent("tenant-acme", "case-pending",
      { actorId: "user-mia", kind: "approved", payload: {} } as never)).rejects.toThrow(/forbidden/);
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `pnpm -C web test http`
Expected: FAIL (module not found).

- [ ] **Step 3: Implement**

`web/lib/api/http.ts`:
```ts
import type {
  ApiClient, BreakQuery, DashboardSummary, MatchSuggestion, NewCaseEvent, RunDetail, RunQuery,
} from "./client";
import type {
  Break, Case, CanonicalTransaction, ReconciliationRun, Tenant, User,
} from "@/lib/domain/types";

export class HttpApiClient implements ApiClient {
  constructor(private readonly baseUrl: string) {}

  private async req<T>(path: string, tenantId: string | null, init?: RequestInit): Promise<T> {
    const headers: Record<string, string> = { ...(init?.headers as Record<string, string>) };
    if (tenantId) headers["X-Tenant-Id"] = tenantId;
    if (init?.body) headers["Content-Type"] = "application/json";
    const res = await fetch(`${this.baseUrl}${path}`, { ...init, headers });
    if (!res.ok) {
      let detail = `${res.status}`;
      try { const b = await res.json(); detail = b?.error?.message ?? b?.error?.code ?? detail; } catch { /* ignore */ }
      throw new Error(`API ${res.status}: ${detail}`);
    }
    return res.json() as Promise<T>;
  }

  private qs(params: Record<string, string | undefined>): string {
    const sp = new URLSearchParams();
    for (const [k, v] of Object.entries(params)) if (v) sp.set(k, v);
    const s = sp.toString();
    return s ? `?${s}` : "";
  }

  listTenants(): Promise<Tenant[]> { return this.req("/api/tenants", null); }
  listUsers(tenantId: string): Promise<User[]> { return this.req("/api/users", tenantId); }
  getDashboard(tenantId: string): Promise<DashboardSummary> { return this.req("/api/dashboard", tenantId); }
  listRuns(tenantId: string, q?: RunQuery): Promise<ReconciliationRun[]> {
    return this.req(`/api/runs${this.qs({ status: q?.status, sourceId: q?.sourceId, from: q?.from, to: q?.to })}`, tenantId);
  }
  getRun(tenantId: string, runId: string): Promise<RunDetail> { return this.req(`/api/runs/${runId}`, tenantId); }
  listBreaks(tenantId: string, q?: BreakQuery): Promise<Break[]> {
    return this.req(`/api/breaks${this.qs({ status: q?.status, type: q?.type, ageingBucket: q?.ageingBucket, assigneeId: q?.assigneeId })}`, tenantId);
  }
  getCase(tenantId: string, caseId: string): Promise<{ case: Case; brk: Break; suggestions: MatchSuggestion[]; transactionsById: Record<string, CanonicalTransaction>; }> {
    return this.req(`/api/cases/${caseId}`, tenantId);
  }
  assignBreak(tenantId: string, breakId: string, userId: string): Promise<Break> {
    return this.req(`/api/breaks/${breakId}/assign`, tenantId, { method: "POST", body: JSON.stringify({ userId }) });
  }
  appendCaseEvent(tenantId: string, caseId: string, event: NewCaseEvent): Promise<Case> {
    return this.req(`/api/cases/${caseId}/events`, tenantId, { method: "POST", body: JSON.stringify(event) });
  }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `pnpm -C web test http`
Expected: PASS.

- [ ] **Step 5: Commit**
```bash
git add web/lib/api/http.ts web/lib/api/http.test.ts
git commit -m "feat(web): HttpApiClient implementing ApiClient"
```

---

### Task 19: Switch the runtime provider to HTTP

**Files:**
- Modify: `web/lib/api/provider.tsx`
- Create: `web/.env.local` (gitignored), `web/.env.example`

- [ ] **Step 1: Update the provider default**

`web/lib/api/provider.tsx` — replace the default client:
```tsx
"use client";

import { createContext, useContext, type ReactNode } from "react";
import type { ApiClient } from "./client";
import { HttpApiClient } from "./http";

const ApiContext = createContext<ApiClient | null>(null);

const API_BASE_URL = process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080";
const defaultClient: ApiClient = new HttpApiClient(API_BASE_URL);

export function ApiProvider({
  client = defaultClient,
  children,
}: {
  client?: ApiClient;
  children: ReactNode;
}) {
  return <ApiContext.Provider value={client}>{children}</ApiContext.Provider>;
}

export function useApi(): ApiClient {
  const ctx = useContext(ApiContext);
  if (!ctx) throw new Error("useApi must be used inside <ApiProvider>");
  return ctx;
}
```
Note: `MockApiClient` is no longer imported here — it remains only in `web/tests/test-utils.tsx` as the test double, so the 140 vitest tests are unaffected.

`web/.env.example`:
```
NEXT_PUBLIC_API_BASE_URL=http://localhost:8080
```
`web/.env.local` (same content; ensure `.env.local` is gitignored — Next.js default `.gitignore` already excludes it).

- [ ] **Step 2: Verify the unit suite still passes (mock injected by test-utils)**

Run: `pnpm -C web test`
Expected: PASS — all existing tests green (they construct `MockApiClient` in `renderWithProviders`).

- [ ] **Step 3: Typecheck + lint**

Run: `pnpm -C web typecheck && pnpm -C web lint`
Expected: no errors.

- [ ] **Step 4: Commit**
```bash
git add web/lib/api/provider.tsx web/.env.example
git commit -m "feat(web): hard-switch runtime ApiClient to HttpApiClient"
```

---

### Task 20: End-to-end against the live backend + run recipe

**Files:**
- Modify: `web/tests/e2e/operator-loop.spec.ts` (reseed before run), `web/README.md`
- Create: `web/playwright.config.ts` web server note (if needed)

- [ ] **Step 1: Add a reseed helper to the E2E spec**

At the top of `web/tests/e2e/operator-loop.spec.ts`, add a `beforeEach` that reseeds the backend so each test starts from a known state (the four-eyes flow mutates `case-pending`):
```ts
const API = process.env.NEXT_PUBLIC_API_BASE_URL ?? "http://localhost:8080";

test.beforeEach(async () => {
  // Reset the backend to seeded state. Requires `recon-api seed` exposed over HTTP
  // OR run the CLI between tests. Simplest: hit a dev-only reseed endpoint.
  const res = await fetch(`${API}/api/dev/reseed`, { method: "POST" });
  if (!res.ok) throw new Error(`reseed failed: ${res.status}`);
});
```
Add the dev reseed route to the backend (guarded by an env flag) in `web`-independent code:
- In `backend/crates/recon-api/src/routes.rs`, add inside `router`:
  ```rust
  .route("/api/dev/reseed", post(dev_reseed))
  ```
  and:
  ```rust
  async fn dev_reseed(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
      s.store.seed().await?;
      Ok(Json(json!({ "ok": true })))
  }
  ```
  Guard it: only mount when `std::env::var("RECON_DEV").is_ok()`. Re-run `cargo test -p recon-api` after adding (still green), then commit the backend change with `git commit -m "feat(api): dev-only reseed endpoint"`.

- [ ] **Step 2: Document the full-stack run recipe**

Append to `web/README.md`:
```markdown
## Running full-stack (frontend + Rust backend)

1. Start Postgres and the API:
   ```bash
   cd backend
   docker compose up -d postgres
   DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo run -p recon-api -- seed
   RECON_DEV=1 DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo run -p recon-api
   ```
2. Start the frontend against it:
   ```bash
   cd web
   echo 'NEXT_PUBLIC_API_BASE_URL=http://localhost:8080' > .env.local
   pnpm dev
   ```
3. Open http://localhost:3000.
```

- [ ] **Step 3: Run the E2E against the live stack**

Run (with Postgres + `RECON_DEV=1 ... cargo run -p recon-api` running, and the web dev server up):
```bash
pnpm -C web e2e
```
Expected: all operator-loop tests PASS (root→dashboard, exceptions, case-pending four-eyes maker-blocked → switch to Theo → approve → Resolved).

- [ ] **Step 4: Commit**
```bash
git add web/tests/e2e/operator-loop.spec.ts web/README.md
git commit -m "test(e2e): operator loop against the live Rust backend"
```

---

### Task 21: Whole-stack verification

**Files:** none (verification only)

- [ ] **Step 1: Backend gates**

Run:
```bash
cd backend
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --workspace
```
Expected: all green (domain unit, matching unit+property, store integration incl. isolation/four-eyes/seed, api integration).

- [ ] **Step 2: Frontend gates**

Run:
```bash
pnpm -C web test
pnpm -C web typecheck
pnpm -C web lint
pnpm -C web build
```
Expected: all green; production build succeeds.

- [ ] **Step 3: Manual full-stack drive**

Bring up Postgres + API (`RECON_DEV=1 ... cargo run -p recon-api`) + `pnpm -C web dev`, open http://localhost:3000, and confirm: dashboard renders engine-computed KPIs; exceptions list populates; `/cases/case-pending` shows the four-eyes panel with Approve **disabled** as Mia; switching to Theo enables Approve; approving moves the pill to **Resolved**.

- [ ] **Step 4: Final commit (if any verification fixups were needed)**
```bash
git add -A
git commit -m "chore: whole-stack verification fixups" || echo "nothing to commit"
```

---

## Self-review notes (for the controller)

- **Spec coverage:** workspace/crates (Task 1, 2–4, 5–7, 8–13, 14–17); multi-tenancy + isolation tests (Task 10); immutable/append-only tables + tests (Tasks 8, 12); all nine endpoints (Tasks 15–16); matching engine + property tests (Tasks 5–7); server-side four-eyes (Tasks 4, 12, 16); error model (Task 14); observability tracing + CORS (Task 17); seed incl. case-pending (Task 13); frontend HttpApiClient + provider swap + E2E (Tasks 18–20); DoD verification (Task 21).
- **Type consistency:** `MatchType`/`BreakType` use `match_type`/`break_type` Rust fields with `#[serde(rename="type")]`; `RunStats` field names are camelCase on the wire and used identically in engine (Task 6) and dashboard (Task 11); `NewCaseEvent` shape (Task 3) matches the POST body parsed in Task 15 and the frontend `http.ts` payloads (Task 18).
- **Known deviation from spec:** `X-User-Id` is not used — the actor is carried in write payloads (`event.actorId`; `assignBreak` uses the assignee as the assignment actor, matching `mock.ts`). The `AuthContext` establishes tenant only; this keeps the frontend change to a one-line provider swap while preserving server-side four-eyes. Documented here intentionally.
