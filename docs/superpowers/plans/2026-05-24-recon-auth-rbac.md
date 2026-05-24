# Auth & RBAC Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the trusted-header auth seam with real email+password login (self-issued JWTs, access+refresh hybrid), multi-tenant membership with per-tenant RBAC, admin user management, self-service password change, password reset by email, and brute-force protection.

**Architecture:** A new IO-light `recon-auth` crate holds the security primitives (argon2id hashing, JWT, refresh-token model, RBAC matrix, lockout policy); a small `recon-mail` crate abstracts email (SMTP via `lettre`, dev Mailpit). `recon-store` gains credential/membership/session tables; `recon-api` swaps its `AuthContext` extractor to validate Bearer access tokens and adds `/auth/*` + admin user routes. The frontend gains an `AuthProvider` (in-memory access token + silent refresh) and login/guard/admin surfaces.

**Tech Stack:** Rust (argon2, jsonwebtoken, lettre, sha2, rand), sqlx/PostgreSQL (citext), Axum 0.7, Next.js 16 / React 19, Playwright, Mailpit.

**Spec:** `docs/superpowers/specs/2026-05-24-recon-auth-rbac-design.md`

---

## Conventions for implementers

- Run backend commands from the repo root with absolute manifest path: `cargo test --manifest-path backend/Cargo.toml -p <crate>`.
- A live Postgres is required for `#[sqlx::test]` and integration tests: `docker compose -f backend/docker-compose.yml up -d --wait postgres`. `DATABASE_URL=postgres://recon:recon@localhost:5432/recon`.
- Cargo is on PATH (`~/.cargo/bin`, sourced from the shell snapshot). If not: `. "$HOME/.cargo/env"`.
- Use **runtime-checked** sqlx queries only (`query`, `query_as`, `query_scalar`) — never the `query!` compile-time macros (build must not need a live DB).
- Frontend commands: `pnpm -C web <script>`. This is **Next.js 16** — read `web/AGENTS.md`; APIs differ from older training data.
- Wire shapes are camelCase (`#[serde(rename_all = "camelCase")]`); enums snake_case as in `recon-domain`.
- Commit after each task with the shown message.

---

## Shared type contract (defined once, referenced throughout)

These signatures are authoritative. Later tasks must match them exactly.

```rust
// recon-domain/src/types.rs  (UserRole already exists: Operator|Approver|Admin, snake_case)
pub struct User { pub id: String, pub name: String, pub email: String, pub disabled: bool, pub role: UserRole }
//   `role` = the user's role in the ACTIVE tenant context whenever materialized inside a tenant scope.
pub struct Membership { pub tenant_id: String, pub tenant_name: String, pub role: UserRole } // wire: camelCase

// recon-auth
pub enum AuthError { InvalidCredentials, TokenExpired, TokenInvalid, Forbidden, Locked, Hash(String) }
pub mod password { pub fn hash_password(plain:&str)->Result<String,AuthError>; pub fn verify_password(plain:&str,hash:&str)->Result<bool,AuthError>; }
pub struct AccessClaims { pub sub:String, pub tid:String, pub role:UserRole, pub jti:String, pub iat:i64, pub exp:i64, pub typ:String }
pub mod token {
    pub fn encode_access(secret:&[u8], user_id:&str, tenant_id:&str, role:UserRole, ttl_secs:i64, now_unix:i64)->Result<String,AuthError>;
    pub fn decode_access(secret:&[u8], token:&str, now_unix:i64)->Result<AccessClaims,AuthError>;
}
pub mod refresh { pub fn generate()->(String,String); /* (plaintext, sha256_hex) */ pub fn hash(token:&str)->String; }
pub enum Permission { ViewRecon, AssignBreak, ProposeResolution, ApproveResolution, ManageUsers }
pub mod rbac { pub fn permitted(role:UserRole, perm:Permission)->bool; pub fn require(role:UserRole, perm:Permission)->Result<(),AuthError>; }
pub struct LockoutDecision { pub locked_until_unix: Option<i64>, pub reset_attempts: bool }
pub mod lockout { pub fn on_failure(attempts_after:i32, now_unix:i64)->LockoutDecision; pub const MAX_ATTEMPTS:i32 = 5; }

// recon-api
pub struct AuthContext { pub user_id:String, pub tenant_id:String, pub role:UserRole }
```

RBAC matrix (authoritative):

| Permission | Operator | Approver | Admin |
|---|---|---|---|
| ViewRecon | ✓ | ✓ | ✓ |
| AssignBreak | ✓ | ✓ | ✓ |
| ProposeResolution | ✓ | ✓ | ✓ |
| ApproveResolution | ✗ | ✓ | ✓ |
| ManageUsers | ✗ | ✗ | ✓ |

(Four-eyes — "not your own proposal" — remains enforced by `recon_domain::can_approve`, layered on top of `ApproveResolution`.)

---

## Task 1: Scaffold `recon-auth` crate + password hashing

**Files:**
- Create: `backend/crates/recon-auth/Cargo.toml`
- Create: `backend/crates/recon-auth/src/lib.rs`
- Create: `backend/crates/recon-auth/src/error.rs`
- Create: `backend/crates/recon-auth/src/password.rs`
- Modify: `backend/Cargo.toml` (workspace members + deps)

- [ ] **Step 1: Add crate to workspace + deps**

In `backend/Cargo.toml`, add `"crates/recon-auth"` to `members`, and under `[workspace.dependencies]` add:
```toml
argon2 = "0.5"
jsonwebtoken = "9"
sha2 = "0.10"
rand = "0.8"
hex = "0.4"
```

`backend/crates/recon-auth/Cargo.toml`:
```toml
[package]
name = "recon-auth"
version = "0.1.0"
edition = "2021"

[dependencies]
recon-domain = { path = "../recon-domain" }
serde = { workspace = true }
argon2 = { workspace = true }
jsonwebtoken = { workspace = true }
sha2 = { workspace = true }
rand = { workspace = true }
hex = { workspace = true }
time = { workspace = true }

[dev-dependencies]
```

`backend/crates/recon-auth/src/lib.rs`:
```rust
pub mod error;
pub mod password;
pub use error::AuthError;
```

`backend/crates/recon-auth/src/error.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    InvalidCredentials,
    TokenExpired,
    TokenInvalid,
    Forbidden,
    Locked,
    Hash(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::InvalidCredentials => write!(f, "invalid credentials"),
            AuthError::TokenExpired => write!(f, "token expired"),
            AuthError::TokenInvalid => write!(f, "token invalid"),
            AuthError::Forbidden => write!(f, "forbidden"),
            AuthError::Locked => write!(f, "account locked"),
            AuthError::Hash(m) => write!(f, "hash error: {m}"),
        }
    }
}
impl std::error::Error for AuthError {}
```

- [ ] **Step 2: Write the failing password test**

`backend/crates/recon-auth/src/password.rs` (test module first):
```rust
use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use crate::error::AuthError;

pub fn hash_password(plain: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Hash(e.to_string()))
}

pub fn verify_password(plain: &str, hash: &str) -> Result<bool, AuthError> {
    let parsed = PasswordHash::new(hash).map_err(|e| AuthError::Hash(e.to_string()))?;
    Ok(Argon2::default().verify_password(plain.as_bytes(), &parsed).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hash_then_verify_roundtrip() {
        let h = hash_password("s3cret-pw").unwrap();
        assert!(h.starts_with("$argon2"));
        assert!(verify_password("s3cret-pw", &h).unwrap());
    }
    #[test]
    fn wrong_password_rejected() {
        let h = hash_password("s3cret-pw").unwrap();
        assert!(!verify_password("nope", &h).unwrap());
    }
    #[test]
    fn malformed_hash_errors() {
        assert!(matches!(verify_password("x", "not-a-hash"), Err(AuthError::Hash(_))));
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --manifest-path backend/Cargo.toml -p recon-auth`
Expected: 3 passed.

- [ ] **Step 4: Commit**
```bash
git add backend/Cargo.toml backend/crates/recon-auth
git commit -m "feat(auth): scaffold recon-auth crate with argon2id password hashing"
```

---

## Task 2: Access-token JWT (`token` module)

**Files:**
- Create: `backend/crates/recon-auth/src/token.rs`
- Modify: `backend/crates/recon-auth/src/lib.rs` (add `pub mod token;`)

- [ ] **Step 1: Write the failing test + implementation**

