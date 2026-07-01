# Phase 9 Polish Bundle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship four isolated polish sub-features — counterparty-aware matching, format auto-detection, per-upload dialect/profile override, and soft-delete/archive sources — in one branch with per-item commits.

**Architecture:** Each sub-feature is independently testable. Counterparty scoring adds a term to the pure `score_pair` with graceful degradation (no counterparty data → identical to today). Auto-detect adds a pure `detect_format` sniff + an `auto` route branch. Per-upload override adds optional multipart fields whose value takes precedence over the source's stored setting. Archive adds a `disabled` column + a dedicated archive/restore path emitting a new additive audit kind.

**Tech Stack:** Rust (Axum, sqlx, chrono), PostgreSQL, Next.js 16 / React 19 / TypeScript / Zod / react-hook-form / Base UI / TanStack Query, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-07-01-recon-phase9-polish-bundle-design.md`

## Global Constraints

- Backend cargo: `~/.cargo/bin/cargo`. DB for store/api tests: `DATABASE_URL=postgres://recon:recon@localhost:5432/recon` (the `sqlx::test` macro creates ephemeral DBs from it and applies all migrations).
- Frontend: run from `web/` with `pnpm`. Read `web/AGENTS.md` first (this Next.js 16 differs from older training data).
- The matching engine MUST stay deterministic and replayable — the three proptest properties in `recon-matching/tests/properties.rs` must keep passing unchanged.
- The audit chain invariant: NEVER mutate an existing `AuditPayload`/`AuditKind` variant; only ADD new ones (existing chains must still verify). Adding a kind requires 5 sync points: `AuditKind` enum, `as_str`, `from_str`, the `AuditPayload` variant, and the `AuditPayload::kind()` matcher.
- Every commit message ends with a trailing `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- Every task ends green on `cargo clippy --workspace --all-targets -- -D warnings` (backend tasks) or `pnpm tsc --noEmit` + `pnpm vitest run` (frontend tasks).

---

## File Structure

**Backend — modify:**
- `backend/crates/recon-matching/src/score.rs` — counterparty term (Task A1).
- `backend/crates/recon-matching/src/config.rs` — version bump (Task A1).
- `backend/crates/recon-ingest/src/detect.rs` (create) + `lib.rs` — `detect_format` (Task B1).
- `backend/crates/recon-api/src/routes.rs` — `auto` branch (B2), override fields (C1), archive routes + ingest guard + list query param (D4).
- `backend/crates/recon-api/src/dto.rs` — (if needed) query struct for list includeArchived (D4).
- `backend/migrations/0008_source_disabled.sql` (create) — `disabled` column (D1).
- `backend/crates/recon-domain/src/types.rs` — `Source.disabled` (D1).
- `backend/crates/recon-store/src/rows.rs` — `SourceRow.disabled` (D1).
- `backend/crates/recon-store/src/sources.rs` — threading + `set_source_disabled` + list filter (D1, D3).
- `backend/crates/recon-audit/src/events.rs` — `DataSourceArchived` kind + payload (D2).

**Frontend — modify:**
- `web/lib/domain/types.ts`, `web/lib/api/client.ts`, `web/lib/api/http.ts`, `web/lib/api/mock.ts` (Task E1).
- `web/app/(app)/sources/page.tsx`, `web/lib/hooks/use-sources.ts` (Task E2).
- `web/components/app/upload-dialog.tsx` (Task E3).
- `web/tests/*`, `web/tests/e2e/*` (E2/E3/F1).

**Docs:** `web/README.md` (F1).

---

## Task A1: Counterparty-aware matching + config bump

**Files:**
- Modify: `backend/crates/recon-matching/src/score.rs`
- Modify: `backend/crates/recon-matching/src/config.rs`

**Interfaces:**
- Produces: `score_pair(a, b) -> f64` (unchanged signature) now factoring counterparty identifiers; `MatchConfig::v1()` returns `version: "v1.1"`.

- [ ] **Step 1: Write failing tests for the counterparty term**

In `score.rs`, add to `mod tests` (the `txn` helper exists but sets counterparty fields to None; add a builder that sets them):
```rust
    fn txn_cp(
        id: &str,
        amt: i64,
        date: &str,
        bic: Option<&str>,
        acct: Option<&str>,
    ) -> CanonicalTransaction {
        CanonicalTransaction {
            id: id.into(),
            tenant_id: "t".into(),
            source_id: "s".into(),
            external_ref: id.into(),
            value_date: date.into(),
            posted_at: format!("{date}T00:00:00Z"),
            amount_minor: amt,
            currency: "GBP".into(),
            direction: Direction::Debit,
            counterparty: None,
            description: "d".into(),
            counterparty_bic: bic.map(|s| s.to_string()),
            counterparty_account: acct.map(|s| s.to_string()),
        }
    }

    #[test]
    fn matching_bic_scores_higher_than_mismatched() {
        // Same amount+date, different external_ref (ref_score=0). Both carry a BIC.
        let a = txn_cp("a", 1000, "2026-05-01", Some("DEUTDEFF"), None);
        let mut b = txn_cp("b", 1000, "2026-05-01", Some("DEUTDEFF"), None);
        let matched = score_pair(&a, &b);
        b.counterparty_bic = Some("CHASGB2L".into());
        let mismatched = score_pair(&a, &b);
        // cpty term is 0.15 when it matches, 0.0 when it doesn't.
        assert!((matched - mismatched - 0.15).abs() < 1e-9, "matched={matched} mismatched={mismatched}");
    }

    #[test]
    fn matching_account_also_boosts() {
        let a = txn_cp("a", 1000, "2026-05-01", None, Some("GB29NWBK..."));
        let b = txn_cp("b", 1000, "2026-05-01", None, Some("GB29NWBK..."));
        // amount 1 + date 1 + ref 0 + cpty 1 -> 0.5 + 0.25 + 0 + 0.15 = 0.90
        assert!((score_pair(&a, &b) - 0.90).abs() < 1e-9);
    }

    #[test]
    fn missing_identifier_falls_back_to_three_term() {
        // a has no identifier -> 3-term formula, identical to legacy behavior.
        let a = txn_cp("a", 1000, "2026-05-01", None, None);
        let b = txn_cp("b", 1000, "2026-05-01", Some("DEUTDEFF"), None);
        // amount 1 + date 1 + ref 0 -> 0.6 + 0.3 = 0.90
        assert!((score_pair(&a, &b) - 0.90).abs() < 1e-9);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd backend && cargo test -p recon-matching --lib score::`
Expected: FAIL (`matching_bic_scores_higher_than_mismatched` etc. — current formula ignores counterparty, so `matched == mismatched`).

- [ ] **Step 3: Implement the counterparty term**

In `score.rs`, replace the `let raw = 0.6 * amount_score + 0.3 * date_score + 0.1 * ref_score;` line with:
```rust
    // Counterparty signal: only when BOTH sides carry an identifier (BIC or
    // account). Otherwise omit the term and renormalize to the original
    // 0.6/0.3/0.1 — data-less pairs score exactly as before (no regression).
    let raw = match counterparty_score(a, b) {
        Some(cpty_score) => {
            0.5 * amount_score + 0.25 * date_score + 0.1 * ref_score + 0.15 * cpty_score
        }
        None => 0.6 * amount_score + 0.3 * date_score + 0.1 * ref_score,
    };
```
And add this helper below `score_pair` (before `#[cfg(test)]`):
```rust
/// Exact-identifier counterparty signal. `Some(1.0)` if BIC or account match,
/// `Some(0.0)` if both sides carry an identifier but neither matches, `None`
/// if either side lacks any identifier (caller uses the 3-term fallback).
fn counterparty_score(a: &CanonicalTransaction, b: &CanonicalTransaction) -> Option<f64> {
    let has_id = |t: &CanonicalTransaction| {
        t.counterparty_bic.as_deref().is_some_and(|s| !s.is_empty())
            || t.counterparty_account.as_deref().is_some_and(|s| !s.is_empty())
    };
    if !has_id(a) || !has_id(b) {
        return None;
    }
    let eq = |x: &Option<String>, y: &Option<String>| match (x.as_deref(), y.as_deref()) {
        (Some(p), Some(q)) => !p.is_empty() && p == q,
        _ => false,
    };
    let matched = eq(&a.counterparty_bic, &b.counterparty_bic)
        || eq(&a.counterparty_account, &b.counterparty_account);
    Some(if matched { 1.0 } else { 0.0 })
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd backend && cargo test -p recon-matching`
Expected: PASS — the 3 new score tests, the existing score tests (all use counterparty=None → fallback → unchanged), the engine tests, and the proptests (generators use counterparty=None → fallback → invariants hold).

- [ ] **Step 5: Bump the config version**

In `config.rs`, change `version: "v1.0".into(),` to `version: "v1.1".into(),` and update the doc comment on `v1()` to:
```rust
    /// The pinned default configuration used by the seed and tests.
    /// Algorithm v1.1: adds an exact-identifier counterparty term to scoring.
    /// The constructor name `v1()` is retained (it is the current pinned config);
    /// `version` is the persisted source of truth for `config_version`.
```

- [ ] **Step 6: Update any test asserting the old version string**

Run: `cd backend && grep -rn '"v1.0"' crates/ | grep -v target`
For each hit in a test that asserts a run's `config_version == "v1.0"`, change the expected value to `"v1.1"`. (Do NOT change historical-data fixtures that intentionally represent old runs — there should be none; if unsure, report.) Then:
Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --workspace 2>&1 | tail -15`
Expected: all PASS.

- [ ] **Step 7: Clippy + commit**

```bash
cd backend && cargo clippy -p recon-matching --all-targets -- -D warnings
git add crates/recon-matching/src/score.rs crates/recon-matching/src/config.rs
# plus any test files touched in Step 6
git commit -m "feat(matching): exact-identifier counterparty term in score_pair; config v1.1"
```

---

## Task B1: `detect_format` sniffer

**Files:**
- Create: `backend/crates/recon-ingest/src/detect.rs`
- Modify: `backend/crates/recon-ingest/src/lib.rs`

**Interfaces:**
- Produces: `recon_ingest::detect::detect_format(bytes: &[u8]) -> Option<&'static str>` returning one of `"pdf"|"camt053"|"bai2"|"mt942"|"mt940"` or `None` (CSV/unknown).

- [ ] **Step 1: Register the module**

In `backend/crates/recon-ingest/src/lib.rs`, add after `pub mod csv;`:
```rust
pub mod detect;
```

- [ ] **Step 2: Write the failing tests**

Create `backend/crates/recon-ingest/src/detect.rs`:
```rust
//! Best-effort format sniffing for `format=auto` uploads. Never returns CSV
//! (no reliable signature; CSV needs an explicit column mapping).

/// Sniff the leading bytes to pick a parser format. `None` = could not detect
/// (e.g. CSV or unknown) — the caller must reject with guidance.
pub fn detect_format(bytes: &[u8]) -> Option<&'static str> {
    // implemented in Step 4
    let _ = bytes;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_pdf() {
        assert_eq!(detect_format(b"%PDF-1.7\n..."), Some("pdf"));
    }
    #[test]
    fn detects_camt_xml() {
        assert_eq!(detect_format(b"<?xml version=\"1.0\"?><Document>"), Some("camt053"));
        assert_eq!(detect_format(b"  \n<Document>"), Some("camt053"));
    }
    #[test]
    fn detects_bai2() {
        assert_eq!(detect_format(b"01,BANK,RECIPIENT,260501,..."), Some("bai2"));
        assert_eq!(detect_format(b"02,..."), Some("bai2"));
    }
    #[test]
    fn detects_mt942_by_totals_tag() {
        let s = b":20:REF\r\n:34F:GBP0,\r\n:90D:3EUR100,\r\n";
        assert_eq!(detect_format(s), Some("mt942"));
    }
    #[test]
    fn detects_mt940_when_no_mt942_tags() {
        let s = b":20:REF\r\n:60F:C260501GBP0,\r\n:62F:C260501GBP0,\r\n";
        assert_eq!(detect_format(s), Some("mt940"));
    }
    #[test]
    fn returns_none_for_csv_or_garbage() {
        assert_eq!(detect_format(b"ref,date,amount,desc\nA1,2026-05-01,10.00,x"), None);
        assert_eq!(detect_format(b"\x00\x01\x02 random"), None);
        assert_eq!(detect_format(b""), None);
    }
}
```

- [ ] **Step 3: Run to verify they fail**

Run: `cd backend && cargo test -p recon-ingest --lib detect::`
Expected: FAIL (stub returns `None` for pdf/camt/bai2/mt94x cases).

- [ ] **Step 4: Implement `detect_format`**

Replace the stub body:
```rust
pub fn detect_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"%PDF") {
        return Some("pdf");
    }
    // First non-whitespace / non-BOM byte.
    let trimmed = {
        let mut b = bytes;
        if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
            b = &b[3..];
        }
        let start = b.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(b.len());
        &b[start..]
    };
    if trimmed.first() == Some(&b'<') {
        return Some("camt053");
    }
    if trimmed.starts_with(b"01,") || trimmed.starts_with(b"02,") {
        return Some("bai2");
    }
    // MT94x: must contain the mandatory :20: transaction-reference tag.
    let text = String::from_utf8_lossy(bytes);
    if text.contains(":20:") {
        // MT942-only markers: floor-limit (:34F:) or debit/credit totals (:90D:/:90C:).
        if text.contains(":34F:") || text.contains(":90D:") || text.contains(":90C:") {
            return Some("mt942");
        }
        return Some("mt940");
    }
    None
}
```

- [ ] **Step 5: Run to verify pass**

Run: `cd backend && cargo test -p recon-ingest --lib detect::`
Expected: PASS (all 6 tests).

- [ ] **Step 6: Clippy + commit**

```bash
cd backend && cargo clippy -p recon-ingest --all-targets -- -D warnings
git add crates/recon-ingest/src/detect.rs crates/recon-ingest/src/lib.rs
git commit -m "feat(ingest): detect_format byte sniffer for format=auto (never CSV)"
```

---

## Task B2: Wire `format=auto` into the ingest route

**Files:**
- Modify: `backend/crates/recon-api/src/routes.rs`
- Modify: `backend/crates/recon-api/tests/ingest_api.rs`

**Interfaces:**
- Consumes: `recon_ingest::detect::detect_format` (Task B1).
- Produces: `POST /api/sources/:id/ingest` accepts `format=auto`.

- [ ] **Step 1: Write the failing integration test**

In `backend/crates/recon-api/tests/ingest_api.rs`, add a test (reuse the existing `token`, `json`, `multipart_body` helpers; a source with an MT940 dialect is created then a `:20:` file uploaded with `format=auto`):
```rust
#[sqlx::test(migrations = "../../migrations")]
async fn auto_detect_dispatches_by_content(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-acme','admin')").execute(&pool).await.unwrap();
    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-acme"));

    // Source with a generic MT940 dialect.
    let req = Request::builder().method("POST").uri("/api/sources").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"kind":"bank","name":"Auto Bank","currency":"GBP","formatDialect":"generic"}"#)).unwrap();
    let (st, src) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "create: {src}");
    let src_id = src["id"].as_str().unwrap().to_string();

    // A minimal MT940 statement uploaded as format=auto.
    let mt940 = ":20:STMT001\r\n:25:12345\r\n:28C:1/1\r\n:60F:C260501GBP0,00\r\n:61:2605010501D45,20NTRFREF//BANK\r\n:86:PAYMENT\r\n:62F:C260501GBP45,20\r\n";
    let boundary = "BOUNDARY";
    let body = multipart_body(boundary, &[("file", Some("s.sta"), mt940), ("format", None, "auto")]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{src_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "auto ingest: {v}");
    assert_eq!(v["ingested"], 1);

    // CSV-ish content with format=auto -> 400 (cannot auto-detect CSV).
    let body = multipart_body(boundary, &[("file", Some("x.csv"), "ref,date,amount\nA1,2026-05-01,10.00\n"), ("format", None, "auto")]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{src_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, _v) = json(&app, req).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "auto cannot detect CSV");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api auto_detect_dispatches_by_content`
Expected: FAIL (the `_ => Err(BadRequest)` arm rejects `auto` → the MT940 upload 400s).

- [ ] **Step 3: Resolve `auto` before dispatch**

In `routes.rs` `ingest_source`, right after `let format = format.ok_or_else(ApiError::BadRequest)?;`, insert:
```rust
    // format=auto: sniff the content to pick a concrete parser. CSV is never
    // auto-detected (no signature; needs a column mapping) -> clear 400.
    let format = if format == "auto" {
        recon_ingest::detect::detect_format(&bytes)
            .map(|f| f.to_string())
            .ok_or_else(|| ApiError::with_details(
                axum::http::StatusCode::BAD_REQUEST,
                "bad_request",
                "could not auto-detect format; select CSV (with a column mapping) or a specific format explicitly",
                json!({}),
            ))?
    } else {
        format
    };
```
The existing `match format.as_str()` dispatch then runs with the resolved concrete format. (The audit's `file_format` will record the resolved format, which is correct.)

- [ ] **Step 4: Run to verify pass**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api auto_detect_dispatches_by_content`
Expected: PASS. Then `cargo test -p recon-api` — all PASS.

- [ ] **Step 5: Clippy + commit**

```bash
cd backend && cargo clippy -p recon-api --all-targets -- -D warnings
git add crates/recon-api/src/routes.rs crates/recon-api/tests/ingest_api.rs
git commit -m "feat(api): format=auto ingest — sniff content, 400 on undetectable/CSV"
```

---

## Task C1: Per-upload dialect/profile override

**Files:**
- Modify: `backend/crates/recon-api/src/routes.rs`
- Modify: `backend/crates/recon-api/tests/ingest_api.rs`

**Interfaces:**
- Produces: `ingest_source` accepts optional `dialect` and `pdfProfile` multipart fields overriding the source's stored values.

- [ ] **Step 1: Write the failing integration test**

Add to `ingest_api.rs`:
```rust
#[sqlx::test(migrations = "../../migrations")]
async fn per_upload_dialect_override(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-acme','admin')").execute(&pool).await.unwrap();
    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-acme"));

    // Source stored as GENERIC.
    let req = Request::builder().method("POST").uri("/api/sources").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"kind":"bank","name":"Ovr Bank","currency":"EUR","formatDialect":"generic"}"#)).unwrap();
    let (_st, src) = json(&app, req).await;
    let src_id = src["id"].as_str().unwrap().to_string();

    // An invalid dialect override -> 400.
    let boundary = "B";
    let mt940 = ":20:REF\r\n:25:1\r\n:28C:1/1\r\n:60F:C260501EUR0,00\r\n:61:2605010501D10,00NTRFR//B\r\n:86:X\r\n:62F:C260501EUR10,00\r\n";
    let body = multipart_body(boundary, &[("file", Some("s.sta"), mt940), ("format", None, "mt940"), ("dialect", None, "bogus")]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{src_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, _v) = json(&app, req).await;
    assert_eq!(st, StatusCode::BAD_REQUEST, "invalid override rejected");

    // A valid subfielded override on a generic source succeeds (parses).
    let body = multipart_body(boundary, &[("file", Some("s.sta"), mt940), ("format", None, "mt940"), ("dialect", None, "subfielded")]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{src_id}/ingest"))
        .header("authorization", &auth)
        .header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "override ingest: {v}");
    assert_eq!(v["ingested"], 1);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api per_upload_dialect_override`
Expected: FAIL (the `dialect` field is ignored; `bogus` override does not 400).

- [ ] **Step 3: Parse + apply the override fields**

In `ingest_source`, add two variables next to `mapping_json`:
```rust
    let mut dialect_override: Option<String> = None;
    let mut pdf_profile_override: Option<String> = None;
```
Add two arms to the multipart `match field.name()`:
```rust
            Some("dialect") => dialect_override = Some(field.text().await.map_err(|_| ApiError::BadRequest())?),
            Some("pdfProfile") => pdf_profile_override = Some(field.text().await.map_err(|_| ApiError::BadRequest())?),
```
After the `format=auto` resolution block (Task B2) and before the `match format.as_str()`, compute effective, validated values:
```rust
    // Per-upload overrides take precedence over the source's stored setting.
    let effective_dialect: Option<String> = match dialect_override.as_deref() {
        None => source.format_dialect.clone(),
        Some("generic") => Some("generic".to_string()),
        Some("subfielded") => Some("subfielded".to_string()),
        Some(_) => return Err(ApiError::BadRequest()),
    };
    let effective_pdf_profile: Option<String> = match pdf_profile_override.as_deref() {
        None => source.pdf_profile.clone(),
        Some(name) if recon_ingest::pdf::resolve_profile(name).is_some() => Some(name.to_string()),
        Some(_) => return Err(ApiError::BadRequest()),
    };
```
Then in the dispatch, replace `source.format_dialect.as_deref()` with `effective_dialect.as_deref()` in BOTH the `"mt940"` and `"mt942"` arms, and replace the `"pdf"` arm's `source.pdf_profile.as_deref()` with `effective_pdf_profile.as_deref()`.

- [ ] **Step 4: Run to verify pass**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api per_upload_dialect_override`
Expected: PASS. Then `cargo test -p recon-api` — all PASS (existing pdf/mt940 tests still green — they pass no override, so `effective_* == source.*`).

- [ ] **Step 5: Clippy + commit**

```bash
cd backend && cargo clippy -p recon-api --all-targets -- -D warnings
git add crates/recon-api/src/routes.rs crates/recon-api/tests/ingest_api.rs
git commit -m "feat(api): per-upload dialect/pdfProfile override (validated, precedence over source)"
```

---

## Task D1: Migration + `Source.disabled` threading

**Files:**
- Create: `backend/migrations/0008_source_disabled.sql`
- Modify: `backend/crates/recon-domain/src/types.rs`, `backend/crates/recon-store/src/rows.rs`, `backend/crates/recon-store/src/sources.rs`
- Modify test call-sites of `list_sources` (see Step 5).

**Interfaces:**
- Produces: `Source.disabled: bool`; `list_sources(tenant_id, include_archived: bool)`.

- [ ] **Step 1: Migration**

Create `backend/migrations/0008_source_disabled.sql`:
```sql
-- Phase 9: soft-delete / archive sources. Additive with a default (safe on
-- populated tables); mirrors users.disabled.
ALTER TABLE sources ADD COLUMN disabled BOOLEAN NOT NULL DEFAULT false;
```

- [ ] **Step 2: Domain + row**

In `recon-domain/src/types.rs` `struct Source`, add after `pub pdf_profile: Option<String>,`:
```rust
    pub disabled: bool,
```
In `recon-store/src/rows.rs` `struct SourceRow`, add after `pub pdf_profile: Option<String>,`:
```rust
    pub disabled: bool,
```
and in `impl From<SourceRow> for Source`, add after `pdf_profile: r.pdf_profile,`:
```rust
            disabled: r.disabled,
```

- [ ] **Step 3: Thread through create/get/list**

In `recon-store/src/sources.rs`:
- `create_source` returned `Source` literal — add after `pdf_profile: pdf_profile.map(|s| s.to_string()),`:
```rust
        disabled: false,
```
(The INSERT need not set `disabled`; the column defaults false.)
- `get_source` SELECT — add `,disabled`:
```rust
            sqlx::query_as("SELECT id,tenant_id,kind,name,currency,format_dialect,pdf_profile,disabled FROM sources WHERE id=$1 AND tenant_id=$2")
```
- `list_sources` — change the signature to `pub async fn list_sources(&self, tenant_id: &str, include_archived: bool) -> Result<Vec<SourceListItem>, StoreError>`; add `disabled: bool,` to the local `Row` struct (after `pdf_profile`); update the query to select + group + filter:
```rust
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT s.id, s.tenant_id, s.kind, s.name, s.currency, s.format_dialect, s.pdf_profile, s.disabled, \
                COUNT(t.id) AS txn_count \
         FROM sources s \
         LEFT JOIN canonical_transactions t ON t.source_id = s.id AND t.tenant_id = s.tenant_id \
         WHERE s.tenant_id = $1 AND ($2 OR NOT s.disabled) \
         GROUP BY s.id, s.tenant_id, s.kind, s.name, s.currency, s.format_dialect, s.pdf_profile, s.disabled \
         ORDER BY s.name",
    )
    .bind(tenant_id)
    .bind(include_archived)
    .fetch_all(&self.pool)
    .await?;
```
and in the constructed `recon_domain::Source`, add after `pdf_profile: r.pdf_profile,`:
```rust
                disabled: r.disabled,
```

- [ ] **Step 4: Compile to find broken literals + callers**

Run: `cd backend && cargo build 2>&1 | tail -30`
Expected: errors at every `Source { .. }` literal missing `disabled` and every `list_sources(x)` call now needing a 2nd arg. Fix each `Source` literal by adding `disabled: false,` and each `list_sources(&tenant)` call to `list_sources(&tenant, false)`.

- [ ] **Step 5: Fix known call-sites**

- `recon-api/src/routes.rs` `list_sources` handler → `s.store.list_sources(&ctx.tenant_id, false).await?` (Task D4 adds the query param; `false` is correct for now).
- Any `recon-store/tests/*.rs` and other `Source { .. }` construction in tests/mocks/seed — add `disabled: false`. Grep: `cd backend && grep -rn 'Source {' crates/ | grep -v target` and `grep -rn 'list_sources(' crates/ | grep -v 'fn list_sources'`.
- `recon-store/src/seed.rs` INSERT INTO sources (if it constructs `Source` literals or relies on column count) — the column now has a default, so plain INSERTs omitting `disabled` still work; only fix `Source { .. }` literals.

- [ ] **Step 6: Store test for the filter (write, run, verify)**

Add to `recon-store/tests/patch_source.rs` (or a new `archive.rs` test file — if new, it needs the same `sqlx::test` harness header as the existing tests; copy the imports from `patch_source.rs`):
```rust
#[sqlx::test(migrations = "../../migrations")]
async fn list_sources_hides_disabled_unless_included(pool: sqlx::PgPool) {
    let store = recon_store::Store::new(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')").execute(store.pool_ref()).await.unwrap();
    let s = store.create_source("t", recon_domain::SourceKind::Bank, "S", "GBP", "actor", None, None).await.unwrap();
    // Visible by default.
    assert_eq!(store.list_sources("t", false).await.unwrap().len(), 1);
    // Disable it directly, then it's hidden by default but shown with include_archived.
    sqlx::query("UPDATE sources SET disabled=true WHERE id=$1").bind(&s.id).execute(store.pool_ref()).await.unwrap();
    assert_eq!(store.list_sources("t", false).await.unwrap().len(), 0);
    assert_eq!(store.list_sources("t", true).await.unwrap().len(), 1);
}
```
NOTE: if `Store` has no `pool_ref()` accessor, use the pattern the existing tests use to run raw SQL (check `patch_source.rs` — it likely calls store methods only; if so, insert the tenant via a store method or replicate the harness's tenant-seed helper). Match the existing test file's approach exactly.

- [ ] **Step 7: Run + commit**

```bash
cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store && cargo clippy --workspace --all-targets -- -D warnings
git add migrations/0008_source_disabled.sql crates/recon-domain/src/types.rs crates/recon-store/src/rows.rs crates/recon-store/src/sources.rs crates/recon-api/src/routes.rs crates/recon-store/tests/
# plus any other files with fixed Source literals
git commit -m "feat(store): sources.disabled column (migration 0008) + list_sources include_archived filter"
```

---

## Task D2: `DataSourceArchived` audit kind

**Files:**
- Modify: `backend/crates/recon-audit/src/events.rs`

**Interfaces:**
- Produces: `AuditKind::DataSourceArchived` (wire `data.source.archived`), `AuditPayload::DataSourceArchived { source_id: String, disabled: bool }`.

- [ ] **Step 1: Write the failing test**

In `events.rs` `mod tests`, add:
```rust
    #[test]
    fn data_source_archived_kind_roundtrips() {
        assert_eq!(AuditKind::DataSourceArchived.as_str(), "data.source.archived");
        assert_eq!(AuditKind::from_str("data.source.archived"), Some(AuditKind::DataSourceArchived));
        let p = AuditPayload::DataSourceArchived { source_id: "src-1".into(), disabled: true };
        assert_eq!(p.kind(), AuditKind::DataSourceArchived);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd backend && cargo test -p recon-audit --lib`
Expected: FAIL to compile (`AuditKind::DataSourceArchived` and the payload variant don't exist).

- [ ] **Step 3: Add the 5 sync points**

In `events.rs`:
- `AuditKind` enum — add `DataSourceArchived` in the `Data*` group (after `DataRunCreated`).
- `as_str` — add `AuditKind::DataSourceArchived => "data.source.archived",`.
- `from_str` — add `"data.source.archived" => AuditKind::DataSourceArchived,`.
- `AuditPayload` enum — add (after the `DataSourceUpdated` variant):
```rust
    DataSourceArchived { source_id: String, disabled: bool },
```
- `AuditPayload::kind()` matcher — add `AuditPayload::DataSourceArchived { .. } => AuditKind::DataSourceArchived,`.

- [ ] **Step 4: Run to verify pass**

Run: `cd backend && cargo test -p recon-audit`
Expected: PASS (new test + the existing `kind_strings_are_stable_dot_notation` still passes; the golden vector is unaffected).

- [ ] **Step 5: Commit**

```bash
cd backend && cargo clippy -p recon-audit --all-targets -- -D warnings
git add crates/recon-audit/src/events.rs
git commit -m "feat(audit): DataSourceArchived kind (data.source.archived) — additive, chain-safe"
```

---

## Task D3: `set_source_disabled` store method + ingest guard

**Files:**
- Modify: `backend/crates/recon-store/src/sources.rs`
- Modify: `backend/crates/recon-store/tests/` (archive round-trip + audit)

**Interfaces:**
- Consumes: `AuditPayload::DataSourceArchived` (D2), `Source.disabled` (D1).
- Produces: `Store::set_source_disabled(tenant_id, source_id, disabled, actor_id) -> Result<(), StoreError>`.

- [ ] **Step 1: Write the failing test**

Add to the archive test file from D1:
```rust
#[sqlx::test(migrations = "../../migrations")]
async fn set_source_disabled_persists_and_audits(pool: sqlx::PgPool) {
    let store = recon_store::Store::new(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')").execute(store.pool_ref()).await.unwrap();
    let s = store.create_source("t", recon_domain::SourceKind::Bank, "S", "GBP", "actor", None, None).await.unwrap();
    store.set_source_disabled("t", &s.id, true, "actor").await.unwrap();
    assert!(store.get_source("t", &s.id).await.unwrap().disabled);
    store.set_source_disabled("t", &s.id, false, "actor").await.unwrap();
    assert!(!store.get_source("t", &s.id).await.unwrap().disabled);
    // Audit chain still verifies after archive+restore events.
    assert!(store.verify_audit("t").await.unwrap().valid);
}
```
NOTE: match the exact `verify_audit` return shape used in `recon-store/tests/audit_schema.rs` (adjust `.valid` to the real field/name). If `pool_ref()` doesn't exist, use the same tenant-seed mechanism the sibling tests use.

- [ ] **Step 2: Run to verify it fails**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store set_source_disabled_persists_and_audits`
Expected: FAIL to compile (`set_source_disabled` undefined).

- [ ] **Step 3: Implement `set_source_disabled`**

In `sources.rs` `impl Store`, add (mirrors `set_user_disabled`):
```rust
    /// Archive (disabled=true) or restore (false) a source. Audited in the same
    /// transaction as the update via `DataSourceArchived`.
    pub async fn set_source_disabled(
        &self,
        tenant_id: &str,
        source_id: &str,
        disabled: bool,
        actor_id: &str,
    ) -> Result<(), StoreError> {
        // Ensure the source exists in this tenant.
        self.get_source(tenant_id, source_id).await?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE sources SET disabled=$3 WHERE id=$1 AND tenant_id=$2")
            .bind(source_id)
            .bind(tenant_id)
            .bind(disabled)
            .execute(&mut *tx)
            .await?;
        self.append_audit(
            &mut tx,
            tenant_id,
            actor_id,
            recon_audit::AuditPayload::DataSourceArchived {
                source_id: source_id.to_string(),
                disabled,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
```

- [ ] **Step 4: Run to verify pass**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cd backend && cargo clippy -p recon-store --all-targets -- -D warnings
git add crates/recon-store/src/sources.rs crates/recon-store/tests/
git commit -m "feat(store): set_source_disabled (archive/restore) emits DataSourceArchived in-tx"
```

---

## Task D4: Archive/restore API routes + ingest guard + list query param

**Files:**
- Modify: `backend/crates/recon-api/src/routes.rs`
- Modify: `backend/crates/recon-api/tests/ingest_api.rs`

**Interfaces:**
- Consumes: `Store::set_source_disabled` (D3), `list_sources(_, include_archived)` (D1).
- Produces: `POST /api/sources/:id/archive`, `POST /api/sources/:id/restore`, `GET /api/sources?includeArchived=1`, ingest→409 on disabled.

- [ ] **Step 1: Write the failing integration test**

Add to `ingest_api.rs`:
```rust
#[sqlx::test(migrations = "../../migrations")]
async fn archive_hides_source_and_blocks_ingest(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('tenant-acme','Acme','acme')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('user-ada','Ada','ada@acme.test',false)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('user-ada','tenant-acme','admin')").execute(&pool).await.unwrap();
    let (app, cfg) = recon_api::test_app(pool);
    let auth = format!("Bearer {}", token(&cfg, "tenant-acme"));

    let req = Request::builder().method("POST").uri("/api/sources").header("authorization", &auth)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"kind":"bank","name":"Arch Bank","currency":"GBP"}"#)).unwrap();
    let (_st, src) = json(&app, req).await;
    let id = src["id"].as_str().unwrap().to_string();

    // Archive it.
    let req = Request::builder().method("POST").uri(format!("/api/sources/{id}/archive"))
        .header("authorization", &auth).body(Body::empty()).unwrap();
    let (st, _v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "archive");

    // Default list excludes it; includeArchived shows it.
    let list = |uri: &str| { let auth = auth.clone(); let app = app.clone(); let uri = uri.to_string();
        async move { let req = Request::builder().method("GET").uri(uri).header("authorization", &auth).body(Body::empty()).unwrap(); json(&app, req).await } };
    let (_st, def) = list("/api/sources").await;
    assert_eq!(def.as_array().unwrap().len(), 0, "archived hidden by default: {def}");
    let (_st, inc) = list("/api/sources?includeArchived=1").await;
    assert_eq!(inc.as_array().unwrap().len(), 1, "includeArchived shows it");

    // Upload to an archived source -> 409.
    let boundary = "B";
    let body = multipart_body(boundary, &[("file", Some("x.csv"), "ref,date,amount\nA1,2026-05-01,10.00\n"), ("format", None, "csv"),
        ("mapping", None, r#"{"hasHeader":true,"delimiter":44,"externalRef":{"header":"ref"},"valueDate":{"header":"date"},"dateFormat":"%Y-%m-%d","amount":{"signed":{"column":{"header":"amount"},"debitWhenNegative":true}},"description":{"header":"ref"}}"#)]);
    let req = Request::builder().method("POST").uri(format!("/api/sources/{id}/ingest"))
        .header("authorization", &auth).header("content-type", format!("multipart/form-data; boundary={boundary}"))
        .body(Body::from(body)).unwrap();
    let (st, _v) = json(&app, req).await;
    assert_eq!(st, StatusCode::CONFLICT, "ingest to archived source is 409");

    // Restore it.
    let req = Request::builder().method("POST").uri(format!("/api/sources/{id}/restore"))
        .header("authorization", &auth).body(Body::empty()).unwrap();
    let (st, _v) = json(&app, req).await;
    assert_eq!(st, StatusCode::OK, "restore");
    let (_st, def2) = list("/api/sources").await;
    assert_eq!(def2.as_array().unwrap().len(), 1, "restored source visible again");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api archive_hides_source_and_blocks_ingest`
Expected: FAIL (archive route 404; ingest to disabled still succeeds).

- [ ] **Step 3: Add the ingest guard**

In `ingest_source`, right after `let source = s.store.get_source(&ctx.tenant_id, &source_id).await?;`, add:
```rust
    if source.disabled {
        return Err(ApiError::with_details(
            axum::http::StatusCode::CONFLICT,
            "conflict",
            "source is archived",
            json!({}),
        ));
    }
```

- [ ] **Step 4: Add the list query param**

Change the `list_sources` handler to accept a query flag. Add near the other `Deserialize` query structs in `dto.rs`:
```rust
#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SourcesQ {
    #[serde(default)]
    pub include_archived: Option<String>,
}
```
Update the handler in `routes.rs`:
```rust
async fn list_sources(
    State(s): State<AppState>,
    ctx: AuthContext,
    Query(q): Query<crate::dto::SourcesQ>,
) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;
    let include_archived = q.include_archived.as_deref() == Some("1")
        || q.include_archived.as_deref() == Some("true");
    Ok(Json(json!(s.store.list_sources(&ctx.tenant_id, include_archived).await?)))
}
```
(`Query` is already imported in routes.rs from earlier handlers.)

- [ ] **Step 5: Add archive/restore routes + handlers**

In `router(...)`, after the `/api/sources/:source_id/ingest` route, add:
```rust
        .route("/api/sources/:source_id/archive", post(archive_source))
        .route("/api/sources/:source_id/restore", post(restore_source))
```
Add the handlers near `patch_source`:
```rust
async fn archive_source(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(source_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;
    s.store.set_source_disabled(&ctx.tenant_id, &source_id, true, &ctx.user_id).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn restore_source(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(source_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;
    s.store.set_source_disabled(&ctx.tenant_id, &source_id, false, &ctx.user_id).await?;
    Ok(Json(json!({ "ok": true })))
}
```

- [ ] **Step 6: Run to verify pass**

Run: `cd backend && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api` — all PASS.
Then workspace: `cargo clippy --workspace --all-targets -- -D warnings && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --workspace` — all green.

- [ ] **Step 7: Commit**

```bash
cd backend && git add crates/recon-api/src/routes.rs crates/recon-api/src/dto.rs crates/recon-api/tests/ingest_api.rs
git commit -m "feat(api): archive/restore source routes + includeArchived list + 409 ingest guard"
```

---

## Task E1: Frontend data layer

**Files:**
- Modify: `web/lib/domain/types.ts`, `web/lib/api/client.ts`, `web/lib/api/http.ts`, `web/lib/api/mock.ts`

**Interfaces:**
- Produces: `Source.disabled`; `IngestFormat` +`"auto"`; `ApiClient.archiveSource`/`restoreSource`; `listSources(tenantId, includeArchived?)`; `ingestFile(..., dialect?, pdfProfile?)`.

- [ ] **Step 1: Zod + client types**

In `web/lib/domain/types.ts` `sourceSchema`, add after `pdfProfile: z.string().nullable().optional(),`:
```typescript
  disabled: z.boolean().optional().default(false),
```
In `web/lib/api/client.ts`:
- `IngestFormat` → add `| "auto"`.
- `ApiClient` interface — change `listSources` to `listSources(tenantId: string, includeArchived?: boolean): Promise<SourceListItem[]>;` and add:
```typescript
  archiveSource(tenantId: string, sourceId: string): Promise<void>;
  restoreSource(tenantId: string, sourceId: string): Promise<void>;
```
- `ingestFile` signature — add two optional params:
```typescript
  ingestFile(tenantId: string, sourceId: string, format: IngestFormat, file: File, mapping?: CsvMapping, dialect?: string, pdfProfile?: string): Promise<IngestResult>;
```

- [ ] **Step 2: HttpApiClient**

In `web/lib/api/http.ts`:
- `listSources`:
```typescript
  listSources(tenantId: string, includeArchived?: boolean): Promise<SourceListItem[]> {
    const q = includeArchived ? "?includeArchived=1" : "";
    return this.req(`/api/sources${q}`, tenantId);
  }
```
- Add archive/restore (mirror a POST action; `this.req` supports `{ method: "POST" }`):
```typescript
  async archiveSource(tenantId: string, sourceId: string): Promise<void> {
    await this.req(`/api/sources/${sourceId}/archive`, tenantId, { method: "POST" });
  }
  async restoreSource(tenantId: string, sourceId: string): Promise<void> {
    await this.req(`/api/sources/${sourceId}/restore`, tenantId, { method: "POST" });
  }
```
(If `this.req` cannot POST with no body, match the existing POST-style call in this file, e.g. how a reseed/assign action is issued.)
- `ingestFile` FormData — add the override params to the signature and append them:
```typescript
  async ingestFile(_tenantId: string, sourceId: string, format: IngestFormat, file: File, mapping?: CsvMapping, dialect?: string, pdfProfile?: string): Promise<IngestResult> {
```
Inside the `send` closure after `if (mapping) fd.append("mapping", JSON.stringify(mapping));`:
```typescript
    if (dialect) fd.append("dialect", dialect);
    if (pdfProfile) fd.append("pdfProfile", pdfProfile);
```

- [ ] **Step 3: MockApiClient**

In `web/lib/api/mock.ts`:
- `listSources` — add the param + filter:
```typescript
  async listSources(tenantId: string, includeArchived?: boolean): Promise<SourceListItem[]> {
    await this.delay();
    return this.state.sources
      .filter((s) => s.tenantId === tenantId && (includeArchived || !s.disabled))
      .map((s) => deepClone({ ...s, txnCount: this.state.transactions.filter((t) => t.sourceId === s.id).length }));
  }
```
- Add archive/restore:
```typescript
  async archiveSource(tenantId: string, sourceId: string): Promise<void> {
    await this.delay();
    const s = this.state.sources.find((x) => x.id === sourceId && x.tenantId === tenantId);
    if (s) s.disabled = true;
  }
  async restoreSource(tenantId: string, sourceId: string): Promise<void> {
    await this.delay();
    const s = this.state.sources.find((x) => x.id === sourceId && x.tenantId === tenantId);
    if (s) s.disabled = false;
  }
```
- `ingestFile` mock (if present) — add the two optional params to its signature so it type-matches the interface (no behavior change needed).
- `createSource` — set `disabled: false` on the created source object if the `Source` type now requires it.

- [ ] **Step 4: Typecheck + commit**

Run: `cd web && pnpm tsc --noEmit`
Expected: no errors. Fix any strict `Source` literal in fixtures/tests by adding `disabled: false`.
Run: `cd web && pnpm vitest run 2>&1 | tail -6` — all green.
```bash
cd web && git add lib/domain/types.ts lib/api/client.ts lib/api/http.ts lib/api/mock.ts
git commit -m "feat(web): Source.disabled + archive/restore + includeArchived + ingest override params"
```

---

## Task E2: Sources page — archive action + archived view

**Files:**
- Modify: `web/app/(app)/sources/page.tsx`, `web/lib/hooks/use-sources.ts`
- Modify/Create: `web/tests/` (a vitest for the archive action)

**Interfaces:**
- Consumes: `api.archiveSource`/`restoreSource`, `listSources(tenantId, includeArchived)` (E1).

- [ ] **Step 1: Hook — optional includeArchived**

In `web/lib/hooks/use-sources.ts`, thread an `includeArchived` flag into the query (key + fn). If the hook is `useSources(tenantId)`, change to `useSources(tenantId, includeArchived = false)` with:
```typescript
    queryKey: ["sources", tenantId, includeArchived],
    queryFn: () => api.listSources(tenantId, includeArchived),
```
(Match the file's actual structure; keep the default `false` so existing callers are unaffected.)

- [ ] **Step 2: Page — toggle + archive action + muted rows**

In `web/app/(app)/sources/page.tsx`:
- Add state: `const [showArchived, setShowArchived] = useState(false);` and pass it to the sources hook/query.
- Add an archive mutation (mirror the existing mutation style in the file / TanStack `useMutation`):
```typescript
  const archiveMutation = useMutation({
    mutationFn: (s: SourceListItem) => (s.disabled ? api.restoreSource(tenantId, s.id) : api.archiveSource(tenantId, s.id)),
    onSuccess: () => { void queryClient.invalidateQueries({ queryKey: ["sources", tenantId] }); },
  });
```
- Add a "Show archived" checkbox/toggle near the page header bound to `showArchived`.
- In the row-actions cell, add a button before/after Upload:
```tsx
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5 mr-1.5"
                onClick={() => archiveMutation.mutate(s)}
              >
                {s.disabled ? "Restore" : "Archive"}
              </Button>
```
- Render archived rows muted: on the `<TableRow>`, add `className={s.disabled ? "opacity-60" : undefined}` and a badge `{s.disabled && <Badge variant="secondary" className="text-xs">Archived</Badge>}` next to the name.

- [ ] **Step 3: Vitest**

Add `web/tests/sources-archive.test.tsx` (mirror the render harness of an existing sources/dialog test — QueryClientProvider + MockAuthProvider + ApiProvider):
```typescript
it("archives a source via the row action", async () => {
  const user = userEvent.setup();
  const base = new MockApiClient({ latencyMs: 0 });
  // seed one enabled source for tenant-acme via the mock's seed or createSource
  const spy = vi.fn(base.archiveSource.bind(base));
  const client: ApiClient = Object.assign(base, { archiveSource: spy });
  renderSourcesPage(client); // use the file's existing render helper pattern
  await user.click(await screen.findByRole("button", { name: /archive/i }));
  await waitFor(() => expect(spy).toHaveBeenCalled());
});
```
NOTE: adapt `renderSourcesPage`/seeding to how existing sources tests set up the page + mock data. If there is no existing page-render test, mirror `web/tests/edit-source-dialog.test.tsx`'s provider wrapper.

- [ ] **Step 4: Verify + commit**

Run: `cd web && pnpm tsc --noEmit && pnpm vitest run 2>&1 | tail -6` — green.
```bash
cd web && git add "app/(app)/sources/page.tsx" lib/hooks/use-sources.ts tests/
git commit -m "feat(web): archive/restore source row action + show-archived toggle + muted rows"
```

---

## Task E3: Upload dialog — auto-detect + per-upload override

**Files:**
- Modify: `web/components/app/upload-dialog.tsx`
- Modify: `web/tests/` (extend upload-dialog coverage if a test exists)

**Interfaces:**
- Consumes: `IngestFormat` +`"auto"`, `ingestFile(..., dialect?, pdfProfile?)` (E1).

- [ ] **Step 1: Add the Auto-detect format option**

In `upload-dialog.tsx` format Select, add as the FIRST item:
```tsx
            <SelectItem value="auto">Auto-detect</SelectItem>
```
When `format === "auto"`, render a one-line hint and hide the CSV mapping form (the `format === "csv"` guard already hides it for non-csv). Add near the other notices:
```tsx
        {format === "auto" && (
          <p className="text-sm text-muted-foreground">
            The format is detected from the file. CSV files must be uploaded with the explicit CSV format.
          </p>
        )}
```

- [ ] **Step 2: Add an optional per-upload override control**

Add local state near `const [format, setFormat] = useState<IngestFormat>("csv");`:
```tsx
  const [dialectOverride, setDialectOverride] = useState<string | "">("");
  const [pdfProfileOverride, setPdfProfileOverride] = useState<string | "">("");
```
When `format === "mt940" || format === "mt942"`, render a dialect override Select (empty = use source default):
```tsx
        {(format === "mt940" || format === "mt942") && (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="up-dialect-ovr">Dialect (override for this upload)</Label>
            <Select value={dialectOverride || "__default__"} onValueChange={(v) => setDialectOverride(v === "__default__" ? "" : v)}>
              <SelectTrigger id="up-dialect-ovr"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="__default__">Use source default{source.formatDialect ? ` (${source.formatDialect})` : ""}</SelectItem>
                <SelectItem value="generic">Generic</SelectItem>
                <SelectItem value="subfielded">Subfielded (DE/NL/BE)</SelectItem>
              </SelectContent>
            </Select>
          </div>
        )}
```
When `format === "pdf"`, render a PDF-profile override Select populated from `api.listPdfProfiles` (this component already has access to `api`/`tenantId`; add a `useQuery(["pdf-profiles", tenantId], () => api.listPdfProfiles(tenantId))` as in the Phase 8 dialogs):
```tsx
        {format === "pdf" && (
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="up-pdf-ovr">PDF profile (override for this upload)</Label>
            <Select value={pdfProfileOverride || "__default__"} onValueChange={(v) => setPdfProfileOverride(v === "__default__" ? "" : v)}>
              <SelectTrigger id="up-pdf-ovr"><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="__default__">Use source default{source.pdfProfile ? ` (${source.pdfProfile})` : ""}</SelectItem>
                {pdfProfiles.map((p) => (<SelectItem key={p} value={p}>{p}</SelectItem>))}
              </SelectContent>
            </Select>
          </div>
        )}
```

- [ ] **Step 3: Pass overrides to ingestFile**

In the upload mutation's `mutationFn`, change the `api.ingestFile(...)` call to forward the overrides (empty string → undefined):
```tsx
    return api.ingestFile(
      tenantId,
      source.id,
      format,
      file,
      format === "csv" ? buildMapping() : undefined,
      dialectOverride || undefined,
      pdfProfileOverride || undefined,
    );
```

- [ ] **Step 4: Verify + commit**

Run: `cd web && pnpm tsc --noEmit && pnpm vitest run 2>&1 | tail -6` — green. (Fix the existing upload-dialog test if it snapshots the format list — it now includes "Auto-detect".)
```bash
cd web && git add components/app/upload-dialog.tsx tests/
git commit -m "feat(web): upload dialog Auto-detect option + per-upload dialect/profile override"
```

---

## Task F1: Playwright E2E + README

**Files:**
- Modify: an existing E2E spec under `web/tests/e2e/`
- Modify: `web/README.md`

- [ ] **Step 1: Playwright archive step**

In the sources/ingestion E2E spec (grep `web/tests/e2e` for `sources`/`Archive`/upload), add a test:
```typescript
test("archives a source so it leaves the active list", async ({ page }) => {
  await signIn(page, "ada@acme.test", "Password123!"); // existing helper
  await page.goto("/sources");
  // create a uniquely-named source, then archive it
  // ... open New source, fill name "E2E Archive Me", currency GBP, save ...
  const row = page.getByRole("row", { name: /E2E Archive Me/i });
  await row.getByRole("button", { name: /archive/i }).click();
  await expect(page.getByRole("row", { name: /E2E Archive Me/i })).toHaveCount(0);
});
```
Adapt selectors/helpers to the existing spec's conventions.

- [ ] **Step 2: Run the E2E**

Run the project's E2E command (from `web/package.json`, e.g. `pnpm e2e`) with the Phase-9 backend running (rebuild+restart `recon-api` if the harness uses an already-running instance, per the deploy env). Confirm the new test + existing E2E pass. If the harness cannot run in this environment, report DONE_WITH_CONCERNS with the exact command, keeping the test committed.

- [ ] **Step 3: README**

In `web/README.md`:
- Formats table / prose: note **Auto-detect** ("choose Auto-detect to sniff the format; CSV must be explicit") and that MT94x/PDF uploads accept a per-upload dialect/profile override.
- Add a short **"Archiving a source"** subsection: Archive hides a source from the active list and blocks new uploads (409); Restore brings it back; use "Show archived" to view archived sources. Archiving is audited (`data.source.archived`).

- [ ] **Step 4: Commit**

```bash
git add web/tests/e2e web/README.md
git commit -m "test(e2e)+docs: archive-source E2E; README auto-detect, override, archiving"
```

---

## Final verification

- [ ] `cd backend && cargo clippy --workspace --all-targets -- -D warnings && DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --workspace` → all green (incl. matching proptests, audit chain, new api integration tests).
- [ ] `cd web && pnpm tsc --noEmit && pnpm vitest run` → all green.
- [ ] E2E spec (including the archive step) passes.
- [ ] Manual smoke on the running local deploy: create a source, archive it (leaves list, 409 on upload), restore it; upload a statement with `Auto-detect`; upload MT940 with a subfielded override on a generic source; run a reconciliation and confirm counterparty-matched pairs score as `Matched`.
- [ ] Update memory (`recon-ui-slice-status.md`) with Phase 9 once merged.

## Self-review notes

- **Spec coverage:** counterparty scoring + config v1.1 (A1); auto-detect helper (B1) + route (B2); per-upload override (C1); archive schema/threading (D1), audit kind (D2), store method + guard (D3), routes + list param + 409 (D4); frontend data layer (E1), sources page archive (E2), upload dialog auto+override (E3); E2E + docs (F1). Every spec section maps to a task.
- **Type consistency:** `disabled: bool`/`disabled` (Rust)/`disabled` (TS); `set_source_disabled`; `DataSourceArchived` / `data.source.archived`; `detect_format -> Option<&'static str>`; `list_sources(tenant, include_archived)`; `ingestFile(..., dialect?, pdfProfile?)`; `IngestFormat` +`"auto"`. Names consistent across tasks.
- **Audit safety:** exactly one new additive `AuditKind`; no existing variant mutated; golden vector untouched.
- **Determinism:** counterparty term degrades to the exact legacy formula when identifiers are absent, so existing score/engine tests and all three proptests pass unchanged; the new branch is covered by targeted unit tests.