`backend/crates/recon-auth/src/token.rs`:
```rust
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation, Algorithm, errors::ErrorKind};
use serde::{Deserialize, Serialize};
use recon_domain::UserRole;
use crate::error::AuthError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccessClaims {
    pub sub: String,
    pub tid: String,
    pub role: UserRole,
    pub jti: String,
    pub iat: i64,
    pub exp: i64,
    pub typ: String,
}

pub fn encode_access(secret: &[u8], user_id: &str, tenant_id: &str, role: UserRole, ttl_secs: i64, now_unix: i64) -> Result<String, AuthError> {
    let claims = AccessClaims {
        sub: user_id.to_string(),
        tid: tenant_id.to_string(),
        role,
        jti: uuidish(),
        iat: now_unix,
        exp: now_unix + ttl_secs,
        typ: "access".to_string(),
    };
    encode(&Header::new(Algorithm::HS256), &claims, &EncodingKey::from_secret(secret))
        .map_err(|_| AuthError::TokenInvalid)
}

pub fn decode_access(secret: &[u8], token: &str, now_unix: i64) -> Result<AccessClaims, AuthError> {
    let mut v = Validation::new(Algorithm::HS256);
    v.validate_exp = false; // we validate exp manually against now_unix for deterministic tests
    let data = decode::<AccessClaims>(token, &DecodingKey::from_secret(secret), &v)
        .map_err(|e| match e.kind() { ErrorKind::ExpiredSignature => AuthError::TokenExpired, _ => AuthError::TokenInvalid })?;
    let c = data.claims;
    if c.typ != "access" { return Err(AuthError::TokenInvalid); }
    if now_unix >= c.exp { return Err(AuthError::TokenExpired); }
    Ok(c)
}

fn uuidish() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    const S: &[u8] = b"test-secret-please-rotate";
    #[test]
    fn roundtrip_valid() {
        let t = encode_access(S, "user-mia", "tenant-acme", UserRole::Operator, 900, 1000).unwrap();
        let c = decode_access(S, &t, 1100).unwrap();
        assert_eq!(c.sub, "user-mia");
        assert_eq!(c.tid, "tenant-acme");
        assert_eq!(c.role, UserRole::Operator);
        assert_eq!(c.typ, "access");
    }
    #[test]
    fn expired_rejected() {
        let t = encode_access(S, "u", "t", UserRole::Admin, 900, 1000).unwrap();
        assert_eq!(decode_access(S, &t, 5000), Err(AuthError::TokenExpired));
    }
    #[test]
    fn tampered_secret_rejected() {
        let t = encode_access(S, "u", "t", UserRole::Admin, 900, 1000).unwrap();
        assert_eq!(decode_access(b"other-secret", &t, 1100), Err(AuthError::TokenInvalid));
    }
}
```
Add `pub mod token;` to `lib.rs`.

- [ ] **Step 2: Run tests**

Run: `cargo test --manifest-path backend/Cargo.toml -p recon-auth token`
Expected: 3 passed.

- [ ] **Step 3: Commit**
```bash
git add backend/crates/recon-auth/src/token.rs backend/crates/recon-auth/src/lib.rs
git commit -m "feat(auth): HS256 access-token encode/decode with manual exp check"
```

---

## Task 3: Refresh-token generation + hashing (`refresh` module)

**Files:**
- Create: `backend/crates/recon-auth/src/refresh.rs`
- Modify: `backend/crates/recon-auth/src/lib.rs`

- [ ] **Step 1: Write test + implementation**

`backend/crates/recon-auth/src/refresh.rs`:
```rust
use sha2::{Digest, Sha256};

/// Returns (plaintext_token, sha256_hex_hash). Only the hash is persisted.
pub fn generate() -> (String, String) {
    use rand::RngCore;
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    let plaintext = hex::encode(b);
    let h = hash(&plaintext);
    (plaintext, h)
}

pub fn hash(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generate_is_unique_and_hash_matches() {
        let (p1, h1) = generate();
        let (p2, _h2) = generate();
        assert_ne!(p1, p2);
        assert_eq!(p1.len(), 64);
        assert_eq!(h1, hash(&p1));
        assert_ne!(h1, p1); // hash != plaintext
    }
    #[test]
    fn hash_is_stable() {
        assert_eq!(hash("abc"), hash("abc"));
        assert_ne!(hash("abc"), hash("abd"));
    }
}
```
Add `pub mod refresh;` to `lib.rs`.

- [ ] **Step 2: Run tests**

Run: `cargo test --manifest-path backend/Cargo.toml -p recon-auth refresh`
Expected: 2 passed.

- [ ] **Step 3: Commit**
```bash
git add backend/crates/recon-auth/src/refresh.rs backend/crates/recon-auth/src/lib.rs
git commit -m "feat(auth): opaque refresh-token generation with sha-256 hashing"
```

---

## Task 4: RBAC matrix (`rbac` module)

**Files:**
- Create: `backend/crates/recon-auth/src/rbac.rs`
- Modify: `backend/crates/recon-auth/src/lib.rs`

- [ ] **Step 1: Write test + implementation**

`backend/crates/recon-auth/src/rbac.rs`:
```rust
use recon_domain::UserRole;
use crate::error::AuthError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission { ViewRecon, AssignBreak, ProposeResolution, ApproveResolution, ManageUsers }

pub fn permitted(role: UserRole, perm: Permission) -> bool {
    use Permission::*;
    use UserRole::*;
    match perm {
        ViewRecon | AssignBreak | ProposeResolution => true, // all roles
        ApproveResolution => matches!(role, Approver | Admin),
        ManageUsers => matches!(role, Admin),
    }
}

pub fn require(role: UserRole, perm: Permission) -> Result<(), AuthError> {
    if permitted(role, perm) { Ok(()) } else { Err(AuthError::Forbidden) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use recon_domain::UserRole::*;
    #[test]
    fn approve_requires_approver_or_admin() {
        assert!(!permitted(Operator, Permission::ApproveResolution));
        assert!(permitted(Approver, Permission::ApproveResolution));
        assert!(permitted(Admin, Permission::ApproveResolution));
    }
    #[test]
    fn manage_users_admin_only() {
        assert!(!permitted(Operator, Permission::ManageUsers));
        assert!(!permitted(Approver, Permission::ManageUsers));
        assert!(permitted(Admin, Permission::ManageUsers));
    }
    #[test]
    fn view_and_assign_open_to_all() {
        for r in [Operator, Approver, Admin] {
            assert!(permitted(r, Permission::ViewRecon));
            assert!(permitted(r, Permission::AssignBreak));
            assert!(permitted(r, Permission::ProposeResolution));
        }
    }
    #[test]
    fn require_maps_to_forbidden() {
        assert_eq!(require(Operator, Permission::ManageUsers), Err(AuthError::Forbidden));
        assert_eq!(require(Admin, Permission::ManageUsers), Ok(()));
    }
}
```
Add `pub mod rbac;` to `lib.rs`.

- [ ] **Step 2: Run tests**

Run: `cargo test --manifest-path backend/Cargo.toml -p recon-auth rbac`
Expected: 4 passed.

- [ ] **Step 3: Commit**
```bash
git add backend/crates/recon-auth/src/rbac.rs backend/crates/recon-auth/src/lib.rs
git commit -m "feat(auth): RBAC permission matrix and require() guard"
```

---

## Task 5: Lockout policy (`lockout` module)

**Files:**
- Create: `backend/crates/recon-auth/src/lockout.rs`
- Modify: `backend/crates/recon-auth/src/lib.rs`

- [ ] **Step 1: Write test + implementation**

`backend/crates/recon-auth/src/lockout.rs`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockoutDecision {
    pub locked_until_unix: Option<i64>,
    pub reset_attempts: bool,
}

pub const MAX_ATTEMPTS: i32 = 5;
const BASE_LOCK_SECS: i64 = 60;

/// Given the failed-attempt count AFTER incrementing, decide lock state.
/// Locks once attempts reach MAX_ATTEMPTS, with exponential backoff per extra failure.
pub fn on_failure(attempts_after: i32, now_unix: i64) -> LockoutDecision {
    if attempts_after < MAX_ATTEMPTS {
        return LockoutDecision { locked_until_unix: None, reset_attempts: false };
    }
    let over = (attempts_after - MAX_ATTEMPTS) as u32; // 0,1,2...
    let secs = BASE_LOCK_SECS.saturating_mul(2_i64.saturating_pow(over.min(10)));
    LockoutDecision { locked_until_unix: Some(now_unix + secs), reset_attempts: false }
}

/// True if the account is currently locked.
pub fn is_locked(locked_until_unix: Option<i64>, now_unix: i64) -> bool {
    matches!(locked_until_unix, Some(t) if now_unix < t)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_lock_below_threshold() {
        for a in 1..MAX_ATTEMPTS { assert_eq!(on_failure(a, 1000).locked_until_unix, None); }
    }
    #[test]
    fn locks_at_threshold_for_base() {
        assert_eq!(on_failure(MAX_ATTEMPTS, 1000).locked_until_unix, Some(1000 + 60));
    }
    #[test]
    fn backoff_doubles() {
        assert_eq!(on_failure(MAX_ATTEMPTS + 1, 1000).locked_until_unix, Some(1000 + 120));
        assert_eq!(on_failure(MAX_ATTEMPTS + 2, 1000).locked_until_unix, Some(1000 + 240));
    }
    #[test]
    fn is_locked_window() {
        assert!(is_locked(Some(2000), 1999));
        assert!(!is_locked(Some(2000), 2000));
        assert!(!is_locked(None, 2000));
    }
}
```
Add `pub mod lockout;` to `lib.rs`.

- [ ] **Step 2: Run tests**

Run: `cargo test --manifest-path backend/Cargo.toml -p recon-auth lockout`
Expected: 4 passed. Then full crate: `cargo test --manifest-path backend/Cargo.toml -p recon-auth` → all green; `cargo clippy --manifest-path backend/Cargo.toml -p recon-auth -- -D warnings`.

- [ ] **Step 3: Commit**
```bash
git add backend/crates/recon-auth/src/lockout.rs backend/crates/recon-auth/src/lib.rs
git commit -m "feat(auth): account lockout policy with exponential backoff"
```

---

## Task 6: `recon-mail` crate + Mailpit in docker-compose

**Files:**
- Create: `backend/crates/recon-mail/Cargo.toml`, `src/lib.rs`
- Modify: `backend/Cargo.toml` (members + deps: `lettre = { version = "0.11", default-features = false, features = ["smtp-transport","tokio1-rustls-tls","builder"] }`, `async-trait = "0.1"`)
- Modify: `backend/docker-compose.yml` (add Mailpit service)
- Modify: `backend/.env.example` (SMTP_* vars)

- [ ] **Step 1: Mailer trait + impls**

`backend/crates/recon-mail/src/lib.rs`:
```rust
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct Email { pub to: String, pub subject: String, pub body: String }

#[derive(Debug, thiserror::Error)]
pub enum MailError { #[error("smtp: {0}")] Smtp(String) }

#[async_trait]
pub trait Mailer: Send + Sync {
    async fn send(&self, email: Email) -> Result<(), MailError>;
}

/// Logs the email instead of sending (no SMTP configured).
pub struct LogMailer;
#[async_trait]
impl Mailer for LogMailer {
    async fn send(&self, email: Email) -> Result<(), MailError> {
        tracing::warn!(to = %email.to, subject = %email.subject, "LogMailer (no SMTP): {}", email.body);
        Ok(())
    }
}

pub struct SmtpMailer { host: String, port: u16, from: String }
impl SmtpMailer {
    pub fn new(host: impl Into<String>, port: u16, from: impl Into<String>) -> Self {
        Self { host: host.into(), port, from: from.into() }
    }
}
#[async_trait]
impl Mailer for SmtpMailer {
    async fn send(&self, email: Email) -> Result<(), MailError> {
        use lettre::{Message, AsyncSmtpTransport, Tokio1Executor, AsyncTransport};
        let msg = Message::builder()
            .from(self.from.parse().map_err(|e| MailError::Smtp(format!("{e}")))?)
            .to(email.to.parse().map_err(|e| MailError::Smtp(format!("{e}")))?)
            .subject(email.subject)
            .body(email.body).map_err(|e| MailError::Smtp(format!("{e}")))?;
        // Mailpit speaks plaintext SMTP; use dangerous (no TLS) builder for the dev catcher.
        let mailer = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&self.host).port(self.port).build();
        mailer.send(msg).await.map_err(|e| MailError::Smtp(format!("{e}")))?;
        Ok(())
    }
}

/// Test double that captures sent mail.
#[cfg(any(test, feature = "test-util"))]
pub mod testing {
    use super::*;
    use std::sync::Mutex;
    #[derive(Default)]
    pub struct CapturingMailer { pub sent: Mutex<Vec<Email>> }
    #[async_trait]
    impl Mailer for CapturingMailer {
        async fn send(&self, email: Email) -> Result<(), MailError> { self.sent.lock().unwrap().push(email); Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn capturing_mailer_records() {
        let m = testing::CapturingMailer::default();
        m.send(Email{to:"a@b.com".into(),subject:"s".into(),body:"hello LINK".into()}).await.unwrap();
        let sent = m.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].body.contains("LINK"));
    }
}
```
`recon-mail/Cargo.toml` deps: `async-trait`, `lettre` (workspace), `tracing`, `thiserror`, `tokio` (dev: `tokio` with `macros,rt`). Add `[features] test-util = []`.

- [ ] **Step 2: docker-compose Mailpit + env**

Add to `backend/docker-compose.yml` under `services:`:
```yaml
  mailpit:
    image: axllent/mailpit:latest
    container_name: recon-mailpit
    ports:
      - "1025:1025"  # SMTP
      - "8025:8025"  # web UI + REST API
```
Append to `backend/.env.example`:
```
SMTP_HOST=localhost
SMTP_PORT=1025
SMTP_FROM=recon@example.com
APP_BASE_URL=http://localhost:3100
```

- [ ] **Step 3: Run tests**

Run: `cargo test --manifest-path backend/Cargo.toml -p recon-mail`
Expected: 1 passed. `cargo clippy --manifest-path backend/Cargo.toml -p recon-mail -- -D warnings`.

- [ ] **Step 4: Commit**
```bash
git add backend/Cargo.toml backend/crates/recon-mail backend/docker-compose.yml backend/.env.example
git commit -m "feat(mail): recon-mail crate (SMTP/log/capturing) + Mailpit dev service"
```

---

## Task 7: Domain types for membership

**Files:**
- Modify: `backend/crates/recon-domain/src/types.rs` (User gains `email`,`disabled`; add `Membership`)
- Modify: `backend/crates/recon-domain/src/approval.rs` (no signature change; verify `can_approve` still uses `user.role`)

- [ ] **Step 1: Update `User` + add `Membership`**

In `types.rs`, replace the `User` struct and add `Membership`:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub disabled: bool,
    /// Role in the active-tenant context.
    pub role: UserRole,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Membership {
    pub tenant_id: String,
    pub tenant_name: String,
    pub role: UserRole,
}
```

- [ ] **Step 2: Test serialization shape**

Add to `types.rs` tests (or create `#[cfg(test)] mod`):
```rust
#[test]
fn user_serializes_camel_case_with_email() {
    let u = User{ id:"u1".into(), name:"Mia".into(), email:"mia@acme.test".into(), disabled:false, role:UserRole::Operator };
    let j = serde_json::to_value(&u).unwrap();
    assert_eq!(j["email"], "mia@acme.test");
    assert_eq!(j["disabled"], false);
    assert_eq!(j["role"], "operator");
}
#[test]
fn membership_camel_case() {
    let m = Membership{ tenant_id:"t1".into(), tenant_name:"Acme".into(), role:UserRole::Admin };
    let j = serde_json::to_value(&m).unwrap();
    assert_eq!(j["tenantId"], "t1");
    assert_eq!(j["tenantName"], "Acme");
    assert_eq!(j["role"], "admin");
}
```
(Add `serde_json` to recon-domain `[dev-dependencies]` if absent.)

- [ ] **Step 3: Run + fix downstream compile**

Run: `cargo test --manifest-path backend/Cargo.toml -p recon-domain`
Expected: pass. Then `cargo build --manifest-path backend/Cargo.toml` will fail where `User` is constructed without `email`/`disabled` — those are fixed in later store/api tasks; for now ensure `recon-domain` + `recon-auth` compile.

- [ ] **Step 4: Commit**
```bash
git add backend/crates/recon-domain/src/types.rs backend/crates/recon-domain/Cargo.toml
git commit -m "feat(domain): User gains email/disabled; add Membership type"
```

---

## Task 8: Migration `0002_auth.sql`

**Files:**
- Create: `backend/migrations/0002_auth.sql`

- [ ] **Step 1: Write the migration**

`backend/migrations/0002_auth.sql`:
```sql
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
```

- [ ] **Step 2: Verify it applies**

Run:
```bash
docker compose -f backend/docker-compose.yml up -d --wait postgres
docker exec recon-postgres psql -U recon -d recon -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo run --manifest-path backend/Cargo.toml -p recon-api -- seed 2>&1 | tail -5
```
Expected: migrations apply cleanly (note: seed itself is updated in Task 12; until then it may error on user columns — acceptable, the goal here is that `0001` + `0002` apply. Verify with: `docker exec recon-postgres psql -U recon -d recon -c "\d memberships"`).

- [ ] **Step 3: Commit**
```bash
git add backend/migrations/0002_auth.sql
git commit -m "feat(store): 0002 migration — memberships, credentials, refresh & reset tokens"
```

---

## Task 9: Store — credentials & membership queries

**Files:**
- Create: `backend/crates/recon-store/src/auth.rs` (new module for auth reads/writes)
- Modify: `backend/crates/recon-store/src/lib.rs` (`pub mod auth;`)
- Modify: `backend/crates/recon-store/src/rows.rs` if row structs centralized (else inline)
- Test: `backend/crates/recon-store/tests/auth.rs`

- [ ] **Step 1: Implement credential + membership reads**

`backend/crates/recon-store/src/auth.rs`:
```rust
use crate::{Store, StoreError};
use recon_domain::{User, UserRole, Membership};

#[derive(sqlx::FromRow)]
struct CredRow { user_id: String, password_hash: String, failed_attempts: i32, locked_until: Option<time::OffsetDateTime> }

pub struct Credential { pub user_id: String, pub password_hash: String, pub failed_attempts: i32, pub locked_until: Option<i64> }

impl Store {
    /// Look up a user + credential by email (global identity). None if no such email.
    pub async fn find_credential_by_email(&self, email: &str) -> Result<Option<(User, Credential)>, StoreError> {
        let row = sqlx::query_as::<_, (String,String,bool,String,i32,Option<time::OffsetDateTime>)>(
            "SELECT u.id, u.name, u.disabled, c.password_hash, c.failed_attempts, c.locked_until \
             FROM users u JOIN user_credentials c ON c.user_id = u.id WHERE u.email = $1")
            .bind(email)
            .fetch_optional(&self.pool).await.map_err(StoreError::from)?;
        Ok(row.map(|(id,name,disabled,hash,fa,lu)| {
            let user = User { id: id.clone(), name, email: email.to_string(), disabled, role: UserRole::Operator /*placeholder; set per tenant*/ };
            let cred = Credential { user_id: id, password_hash: hash, failed_attempts: fa, locked_until: lu.map(|t| t.unix_timestamp()) };
            (user, cred)
        }))
    }

    /// All memberships for a user (with tenant names).
    pub async fn memberships_for(&self, user_id: &str) -> Result<Vec<Membership>, StoreError> {
        let rows = sqlx::query_as::<_, (String,String,String)>(
            "SELECT m.tenant_id, t.name, m.role FROM memberships m JOIN tenants t ON t.id = m.tenant_id \
             WHERE m.user_id = $1 ORDER BY t.name")
            .bind(user_id).fetch_all(&self.pool).await.map_err(StoreError::from)?;
        Ok(rows.into_iter().map(|(tid,tn,role)| Membership { tenant_id: tid, tenant_name: tn, role: parse_role(&role) }).collect())
    }

    /// Role for a user in a specific tenant, or None if not a member.
    pub async fn role_in_tenant(&self, user_id: &str, tenant_id: &str) -> Result<Option<UserRole>, StoreError> {
        let r = sqlx::query_scalar::<_, String>("SELECT role FROM memberships WHERE user_id=$1 AND tenant_id=$2")
            .bind(user_id).bind(tenant_id).fetch_optional(&self.pool).await.map_err(StoreError::from)?;
        Ok(r.map(|s| parse_role(&s)))
    }
}

fn parse_role(s: &str) -> UserRole {
    match s { "approver" => UserRole::Approver, "admin" => UserRole::Admin, _ => UserRole::Operator }
}
```
Note: `_ = CredRow` may be unused if using the tuple form — delete `CredRow` if not used to keep clippy clean.

- [ ] **Step 2: Test (`#[sqlx::test]`)**

`backend/crates/recon-store/tests/auth.rs`:
```rust
use recon_store::Store;
use recon_domain::UserRole;

async fn seed_user(pool: &sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t1','Acme','acme')").execute(pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('u1','Mia','mia@acme.test',false)").execute(pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('u1','t1','approver')").execute(pool).await.unwrap();
    sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES ('u1','$argon2id$dummy')").execute(pool).await.unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn find_credential_and_roles(pool: sqlx::PgPool) {
    seed_user(&pool).await;
    let store = Store::from_pool(pool);
    let (user, cred) = store.find_credential_by_email("mia@acme.test").await.unwrap().unwrap();
    assert_eq!(user.id, "u1");
    assert_eq!(cred.password_hash, "$argon2id$dummy");
    assert_eq!(store.role_in_tenant("u1","t1").await.unwrap(), Some(UserRole::Approver));
    assert_eq!(store.role_in_tenant("u1","nope").await.unwrap(), None);
    let ms = store.memberships_for("u1").await.unwrap();
    assert_eq!(ms.len(), 1);
    assert_eq!(ms[0].tenant_name, "Acme");
}
```
If `Store::from_pool` does not exist, add `pub fn from_pool(pool: sqlx::PgPool) -> Self { Self { pool } }` to `recon-store/src/lib.rs`.

- [ ] **Step 3: Run**

Run: `DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --manifest-path backend/Cargo.toml -p recon-store auth`
Expected: pass.

- [ ] **Step 4: Commit**
```bash
git add backend/crates/recon-store/src/auth.rs backend/crates/recon-store/src/lib.rs backend/crates/recon-store/tests/auth.rs
git commit -m "feat(store): credential + membership lookups"
```

---

## Task 10: Store — failed-attempt / lockout mutations

**Files:**
- Modify: `backend/crates/recon-store/src/auth.rs`
- Test: append to `backend/crates/recon-store/tests/auth.rs`

- [ ] **Step 1: Add mutations**

Append to `impl Store` in `auth.rs`:
```rust
pub async fn record_login_failure(&self, user_id: &str, locked_until_unix: Option<i64>) -> Result<(), StoreError> {
    let lu = locked_until_unix.map(|u| time::OffsetDateTime::from_unix_timestamp(u).unwrap());
    sqlx::query("UPDATE user_credentials SET failed_attempts = failed_attempts + 1, locked_until = $2 WHERE user_id = $1")
        .bind(user_id).bind(lu).execute(&self.pool).await.map_err(StoreError::from)?;
    Ok(())
}
pub async fn reset_login_failures(&self, user_id: &str) -> Result<(), StoreError> {
    sqlx::query("UPDATE user_credentials SET failed_attempts = 0, locked_until = NULL WHERE user_id = $1")
        .bind(user_id).execute(&self.pool).await.map_err(StoreError::from)?;
    Ok(())
}
pub async fn current_failed_attempts(&self, user_id: &str) -> Result<i32, StoreError> {
    Ok(sqlx::query_scalar::<_,i32>("SELECT failed_attempts FROM user_credentials WHERE user_id=$1")
        .bind(user_id).fetch_one(&self.pool).await.map_err(StoreError::from)?)
}
pub async fn set_password(&self, user_id: &str, password_hash: &str) -> Result<(), StoreError> {
    sqlx::query("UPDATE user_credentials SET password_hash=$2, password_updated_at=now(), failed_attempts=0, locked_until=NULL WHERE user_id=$1")
        .bind(user_id).bind(password_hash).execute(&self.pool).await.map_err(StoreError::from)?;
    Ok(())
}
```

- [ ] **Step 2: Test**

Append:
```rust
#[sqlx::test(migrations = "../../migrations")]
async fn lockout_counters(pool: sqlx::PgPool) {
    seed_user(&pool).await;
    let store = Store::from_pool(pool);
    store.record_login_failure("u1", None).await.unwrap();
    store.record_login_failure("u1", Some(9999999999)).await.unwrap();
    assert_eq!(store.current_failed_attempts("u1").await.unwrap(), 2);
    let (_, cred) = store.find_credential_by_email("mia@acme.test").await.unwrap().unwrap();
    assert_eq!(cred.locked_until, Some(9999999999));
    store.reset_login_failures("u1").await.unwrap();
    assert_eq!(store.current_failed_attempts("u1").await.unwrap(), 0);
}
```

- [ ] **Step 3: Run + commit**
```bash
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --manifest-path backend/Cargo.toml -p recon-store auth
git add backend/crates/recon-store/src/auth.rs backend/crates/recon-store/tests/auth.rs
git commit -m "feat(store): login-failure counters and password set"
```

---

## Task 11: Store — refresh & reset token lifecycle

**Files:**
- Modify: `backend/crates/recon-store/src/auth.rs`
- Test: append to `backend/crates/recon-store/tests/auth.rs`

- [ ] **Step 1: Implement**

Append to `impl Store`:
```rust
pub async fn insert_refresh(&self, id:&str, user_id:&str, tenant_id:&str, token_hash:&str, expires_at_unix:i64, rotated_from:Option<&str>) -> Result<(), StoreError> {
    let exp = time::OffsetDateTime::from_unix_timestamp(expires_at_unix).unwrap();
    sqlx::query("INSERT INTO refresh_tokens(id,user_id,tenant_id,token_hash,expires_at,rotated_from) VALUES ($1,$2,$3,$4,$5,$6)")
        .bind(id).bind(user_id).bind(tenant_id).bind(token_hash).bind(exp).bind(rotated_from)
        .execute(&self.pool).await.map_err(StoreError::from)?;
    Ok(())
}
/// Returns (id, user_id, tenant_id) if the token is live (not revoked, not expired). None otherwise.
pub async fn find_live_refresh(&self, token_hash:&str, now_unix:i64) -> Result<Option<(String,String,String)>, StoreError> {
    let now = time::OffsetDateTime::from_unix_timestamp(now_unix).unwrap();
    let r = sqlx::query_as::<_,(String,String,String)>(
        "SELECT id,user_id,tenant_id FROM refresh_tokens WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at > $2")
        .bind(token_hash).bind(now).fetch_optional(&self.pool).await.map_err(StoreError::from)?;
    Ok(r)
}
/// True if a row exists for this hash but is already revoked (reuse → theft signal).
pub async fn refresh_is_revoked(&self, token_hash:&str) -> Result<bool, StoreError> {
    Ok(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM refresh_tokens WHERE token_hash=$1 AND revoked_at IS NOT NULL")
        .bind(token_hash).fetch_one(&self.pool).await.map_err(StoreError::from)? > 0)
}
pub async fn revoke_refresh(&self, id:&str) -> Result<(), StoreError> {
    sqlx::query("UPDATE refresh_tokens SET revoked_at=now() WHERE id=$1 AND revoked_at IS NULL").bind(id).execute(&self.pool).await.map_err(StoreError::from)?;
    Ok(())
}
pub async fn revoke_all_refresh(&self, user_id:&str) -> Result<(), StoreError> {
    sqlx::query("UPDATE refresh_tokens SET revoked_at=now() WHERE user_id=$1 AND revoked_at IS NULL").bind(user_id).execute(&self.pool).await.map_err(StoreError::from)?;
    Ok(())
}
pub async fn insert_reset_token(&self, id:&str, user_id:&str, token_hash:&str, expires_at_unix:i64) -> Result<(), StoreError> {
    let exp = time::OffsetDateTime::from_unix_timestamp(expires_at_unix).unwrap();
    sqlx::query("INSERT INTO password_reset_tokens(id,user_id,token_hash,expires_at) VALUES ($1,$2,$3,$4)")
        .bind(id).bind(user_id).bind(token_hash).bind(exp).execute(&self.pool).await.map_err(StoreError::from)?;
    Ok(())
}
/// Consume a live reset token (mark used) and return user_id. None if invalid/used/expired.
pub async fn consume_reset_token(&self, token_hash:&str, now_unix:i64) -> Result<Option<String>, StoreError> {
    let now = time::OffsetDateTime::from_unix_timestamp(now_unix).unwrap();
    let r = sqlx::query_scalar::<_,String>(
        "UPDATE password_reset_tokens SET used_at=now() WHERE token_hash=$1 AND used_at IS NULL AND expires_at > $2 RETURNING user_id")
        .bind(token_hash).bind(now).fetch_optional(&self.pool).await.map_err(StoreError::from)?;
    Ok(r)
}
```

- [ ] **Step 2: Test**

Append:
```rust
#[sqlx::test(migrations = "../../migrations")]
async fn refresh_rotation_and_reuse(pool: sqlx::PgPool) {
    seed_user(&pool).await;
    let store = Store::from_pool(pool);
    store.insert_refresh("r1","u1","t1","hash1", 9999999999, None).await.unwrap();
    assert!(store.find_live_refresh("hash1", 1000).await.unwrap().is_some());
    store.revoke_refresh("r1").await.unwrap();
    assert!(store.find_live_refresh("hash1", 1000).await.unwrap().is_none());
    assert!(store.refresh_is_revoked("hash1").await.unwrap());
}
#[sqlx::test(migrations = "../../migrations")]
async fn reset_token_single_use(pool: sqlx::PgPool) {
    seed_user(&pool).await;
    let store = Store::from_pool(pool);
    store.insert_reset_token("rt1","u1","rhash", 9999999999).await.unwrap();
    assert_eq!(store.consume_reset_token("rhash", 1000).await.unwrap(), Some("u1".into()));
    assert_eq!(store.consume_reset_token("rhash", 1000).await.unwrap(), None); // already used
}
```

- [ ] **Step 3: Run + commit**
```bash
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --manifest-path backend/Cargo.toml -p recon-store auth
git add backend/crates/recon-store/src/auth.rs backend/crates/recon-store/tests/auth.rs
git commit -m "feat(store): refresh-token rotation/reuse-detection + reset-token lifecycle"
```

---

## Task 12: Store — admin user management + updated seed

**Files:**
- Modify: `backend/crates/recon-store/src/auth.rs` (user CRUD within tenant)
- Modify: `backend/crates/recon-store/src/seed.rs` (global users + memberships + credentials)
- Test: append to `backend/crates/recon-store/tests/auth.rs`

- [ ] **Step 1: User management queries**

Append to `impl Store`:
```rust
/// Users who are members of `tenant_id`, with their role there.
pub async fn list_users_in_tenant(&self, tenant_id:&str) -> Result<Vec<User>, StoreError> {
    let rows = sqlx::query_as::<_,(String,String,String,bool,String)>(
        "SELECT u.id,u.name,u.email,u.disabled,m.role FROM users u JOIN memberships m ON m.user_id=u.id \
         WHERE m.tenant_id=$1 ORDER BY u.name").bind(tenant_id).fetch_all(&self.pool).await.map_err(StoreError::from)?;
    Ok(rows.into_iter().map(|(id,name,email,disabled,role)| User{id,name,email,disabled,role:parse_role(&role)}).collect())
}
pub async fn create_user_with_membership(&self, id:&str, name:&str, email:&str, password_hash:&str, tenant_id:&str, role:UserRole) -> Result<(), StoreError> {
    let mut tx = self.pool.begin().await.map_err(StoreError::from)?;
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ($1,$2,$3,false)").bind(id).bind(name).bind(email)
        .execute(&mut *tx).await.map_err(StoreError::from)?;
    sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES ($1,$2)").bind(id).bind(password_hash)
        .execute(&mut *tx).await.map_err(StoreError::from)?;
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ($1,$2,$3)").bind(id).bind(tenant_id).bind(role_str(role))
        .execute(&mut *tx).await.map_err(StoreError::from)?;
    tx.commit().await.map_err(StoreError::from)?;
    Ok(())
}
pub async fn update_membership_role(&self, user_id:&str, tenant_id:&str, role:UserRole) -> Result<u64, StoreError> {
    let r = sqlx::query("UPDATE memberships SET role=$3 WHERE user_id=$1 AND tenant_id=$2")
        .bind(user_id).bind(tenant_id).bind(role_str(role)).execute(&self.pool).await.map_err(StoreError::from)?;
    Ok(r.rows_affected())
}
pub async fn set_user_disabled(&self, user_id:&str, disabled:bool) -> Result<(), StoreError> {
    sqlx::query("UPDATE users SET disabled=$2 WHERE id=$1").bind(user_id).bind(disabled).execute(&self.pool).await.map_err(StoreError::from)?;
    Ok(())
}
pub async fn remove_membership(&self, user_id:&str, tenant_id:&str) -> Result<u64, StoreError> {
    let r = sqlx::query("DELETE FROM memberships WHERE user_id=$1 AND tenant_id=$2").bind(user_id).bind(tenant_id).execute(&self.pool).await.map_err(StoreError::from)?;
    Ok(r.rows_affected())
}
```
Add helper: `fn role_str(r: UserRole) -> &'static str { match r { UserRole::Operator=>"operator", UserRole::Approver=>"approver", UserRole::Admin=>"admin" } }`

- [ ] **Step 2: Update seed**

In `seed.rs`, change user seeding so users are global with email + credentials + memberships. Use a fixed dev password. Add `recon-auth` as a dep of `recon-store` (`recon-auth = { path = "../recon-auth" }`) to hash the dev password. Seed:
- `user-mia` (mia@acme.test) → membership operator @ tenant-acme
- `user-theo` (theo@acme.test) → membership approver @ tenant-acme
- `user-ada` (ada@acme.test) → membership admin @ tenant-acme **and** admin @ tenant-globex (dual-tenant for switcher demo)
- All passwords = `Password123!` (argon2id hashed via `recon_auth::password::hash_password`).
Replace any old `INSERT INTO users(... tenant_id, role ...)` with the new shape: `INSERT INTO users(id,name,email,disabled)` + `user_credentials` + `memberships`.

Document the dev creds in a code comment and in README (Task 27).

- [ ] **Step 3: Test**

Append:
```rust
#[sqlx::test(migrations = "../../migrations")]
async fn create_and_list_users(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t1','Acme','acme')").execute(&pool).await.unwrap();
    let store = Store::from_pool(pool);
    store.create_user_with_membership("u9","New Op","op@acme.test","$argon2id$x","t1",UserRole::Operator).await.unwrap();
    let users = store.list_users_in_tenant("t1").await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].email, "op@acme.test");
    assert_eq!(store.update_membership_role("u9","t1",UserRole::Approver).await.unwrap(), 1);
    assert_eq!(store.role_in_tenant("u9","t1").await.unwrap(), Some(UserRole::Approver));
}
```

- [ ] **Step 4: Run full seed end-to-end**
```bash
docker exec recon-postgres psql -U recon -d recon -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo run --manifest-path backend/Cargo.toml -p recon-api -- seed 2>&1 | tail -3
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --manifest-path backend/Cargo.toml -p recon-store
```
Expected: seed completes; store tests pass.

- [ ] **Step 5: Commit**
```bash
git add backend/crates/recon-store
git commit -m "feat(store): admin user management + auth-aware seed (global users, memberships, credentials)"
```

---

## Task 13: API — AppState, config, AuthContext via Bearer token

**Files:**
- Modify: `backend/crates/recon-api/src/state.rs` (config: jwt secret, ttls, smtp, app base url, mailer, rate limiter)
- Modify: `backend/crates/recon-api/src/auth.rs` (extractor validates Bearer token)
- Modify: `backend/crates/recon-api/Cargo.toml` (deps: recon-auth, recon-mail, axum-extra for cookies, tower-governor or custom limiter)

- [ ] **Step 1: Extend AppState**

In `state.rs`:
```rust
use std::sync::Arc;
use recon_mail::Mailer;

#[derive(Clone)]
pub struct AppState {
    pub store: recon_store::Store,
    pub cfg: Arc<AuthConfig>,
    pub mailer: Arc<dyn Mailer>,
    pub login_limiter: Arc<crate::ratelimit::IpLimiter>, // Task 14
}

pub struct AuthConfig {
    pub jwt_secret: Vec<u8>,
    pub access_ttl_secs: i64,   // 900
    pub refresh_ttl_secs: i64,  // 2_592_000
    pub app_base_url: String,
    pub secure_cookie: bool,
}
impl AuthConfig {
    pub fn from_env() -> Self {
        let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| { tracing::warn!("JWT_SECRET unset — using insecure dev secret"); "dev-insecure-secret-change-me".into() });
        Self {
            jwt_secret: secret.into_bytes(),
            access_ttl_secs: 900,
            refresh_ttl_secs: 2_592_000,
            app_base_url: std::env::var("APP_BASE_URL").unwrap_or_else(|_| "http://localhost:3100".into()),
            secure_cookie: std::env::var("SECURE_COOKIE").map(|v| v=="1"||v=="true").unwrap_or(false),
        }
    }
}
```

- [ ] **Step 2: Rewrite the AuthContext extractor**

Replace `auth.rs` extractor body so it reads `Authorization: Bearer <jwt>` and validates via `recon_auth::token::decode_access`, populating `AuthContext { user_id, tenant_id, role }`. On missing/invalid → `ApiError::Unauthorized`. Keep `#[axum::async_trait]`.
```rust
#[axum::async_trait]
impl axum::extract::FromRequestParts<crate::state::AppState> for AuthContext {
    type Rejection = crate::error::ApiError;
    async fn from_request_parts(parts: &mut axum::http::request::Parts, state: &crate::state::AppState) -> Result<Self, Self::Rejection> {
        let header = parts.headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or("");
        let token = header.strip_prefix("Bearer ").ok_or(crate::error::ApiError::Unauthorized)?;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let claims = recon_auth::token::decode_access(&state.cfg.jwt_secret, token, now).map_err(|_| crate::error::ApiError::Unauthorized)?;
        Ok(AuthContext { user_id: claims.sub, tenant_id: claims.tid, role: claims.role })
    }
}
```
Add a `RequirePermission` helper or inline `recon_auth::rbac::require(ctx.role, perm).map_err(|_| ApiError::Forbidden)?` in handlers.

- [ ] **Step 3: Compile check**

Run: `cargo build --manifest-path backend/Cargo.toml -p recon-api` (handlers for /auth come next; ensure state + extractor compile). Fix `AppState` construction in `main.rs`/`lib.rs` minimally to pass config + a `LogMailer` default.

- [ ] **Step 4: Commit**
```bash
git add backend/crates/recon-api
git commit -m "feat(api): AuthConfig + Bearer-token AuthContext extractor"
```

---

## Task 14: API — `/auth/login` with rate limit + lockout

**Files:**
- Create: `backend/crates/recon-api/src/ratelimit.rs` (in-memory per-IP token bucket)
- Create: `backend/crates/recon-api/src/routes_auth.rs` (auth handlers)
- Modify: `backend/crates/recon-api/src/dto.rs` (auth DTOs)
- Modify: `backend/crates/recon-api/src/routes.rs` (mount `/auth/login`)
- Test: `backend/crates/recon-api/tests/auth_flow.rs`

- [ ] **Step 1: IP limiter**

`ratelimit.rs`: a `Mutex<HashMap<String,(tokens,last_refill)>>` token bucket, `pub fn check(&self, ip:&str)->bool` (e.g., 10 attempts / 60s). Unit-test refill + exhaustion deterministically by injecting `now`.
```rust
pub struct IpLimiter { inner: std::sync::Mutex<std::collections::HashMap<String,(f64,i64)>>, capacity:f64, refill_per_sec:f64 }
impl IpLimiter {
    pub fn new(capacity:f64, refill_per_sec:f64)->Self{ Self{ inner:Default::default(), capacity, refill_per_sec } }
    pub fn check_at(&self, ip:&str, now:i64)->bool {
        let mut g = self.inner.lock().unwrap();
        let e = g.entry(ip.to_string()).or_insert((self.capacity, now));
        let elapsed = (now - e.1).max(0) as f64;
        e.0 = (e.0 + elapsed*self.refill_per_sec).min(self.capacity);
        e.1 = now;
        if e.0 >= 1.0 { e.0 -= 1.0; true } else { false }
    }
    pub fn check(&self, ip:&str)->bool { self.check_at(ip, time::OffsetDateTime::now_utc().unix_timestamp()) }
}
#[cfg(test)]
mod tests { use super::*;
    #[test] fn exhausts_then_refills() {
        let l = IpLimiter::new(2.0, 1.0);
        assert!(l.check_at("ip",0)); assert!(l.check_at("ip",0)); assert!(!l.check_at("ip",0));
        assert!(l.check_at("ip",2)); // refilled
    }
}
```

- [ ] **Step 2: login handler**

`routes_auth.rs` — `login(State, ConnectInfo<SocketAddr> or X-Forwarded-For, Json<LoginReq>)`:
- `if !limiter.check(ip) → 429`.
- `find_credential_by_email`; if none → run a dummy argon2 verify to equalize timing, return `401`.
- if `is_locked(cred.locked_until, now)` → `429`.
- `verify_password`; on fail → `attempts+1`, `lockout::on_failure`, `record_login_failure`, return `401` (or `429` if now locked).
- on success → `reset_login_failures`; pick active tenant = first membership (or 404 if none); `role_in_tenant`; `encode_access`; `refresh::generate` + `insert_refresh`; set refresh cookie; return `{accessToken, user, activeTenant, memberships}`.

DTOs in `dto.rs`:
```rust
#[derive(serde::Deserialize)] #[serde(rename_all="camelCase")] pub struct LoginReq { pub email:String, pub password:String }
#[derive(serde::Serialize)] #[serde(rename_all="camelCase")] pub struct LoginResp { pub access_token:String, pub user:recon_domain::User, pub active_tenant:recon_domain::Tenant, pub memberships:Vec<recon_domain::Membership> }
```
Cookie via `axum_extra::extract::cookie::CookieJar` returning `(jar, Json(resp))`. Cookie: name `recon_refresh`, httpOnly, path `/auth`, same_site Strict, secure = cfg.secure_cookie, max_age = refresh_ttl.

- [ ] **Step 3: Integration test (`#[sqlx::test]`)**

`tests/auth_flow.rs` builds the router with the test pool (add a `recon_api::test_support::app(pool, cfg, mailer)` constructor in `lib.rs`), seeds a user with a known argon2 hash, then:
```
- POST /auth/login wrong password → 401
- POST /auth/login correct → 200, body has accessToken; Set-Cookie recon_refresh present (httpOnly)
- 5 wrong attempts → 429 (locked)
```
Use `tower::ServiceExt::oneshot` to drive the router.

- [ ] **Step 4: Run + commit**
```bash
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --manifest-path backend/Cargo.toml -p recon-api auth_flow
git add backend/crates/recon-api
git commit -m "feat(api): /auth/login with per-IP rate limit and account lockout"
```

---

## Task 15: API — `/auth/refresh` (rotation) + `/auth/logout`

**Files:**
- Modify: `backend/crates/recon-api/src/routes_auth.rs`, `routes.rs`
- Test: append to `tests/auth_flow.rs`

- [ ] **Step 1: refresh handler**

`refresh(jar, State)`:
- read `recon_refresh` cookie; if absent → `401`.
- `h = refresh::hash(cookie)`.
- if `refresh_is_revoked(h)` → **reuse detected**: `revoke_all_refresh(user)` (look up via a `find_any_refresh_user(h)` helper) + `401`.
- `find_live_refresh(h, now)` → else `401`.
- `revoke_refresh(old_id)`; `role_in_tenant(user,tenant)`; new access token; `refresh::generate` + `insert_refresh(rotated_from=old_id)`; set new cookie; return `{accessToken}`.

`logout(jar, State)`: if cookie present, `revoke_refresh` for its hash's row; clear cookie (max_age 0); `204`.

- [ ] **Step 2: Test**

Append: login → capture cookie → `/auth/refresh` with cookie → 200 + new cookie; reuse the **old** cookie again → 401 (revoked). `/auth/logout` → 204; subsequent refresh with that cookie → 401.

- [ ] **Step 3: Run + commit**
```bash
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --manifest-path backend/Cargo.toml -p recon-api auth_flow
git add backend/crates/recon-api
git commit -m "feat(api): /auth/refresh with rotation+reuse-detection and /auth/logout"
```

---

## Task 16: API — `/auth/switch-tenant` + `/auth/password`

**Files:**
- Modify: `backend/crates/recon-api/src/routes_auth.rs`, `routes.rs`, `dto.rs`
- Test: append to `tests/auth_flow.rs`

- [ ] **Step 1: handlers**

`switch_tenant(ctx: AuthContext, jar, State, Json{tenantId})`:
- `role_in_tenant(ctx.user_id, tenantId)` → `None` ⇒ `403`.
- issue new access token scoped to tenantId+role; rotate refresh (revoke current cookie row, insert new scoped to tenantId); return `{accessToken}` + cookie.

`change_password(ctx, State, Json{currentPassword,newPassword})`:
- load credential by user; `verify_password(current)` → false ⇒ `403`.
- validate newPassword length ≥ 8 (else `400`); `hash_password`; `set_password`; `revoke_all_refresh(user)` except... (simplest: revoke all; client silently re-logs via its current access token until expiry). `204`.

DTOs: `SwitchTenantReq{tenantId}`, `ChangePasswordReq{currentPassword,newPassword}`.

- [ ] **Step 2: Test**

Append: login as ada (dual-tenant) → switch-tenant to globex → 200; switch to a non-member tenant → 403. Change password with wrong current → 403; correct → 204; re-login with new password → 200.

- [ ] **Step 3: Run + commit**
```bash
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --manifest-path backend/Cargo.toml -p recon-api auth_flow
git add backend/crates/recon-api
git commit -m "feat(api): /auth/switch-tenant and /auth/password"
```

---

## Task 17: API — `/auth/forgot` + `/auth/reset` (email)

**Files:**
- Modify: `backend/crates/recon-api/src/routes_auth.rs`, `routes.rs`, `dto.rs`
- Test: append to `tests/auth_flow.rs` (use `recon_mail::testing::CapturingMailer`)

- [ ] **Step 1: handlers**

`forgot(State, Json{email})`:
- look up user by email (reuse `find_credential_by_email`); if present: `reset::generate` (reuse `refresh::generate` pattern → plaintext+hash), `insert_reset_token(ttl=3600)`, send email via `state.mailer` with link `"{app_base_url}/reset?token={plaintext}"`.
- **always** return `202` (no enumeration).

`reset(State, Json{token,newPassword})`:
- validate newPassword length; `h=refresh::hash(token)`; `consume_reset_token(h,now)` → `None` ⇒ `400`; else `set_password`, `revoke_all_refresh(user)`, `204`.

- [ ] **Step 2: Test**

Append (build app with `CapturingMailer`): `/auth/forgot` for known email → 202 and mailer captured 1 message containing `/reset?token=`; extract token; `/auth/reset` → 204; re-login with new password → 200. `/auth/forgot` for unknown email → 202 and **no** message captured.

- [ ] **Step 3: Run + commit**
```bash
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --manifest-path backend/Cargo.toml -p recon-api auth_flow
git add backend/crates/recon-api
git commit -m "feat(api): /auth/forgot + /auth/reset with emailed reset link"
```

---

## Task 18: API — admin user routes + RBAC on existing routes

**Files:**
- Create: `backend/crates/recon-api/src/routes_users.rs`
- Modify: `backend/crates/recon-api/src/routes.rs` (mount; apply guards), existing handlers in `routes.rs` (read tenant/role from `ctx`, add `rbac::require`)
- Test: append to `tests/auth_flow.rs`

- [ ] **Step 1: admin handlers (all require `ManageUsers`)**

In `routes_users.rs`: `list_users`, `create_user`, `patch_user`, `delete_user` — each starts with `rbac::require(ctx.role, Permission::ManageUsers).map_err(|_| ApiError::Forbidden)?` and scopes to `ctx.tenant_id`. `create_user` hashes a provided temp password. DTOs: `CreateUserReq{name,email,role,password}`, `PatchUserReq{role:Option<UserRole>,disabled:Option<bool>}`.

- [ ] **Step 2: enforce four-eyes role from token on existing approval path**

In the existing `append_event` handler, when the event is an approval, call `rbac::require(ctx.role, Permission::ApproveResolution)` before the store call; the store's `can_approve` continues to enforce maker≠checker. The acting user/role now come from `ctx` (token), not headers — confirm the old `X-User-Id` binding is replaced by `ctx.user_id`.

- [ ] **Step 3: Test**

Append: as operator token → `GET /api/users` → 403; `POST /api/users` → 403. As admin token → create user → 200; list shows it; patch role → 200; delete (remove membership) → 200. As operator → approval event → 403 (ApproveResolution). As approver (not maker) → approval → 200.

- [ ] **Step 4: Run + commit**
```bash
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --manifest-path backend/Cargo.toml -p recon-api
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
git add backend/crates/recon-api
git commit -m "feat(api): admin user management + RBAC guards on protected routes"
```

---

## Task 19: API — wire real config in main.rs (mailer/limiter/cors/dev)

**Files:**
- Modify: `backend/crates/recon-api/src/main.rs`

- [ ] **Step 1: Build state from env**

In `main.rs serve`: construct `AuthConfig::from_env()`; choose mailer: if `SMTP_HOST` set → `SmtpMailer::new(host,port,from)` else `LogMailer`; `IpLimiter::new(10.0, 10.0/60.0)`. Mount `/auth/*`, `/api/users`. Keep `/api/dev/reseed` behind `RECON_DEV`. CORS already allows `WEB_ORIGIN` with credentials — **add** `.allow_credentials(true)` and ensure `allow_headers` includes `authorization` and `content-type`, `allow_methods` includes the verbs used. (With credentials, origin must be explicit — already is.)

- [ ] **Step 2: Manual smoke**
```bash
docker compose -f backend/docker-compose.yml up -d --wait postgres mailpit
docker exec recon-postgres psql -U recon -d recon -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo run --manifest-path backend/Cargo.toml -p recon-api -- seed
RECON_DEV=1 DATABASE_URL=postgres://recon:recon@localhost:5432/recon SMTP_HOST=localhost SMTP_PORT=1025 cargo run --manifest-path backend/Cargo.toml -p recon-api &
curl -s -i -X POST localhost:8080/auth/login -H 'content-type: application/json' -d '{"email":"mia@acme.test","password":"Password123!"}' | head -20
```
Expected: 200 with `accessToken` and `Set-Cookie: recon_refresh`.

- [ ] **Step 3: Commit**
```bash
git add backend/crates/recon-api/src/main.rs
git commit -m "feat(api): wire AuthConfig, mailer, rate limiter, credentialed CORS"
```

---

## Task 20: Frontend — AuthProvider + auth API client methods

**Files:**
- Create: `web/lib/auth/provider.tsx` (`AuthProvider`, `useAuth`)
- Modify: `web/lib/api/http.ts` (Bearer header + 401→refresh retry; `credentials:"include"` on `/auth/*`)
- Modify: `web/lib/api/types.ts` (or wherever) — `User` gains `email`; add `Membership`, auth DTOs
- Test: `web/lib/auth/provider.test.tsx`

- [ ] **Step 1: auth client + provider**

`http.ts`: add `login/refresh/logout/switchTenant/changePassword/forgotPassword/resetPassword` calling `/auth/*` with `credentials:"include"`. Add a request wrapper that injects `Authorization: Bearer ${token}` (token from a setter the provider wires in), and on `401` calls refresh once and retries; on refresh failure throws an `Unauthorized` sentinel.

`provider.tsx`: React context holding `{user, memberships, activeTenant, role, status}` and `accessToken` in a `useRef` (memory). On mount, call `refresh()`; set token + session or mark unauthenticated. Provide `login`, `logout`, `switchTenant`, `changePassword`. Schedule silent refresh at `ttl-60s` (decode exp from JWT payload, no verification needed client-side).

- [ ] **Step 2: Test (vitest + MockApiClient extension)**

`provider.test.tsx`: render a consumer; mock the http auth calls; assert: bootstrap refresh success → status `authenticated` with user; refresh failure → `unauthenticated`; `login` populates session; `logout` clears it.

- [ ] **Step 3: Run + commit**
```bash
pnpm -C web test provider
git add web/lib/auth web/lib/api
git commit -m "feat(web): AuthProvider with in-memory token, bootstrap + silent refresh"
```

---

## Task 21: Frontend — login / forgot / reset pages + route guard

**Files:**
- Create: `web/app/login/page.tsx`, `web/app/forgot/page.tsx`, `web/app/reset/page.tsx`
- Create: `web/components/auth/RouteGuard.tsx` (or integrate in layout/provider)
- Modify: `web/app/layout.tsx` (wrap with `AuthProvider` + guard; public allowlist)
- Test: `web/app/login/login.test.tsx`

- [ ] **Step 1: pages + guard**

Login form (react-hook-form + zod: email, password) → `useAuth().login`; show inline error on 401/429; on success `router.push("/dashboard")`. Forgot form → `forgotPassword(email)` → always show confirmation copy. Reset page reads `token` from `useSearchParams`, new-password form → `resetPassword` → redirect `/login`. Guard: while `status==="loading"` show spinner; if `unauthenticated` and path not in `["/login","/forgot","/reset"]` → redirect `/login`; if `authenticated` and path `==="/login"` → `/dashboard`.

(Next.js 16: read `web/AGENTS.md` and the local docs for `useSearchParams`/route conventions before coding.)

- [ ] **Step 2: Test**

`login.test.tsx`: renders form; submit with mocked failing login → shows error; submit success → calls router push. Guard unit test: unauthenticated on `/dashboard` triggers redirect.

- [ ] **Step 3: Run + commit**
```bash
pnpm -C web test login
git add web/app web/components/auth
git commit -m "feat(web): login/forgot/reset pages + route guard"
```

---

## Task 22: Frontend — tenant switcher re-scope, UserMenu logout, remove localStorage switcher, admin Users + password change

**Files:**
- Modify: top-bar tenant switcher component, `UserMenu`
- Delete usages of `recon:currentUserId` / `recon:activeTenantId`
- Create: `web/app/users/page.tsx` (admin-only), password-change dialog
- Test: relevant vitest specs

- [ ] **Step 1: switcher + menu**

Tenant switcher lists `useAuth().memberships`; on select → `switchTenant(tenantId)` → after token swap, `queryClient.invalidateQueries()` so all data refetches under the new tenant. `UserMenu` shows `user.name`/`email` + **Logout** (`useAuth().logout()` → redirect `/login`). Remove the old localStorage-based user switcher and `currentUserId()` in `http.ts` (identity now comes from the token).

- [ ] **Step 2: admin Users screen + password dialog**

`/users` (guarded to `role==="admin"`, else redirect/hide nav): table of `GET /api/users`; "Add user" dialog (name,email,role,temp password) → `POST /api/users`; row actions: change role (`PATCH`), disable (`PATCH`), remove (`DELETE`); invalidate the users query after each. Password-change dialog from `UserMenu` → `changePassword(current,new)`.

Show the "Users" nav item only when `role==="admin"`.

- [ ] **Step 3: Test + run full FE suite**
```bash
pnpm -C web test
pnpm -C web lint && pnpm -C web typecheck
git add web
git commit -m "feat(web): tenant switch re-scope, logout, admin Users screen, password change"
```

---

## Task 23: E2E — Playwright against the live auth stack + Mailpit

**Files:**
- Modify: `web/tests/e2e/operator-loop.spec.ts` (login instead of localStorage seeding)
- Create: `web/tests/e2e/auth.spec.ts`
- Modify: `web/playwright.config.ts` if needed (webServer already :3100)

- [ ] **Step 1: rewrite seeding to real login**

Replace `seedStorage` with a `loginAs(page, email, password)` helper that performs the `/login` UI flow (or seeds the session by calling `/auth/login` and injecting the cookie + bootstrapping). `test.beforeEach` still reseeds the backend via `/api/dev/reseed`.

- [ ] **Step 2: auth E2E**

`auth.spec.ts`:
- login as Mia (operator) → open `case-pending` → Approve disabled.
- logout → login as Theo (approver) → Approve enabled → click → resolves.
- login as Ada (admin) → tenant switcher shows Acme + Globex → switch → dashboard data changes; `/users` reachable and lists users; create a user succeeds.
- password reset: `/forgot` for mia → poll Mailpit REST (`http://localhost:8025/api/v1/messages`) for the latest message → extract `/reset?token=...` → set new password → login with it.
- reload after login stays authenticated (refresh cookie bootstrap).

- [ ] **Step 3: Run (full stack up)**
```bash
docker compose -f backend/docker-compose.yml up -d --wait postgres mailpit
docker exec recon-postgres psql -U recon -d recon -c "DROP SCHEMA public CASCADE; CREATE SCHEMA public;"
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo run --manifest-path backend/Cargo.toml -p recon-api -- seed
RECON_DEV=1 DATABASE_URL=postgres://recon:recon@localhost:5432/recon SMTP_HOST=localhost SMTP_PORT=1025 cargo run --manifest-path backend/Cargo.toml -p recon-api &
pnpm -C web e2e
```
Expected: all E2E pass.

- [ ] **Step 4: Commit**
```bash
git add web/tests/e2e
git commit -m "test(e2e): auth flows — login, RBAC, tenant switch, admin, password reset via Mailpit"
```

---

## Task 24: Docs + env wiring

**Files:**
- Modify: `web/README.md` (full-stack run with Mailpit + dev creds), `backend/.env.example` (JWT_SECRET, SECURE_COOKIE), `web/.env.local`/`.env.example` (no change unless API base differs)

- [ ] **Step 1: Document run recipe + dev credentials**

Update the README full-stack section: add `mailpit` to compose up; list dev logins (mia/theo/ada @ example.test / `Password123!`); note Mailpit UI at `http://localhost:8025`; note `JWT_SECRET` for prod. Add `JWT_SECRET=` and `SECURE_COOKIE=` to `backend/.env.example`.

- [ ] **Step 2: Commit**
```bash
git add web/README.md backend/.env.example
git commit -m "docs: full-stack run with auth, Mailpit, and dev credentials"
```

---

## Final verification (run by controller after all tasks)

```bash
# Backend: all crates green + clippy clean
docker compose -f backend/docker-compose.yml up -d --wait postgres mailpit
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --manifest-path backend/Cargo.toml
cargo clippy --manifest-path backend/Cargo.toml --all-targets -- -D warnings
# Frontend
pnpm -C web test && pnpm -C web lint && pnpm -C web typecheck
# E2E against live stack (as in Task 23)
```

Then dispatch the final whole-implementation review subagent, and use superpowers:finishing-a-development-branch.
