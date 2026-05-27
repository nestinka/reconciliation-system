# Phase 7 — Phase 5/6 Polish Bundle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Land the five deferred items from Phases 5/6 in a single PR: counterparty BIC/account columns + MT942 parser + PATCH `/sources/:id` + concurrent-appender stress test + `audit/page.tsx` split.

**Architecture:** One additive migration; counterparty plumbing through `ParsedTxn` + `CanonicalTransaction` + parsers + store + UI; MT942 reuses shared MT940 helpers via a new `mt94x_shared` module; PATCH endpoint is admin-only and audited as `source.updated`; concurrent-appender test proves the audit chain holds under contention; audit page split is a pure functional-equivalence refactor whose acceptance gate is the existing tests.

**Tech Stack:** Rust 1.95 + sqlx (Postgres 16) + axum + Next.js 16 (Turbopack) + Base UI + Tailwind + react-hook-form + zod + vitest + Playwright.

**Branch:** `feat/phase7-polish-bundle` (already created off master).

**Working directory:** `/home/nestinka/assistant/reconciliation-system`. Backend commands run from `backend/`; frontend commands run from `web/`.

---

## Task 1: Migration 0006 — counterparty_bic + counterparty_account on transactions

**Files:**
- Create: `backend/migrations/0006_transactions_counterparty.sql`
- Create: `backend/crates/recon-store/tests/counterparty_schema.rs`

- [ ] **Step 1: Write the failing schema test**

Create `backend/crates/recon-store/tests/counterparty_schema.rs`:

```rust
//! Migration 0006 schema invariants for canonical_transactions.counterparty_*.

use sqlx::{PgPool, Row};

async fn pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for store tests");
    PgPool::connect(&url).await.expect("connect")
}

async fn ensure_tenant_and_source(p: &PgPool) -> (String, String) {
    let tenant_id = format!("tenant-test-{}", uuid::Uuid::new_v4());
    let source_id = format!("src-test-{}", uuid::Uuid::new_v4());
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,$1,$1)")
        .bind(&tenant_id)
        .execute(p)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ($1,$2,'bank','t','EUR')")
        .bind(&source_id)
        .bind(&tenant_id)
        .execute(p)
        .await
        .unwrap();
    (tenant_id, source_id)
}

async fn try_insert_bic(p: &PgPool, bic: Option<&str>) -> Result<(), sqlx::Error> {
    let (tenant_id, source_id) = ensure_tenant_and_source(p).await;
    let txn_id = format!("txn-{}", uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO canonical_transactions(\
            id, tenant_id, source_id, external_ref, value_date, posted_at, \
            amount_minor, currency, direction, counterparty, description, \
            counterparty_bic, counterparty_account) \
         VALUES ($1,$2,$3,$4,'2026-01-01'::date,'2026-01-01T00:00:00Z'::timestamptz,\
                 100,'EUR','credit',NULL,'',$5,NULL)",
    )
    .bind(&txn_id)
    .bind(&tenant_id)
    .bind(&source_id)
    .bind(format!("ref-{}", uuid::Uuid::new_v4()))
    .bind(bic)
    .execute(p)
    .await?;
    Ok(())
}

#[tokio::test]
async fn valid_8_char_bic_accepted() {
    let p = pool().await;
    try_insert_bic(&p, Some("DEUTDEFF")).await.unwrap();
}

#[tokio::test]
async fn valid_11_char_bic_accepted() {
    let p = pool().await;
    try_insert_bic(&p, Some("DEUTDEFF500")).await.unwrap();
}

#[tokio::test]
async fn null_bic_accepted() {
    let p = pool().await;
    try_insert_bic(&p, None).await.unwrap();
}

#[tokio::test]
async fn lowercase_bic_rejected() {
    let p = pool().await;
    let err = try_insert_bic(&p, Some("deutdeff")).await.unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("chk_counterparty_bic_shape"), "got: {msg}");
}

#[tokio::test]
async fn wrong_length_bic_rejected() {
    let p = pool().await;
    let err = try_insert_bic(&p, Some("DEUTDEF")).await.unwrap_err(); // 7 chars
    let msg = format!("{err}");
    assert!(msg.contains("chk_counterparty_bic_shape"), "got: {msg}");
}

#[tokio::test]
async fn account_round_trips() {
    let p = pool().await;
    let (tenant_id, source_id) = ensure_tenant_and_source(&p).await;
    let txn_id = format!("txn-{}", uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO canonical_transactions(\
            id, tenant_id, source_id, external_ref, value_date, posted_at, \
            amount_minor, currency, direction, counterparty, description, \
            counterparty_bic, counterparty_account) \
         VALUES ($1,$2,$3,'r1','2026-01-01'::date,'2026-01-01T00:00:00Z'::timestamptz,\
                 100,'EUR','credit',NULL,'','DEUTDEFF','DE89370400440532013000')",
    )
    .bind(&txn_id)
    .bind(&tenant_id)
    .bind(&source_id)
    .execute(&p)
    .await
    .unwrap();
    let row = sqlx::query("SELECT counterparty_bic, counterparty_account FROM canonical_transactions WHERE id=$1")
        .bind(&txn_id)
        .fetch_one(&p)
        .await
        .unwrap();
    let bic: Option<String> = row.try_get("counterparty_bic").unwrap();
    let acc: Option<String> = row.try_get("counterparty_account").unwrap();
    assert_eq!(bic.as_deref(), Some("DEUTDEFF"));
    assert_eq!(acc.as_deref(), Some("DE89370400440532013000"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd backend
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store --test counterparty_schema 2>&1 | tail -30
```

Expected: all 6 tests FAIL with errors like `column "counterparty_bic" of relation "canonical_transactions" does not exist`.

- [ ] **Step 3: Write the migration**

Create `backend/migrations/0006_transactions_counterparty.sql`:

```sql
-- Phase 7 — Add counterparty BIC + account to canonical_transactions.
-- Purely additive: two nullable columns + one shape CHECK on BIC.
-- Postgres adds nullable columns without table rewrite (metadata-only).

ALTER TABLE canonical_transactions
    ADD COLUMN counterparty_bic     TEXT NULL,
    ADD COLUMN counterparty_account TEXT NULL;

ALTER TABLE canonical_transactions
    ADD CONSTRAINT chk_counterparty_bic_shape
    CHECK (
        counterparty_bic IS NULL
        OR counterparty_bic ~ '^[A-Z0-9]{8}([A-Z0-9]{3})?$'
    );
```

- [ ] **Step 4: Apply migration and re-run test**

The Rust API binary runs sqlx migrations on startup. Restart it to apply 0006:

```bash
cd /home/nestinka/assistant/reconciliation-system/backend
# Kill any running recon-api on :8080 (the dev one started earlier in the session).
pkill -f 'target/release/recon-api' 2>/dev/null
DATABASE_URL=postgres://recon:recon@localhost:5432/recon ./target/release/recon-api seed 2>&1 | tail -5
```

Expected last line: `seed complete` (after `applied migration 0006`-style log lines).

Then re-run:

```bash
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store --test counterparty_schema 2>&1 | tail -30
```

Expected: `test result: ok. 6 passed; 0 failed`.

- [ ] **Step 5: Restart the API**

```bash
cd /home/nestinka/assistant/reconciliation-system/backend
RECON_DEV=1 DATABASE_URL=postgres://recon:recon@localhost:5432/recon \
    SMTP_HOST=localhost SMTP_PORT=1025 \
    nohup ./target/release/recon-api > /tmp/recon-api.log 2>&1 &
sleep 1 && curl -sS http://localhost:8080/healthz
```

Expected: `ok`.

- [ ] **Step 6: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add backend/migrations/0006_transactions_counterparty.sql \
        backend/crates/recon-store/tests/counterparty_schema.rs
git commit -m "chore(store): migration 0006 — counterparty_bic + counterparty_account on canonical_transactions"
```

---

## Task 2: CanonicalTransaction + ParsedTxn + store-layer plumbing

**Files:**
- Modify: `backend/crates/recon-domain/src/types.rs` (around the `CanonicalTransaction` struct)
- Modify: `backend/crates/recon-ingest/src/lib.rs` (around `ParsedTxn`)
- Modify: `backend/crates/recon-store/src/sources.rs` (around `ingest_transactions`)
- Modify: `backend/crates/recon-store/src/rows.rs` (the `TransactionRow` struct + From impl)
- Modify: `backend/crates/recon-api/src/routes.rs` (the `ingest_source` map step)

- [ ] **Step 1: Write the failing type-level test**

Add to the bottom of `backend/crates/recon-domain/src/types.rs` (inside the existing `mod tests`):

```rust
    #[test]
    fn canonical_transaction_has_optional_counterparty_bic_and_account() {
        let t = CanonicalTransaction {
            id: "txn-x".into(),
            tenant_id: "t".into(),
            source_id: "s".into(),
            external_ref: "r".into(),
            value_date: "2026-01-01".into(),
            posted_at: "2026-01-01T00:00:00Z".into(),
            amount_minor: 100,
            currency: "EUR".into(),
            direction: Direction::Credit,
            counterparty: None,
            description: "".into(),
            counterparty_bic: Some("DEUTDEFF".into()),
            counterparty_account: Some("DE89370400440532013000".into()),
        };
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["counterpartyBic"], "DEUTDEFF");
        assert_eq!(v["counterpartyAccount"], "DE89370400440532013000");
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd backend
cargo test -p recon-domain canonical_transaction_has_optional_counterparty_bic_and_account 2>&1 | tail -10
```

Expected: compile error — `missing field counterparty_bic in initializer of CanonicalTransaction` (or similar).

- [ ] **Step 3: Add the two fields to CanonicalTransaction**

In `backend/crates/recon-domain/src/types.rs`, modify the `CanonicalTransaction` struct (currently lines ~111–126) to:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalTransaction {
    pub id: String,
    pub tenant_id: String,
    pub source_id: String,
    pub external_ref: String,
    pub value_date: String, // "YYYY-MM-DD"
    pub posted_at: String,  // RFC3339
    pub amount_minor: i64,
    pub currency: String,
    pub direction: Direction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty: Option<String>,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty_bic: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counterparty_account: Option<String>,
}
```

- [ ] **Step 4: Add the two fields to ParsedTxn**

In `backend/crates/recon-ingest/src/lib.rs`, change the `ParsedTxn` struct (currently lines 11–21) to:

```rust
/// A parsed transaction draft. No id / tenant / source yet — the API assigns
/// those when mapping to a `CanonicalTransaction`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTxn {
    pub external_ref: String,
    pub value_date: String,
    pub posted_at: Option<String>,
    pub amount_minor: i64,
    pub currency: Option<String>,
    pub direction: Direction,
    pub counterparty: Option<String>,
    pub description: String,
    pub counterparty_bic: Option<String>,
    pub counterparty_account: Option<String>,
}
```

- [ ] **Step 5: Update the ingest_source mapper**

In `backend/crates/recon-api/src/routes.rs`, modify the `Map ParsedTxn -> CanonicalTransaction` block (currently lines ~333–349) so the mapping passes through the two new fields:

```rust
    // Map ParsedTxn -> CanonicalTransaction (assign ids + defaults).
    let txns: Vec<recon_domain::CanonicalTransaction> = parsed
        .into_iter()
        .map(|p| recon_domain::CanonicalTransaction {
            id: format!("txn-{}", uuid::Uuid::new_v4()),
            tenant_id: ctx.tenant_id.clone(),
            source_id: source_id.clone(),
            external_ref: p.external_ref,
            value_date: p.value_date.clone(),
            posted_at: p.posted_at.unwrap_or_else(|| format!("{}T00:00:00Z", p.value_date)),
            amount_minor: p.amount_minor,
            currency: p.currency.unwrap_or_else(|| source.currency.clone()),
            direction: p.direction,
            counterparty: p.counterparty,
            description: p.description,
            counterparty_bic: p.counterparty_bic,
            counterparty_account: p.counterparty_account,
        })
        .collect();
```

- [ ] **Step 6: Update the INSERT and TransactionRow**

In `backend/crates/recon-store/src/sources.rs`, modify the `INSERT INTO canonical_transactions` statement inside `ingest_transactions` (currently lines ~175–197) to include the two new columns:

```rust
        for t in txns {
            sqlx::query(
                "INSERT INTO canonical_transactions(id,tenant_id,source_id,external_ref,value_date,posted_at,amount_minor,currency,direction,counterparty,description,counterparty_bic,counterparty_account) \
                 VALUES ($1,$2,$3,$4,$5::date,$6::timestamptz,$7,$8,$9,$10,$11,$12,$13)",
            )
            .bind(&t.id)
            .bind(tenant_id)
            .bind(source_id)
            .bind(&t.external_ref)
            .bind(&t.value_date)
            .bind(&t.posted_at)
            .bind(t.amount_minor)
            .bind(&t.currency)
            .bind(direction_str(t.direction))
            .bind(&t.counterparty)
            .bind(&t.description)
            .bind(&t.counterparty_bic)
            .bind(&t.counterparty_account)
            .execute(&mut *tx)
            .await
            .map_err(|e| match e {
                sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => {
                    StoreError::DuplicateRefs(vec![t.external_ref.clone()])
                }
                other => StoreError::Db(other),
            })?;
        }
```

In `backend/crates/recon-store/src/rows.rs`, find the `TransactionRow` struct (search for `struct TransactionRow`) and:

- Add `pub counterparty_bic: Option<String>` and `pub counterparty_account: Option<String>` to the struct (in the same shape as the existing `counterparty: Option<String>`).
- Add those two fields to the `From<TransactionRow> for CanonicalTransaction` impl (mirror the existing `counterparty` mapping).
- Wherever a SELECT exists that lists transaction columns (search `rows.rs` for `external_ref` SELECTs), append `, counterparty_bic, counterparty_account` to the column list.

- [ ] **Step 7: Find all transaction SELECTs and extend them**

```bash
cd /home/nestinka/assistant/reconciliation-system
rg -n 'FROM canonical_transactions|canonical_transactions WHERE|canonical_transactions\.' backend/crates/recon-store/src 2>&1 | head -40
```

For every SELECT that returns transaction rows (currently in `rows.rs`, `transactions.rs`, etc.), append `, counterparty_bic, counterparty_account` to the column list **in the same order they appear in the struct**. Order matters because sqlx maps positionally when the FromRow derive is column-name-based — verify your struct field names exactly match the SQL aliases.

- [ ] **Step 8: Run the domain test**

```bash
cd backend
cargo test -p recon-domain canonical_transaction_has_optional_counterparty_bic_and_account 2>&1 | tail -10
```

Expected: PASS.

- [ ] **Step 9: Run the full backend test suite**

```bash
cd backend
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --workspace 2>&1 | tail -30
```

Expected: all existing tests still pass + the new domain test passes. If any test fails, fix the SELECT column list / struct field mismatch.

- [ ] **Step 10: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add backend/crates/recon-domain/src/types.rs \
        backend/crates/recon-ingest/src/lib.rs \
        backend/crates/recon-store/src/sources.rs \
        backend/crates/recon-store/src/rows.rs \
        backend/crates/recon-store/src/transactions.rs \
        backend/crates/recon-api/src/routes.rs
git commit -m "feat(domain): CanonicalTransaction + ParsedTxn gain counterparty_bic + counterparty_account; threaded through store and ingest mapper"
```

---

## Task 3: CSV parser counterparty fields

**Files:**
- Modify: `backend/crates/recon-ingest/src/csv.rs`

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block in `backend/crates/recon-ingest/src/csv.rs`:

```rust
    #[test]
    fn counterparty_bic_and_account_columns_extracted_and_bic_uppercased() {
        let csv = "ref,date,amount,bic,acc\n\
                   R1,2026-01-01,100.00,deutdeff,DE89370400440532013000\n\
                   R2,2026-01-02,200.00,,\n";
        let mapping = CsvMapping {
            external_ref_col: 0,
            value_date_col: 1,
            amount_col: Some(2),
            debit_col: None,
            credit_col: None,
            counterparty_col: None,
            description_col: None,
            currency_col: None,
            direction_col: None,
            has_header: true,
            counterparty_bic_col: Some(3),
            counterparty_account_col: Some(4),
        };
        let txns = CsvParser::new(mapping).parse(csv.as_bytes()).unwrap();
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].counterparty_bic.as_deref(), Some("DEUTDEFF")); // uppercased
        assert_eq!(
            txns[0].counterparty_account.as_deref(),
            Some("DE89370400440532013000")
        );
        // Row 2 has empty values -> None.
        assert!(txns[1].counterparty_bic.is_none());
        assert!(txns[1].counterparty_account.is_none());
    }

    #[test]
    fn counterparty_columns_default_none_when_mapping_omits_them() {
        let csv = "ref,date,amount\nR1,2026-01-01,100.00\n";
        let mapping = CsvMapping {
            external_ref_col: 0,
            value_date_col: 1,
            amount_col: Some(2),
            debit_col: None,
            credit_col: None,
            counterparty_col: None,
            description_col: None,
            currency_col: None,
            direction_col: None,
            has_header: true,
            counterparty_bic_col: None,
            counterparty_account_col: None,
        };
        let txns = CsvParser::new(mapping).parse(csv.as_bytes()).unwrap();
        assert!(txns[0].counterparty_bic.is_none());
        assert!(txns[0].counterparty_account.is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd backend
cargo test -p recon-ingest --lib csv:: 2>&1 | tail -10
```

Expected: compile error — `no field counterparty_bic_col on CsvMapping`.

- [ ] **Step 3: Add the two fields to CsvMapping**

In `backend/crates/recon-ingest/src/csv.rs`, find the `CsvMapping` struct (it derives `Deserialize`) and add at the bottom:

```rust
    #[serde(default)]
    pub counterparty_bic_col: Option<usize>,
    #[serde(default)]
    pub counterparty_account_col: Option<usize>,
```

- [ ] **Step 4: Populate the fields in the parse loop**

Find the row-building loop in `csv.rs` (the place where each `ParsedTxn` is constructed). For each row, add before the `ParsedTxn { ... }` literal:

```rust
            let counterparty_bic = mapping
                .counterparty_bic_col
                .and_then(|i| row.get(i))
                .map(|s| s.trim().to_uppercase())
                .filter(|s| !s.is_empty());
            let counterparty_account = mapping
                .counterparty_account_col
                .and_then(|i| row.get(i))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
```

Then include both fields in the `ParsedTxn { ... }` initializer:

```rust
            ParsedTxn {
                // ... existing fields ...
                counterparty_bic,
                counterparty_account,
            }
```

(The exact `row.get(i)` API depends on what type `row` is in the existing parser — it's likely a `csv::StringRecord` where `.get(i)` returns `Option<&str>`. Match the existing extractor pattern in the file.)

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd backend
cargo test -p recon-ingest --lib csv:: 2>&1 | tail -10
```

Expected: all CSV tests pass (existing + 2 new).

- [ ] **Step 6: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add backend/crates/recon-ingest/src/csv.rs
git commit -m "feat(ingest/csv): optional counterparty_bic_col + counterparty_account_col in mapping; BIC uppercased on extract"
```

---

## Task 4: CAMT.053 parser counterparty fields

**Files:**
- Modify: `backend/crates/recon-ingest/src/camt053.rs`

- [ ] **Step 1: Read the existing parser**

```bash
cat backend/crates/recon-ingest/src/camt053.rs
```

Identify where each `<Ntry>` (entry) is converted to a `ParsedTxn`. Note the `quick-xml` reader style. The counterparty information lives at:

```
<Ntry>
  <NtryDtls>
    <TxDtls>
      <RltdPties>
        <Cdtr>             <- for credits (CRDT entry)
          <Nm>...</Nm>
        </Cdtr>
        <CdtrAcct>
          <Id>
            <IBAN>...</IBAN>     <- or <Othr><Id>...</Id></Othr>
          </Id>
        </CdtrAcct>
        <Dbtr>             <- for debits (DBIT entry)
          <Nm>...</Nm>
        </Dbtr>
        <DbtrAcct>
          <Id>
            <IBAN>...</IBAN>
          </Id>
        </DbtrAcct>
      </RltdPties>
      <RltdAgts>
        <CdtrAgt>
          <FinInstnId>
            <BIC>...</BIC>       <- or <BICFI>
          </FinInstnId>
        </CdtrAgt>
        <DbtrAgt>
          <FinInstnId>
            <BIC>...</BIC>
          </FinInstnId>
        </DbtrAgt>
      </RltdAgts>
    </TxDtls>
  </NtryDtls>
</Ntry>
```

- [ ] **Step 2: Write the failing test**

Add to the existing `mod tests` in `backend/crates/recon-ingest/src/camt053.rs`:

```rust
    #[test]
    fn credit_entry_extracts_counterparty_bic_from_cdtr_agt_and_account_from_cdtr_acct() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
 <BkToCstmrStmt>
  <Stmt>
   <Id>S1</Id>
   <Acct><Id><IBAN>DE00000000000000000001</IBAN></Id></Acct>
   <Ntry>
    <Amt Ccy="EUR">100.00</Amt>
    <CdtDbtInd>CRDT</CdtDbtInd>
    <BookgDt><Dt>2026-01-01</Dt></BookgDt>
    <ValDt><Dt>2026-01-01</Dt></ValDt>
    <AcctSvcrRef>R1</AcctSvcrRef>
    <NtryDtls>
     <TxDtls>
      <Refs><EndToEndId>R1</EndToEndId></Refs>
      <RltdPties>
       <Cdtr><Nm>Receiver</Nm></Cdtr>
       <CdtrAcct><Id><IBAN>DE89370400440532013000</IBAN></Id></CdtrAcct>
      </RltdPties>
      <RltdAgts>
       <CdtrAgt><FinInstnId><BIC>DEUTDEFF</BIC></FinInstnId></CdtrAgt>
      </RltdAgts>
     </TxDtls>
    </NtryDtls>
   </Ntry>
  </Stmt>
 </BkToCstmrStmt>
</Document>"#;
        let txns = Camt053Parser.parse(xml.as_bytes()).unwrap();
        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].counterparty_bic.as_deref(), Some("DEUTDEFF"));
        assert_eq!(
            txns[0].counterparty_account.as_deref(),
            Some("DE89370400440532013000")
        );
    }

    #[test]
    fn debit_entry_extracts_counterparty_from_dbtr_branches() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
 <BkToCstmrStmt><Stmt>
  <Id>S1</Id>
  <Acct><Id><IBAN>DE00000000000000000001</IBAN></Id></Acct>
  <Ntry>
   <Amt Ccy="EUR">50.00</Amt>
   <CdtDbtInd>DBIT</CdtDbtInd>
   <BookgDt><Dt>2026-01-01</Dt></BookgDt>
   <ValDt><Dt>2026-01-01</Dt></ValDt>
   <AcctSvcrRef>R2</AcctSvcrRef>
   <NtryDtls><TxDtls>
    <Refs><EndToEndId>R2</EndToEndId></Refs>
    <RltdPties>
     <Dbtr><Nm>Payer</Nm></Dbtr>
     <DbtrAcct><Id><IBAN>FR1420041010050500013M02606</IBAN></Id></DbtrAcct>
    </RltdPties>
    <RltdAgts>
     <DbtrAgt><FinInstnId><BIC>BNPAFRPP</BIC></FinInstnId></DbtrAgt>
    </RltdAgts>
   </TxDtls></NtryDtls>
  </Ntry>
 </Stmt></BkToCstmrStmt></Document>"#;
        let txns = Camt053Parser.parse(xml.as_bytes()).unwrap();
        assert_eq!(txns[0].counterparty_bic.as_deref(), Some("BNPAFRPP"));
        assert_eq!(
            txns[0].counterparty_account.as_deref(),
            Some("FR1420041010050500013M02606")
        );
    }

    #[test]
    fn missing_rltd_pties_leaves_counterparty_fields_none() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
 <BkToCstmrStmt><Stmt>
  <Id>S1</Id>
  <Acct><Id><IBAN>DE00000000000000000001</IBAN></Id></Acct>
  <Ntry>
   <Amt Ccy="EUR">10.00</Amt>
   <CdtDbtInd>CRDT</CdtDbtInd>
   <BookgDt><Dt>2026-01-01</Dt></BookgDt>
   <ValDt><Dt>2026-01-01</Dt></ValDt>
   <AcctSvcrRef>R3</AcctSvcrRef>
   <NtryDtls><TxDtls><Refs><EndToEndId>R3</EndToEndId></Refs></TxDtls></NtryDtls>
  </Ntry>
 </Stmt></BkToCstmrStmt></Document>"#;
        let txns = Camt053Parser.parse(xml.as_bytes()).unwrap();
        assert!(txns[0].counterparty_bic.is_none());
        assert!(txns[0].counterparty_account.is_none());
    }

    #[test]
    fn non_iban_account_via_othr_id_is_picked_up() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="urn:iso:std:iso:20022:tech:xsd:camt.053.001.08">
 <BkToCstmrStmt><Stmt>
  <Id>S1</Id>
  <Acct><Id><IBAN>DE00000000000000000001</IBAN></Id></Acct>
  <Ntry>
   <Amt Ccy="USD">10.00</Amt>
   <CdtDbtInd>CRDT</CdtDbtInd>
   <BookgDt><Dt>2026-01-01</Dt></BookgDt>
   <ValDt><Dt>2026-01-01</Dt></ValDt>
   <AcctSvcrRef>R4</AcctSvcrRef>
   <NtryDtls><TxDtls>
    <Refs><EndToEndId>R4</EndToEndId></Refs>
    <RltdPties>
     <Cdtr><Nm>US Vendor</Nm></Cdtr>
     <CdtrAcct><Id><Othr><Id>1234567890</Id></Othr></Id></CdtrAcct>
    </RltdPties>
   </TxDtls></NtryDtls>
  </Ntry>
 </Stmt></BkToCstmrStmt></Document>"#;
        let txns = Camt053Parser.parse(xml.as_bytes()).unwrap();
        assert_eq!(txns[0].counterparty_account.as_deref(), Some("1234567890"));
        assert!(txns[0].counterparty_bic.is_none());
    }
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd backend
cargo test -p recon-ingest --lib camt053:: 2>&1 | tail -20
```

Expected: 4 new tests FAIL (counterparty fields are `None`).

- [ ] **Step 4: Extend the parser**

The exact change depends on how `camt053.rs` walks the XML; the pattern below assumes a state-machine over `quick_xml::events::Event::Start/End/Text`. Add three tracking variables alongside whatever exists for `description`/`counterparty`:

```rust
let mut cpty_bic: Option<String> = None;
let mut cpty_account: Option<String> = None;
let mut in_rltd_pties = false;
let mut in_cdtr_acct = false;
let mut in_dbtr_acct = false;
let mut in_cdtr_agt = false;
let mut in_dbtr_agt = false;
let mut acct_id_capture = false;
let mut bic_capture = false;
```

In the `Start` branch, when seeing `RltdPties` set `in_rltd_pties = true`; for `CdtrAcct`/`DbtrAcct` set the corresponding flag; for `CdtrAgt`/`DbtrAgt` likewise; for `IBAN` or `Othr` set `acct_id_capture = true` (only when the enclosing `*Acct` flag is on); for `BIC` or `BICFI` set `bic_capture = true` (only when the enclosing `*Agt` flag is on).

In the `Text` branch:

```rust
if acct_id_capture && cpty_account.is_none() {
    cpty_account = Some(t.unescape().unwrap_or_default().trim().to_string()).filter(|s| !s.is_empty());
}
if bic_capture && cpty_bic.is_none() {
    let raw = t.unescape().unwrap_or_default().trim().to_uppercase();
    if !raw.is_empty() { cpty_bic = Some(raw); }
}
```

In the `End` branch, reset the per-element flag (`acct_id_capture = false` after `</IBAN>` / `</Id>` (for `<Othr><Id>`), `bic_capture = false` after `</BIC>` / `</BICFI>`).

When the entry ends (`</Ntry>`), include the captured values in the emitted `ParsedTxn`:

```rust
ParsedTxn {
    // ... existing fields ...
    counterparty_bic: cpty_bic.take(),
    counterparty_account: cpty_account.take(),
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd backend
cargo test -p recon-ingest --lib camt053:: 2>&1 | tail -10
```

Expected: all CAMT.053 tests pass.

- [ ] **Step 6: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add backend/crates/recon-ingest/src/camt053.rs
git commit -m "feat(ingest/camt053): extract counterparty BIC + account from <RltdPties>/<RltdAgts>"
```

---

## Task 5: MT940 subfielded counterparty fields

**Files:**
- Modify: `backend/crates/recon-ingest/src/mt940.rs`
- Modify: `backend/crates/recon-ingest/tests/fixtures/mt940-subfielded.sta`

- [ ] **Step 1: Update the subfielded fixture to include ?32/?33**

Inspect the current fixture:

```bash
cat backend/crates/recon-ingest/tests/fixtures/mt940-subfielded.sta
```

Rewrite it so its `:86:` line includes `?32` (account) and `?33` (BIC) subfields. Replace the file contents with:

```
:20:REF12345
:25:DE89370400440532013000
:28C:1/1
:60F:C260101EUR500,00
:61:260102C100,00NTRFCUSTREF-1//BNKREF-A
:86:?00FAKTURA?20Invoice payment?21INV-12345?32DE89370400440532013000?33DEUTDEFF
:62F:C260102EUR600,00
```

- [ ] **Step 2: Write the failing test**

In `backend/crates/recon-ingest/src/mt940.rs`, **modify** the existing `subfielded_86_extracts_counterparty` test and add a new BIC-and-account test:

```rust
    #[test]
    fn subfielded_86_extracts_counterparty_bic_and_account() {
        let bytes = load("mt940-subfielded.sta");
        let txns = Mt940Parser {
            dialect: Mt940Dialect::Subfielded,
        }
        .parse(&bytes)
        .unwrap();
        assert_eq!(txns.len(), 1);
        let t = &txns[0];
        assert_eq!(
            t.counterparty_account.as_deref(),
            Some("DE89370400440532013000")
        );
        assert_eq!(t.counterparty_bic.as_deref(), Some("DEUTDEFF"));
        assert!(t.description.contains("Invoice payment"));
        assert!(t.description.contains("INV-12345"));
    }

    #[test]
    fn generic_86_does_not_populate_counterparty_fields() {
        let bytes = load("mt940-subfielded.sta");
        let txns = Mt940Parser {
            dialect: Mt940Dialect::Generic,
        }
        .parse(&bytes)
        .unwrap();
        assert!(txns[0].counterparty_bic.is_none());
        assert!(txns[0].counterparty_account.is_none());
    }
```

Then **delete** the older `subfielded_86_extracts_counterparty` test (the one asserting the old combined `counterparty` field contained `"Acme Supplier Ltd London Branch"`) — its expectation is obsolete now that `?32`/`?33` no longer feed the combined blob.

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd backend
cargo test -p recon-ingest --lib mt940:: 2>&1 | tail -20
```

Expected: new tests FAIL (the parser doesn't populate `counterparty_bic`/`counterparty_account` yet).

- [ ] **Step 4: Update parse_subfielded_86 to extract BIC and account**

Replace the existing `parse_subfielded_86` function in `mt940.rs` with:

```rust
/// Parsed result of a Subfielded `:86:` field.
struct SubfieldedInfo {
    description: String,
    counterparty: Option<String>,         // name blob (legacy, from ?32/?33 in DE-name convention)
    counterparty_bic: Option<String>,     // dedicated BIC field (?33 here is BIC by Phase 7 spec)
    counterparty_account: Option<String>, // ?32 here is account by Phase 7 spec
}

fn parse_subfielded_86(raw: &str) -> SubfieldedInfo {
    let mut desc_parts: Vec<String> = Vec::new();
    let mut cpty_bic: Option<String> = None;
    let mut cpty_account: Option<String> = None;
    let mut prefix = String::new();
    let mut chunks = raw.split('?');
    if let Some(first) = chunks.next() {
        if !first.is_empty() {
            prefix.push_str(first);
        }
    }
    if !prefix.is_empty() {
        desc_parts.push(prefix);
    }
    for chunk in chunks {
        if chunk.len() < 2 {
            continue;
        }
        let code = &chunk[..2];
        let val = &chunk[2..];
        match code {
            "20" | "21" | "22" | "23" | "24" | "25" | "26" | "27" | "28" | "29" => {
                desc_parts.push(val.to_string());
            }
            "32" => {
                if cpty_account.is_none() {
                    let v = val.trim();
                    if !v.is_empty() { cpty_account = Some(v.to_string()); }
                }
            }
            "33" => {
                if cpty_bic.is_none() {
                    let v = val.trim().to_uppercase();
                    if !v.is_empty() { cpty_bic = Some(v); }
                }
            }
            "30" | "31" => {
                // counterparty bank BLZ + account — append to description for transparency.
                desc_parts.push(format!("[{code}:{val}]"));
            }
            _ => {
                desc_parts.push(format!("[?{code}:{val}]"));
            }
        }
    }
    let description = desc_parts.join(" ").trim().to_string();
    SubfieldedInfo {
        description,
        counterparty: None, // ?32/?33 are now structured fields, not the name blob
        counterparty_bic: cpty_bic,
        counterparty_account: cpty_account,
    }
}
```

Then update `build_txn` to consume the new struct shape:

```rust
fn build_txn(
    p: &Mt61,
    info_lines: &[String],
    dialect: Mt940Dialect,
) -> Result<ParsedTxn, (&'static str, String)> {
    let external_ref = p
        .customer_ref
        .clone()
        .or_else(|| p.bank_ref.clone())
        .ok_or((
            "external_ref",
            "no customer-ref or bank-ref on :61:".to_string(),
        ))?;
    let raw_info = info_lines.join("");
    let (description, counterparty, counterparty_bic, counterparty_account) = match dialect {
        Mt940Dialect::Generic => (raw_info, None, None, None),
        Mt940Dialect::Subfielded => {
            let s = parse_subfielded_86(&raw_info);
            (s.description, s.counterparty, s.counterparty_bic, s.counterparty_account)
        }
    };
    Ok(ParsedTxn {
        external_ref,
        value_date: p.value_date.clone(),
        posted_at: None,
        amount_minor: p.amount_minor,
        currency: None,
        direction: p.direction,
        counterparty,
        description,
        counterparty_bic,
        counterparty_account,
    })
}
```

Also update any other `ParsedTxn { ... }` literal in `mt940.rs` (if there's one) to include the two new fields (defaulting to `None`).

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd backend
cargo test -p recon-ingest --lib mt940:: 2>&1 | tail -20
```

Expected: all MT940 tests pass.

- [ ] **Step 6: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add backend/crates/recon-ingest/src/mt940.rs \
        backend/crates/recon-ingest/tests/fixtures/mt940-subfielded.sta
git commit -m "feat(ingest/mt940): map ?32 → counterparty_account, ?33 → counterparty_bic in Subfielded dialect"
```

---

## Task 6: Refactor — extract mt94x_shared helpers from mt940

**Files:**
- Create: `backend/crates/recon-ingest/src/mt94x_shared.rs`
- Modify: `backend/crates/recon-ingest/src/mt940.rs`
- Modify: `backend/crates/recon-ingest/src/lib.rs`

This task is a pure refactor — no behavioural change, the existing MT940 tests are the regression gate.

- [ ] **Step 1: Create the shared module**

Create `backend/crates/recon-ingest/src/mt94x_shared.rs`:

```rust
//! Shared parser helpers for the MT94x family (MT940 customer statement,
//! MT942 intra-day statement). The two messages differ in tag set and
//! state machine, but share several lexical helpers.

use recon_domain::Direction;

/// Per-source dialect (re-used by MT940 and MT942 — they share the same
/// `?nn` subfield grammar in the `:86:` info field).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mt94xDialect {
    Generic,
    Subfielded,
}

/// Decode bytes as UTF-8; on failure fall back to Latin-1 (one byte → one
/// char). The fallback is silent and always succeeds.
pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Split a tag line like `:20:REF12345` into `(":20:", "REF12345")`.
pub fn parse_tag(raw: &str) -> (&str, &str) {
    let mut idx = 0usize;
    let mut count = 0;
    for (i, c) in raw.char_indices() {
        if c == ':' {
            count += 1;
            if count == 2 {
                idx = i + 1;
                break;
            }
        }
    }
    if count < 2 {
        return (raw, "");
    }
    let tag = &raw[..idx];
    let content = &raw[idx..];
    (tag, content)
}

/// Parsed result of a single `:61:` statement line — common to MT940 and MT942.
pub struct Mt61 {
    pub value_date: String, // ISO 8601 YYYY-MM-DD
    pub direction: Direction,
    pub amount_minor: i64,
    pub customer_ref: Option<String>,
    pub bank_ref: Option<String>,
}

/// Parse the content portion of a `:61:` tag line.
pub fn parse_61(content: &str) -> Result<Mt61, (&'static str, String)> {
    let mut idx;
    let bytes = content.as_bytes();
    let n = bytes.len();

    if n < 6 {
        return Err(("date", "too short".into()));
    }
    let yy = parse_digits(&content[0..2]).map_err(|_| ("date", "yy not digits".to_string()))?;
    let mm = parse_digits(&content[2..4]).map_err(|_| ("date", "mm not digits".to_string()))?;
    let dd = parse_digits(&content[4..6]).map_err(|_| ("date", "dd not digits".to_string()))?;
    let year = 2000 + yy;
    let value_date = format!("{:04}-{:02}-{:02}", year, mm, dd);
    idx = 6;

    if idx + 4 <= n
        && bytes[idx].is_ascii_digit()
        && bytes[idx + 1].is_ascii_digit()
        && bytes[idx + 2].is_ascii_digit()
        && bytes[idx + 3].is_ascii_digit()
    {
        idx += 4;
    }

    let direction = if idx < n && bytes[idx] == b'R' {
        idx += 1;
        if idx >= n {
            return Err(("dc_mark", "expected D or C after R".into()));
        }
        match bytes[idx] {
            b'D' => { idx += 1; Direction::Credit }
            b'C' => { idx += 1; Direction::Debit }
            _ => return Err(("dc_mark", "expected D or C after R".into())),
        }
    } else if idx < n {
        match bytes[idx] {
            b'D' => { idx += 1; Direction::Debit }
            b'C' => { idx += 1; Direction::Credit }
            _ => return Err(("dc_mark", "expected D, C, RD or RC".into())),
        }
    } else {
        return Err(("dc_mark", "missing D/C mark".into()));
    };

    if idx < n
        && bytes[idx].is_ascii_alphabetic()
        && idx + 1 < n
        && bytes[idx + 1].is_ascii_digit()
    {
        idx += 1;
    }

    let amt_start = idx;
    while idx < n && (bytes[idx].is_ascii_digit() || bytes[idx] == b',') {
        idx += 1;
    }
    if idx == amt_start {
        return Err(("amount", "missing amount".into()));
    }
    let amount_str = content[amt_start..idx].replace(',', ".");
    let amount_minor =
        crate::money::parse_decimal_to_minor(&amount_str).map_err(|e| ("amount", e))?;

    if idx + 4 <= n
        && bytes[idx] == b'N'
        && bytes[idx + 1..idx + 4]
            .iter()
            .all(|b| b.is_ascii_alphabetic())
    {
        idx += 4;
    } else if idx + 4 <= n
        && bytes[idx] == b'S'
        && bytes[idx + 1..idx + 4]
            .iter()
            .all(|b| b.is_ascii_alphabetic())
    {
        idx += 4;
    } else {
        return Err(("type_code", "missing/invalid transaction type code".into()));
    }

    let tail = &content[idx..];
    let (customer_ref, bank_ref) = if let Some(slash_idx) = tail.find("//") {
        let cust = tail[..slash_idx].trim();
        let bank = tail[slash_idx + 2..].trim();
        let cust = if cust.is_empty() { None } else { Some(cust.to_string()) };
        let bank = if bank.is_empty() { None } else { Some(bank.to_string()) };
        (cust, bank)
    } else {
        let cust = tail.trim();
        let cust = if cust.is_empty() { None } else { Some(cust.to_string()) };
        (cust, None)
    };

    Ok(Mt61 {
        value_date,
        direction,
        amount_minor,
        customer_ref,
        bank_ref,
    })
}

fn parse_digits(s: &str) -> Result<u32, ()> {
    if s.chars().all(|c| c.is_ascii_digit()) {
        s.parse().map_err(|_| ())
    } else {
        Err(())
    }
}

/// Parsed result of a Subfielded `:86:` field.
pub struct SubfieldedInfo {
    pub description: String,
    pub counterparty: Option<String>,
    pub counterparty_bic: Option<String>,
    pub counterparty_account: Option<String>,
}

pub fn parse_subfielded_86(raw: &str) -> SubfieldedInfo {
    let mut desc_parts: Vec<String> = Vec::new();
    let mut cpty_bic: Option<String> = None;
    let mut cpty_account: Option<String> = None;
    let mut prefix = String::new();
    let mut chunks = raw.split('?');
    if let Some(first) = chunks.next() {
        if !first.is_empty() {
            prefix.push_str(first);
        }
    }
    if !prefix.is_empty() {
        desc_parts.push(prefix);
    }
    for chunk in chunks {
        if chunk.len() < 2 {
            continue;
        }
        let code = &chunk[..2];
        let val = &chunk[2..];
        match code {
            "20" | "21" | "22" | "23" | "24" | "25" | "26" | "27" | "28" | "29" => {
                desc_parts.push(val.to_string());
            }
            "32" => {
                if cpty_account.is_none() {
                    let v = val.trim();
                    if !v.is_empty() { cpty_account = Some(v.to_string()); }
                }
            }
            "33" => {
                if cpty_bic.is_none() {
                    let v = val.trim().to_uppercase();
                    if !v.is_empty() { cpty_bic = Some(v); }
                }
            }
            "30" | "31" => {
                desc_parts.push(format!("[{code}:{val}]"));
            }
            _ => {
                desc_parts.push(format!("[?{code}:{val}]"));
            }
        }
    }
    let description = desc_parts.join(" ").trim().to_string();
    SubfieldedInfo {
        description,
        counterparty: None,
        counterparty_bic: cpty_bic,
        counterparty_account: cpty_account,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tag_splits_at_second_colon() {
        assert_eq!(parse_tag(":20:REF"), (":20:", "REF"));
        assert_eq!(parse_tag(":86:?20stuff"), (":86:", "?20stuff"));
    }

    #[test]
    fn decode_falls_back_to_latin1_for_invalid_utf8() {
        let bytes = b"Caf\xe9";
        let s = decode(bytes);
        assert!(s.contains('é'));
    }
}
```

- [ ] **Step 2: Register the module**

In `backend/crates/recon-ingest/src/lib.rs`, add to the module list at the top:

```rust
pub mod mt94x_shared;
```

- [ ] **Step 3: Replace MT940's local helpers with shared ones**

In `backend/crates/recon-ingest/src/mt940.rs`:

- Delete the local `decode`, `parse_tag`, `parse_61`, `parse_digits`, `parse_subfielded_86`, `Mt61`, `SubfieldedInfo` definitions (they live in `mt94x_shared` now).
- Replace the `Mt940Dialect` enum with a type alias and re-export:
  ```rust
  pub use crate::mt94x_shared::Mt94xDialect as Mt940Dialect;
  ```
- At the top of the file, add:
  ```rust
  use crate::mt94x_shared::{decode, parse_61, parse_subfielded_86, parse_tag, Mt61, SubfieldedInfo};
  ```
- The `Mt940Parser::parse` body stays — it now calls the shared functions instead of local ones.

- [ ] **Step 4: Run all ingest tests**

```bash
cd backend
cargo test -p recon-ingest 2>&1 | tail -20
```

Expected: every existing MT940 test (and CSV, CAMT.053, BAI2) still passes. **This is a refactor — no test count or assertion should change.**

- [ ] **Step 5: Run clippy**

```bash
cd backend
cargo clippy -p recon-ingest --all-targets -- -D warnings 2>&1 | tail -10
```

Expected: no warnings.

- [ ] **Step 6: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add backend/crates/recon-ingest/src/mt94x_shared.rs \
        backend/crates/recon-ingest/src/mt940.rs \
        backend/crates/recon-ingest/src/lib.rs
git commit -m "refactor(ingest): extract MT94x helpers (decode, parse_tag, parse_61, parse_subfielded_86, Mt94xDialect) into mt94x_shared"
```

---

## Task 7: MT942 intra-day parser

**Files:**
- Create: `backend/crates/recon-ingest/src/mt942.rs`
- Create: `backend/crates/recon-ingest/tests/fixtures/mt942-single-message.sta`
- Create: `backend/crates/recon-ingest/tests/fixtures/mt942-subfielded.sta`
- Modify: `backend/crates/recon-ingest/src/lib.rs`
- Modify: `backend/crates/recon-api/src/routes.rs` (ingest dispatcher)

- [ ] **Step 1: Create the fixtures**

`backend/crates/recon-ingest/tests/fixtures/mt942-single-message.sta`:

```
:20:INTRA-DAY-1
:25:DE89370400440532013000
:28C:1/1
:34F:EUR0,00
:13D:2601011200+0100
:61:260101D250,00NTRFCUSTREF-A//BNKREF-1
:86:Intra-day debit one
:61:260101C500,00NTRFCUSTREF-B//BNKREF-2
:86:Intra-day credit one
:90D:1EUR250,00
:90C:1EUR500,00
```

`backend/crates/recon-ingest/tests/fixtures/mt942-subfielded.sta`:

```
:20:INTRA-DAY-2
:25:DE89370400440532013000
:28C:1/1
:13D:2601011200+0100
:61:260101C300,00NTRFCPTYREF//BNKREF
:86:?00FAKTURA?20Intra-day invoice?21INV-AAA?32DE89370400440532013000?33DEUTDEFF
:90D:0EUR0,00
:90C:1EUR300,00
```

- [ ] **Step 2: Write failing tests**

Create `backend/crates/recon-ingest/src/mt942.rs`:

```rust
//! SWIFT MT942 (Interim Transaction Report / intra-day) parser.
//!
//! Tag-based block format, very close to MT940. Differences:
//!  - No `:60F:`/`:62F:` opening/closing balance tags — intra-day has no balance.
//!  - Adds `:34F:` (floor-limit indicator — parsed and discarded for state-machine cleanliness).
//!  - Adds `:13D:` (date/time of the statement — used as fallback for `value_date` and not currently emitted).
//!  - Adds `:90D:` / `:90C:` totals — used for a sanity check that the parsed
//!    debit/credit count and minor-amount sums match what the file claims.
//!
//! Reuses MT940's `parse_61`, `parse_subfielded_86`, `parse_tag`, `decode`, and the
//! `Mt94xDialect` enum via the `mt94x_shared` module.

use crate::mt94x_shared::{decode, parse_61, parse_subfielded_86, parse_tag, Mt61, Mt94xDialect, SubfieldedInfo};
use crate::{ParsedTxn, Parser, RowError};
use recon_domain::Direction;

pub struct Mt942Parser {
    pub dialect: Mt94xDialect,
}

impl Parser for Mt942Parser {
    fn parse(&self, bytes: &[u8]) -> Result<Vec<ParsedTxn>, Vec<RowError>> {
        let text = decode(bytes);
        let mut txns: Vec<ParsedTxn> = Vec::new();
        let mut errors: Vec<RowError> = Vec::new();

        let mut pending: Option<(usize, Mt61)> = None;
        let mut info_buf: Vec<String> = Vec::new();

        // For the :90D:/:90C: sanity check.
        let mut declared_debit_count: Option<i64> = None;
        let mut declared_credit_count: Option<i64> = None;
        let mut declared_debit_minor: Option<i64> = None;
        let mut declared_credit_minor: Option<i64> = None;

        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0usize;
        while i < lines.len() {
            let raw = lines[i];
            let line_no = i + 1;
            if !raw.starts_with(':') {
                if pending.is_some() {
                    info_buf.push(raw.to_string());
                }
                i += 1;
                continue;
            }
            let (tag, content) = parse_tag(raw);
            match tag {
                // MT940 balance tags are illegal in MT942 — reject loudly.
                ":60F:" | ":60M:" | ":62F:" | ":62M:" | ":64:" | ":65:" => {
                    errors.push(RowError::new(line_no, "tag", format!("balance tag {tag} is not valid in MT942")));
                }
                ":20:" | ":25:" | ":28C:" => {
                    if let Some((line, p61)) = pending.take() {
                        let info = std::mem::take(&mut info_buf);
                        match build_txn(&p61, &info, self.dialect) {
                            Ok(t) => txns.push(t),
                            Err(e) => errors.push(RowError::new(line, e.0, e.1)),
                        }
                    }
                }
                ":34F:" | ":13D:" => {
                    // Parsed and discarded. They're informational and don't open/close any state.
                }
                ":61:" => {
                    if let Some((line, p61)) = pending.take() {
                        let info = std::mem::take(&mut info_buf);
                        match build_txn(&p61, &info, self.dialect) {
                            Ok(t) => txns.push(t),
                            Err(e) => errors.push(RowError::new(line, e.0, e.1)),
                        }
                    }
                    match parse_61(content) {
                        Ok(p) => pending = Some((line_no, p)),
                        Err(e) => errors.push(RowError::new(line_no, e.0, e.1)),
                    }
                }
                ":86:" if pending.is_some() => {
                    info_buf.push(content.to_string());
                }
                ":90D:" | ":90C:" => {
                    // Flush any pending :61: first.
                    if let Some((line, p61)) = pending.take() {
                        let info = std::mem::take(&mut info_buf);
                        match build_txn(&p61, &info, self.dialect) {
                            Ok(t) => txns.push(t),
                            Err(e) => errors.push(RowError::new(line, e.0, e.1)),
                        }
                    }
                    match parse_90_totals(content) {
                        Ok((count, minor)) => {
                            if tag == ":90D:" {
                                declared_debit_count = Some(count);
                                declared_debit_minor = Some(minor);
                            } else {
                                declared_credit_count = Some(count);
                                declared_credit_minor = Some(minor);
                            }
                        }
                        Err(e) => errors.push(RowError::new(line_no, e.0, e.1)),
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if let Some((line, p61)) = pending {
            let info = std::mem::take(&mut info_buf);
            match build_txn(&p61, &info, self.dialect) {
                Ok(t) => txns.push(t),
                Err(e) => errors.push(RowError::new(line, e.0, e.1)),
            }
        }

        // Sanity check: declared totals must match parsed totals (when both :90 tags present).
        if let (Some(dc), Some(dm)) = (declared_debit_count, declared_debit_minor) {
            let pc = txns.iter().filter(|t| t.direction == Direction::Debit).count() as i64;
            let pm: i64 = txns.iter().filter(|t| t.direction == Direction::Debit).map(|t| t.amount_minor).sum();
            if pc != dc || pm != dm {
                errors.push(RowError::new(0, "totals", format!(
                    ":90D: declared {dc} debits totalling {dm} minor; parsed {pc} totalling {pm}"
                )));
            }
        }
        if let (Some(cc), Some(cm)) = (declared_credit_count, declared_credit_minor) {
            let pc = txns.iter().filter(|t| t.direction == Direction::Credit).count() as i64;
            let pm: i64 = txns.iter().filter(|t| t.direction == Direction::Credit).map(|t| t.amount_minor).sum();
            if pc != cc || pm != cm {
                errors.push(RowError::new(0, "totals", format!(
                    ":90C: declared {cc} credits totalling {cm} minor; parsed {pc} totalling {pm}"
                )));
            }
        }

        if errors.is_empty() {
            Ok(txns)
        } else {
            Err(errors)
        }
    }
}

/// Parse `:90D:` / `:90C:` content like `3EUR1500,00` → (count, minor).
fn parse_90_totals(content: &str) -> Result<(i64, i64), (&'static str, String)> {
    let bytes = content.as_bytes();
    let n = bytes.len();
    let mut idx = 0;
    while idx < n && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == 0 {
        return Err(("totals", "missing count".into()));
    }
    let count: i64 = content[..idx].parse().map_err(|_| ("totals", "bad count".to_string()))?;
    // Skip the 3-letter currency code.
    if idx + 3 > n || !bytes[idx..idx + 3].iter().all(|b| b.is_ascii_alphabetic()) {
        return Err(("totals", "missing currency".into()));
    }
    idx += 3;
    let amount_str = content[idx..].replace(',', ".");
    let minor = crate::money::parse_decimal_to_minor(&amount_str).map_err(|e| ("totals", e))?;
    Ok((count, minor))
}

fn build_txn(
    p: &Mt61,
    info_lines: &[String],
    dialect: Mt94xDialect,
) -> Result<ParsedTxn, (&'static str, String)> {
    let external_ref = p
        .customer_ref
        .clone()
        .or_else(|| p.bank_ref.clone())
        .ok_or(("external_ref", "no customer-ref or bank-ref on :61:".to_string()))?;
    let raw_info = info_lines.join("");
    let (description, counterparty, counterparty_bic, counterparty_account) = match dialect {
        Mt94xDialect::Generic => (raw_info, None, None, None),
        Mt94xDialect::Subfielded => {
            let SubfieldedInfo { description, counterparty, counterparty_bic, counterparty_account } =
                parse_subfielded_86(&raw_info);
            (description, counterparty, counterparty_bic, counterparty_account)
        }
    };
    Ok(ParsedTxn {
        external_ref,
        value_date: p.value_date.clone(),
        posted_at: None,
        amount_minor: p.amount_minor,
        currency: None,
        direction: p.direction,
        counterparty,
        description,
        counterparty_bic,
        counterparty_account,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> Vec<u8> {
        std::fs::read(format!("tests/fixtures/{name}")).expect("fixture file")
    }

    #[test]
    fn generic_parses_two_txns_and_passes_sanity_check() {
        let bytes = load("mt942-single-message.sta");
        let txns = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(&bytes)
        .unwrap();
        assert_eq!(txns.len(), 2);
        assert_eq!(txns[0].direction, Direction::Debit);
        assert_eq!(txns[0].amount_minor, 25000);
        assert_eq!(txns[1].direction, Direction::Credit);
        assert_eq!(txns[1].amount_minor, 50000);
    }

    #[test]
    fn balance_tag_rejected() {
        let bad = b":20:R\n:25:A\n:28C:1/1\n:60F:C260101EUR100,00\n:61:260101D50,00NTRFREF\n";
        let err = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(bad)
        .unwrap_err();
        assert!(err.iter().any(|e| e.field == "tag"));
    }

    #[test]
    fn declared_totals_mismatch_returns_error() {
        let bad = b":20:R\n:25:A\n:28C:1/1\n:61:260101D250,00NTRFREF\n:90D:2EUR500,00\n";
        let err = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(bad)
        .unwrap_err();
        assert!(err.iter().any(|e| e.field == "totals"));
    }

    #[test]
    fn floor_limit_and_date_time_tags_silently_consumed() {
        let bytes = load("mt942-single-message.sta");
        let txns = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(&bytes)
        .unwrap();
        // The :34F: and :13D: are present in the fixture; they must not break parsing
        // and must not appear in any txn description.
        assert!(txns.iter().all(|t| !t.description.contains("34F")));
        assert!(txns.iter().all(|t| !t.description.contains("13D")));
    }

    #[test]
    fn subfielded_dialect_extracts_counterparty_account_and_bic() {
        let bytes = load("mt942-subfielded.sta");
        let txns = Mt942Parser {
            dialect: Mt94xDialect::Subfielded,
        }
        .parse(&bytes)
        .unwrap();
        assert_eq!(txns.len(), 1);
        let t = &txns[0];
        assert_eq!(t.counterparty_account.as_deref(), Some("DE89370400440532013000"));
        assert_eq!(t.counterparty_bic.as_deref(), Some("DEUTDEFF"));
        assert!(t.description.contains("Intra-day invoice"));
    }

    #[test]
    fn no_external_ref_returns_row_error() {
        let bad = b":20:R\n:25:A\n:28C:1/1\n:61:260101D50,00NTRF//\n:90D:1EUR50,00\n:90C:0EUR0,00\n";
        let err = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(bad)
        .unwrap_err();
        assert!(err.iter().any(|e| e.field == "external_ref"));
    }

    #[test]
    fn empty_file_returns_empty_txn_list() {
        let txns = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(b"")
        .unwrap();
        assert!(txns.is_empty());
    }

    #[test]
    fn bad_dc_mark_returns_row_error() {
        let bad = b":20:R\n:25:A\n:28C:1/1\n:61:260101X50,00NTRFREF\n";
        let err = Mt942Parser {
            dialect: Mt94xDialect::Generic,
        }
        .parse(bad)
        .unwrap_err();
        assert!(err.iter().any(|e| e.field == "dc_mark"));
    }
}
```

- [ ] **Step 3: Register the module**

In `backend/crates/recon-ingest/src/lib.rs`, add:

```rust
pub mod mt942;
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd backend
cargo test -p recon-ingest --lib mt942:: 2>&1 | tail -20
```

Expected: all 8 MT942 tests pass.

- [ ] **Step 5: Wire MT942 into the ingest dispatcher**

In `backend/crates/recon-api/src/routes.rs`, find the `"mt940" => { ... }` branch in `ingest_source` (currently lines ~305–312) and add an `mt942` branch immediately after it:

```rust
        "mt940" => {
            let dialect = match source.format_dialect.as_deref() {
                Some("subfielded") => recon_ingest::mt94x_shared::Mt94xDialect::Subfielded,
                _ => recon_ingest::mt94x_shared::Mt94xDialect::Generic,
            };
            recon_ingest::mt940::Mt940Parser { dialect }.parse(&bytes)
        }
        "mt942" => {
            let dialect = match source.format_dialect.as_deref() {
                Some("subfielded") => recon_ingest::mt94x_shared::Mt94xDialect::Subfielded,
                _ => recon_ingest::mt94x_shared::Mt94xDialect::Generic,
            };
            recon_ingest::mt942::Mt942Parser { dialect }.parse(&bytes)
        }
```

Note the MT940 branch now uses `mt94x_shared::Mt94xDialect` too — the existing `recon_ingest::mt940::Mt940Dialect` is now a re-export alias, but using the canonical path keeps imports tidy.

- [ ] **Step 6: Add API integration test for MT942**

In `backend/crates/recon-api/tests/ingest_api.rs`, find the `MT940_FIXTURE` constant and add a `MT942_FIXTURE` constant + a new happy-path test below the existing MT940 test:

```rust
const MT942_FIXTURE: &[u8] = b":20:INTRA-DAY-1
:25:DE89370400440532013000
:28C:1/1
:34F:EUR0,00
:13D:2601011200+0100
:61:260101D250,00NTRFCUSTREF-A//BNKREF-1
:86:Intra-day debit one
:61:260101C500,00NTRFCUSTREF-B//BNKREF-2
:86:Intra-day credit one
:90D:1EUR250,00
:90C:1EUR500,00
";

#[tokio::test]
async fn mt942_happy_path_ingests_two_txns() {
    let (app, _, ada) = test_app().await;
    let src = create_source_as(&app, &ada, "Intra-day bank", "bank", "EUR", None).await;
    let resp = ingest_as(&app, &ada, &src.id, "mt942", MT942_FIXTURE, None).await;
    let body: serde_json::Value = resp.into_inner();
    assert_eq!(body["ingested"], 2);
}
```

(Use whatever helper signatures `create_source_as`/`ingest_as` actually have — match the existing MT940 test in the file.)

- [ ] **Step 7: Run the full backend test suite**

```bash
cd backend
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --workspace 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 8: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add backend/crates/recon-ingest/src/mt942.rs \
        backend/crates/recon-ingest/src/lib.rs \
        backend/crates/recon-ingest/tests/fixtures/mt942-single-message.sta \
        backend/crates/recon-ingest/tests/fixtures/mt942-subfielded.sta \
        backend/crates/recon-api/src/routes.rs \
        backend/crates/recon-api/tests/ingest_api.rs
git commit -m "feat(ingest): MT942 intra-day parser (generic + subfielded) wired through /sources/:id/ingest"
```

---

## Task 8: PATCH /sources/:id (backend + audit)

**Files:**
- Modify: `backend/crates/recon-audit/src/events.rs` (new AuditKind + AuditPayload variant)
- Modify: `backend/crates/recon-api/src/dto.rs` (new UpdateSourceReq)
- Modify: `backend/crates/recon-api/src/routes.rs` (new patch_source handler + route)
- Modify: `backend/crates/recon-store/src/sources.rs` (new update_source method)
- Create: `backend/crates/recon-store/tests/patch_source.rs`

- [ ] **Step 1: Add AuditKind::SourceUpdated**

In `backend/crates/recon-audit/src/events.rs`:

- Add `SourceUpdated,` to the `AuditKind` enum (after `DataSourceCreated`).
- Add `AuditKind::SourceUpdated => "source.updated",` to the `as_str` match.
- Add `"source.updated" => AuditKind::SourceUpdated,` to the `from_str` match.
- Add `SourceUpdated { source_id: String, before_name: String, after_name: String, before_format_dialect: Option<String>, after_format_dialect: Option<String> }` to the `AuditPayload` enum (after `DataSourceCreated`).
- Add `AuditPayload::SourceUpdated { .. } => AuditKind::SourceUpdated,` to the `kind()` match.
- Extend the test `kind_strings_are_stable_dot_notation` with `assert_eq!(AuditKind::SourceUpdated.as_str(), "source.updated");`.

- [ ] **Step 2: Add UpdateSourceReq DTO**

In `backend/crates/recon-api/src/dto.rs`, add at the bottom:

```rust
/// PATCH /sources/:id request body.
///
/// `format_dialect` uses a double-`Option` so we can distinguish three states:
///   - field absent in JSON           → don't change
///   - field present with `null`      → clear the dialect
///   - field present with a value     → set it
#[derive(Debug, Deserialize)]
pub struct UpdateSourceReq {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub format_dialect: Option<Option<String>>,
}

fn deserialize_double_option<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(de).map(Some)
}
```

Make sure `serde::Deserialize` is in scope at the top of the file (it likely already is for the other DTOs).

- [ ] **Step 3: Add update_source store method**

In `backend/crates/recon-store/src/sources.rs`, append a new method to the `impl Store` block:

```rust
    /// Apply a partial update to a source. Audited as `source.updated` inside the
    /// same transaction as the UPDATE. Returns the updated source.
    pub async fn update_source(
        &self,
        tenant_id: &str,
        source_id: &str,
        actor_id: &str,
        new_name: Option<&str>,
        // None = field absent; Some(None) = clear; Some(Some(v)) = set to v.
        new_format_dialect: Option<Option<&str>>,
    ) -> Result<Source, StoreError> {
        let before = self.get_source(tenant_id, source_id).await?;

        let mut tx = self.pool.begin().await?;

        let after_name = new_name.unwrap_or(&before.name).to_string();
        let after_dialect: Option<String> = match new_format_dialect {
            None => before.format_dialect.clone(),
            Some(v) => v.map(|s| s.to_string()),
        };

        sqlx::query("UPDATE sources SET name=$1, format_dialect=$2 WHERE id=$3 AND tenant_id=$4")
            .bind(&after_name)
            .bind(&after_dialect)
            .bind(source_id)
            .bind(tenant_id)
            .execute(&mut *tx)
            .await?;

        self.append_audit(
            &mut tx,
            tenant_id,
            actor_id,
            recon_audit::AuditPayload::SourceUpdated {
                source_id: source_id.to_string(),
                before_name: before.name.clone(),
                after_name: after_name.clone(),
                before_format_dialect: before.format_dialect.clone(),
                after_format_dialect: after_dialect.clone(),
            },
        )
        .await?;
        tx.commit().await?;

        Ok(Source {
            id: source_id.to_string(),
            tenant_id: tenant_id.to_string(),
            kind: before.kind,
            name: after_name,
            currency: before.currency,
            format_dialect: after_dialect,
        })
    }
```

- [ ] **Step 4: Write failing store-level test**

Create `backend/crates/recon-store/tests/patch_source.rs`:

```rust
//! update_source semantics: each PATCH variant + audit emission.

use sqlx::PgPool;

async fn pool() -> recon_store::Store {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    recon_store::Store::new(PgPool::connect(&url).await.unwrap())
}

async fn fixture_source(store: &recon_store::Store) -> (String, String, String) {
    let tenant_id = format!("tenant-test-{}", uuid::Uuid::new_v4());
    let actor_id = format!("user-{}", uuid::Uuid::new_v4());
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,$1,$1)")
        .bind(&tenant_id)
        .execute(store.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ($1,'A','a@a.test',false)")
        .bind(&actor_id)
        .execute(store.pool())
        .await
        .unwrap();
    let src = store
        .create_source(&tenant_id, recon_domain::SourceKind::Bank, "Original", "EUR", &actor_id, None)
        .await
        .unwrap();
    (tenant_id, actor_id, src.id)
}

#[tokio::test]
async fn rename_only_changes_name_and_keeps_dialect_null() {
    let store = pool().await;
    let (t, a, sid) = fixture_source(&store).await;
    let updated = store
        .update_source(&t, &sid, &a, Some("Renamed"), None)
        .await
        .unwrap();
    assert_eq!(updated.name, "Renamed");
    assert!(updated.format_dialect.is_none());
}

#[tokio::test]
async fn set_dialect_only_keeps_name_and_sets_dialect() {
    let store = pool().await;
    let (t, a, sid) = fixture_source(&store).await;
    let updated = store
        .update_source(&t, &sid, &a, None, Some(Some("subfielded")))
        .await
        .unwrap();
    assert_eq!(updated.name, "Original");
    assert_eq!(updated.format_dialect.as_deref(), Some("subfielded"));
}

#[tokio::test]
async fn clear_dialect_sets_it_back_to_null() {
    let store = pool().await;
    let (t, a, sid) = fixture_source(&store).await;
    let _ = store
        .update_source(&t, &sid, &a, None, Some(Some("subfielded")))
        .await
        .unwrap();
    let updated = store
        .update_source(&t, &sid, &a, None, Some(None))
        .await
        .unwrap();
    assert!(updated.format_dialect.is_none());
}

#[tokio::test]
async fn empty_patch_no_changes_still_emits_audit_row() {
    let store = pool().await;
    let (t, a, sid) = fixture_source(&store).await;
    let updated = store.update_source(&t, &sid, &a, None, None).await.unwrap();
    assert_eq!(updated.name, "Original");
    // Audit row should exist for source.updated.
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_log WHERE tenant_id=$1 AND kind='source.updated'",
    )
    .bind(&t)
    .fetch_one(store.pool())
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn cross_tenant_update_returns_not_found() {
    let store = pool().await;
    let (_, a, sid) = fixture_source(&store).await;
    let other = format!("tenant-other-{}", uuid::Uuid::new_v4());
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,$1,$1)")
        .bind(&other)
        .execute(store.pool())
        .await
        .unwrap();
    let err = store.update_source(&other, &sid, &a, Some("X"), None).await.unwrap_err();
    matches!(err, recon_store::StoreError::NotFound);
}
```

(Replace `store.pool()` with whatever public accessor `recon_store::Store` exposes. If the field is `pub(crate) pool` only, add a `pub fn pool(&self) -> &PgPool { &self.pool }` accessor on `Store` first, in `recon-store/src/lib.rs` — or, if a public accessor exists already, use it. Check `recon-store/src/lib.rs` and adjust.)

- [ ] **Step 5: Run the store test to verify it fails or passes**

```bash
cd backend
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store --test patch_source 2>&1 | tail -20
```

Expected: PASS (we implemented `update_source` in step 3 already — store-level is done; HTTP layer next).

- [ ] **Step 6: Add the HTTP handler**

In `backend/crates/recon-api/src/routes.rs`, add a `patch_source` handler after `create_source`:

```rust
async fn patch_source(
    State(s): State<AppState>,
    ctx: AuthContext,
    Path(source_id): Path<String>,
    Json(body): Json<UpdateSourceReq>,
) -> Result<Json<Value>, ApiError> {
    require_manage_data(&ctx)?;
    // Validate name if present.
    if let Some(ref name) = body.name {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.chars().count() > 80 {
            return Err(ApiError::BadRequest());
        }
    }
    // Validate format_dialect if present.
    let dialect_patch: Option<Option<&str>> = match body.format_dialect {
        None => None,
        Some(None) => Some(None),
        Some(Some(ref s)) => match s.as_str() {
            "generic" | "subfielded" => Some(Some(s.as_str())),
            _ => return Err(ApiError::BadRequest()),
        },
    };
    let updated = s
        .store
        .update_source(
            &ctx.tenant_id,
            &source_id,
            &ctx.user_id,
            body.name.as_deref().map(str::trim),
            dialect_patch,
        )
        .await?;
    Ok(Json(json!(updated)))
}
```

Wire it into the router (in the `router` function, find the `.route("/api/sources/:source_id/ingest", ...)` line and add above it):

```rust
        .route(
            "/api/sources/:source_id",
            axum::routing::patch(patch_source),
        )
```

- [ ] **Step 7: Write failing HTTP test**

In `backend/crates/recon-api/tests/api.rs`, append:

```rust
#[tokio::test]
async fn patch_source_admin_rename_and_set_dialect() {
    let (app, _, ada) = test_app().await;
    let src = create_source_as(&app, &ada, "Bank A", "bank", "EUR", None).await;
    let resp = patch_as(&app, &ada, &format!("/api/sources/{}", src.id),
        serde_json::json!({"name": "Bank A renamed", "format_dialect": "subfielded"})).await;
    let body: serde_json::Value = resp.into_inner();
    assert_eq!(body["name"], "Bank A renamed");
    assert_eq!(body["formatDialect"], "subfielded");
}

#[tokio::test]
async fn patch_source_clear_dialect_with_null() {
    let (app, _, ada) = test_app().await;
    let src = create_source_as(&app, &ada, "Bank A", "bank", "EUR", Some("subfielded")).await;
    let resp = patch_as(&app, &ada, &format!("/api/sources/{}", src.id),
        serde_json::json!({"format_dialect": null})).await;
    let body: serde_json::Value = resp.into_inner();
    assert!(body["formatDialect"].is_null());
}

#[tokio::test]
async fn patch_source_invalid_dialect_400() {
    let (app, _, ada) = test_app().await;
    let src = create_source_as(&app, &ada, "Bank A", "bank", "EUR", None).await;
    let status = patch_as_status(&app, &ada, &format!("/api/sources/{}", src.id),
        serde_json::json!({"format_dialect": "bogus"})).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn patch_source_non_admin_403() {
    let (app, _, _, mia) = test_app_with_operator().await;
    let status = patch_as_status(&app, &mia, "/api/sources/src-something",
        serde_json::json!({"name": "X"})).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn patch_source_cross_tenant_404() {
    let (app, _, ada) = test_app().await;
    let status = patch_as_status(&app, &ada, "/api/sources/src-nonexistent",
        serde_json::json!({"name": "X"})).await;
    assert_eq!(status, 404);
}
```

(If `patch_as` / `patch_as_status` / `test_app_with_operator` helpers don't exist yet, add them mirroring the existing `post_as` / `get_as` helpers in the same test file.)

- [ ] **Step 8: Run API tests to verify they pass**

```bash
cd backend
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-api --test api patch_source 2>&1 | tail -20
```

Expected: all 5 PATCH tests pass.

- [ ] **Step 9: Restart the API and commit**

```bash
cd /home/nestinka/assistant/reconciliation-system/backend
cargo build -p recon-api --release 2>&1 | tail -3
pkill -f 'target/release/recon-api' 2>/dev/null
sleep 1
RECON_DEV=1 DATABASE_URL=postgres://recon:recon@localhost:5432/recon \
    SMTP_HOST=localhost SMTP_PORT=1025 \
    nohup ./target/release/recon-api > /tmp/recon-api.log 2>&1 &
sleep 1 && curl -sS http://localhost:8080/healthz
cd /home/nestinka/assistant/reconciliation-system
git add backend/crates/recon-audit/src/events.rs \
        backend/crates/recon-api/src/dto.rs \
        backend/crates/recon-api/src/routes.rs \
        backend/crates/recon-store/src/sources.rs \
        backend/crates/recon-store/tests/patch_source.rs \
        backend/crates/recon-api/tests/api.rs
git commit -m "feat(api): PATCH /sources/:id (name, format_dialect); admin-only; audited as source.updated"
```

---

## Task 9: Edit-source dialog + sources page "Edit" action (frontend)

**Files:**
- Create: `web/components/app/edit-source-dialog.tsx`
- Create: `web/tests/edit-source-dialog.test.tsx`
- Modify: `web/lib/api/client.ts` (add `updateSource` to interface)
- Modify: `web/lib/api/mock.ts` (implement `updateSource`)
- Modify: `web/app/(app)/sources/page.tsx` (add Edit button)
- Modify: `web/tests/sources-page.test.tsx` (add Edit-button tests)

- [ ] **Step 1: Extend the API client interface**

In `web/lib/api/client.ts`, find the `interface ApiClient` (look for `createSource(`). Add below it:

```ts
  updateSource(
    tenantId: string,
    sourceId: string,
    patch: UpdateSourceInput,
  ): Promise<Source>;
```

Add the input type near the other input types (search for `CreateSourceInput`):

```ts
export interface UpdateSourceInput {
  name?: string;
  // null = clear, undefined = don't change, string = set
  formatDialect?: string | null;
}
```

- [ ] **Step 2: Implement updateSource in the mock**

In `web/lib/api/mock.ts`, find the `createSource` implementation. Add immediately below it:

```ts
  async updateSource(
    tenantId: string,
    sourceId: string,
    patch: UpdateSourceInput,
  ): Promise<Source> {
    const sources = this.byTenant.get(tenantId)?.sources ?? [];
    const idx = sources.findIndex((s) => s.id === sourceId);
    if (idx < 0) throw new ApiError(404, "source.notFound");
    const before = sources[idx];
    const after: Source = {
      ...before,
      name: patch.name ?? before.name,
      formatDialect:
        patch.formatDialect === undefined
          ? before.formatDialect
          : patch.formatDialect, // may be null
    };
    sources[idx] = after;
    return after;
  },
```

(The exact `byTenant.get(tenantId)?.sources` shape depends on the existing mock implementation — match the pattern in `createSource`.)

- [ ] **Step 3: Implement updateSource in the HTTP client**

In `web/lib/api/http.ts` (or wherever the HTTP client implementation lives — find it by searching for the `createSource` http implementation):

```ts
  async updateSource(tenantId, sourceId, patch) {
    const body: Record<string, unknown> = {};
    if (patch.name !== undefined) body.name = patch.name;
    if (patch.formatDialect !== undefined) body.format_dialect = patch.formatDialect;
    const res = await this.fetch(`/api/sources/${sourceId}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json", "X-Tenant-Id": tenantId },
      body: JSON.stringify(body),
    });
    return sourceSchema.parse(await res.json());
  },
```

- [ ] **Step 4: Write the failing dialog test**

Create `web/tests/edit-source-dialog.test.tsx`:

```tsx
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { TestProviders } from "./helpers/test-providers";
import { EditSourceDialog } from "@/components/app/edit-source-dialog";
import type { Source } from "@/lib/domain/types";

const base: Source = {
  id: "src-1",
  tenantId: "tenant-acme",
  kind: "bank",
  name: "Bank A",
  currency: "EUR",
  formatDialect: null,
};

describe("EditSourceDialog", () => {
  it("renders pre-filled name and dialect", async () => {
    render(
      <TestProviders>
        <EditSourceDialog source={{ ...base, formatDialect: "subfielded" }} open onOpenChange={() => {}} onSaved={() => {}} />
      </TestProviders>,
    );
    expect(screen.getByLabelText(/name/i)).toHaveValue("Bank A");
    expect(screen.getByText(/subfielded/i)).toBeInTheDocument();
  });

  it("submits only changed fields", async () => {
    const user = userEvent.setup();
    const onSaved = vi.fn();
    render(
      <TestProviders>
        <EditSourceDialog source={base} open onOpenChange={() => {}} onSaved={onSaved} />
      </TestProviders>,
    );
    const nameInput = screen.getByLabelText(/name/i);
    await user.clear(nameInput);
    await user.type(nameInput, "Bank A renamed");
    await user.click(screen.getByRole("button", { name: /save/i }));
    await waitFor(() => expect(onSaved).toHaveBeenCalledTimes(1));
    const savedWith = onSaved.mock.calls[0][0] as Partial<Source>;
    expect(savedWith.name).toBe("Bank A renamed");
  });

  it("Cancel resets the form and closes", async () => {
    const user = userEvent.setup();
    const onOpenChange = vi.fn();
    render(
      <TestProviders>
        <EditSourceDialog source={base} open onOpenChange={onOpenChange} onSaved={() => {}} />
      </TestProviders>,
    );
    const nameInput = screen.getByLabelText(/name/i);
    await user.clear(nameInput);
    await user.type(nameInput, "Different");
    await user.click(screen.getByRole("button", { name: /cancel/i }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 5: Run test to verify it fails**

```bash
cd web
pnpm vitest run tests/edit-source-dialog.test.tsx 2>&1 | tail -10
```

Expected: import error — `EditSourceDialog` does not exist.

- [ ] **Step 6: Implement EditSourceDialog**

Create `web/components/app/edit-source-dialog.tsx`. Inspect the existing new-source dialog (likely `web/components/app/new-source-dialog.tsx` or inline in the sources page) to match its Base UI + react-hook-form + zod pattern. The new dialog should:

```tsx
"use client";

import * as React from "react";
import { useForm, Controller } from "react-hook-form";
import { zodResolver } from "@hookform/resolvers/zod";
import { z } from "zod";
import { toast } from "sonner";
import { useApi } from "@/lib/api/provider";
import { useTenant } from "@/lib/providers/tenant-provider";
import type { Source } from "@/lib/domain/types";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const DIALECT_SENTINEL = "__none__";

const schema = z.object({
  name: z.string().trim().min(1, "Name is required").max(80, "Name too long"),
  formatDialect: z.union([z.literal("generic"), z.literal("subfielded"), z.literal(DIALECT_SENTINEL)]),
});
type FormValues = z.infer<typeof schema>;

interface Props {
  source: Source;
  open: boolean;
  onOpenChange: (v: boolean) => void;
  onSaved: (s: Source) => void;
}

export function EditSourceDialog({ source, open, onOpenChange, onSaved }: Props) {
  const api = useApi();
  const { tenantId } = useTenant();
  const { control, handleSubmit, reset, formState: { isSubmitting } } = useForm<FormValues>({
    resolver: zodResolver(schema),
    defaultValues: {
      name: source.name,
      formatDialect: source.formatDialect ?? DIALECT_SENTINEL,
    },
  });

  // Re-seed when the dialog opens with a different source.
  React.useEffect(() => {
    if (open) {
      reset({ name: source.name, formatDialect: source.formatDialect ?? DIALECT_SENTINEL });
    }
  }, [open, source.id, source.name, source.formatDialect, reset]);

  const onSubmit = handleSubmit(async (values) => {
    const patch: Record<string, unknown> = {};
    if (values.name !== source.name) patch.name = values.name;
    const dialectAfter = values.formatDialect === DIALECT_SENTINEL ? null : values.formatDialect;
    if (dialectAfter !== source.formatDialect) patch.formatDialect = dialectAfter;
    try {
      const updated = await api.updateSource(tenantId, source.id, patch);
      onSaved(updated);
      toast.success("Source updated");
      onOpenChange(false);
    } catch {
      toast.error("Failed to update source");
    }
  });

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Edit source</DialogTitle>
        </DialogHeader>
        <form onSubmit={onSubmit} className="space-y-4">
          <div>
            <Label htmlFor="edit-source-name">Name</Label>
            <Controller
              control={control}
              name="name"
              render={({ field }) => <Input id="edit-source-name" {...field} />}
            />
          </div>
          <div>
            <Label htmlFor="edit-source-dialect">MT940 / MT942 dialect</Label>
            <Controller
              control={control}
              name="formatDialect"
              render={({ field }) => (
                <Select value={field.value} onValueChange={field.onChange}>
                  <SelectTrigger id="edit-source-dialect">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={DIALECT_SENTINEL}>Not applicable</SelectItem>
                    <SelectItem value="generic">Generic</SelectItem>
                    <SelectItem value="subfielded">Subfielded (DE/NL/BE)</SelectItem>
                  </SelectContent>
                </Select>
              )}
            />
          </div>
          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={isSubmitting}>Save</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
```

- [ ] **Step 7: Run dialog tests to verify they pass**

```bash
cd web
pnpm vitest run tests/edit-source-dialog.test.tsx 2>&1 | tail -10
```

Expected: all 3 dialog tests pass.

- [ ] **Step 8: Add "Edit" button on sources page**

In `web/app/(app)/sources/page.tsx`, locate the table row (search for `<TableRow` or `mockSources.map`) and add an "Edit" button cell for admins:

```tsx
{isAdmin && (
  <Button
    size="sm"
    variant="outline"
    onClick={() => { setEditing(source); setEditOpen(true); }}
  >
    Edit
  </Button>
)}
```

At the top of the page component, add the editing state:

```tsx
const [editing, setEditing] = React.useState<Source | null>(null);
const [editOpen, setEditOpen] = React.useState(false);
```

And render the dialog near the bottom of the page JSX:

```tsx
{editing && (
  <EditSourceDialog
    source={editing}
    open={editOpen}
    onOpenChange={(v) => { setEditOpen(v); if (!v) setEditing(null); }}
    onSaved={() => { refetchSources(); }}
  />
)}
```

(`refetchSources()` is whatever React Query refetch trigger the existing page uses; if the page uses `queryClient.invalidateQueries(["sources", tenantId])` then call that.)

- [ ] **Step 9: Add Edit-button tests**

In `web/tests/sources-page.test.tsx`, append:

```tsx
describe("SourcesPage edit action", () => {
  it("shows Edit button for admins", async () => {
    render(
      <TestProviders user={adminUser}>
        <SourcesPage />
      </TestProviders>,
    );
    const editButtons = await screen.findAllByRole("button", { name: /^edit$/i });
    expect(editButtons.length).toBeGreaterThan(0);
  });

  it("hides Edit button for operators", async () => {
    render(
      <TestProviders user={operatorUser}>
        <SourcesPage />
      </TestProviders>,
    );
    // Wait for the rows to render before asserting absence.
    await screen.findByText(/Operating EUR Bank/i);
    expect(screen.queryByRole("button", { name: /^edit$/i })).not.toBeInTheDocument();
  });

  it("clicking Edit opens the EditSourceDialog", async () => {
    const user = userEvent.setup();
    render(
      <TestProviders user={adminUser}>
        <SourcesPage />
      </TestProviders>,
    );
    const editButtons = await screen.findAllByRole("button", { name: /^edit$/i });
    await user.click(editButtons[0]);
    expect(await screen.findByRole("heading", { name: /edit source/i })).toBeVisible();
  });
});
```

(Replace `adminUser`/`operatorUser`/`SourcesPage`/the row name string with whatever the existing tests in this file already import/use.)

- [ ] **Step 10: Run tests**

```bash
cd web
pnpm vitest run tests/edit-source-dialog.test.tsx tests/sources-page.test.tsx 2>&1 | tail -20
```

Expected: all dialog + sources-page tests pass.

- [ ] **Step 11: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add web/components/app/edit-source-dialog.tsx \
        web/tests/edit-source-dialog.test.tsx \
        web/lib/api/client.ts \
        web/lib/api/mock.ts \
        web/lib/api/http.ts \
        web/app/\(app\)/sources/page.tsx \
        web/tests/sources-page.test.tsx
git commit -m "feat(web): edit-source-dialog + admin-only Edit row action on sources page"
```

---

## Task 10: MT942 in upload dialog

**Files:**
- Modify: `web/lib/api/client.ts` (extend `IngestFormat`)
- Modify: `web/components/app/upload-dialog.tsx`
- Modify: `web/tests/upload-dialog.test.tsx`

- [ ] **Step 1: Extend IngestFormat union**

In `web/lib/api/client.ts`, find `IngestFormat` and add `"mt942"`:

```ts
export type IngestFormat = "csv" | "camt053" | "mt940" | "mt942" | "bai2";
```

- [ ] **Step 2: Write failing tests**

Append to `web/tests/upload-dialog.test.tsx`:

```tsx
it("offers MT942 as the fifth format option", async () => {
  const user = userEvent.setup();
  render(
    <TestProviders>
      <UploadDialog source={base} open onOpenChange={() => {}} onUploaded={() => {}} />
    </TestProviders>,
  );
  await user.click(screen.getByRole("combobox", { name: /format/i }));
  expect(screen.getByText(/MT942 \(intra-day\)/i)).toBeInTheDocument();
});

it("hides CSV mapping fields when MT942 is selected", async () => {
  const user = userEvent.setup();
  render(
    <TestProviders>
      <UploadDialog source={base} open onOpenChange={() => {}} onUploaded={() => {}} />
    </TestProviders>,
  );
  await user.click(screen.getByRole("combobox", { name: /format/i }));
  await user.click(screen.getByText(/MT942 \(intra-day\)/i));
  expect(screen.queryByLabelText(/external ref column/i)).not.toBeInTheDocument();
});

it("shows the amber dialect-missing notice for MT942 when source has no dialect", async () => {
  const user = userEvent.setup();
  render(
    <TestProviders>
      <UploadDialog source={{ ...base, formatDialect: null }} open onOpenChange={() => {}} onUploaded={() => {}} />
    </TestProviders>,
  );
  await user.click(screen.getByRole("combobox", { name: /format/i }));
  await user.click(screen.getByText(/MT942 \(intra-day\)/i));
  expect(screen.getByText(/dialect is not set on this source/i)).toBeInTheDocument();
});
```

(Use whatever the existing test file calls the base source — likely `mockSource` or similar.)

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd web
pnpm vitest run tests/upload-dialog.test.tsx 2>&1 | tail -10
```

Expected: 3 new tests FAIL.

- [ ] **Step 4: Add MT942 option to the dialog**

In `web/components/app/upload-dialog.tsx`, find the `<SelectItem value="bai2">` line (currently line ~148) and add immediately above it:

```tsx
<SelectItem value="mt942">MT942 (intra-day)</SelectItem>
```

Find the conditional that shows the amber MT940 dialect-missing notice (currently around line ~153 — `{format === "mt940" && !source.formatDialect && (`) and broaden the condition to include MT942:

```tsx
{(format === "mt940" || format === "mt942") && !source.formatDialect && (
```

Update the notice copy to mention both formats:

```tsx
<div className="rounded border-amber-500/40 bg-amber-50 dark:bg-amber-950/40 p-3 text-sm">
  <strong>Dialect is not set on this source.</strong> MT940 and MT942 will parse
  as Generic. If this source receives Subfielded (DE/NL/BE) statements, edit the
  source first and set the dialect.
</div>
```

Find the file-accept extension switch (lines ~328–334) and add an `"mt942"` arm matching the MT940 one:

```tsx
case "mt942":
  return ".mt942,.sta,.txt,text/plain";
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd web
pnpm vitest run tests/upload-dialog.test.tsx 2>&1 | tail -10
```

Expected: all upload-dialog tests pass.

- [ ] **Step 6: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add web/lib/api/client.ts \
        web/components/app/upload-dialog.tsx \
        web/tests/upload-dialog.test.tsx
git commit -m "feat(web): MT942 in upload dialog (5 formats); amber dialect-missing notice extends to MT942"
```

---

## Task 11: Concurrent appender stress test

**Files:**
- Create: `backend/crates/recon-store/tests/audit_concurrent_appender.rs`

- [ ] **Step 1: Write the test**

Create `backend/crates/recon-store/tests/audit_concurrent_appender.rs`:

```rust
//! Concurrent-appender stress test for the audit chain.
//!
//! Spawns N parallel tasks that each append one audit event to the same
//! tenant's chain. Asserts:
//!   - all returned sequences are unique
//!   - the chain verifies clean
//!   - per-tenant isolation: two tenants writing in parallel both stay valid

use sqlx::PgPool;

async fn store() -> recon_store::Store {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    recon_store::Store::new(PgPool::connect(&url).await.unwrap())
}

async fn fresh_tenant(s: &recon_store::Store) -> (String, String) {
    let tid = format!("tenant-stress-{}", uuid::Uuid::new_v4());
    let aid = format!("user-stress-{}", uuid::Uuid::new_v4());
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,$1,$1)")
        .bind(&tid)
        .execute(s.pool())
        .await
        .unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ($1,'S','s@s.test',false)")
        .bind(&aid)
        .execute(s.pool())
        .await
        .unwrap();
    (tid, aid)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn fifty_parallel_appends_have_unique_sequences_and_verify_clean() {
    let s = std::sync::Arc::new(store().await);
    let (tid, aid) = fresh_tenant(&s).await;
    let n = 50i64;

    let handles: Vec<_> = (0..n)
        .map(|i| {
            let s = s.clone();
            let tid = tid.clone();
            let aid = aid.clone();
            tokio::spawn(async move {
                s.append_audit_standalone(
                    &tid,
                    &aid,
                    recon_audit::AuditPayload::AdminUserCreated {
                        user_id: format!("u-{i}"),
                        email: format!("u{i}@s.test"),
                        role: "operator".into(),
                    },
                )
                .await
                .unwrap()
            })
        })
        .collect();

    let mut seqs: Vec<i64> = Vec::with_capacity(n as usize);
    for h in handles {
        let row = h.await.unwrap();
        seqs.push(row.seq);
    }
    seqs.sort();
    assert_eq!(seqs, (1..=n).collect::<Vec<_>>());

    let result = s.verify_audit_chain(&tid, 1, n).await.unwrap();
    assert!(result.valid, "chain verify reported invalid: {result:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn two_tenants_interleaved_both_chains_valid() {
    let s = std::sync::Arc::new(store().await);
    let (t1, a1) = fresh_tenant(&s).await;
    let (t2, a2) = fresh_tenant(&s).await;
    let n_each = 25i64;

    let mut handles = Vec::new();
    for i in 0..(2 * n_each) {
        let s = s.clone();
        let (tid, aid) = if i % 2 == 0 { (t1.clone(), a1.clone()) } else { (t2.clone(), a2.clone()) };
        handles.push(tokio::spawn(async move {
            s.append_audit_standalone(
                &tid,
                &aid,
                recon_audit::AuditPayload::AdminUserCreated {
                    user_id: format!("u-{i}"),
                    email: format!("u{i}@s.test"),
                    role: "operator".into(),
                },
            )
            .await
            .unwrap()
        }));
    }
    for h in handles {
        let _ = h.await.unwrap();
    }

    let r1 = s.verify_audit_chain(&t1, 1, n_each).await.unwrap();
    let r2 = s.verify_audit_chain(&t2, 1, n_each).await.unwrap();
    assert!(r1.valid);
    assert!(r2.valid);
}
```

The test uses a helper `Store::append_audit_standalone` which begins its own transaction (the existing `append_audit` takes a `&mut Transaction` and is meant for same-tx use). Check whether such a standalone wrapper exists:

```bash
rg -n 'append_audit_standalone|pub async fn append_audit' backend/crates/recon-store/src 2>&1 | head -10
```

If it doesn't exist, add it to `recon-store/src/audit.rs` (or `lib.rs`):

```rust
impl Store {
    /// Append an audit event in its own transaction. Used by tests / out-of-band emitters.
    pub async fn append_audit_standalone(
        &self,
        tenant_id: &str,
        actor_id: &str,
        payload: recon_audit::AuditPayload,
    ) -> Result<recon_audit::AuditRow, StoreError> {
        let mut tx = self.pool.begin().await?;
        let row = self.append_audit(&mut tx, tenant_id, actor_id, payload).await?;
        tx.commit().await?;
        Ok(row)
    }
}
```

(The exact `AuditRow` return type already exists from `append_audit`; check `chain.rs` if the helper isn't already in this shape.)

- [ ] **Step 2: Run the test**

```bash
cd backend
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test -p recon-store --test audit_concurrent_appender 2>&1 | tail -20
```

Expected: both tests pass. If they fail with a unique-constraint violation, the chain logic has a race — file as a P0 bug and fix before continuing.

- [ ] **Step 3: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add backend/crates/recon-store/tests/audit_concurrent_appender.rs \
        backend/crates/recon-store/src/audit.rs 2>/dev/null || \
git add backend/crates/recon-store/tests/audit_concurrent_appender.rs \
        backend/crates/recon-store/src/lib.rs
git commit -m "test(audit): concurrent-appender stress test — 50 parallel appends + two-tenant interleave both verify clean"
```

---

## Task 12: Split audit/page.tsx into focused components

**Files:**
- Create: `web/app/(app)/audit/_components/audit-filter-bar.tsx`
- Create: `web/app/(app)/audit/_components/audit-table.tsx`
- Create: `web/app/(app)/audit/_components/verify-chain-dialog.tsx`
- Create: `web/app/(app)/audit/_components/anchor-now-button.tsx`
- Create: `web/app/(app)/audit/_components/event-detail-drawer.tsx`
- Modify: `web/app/(app)/audit/page.tsx` (shell only)

This task is a pure functional-equivalence refactor. The acceptance gate is: **all existing tests pass unchanged**. Any test that needs updating means the refactor changed behaviour — back it out and re-attempt the split.

- [ ] **Step 1: Snapshot existing test pass count**

```bash
cd web
pnpm vitest run --reporter=default 2>&1 | tail -3
```

Note the "passed" count from the summary line (e.g. `Tests: 197 passed`).

Then E2E:

```bash
cd web
pnpm e2e --reporter=line 2>&1 | tail -10
```

Note the E2E pass count (should be 16).

- [ ] **Step 2: Read the current page**

```bash
wc -l 'web/app/(app)/audit/page.tsx'
```

Open and identify the five extractable units:

1. `KindMultiSelect` and the filter form around it → `audit-filter-bar.tsx`
2. The main `<Table>` rendering + row-click handler → `audit-table.tsx`
3. `VerifyDialog` (and its trigger button) → `verify-chain-dialog.tsx`
4. The "Anchor now" button + toast logic + `AnchorHistory` → `anchor-now-button.tsx` (history can stay inline if it's small or split into its own component)
5. The selected-event payload viewer (currently a `Dialog`, around line ~252) → `event-detail-drawer.tsx`

- [ ] **Step 3: Extract each component, one at a time**

For each of the five components, follow this pattern:

1. Copy the component (with all its imports and types) into the new file under `_components/`.
2. Export it from the new file.
3. In `page.tsx`, replace the inline definition with an import + invocation.
4. Run the tests after each extraction to catch regressions early.

```bash
cd web
pnpm vitest run tests/audit-page.test.tsx 2>&1 | tail -5     # any existing audit-page test
pnpm e2e tests/e2e/compliance.spec.ts --reporter=line 2>&1 | tail -10
```

Both must continue to pass after each extraction.

- [ ] **Step 4: Final shell**

After all five extractions, `page.tsx` should look roughly like:

```tsx
"use client";

import * as React from "react";
import { useSearchParams } from "next/navigation";
import { AuditFilterBar } from "./_components/audit-filter-bar";
import { AuditTable } from "./_components/audit-table";
import { VerifyChainDialog } from "./_components/verify-chain-dialog";
import { AnchorNowButton } from "./_components/anchor-now-button";
import { EventDetailDrawer } from "./_components/event-detail-drawer";

export default function AuditPage() {
  const params = useSearchParams();
  const filter = React.useMemo(() => parseFilter(params), [params]);
  const [selectedEventId, setSelectedEventId] = React.useState<string | null>(null);

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Audit</h1>
        <div className="flex gap-2">
          <VerifyChainDialog />
          <AnchorNowButton />
        </div>
      </div>
      <AuditFilterBar filter={filter} />
      <AuditTable filter={filter} onRowClick={setSelectedEventId} />
      <EventDetailDrawer eventId={selectedEventId} onClose={() => setSelectedEventId(null)} />
    </div>
  );
}
```

(`parseFilter` is the existing URL-parsing helper — move it into a tiny `_components/filter-parser.ts` if it's complex, or keep it inline if it's a one-liner.)

- [ ] **Step 5: Verify page is under 150 LOC**

```bash
cd web
wc -l 'app/(app)/audit/page.tsx'
```

Expected: ≤150 lines.

- [ ] **Step 6: Run the full test suite**

```bash
cd web
pnpm vitest run --reporter=default 2>&1 | tail -3
pnpm e2e --reporter=line 2>&1 | tail -10
```

Expected: same pass count as the snapshot in Step 1. **If any test that was previously passing now fails, the refactor broke behaviour** — revert the breaking extraction and try a smaller incremental change.

- [ ] **Step 7: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add 'web/app/(app)/audit/page.tsx' 'web/app/(app)/audit/_components/'
git commit -m "refactor(web): split audit/page.tsx (853 LOC → ~100 LOC shell + 5 focused components) — pure functional-equivalence"
```

---

## Task 13: Display counterparty BIC/account in run-detail and exceptions tables

**Files:**
- Modify: `web/app/(app)/runs/[runId]/page.tsx` (or wherever the run-detail transactions table lives)
- Modify: `web/app/(app)/exceptions/page.tsx` (or wherever its transactions table lives)
- Modify: `web/lib/domain/types.ts` (extend `Transaction` schema)
- Modify: `web/tests/run-detail.test.tsx` and/or `web/tests/exceptions-page.test.tsx`

- [ ] **Step 1: Extend the Transaction zod schema**

In `web/lib/domain/types.ts`, find the `transactionSchema` (search for `external_ref` or `externalRef` and the schema definition) and add at the bottom:

```ts
  counterpartyBic: z.string().nullable().optional(),
  counterpartyAccount: z.string().nullable().optional(),
```

This makes both fields backward-compatible (older mock data without them still parses).

- [ ] **Step 2: Write the failing display test**

In `web/tests/run-detail.test.tsx` (or the equivalent for exceptions), append:

```tsx
it("shows counterparty BIC and account columns when any row has them", async () => {
  const txnsWithCpty = [
    { ...mockTxn, id: "txn-cpty", counterpartyBic: "DEUTDEFF", counterpartyAccount: "DE89370400440532013000" },
  ];
  render(
    <TestProviders fixtures={{ transactions: txnsWithCpty }}>
      <RunDetailPage runId="run-1" />
    </TestProviders>,
  );
  expect(await screen.findByText(/Cpty BIC/i)).toBeInTheDocument();
  expect(screen.getByText(/DEUTDEFF/)).toBeInTheDocument();
  expect(screen.getByText(/DE89370400440532013000/)).toBeInTheDocument();
});

it("hides counterparty columns when every row has both fields null", async () => {
  const txnsWithoutCpty = [
    { ...mockTxn, id: "txn-x", counterpartyBic: null, counterpartyAccount: null },
  ];
  render(
    <TestProviders fixtures={{ transactions: txnsWithoutCpty }}>
      <RunDetailPage runId="run-1" />
    </TestProviders>,
  );
  await screen.findByText(/Run/i); // wait for render
  expect(screen.queryByText(/Cpty BIC/i)).not.toBeInTheDocument();
});
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cd web
pnpm vitest run tests/run-detail.test.tsx 2>&1 | tail -10
```

Expected: 2 new tests FAIL.

- [ ] **Step 4: Add conditional columns to the transactions table**

In the run-detail page, find the transactions `<Table>` rendering. Compute upfront whether any row has counterparty data:

```tsx
const showCpty = transactions.some(
  (t) => t.counterpartyBic || t.counterpartyAccount,
);
```

In the `<TableHeader>`, after the existing column headers, add (only if `showCpty`):

```tsx
{showCpty && (
  <>
    <TableHead>Cpty BIC</TableHead>
    <TableHead>Cpty account</TableHead>
  </>
)}
```

In the `<TableBody>` row template, after the existing cells:

```tsx
{showCpty && (
  <>
    <TableCell className="font-mono text-xs">{t.counterpartyBic ?? "—"}</TableCell>
    <TableCell className="font-mono text-xs">{t.counterpartyAccount ?? "—"}</TableCell>
  </>
)}
```

Repeat the same pattern in `web/app/(app)/exceptions/page.tsx` (or wherever the exceptions transactions table is).

- [ ] **Step 5: Run tests**

```bash
cd web
pnpm vitest run tests/run-detail.test.tsx tests/exceptions-page.test.tsx 2>&1 | tail -10
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add web/lib/domain/types.ts \
        'web/app/(app)/runs/[runId]/page.tsx' \
        'web/app/(app)/exceptions/page.tsx' \
        web/tests/run-detail.test.tsx \
        web/tests/exceptions-page.test.tsx
git commit -m "feat(web): show conditional Cpty BIC + account columns in run-detail and exceptions transactions tables"
```

---

## Task 14: README documentation

**Files:**
- Modify: `web/README.md`

- [ ] **Step 1: Update the formats table**

In `web/README.md`, find the supported-formats table (currently around line 87) and add a row for MT942:

```markdown
| MT942 | Intra-day SWIFT report; same dialects as MT940; declared `:90D:`/`:90C:` totals are sanity-checked against parsed counts/sums |
```

In the bullet about source creation (currently around line 73), broaden "MT940 statements" to "MT940 / MT942 statements".

- [ ] **Step 2: Add a "Editing sources" subsection**

Append after the ingestion section:

```markdown
### Editing a source

Admins can rename a source and change its MT940/MT942 dialect after creation
via the **Edit** button on each row. Other fields (`kind`, `default currency`)
are immutable — to change them, create a new source. The change emits a
`source.updated` audit row inside the same transaction as the UPDATE.
```

- [ ] **Step 3: Run the README link-check (if any)**

If `pnpm` has a docs check, run it; otherwise just visually scan.

- [ ] **Step 4: Commit**

```bash
cd /home/nestinka/assistant/reconciliation-system
git add web/README.md
git commit -m "docs: README — MT942 + Editing a source"
```

---

## Task 15: E2E smoke + final verification

**Files:** (none — verification only)

- [ ] **Step 1: Run the entire backend suite**

```bash
cd /home/nestinka/assistant/reconciliation-system/backend
DATABASE_URL=postgres://recon:recon@localhost:5432/recon cargo test --workspace 2>&1 | tail -5
```

Expected: all tests pass; pass count is roughly 184 (Phase 6) + ~25 (Phase 7) = ~209.

- [ ] **Step 2: Run clippy on the whole workspace**

```bash
cd backend
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -10
```

Expected: no warnings.

- [ ] **Step 3: Run all frontend unit tests**

```bash
cd ../web
pnpm vitest run 2>&1 | tail -3
```

Expected: all tests pass; pass count is roughly 194 (Phase 6) + ~10 (Phase 7) = ~204.

- [ ] **Step 4: Run typecheck**

```bash
cd web
pnpm typecheck 2>&1 | tail -10
```

Expected: no errors.

- [ ] **Step 5: Run all 16 E2E tests against the live stack**

Make sure the stack is up first:

```bash
curl -sS http://localhost:8080/healthz   # expect: ok
curl -sS -o /dev/null -w '%{http_code}\n' http://localhost:3100/   # expect: 307
```

Then:

```bash
cd web
pnpm e2e --reporter=line 2>&1 | tail -10
```

Expected: 16/16 pass.

- [ ] **Step 6: Final smoke through the UI**

Open http://localhost:3100, sign in as `ada@acme.test` / `Password123!`, and manually verify:

1. **Sources page** — "Edit" button visible; clicking it opens the edit dialog pre-filled; renaming + setting dialect saves and refreshes.
2. **Upload dialog** — opening it shows 5 formats; selecting MT942 hides the CSV mapping; the dialect notice appears when the source has no dialect.
3. **Audit page** — looks and behaves identically to before the refactor; verify-chain still reports Valid; Anchor now still toasts seq.
4. **Run detail** — if a transaction has counterparty BIC/account (use the seed or upload an MT940 subfielded file to populate one), the columns appear; otherwise hidden.

- [ ] **Step 7: Memory update**

Update the memory pointer to reflect Phase 7 completion:

```bash
$EDITOR /home/nestinka/.claude/projects/-home-nestinka-assistant-reconciliation-system/memory/recon-ui-slice-status.md
```

Add a line like:

```
Phase 7 (Phase 5/6 polish) merged: counterparty BIC + account columns, MT942 parser (both dialects), PATCH /sources/:id, concurrent-appender stress test, audit/page.tsx split. Deferred: PDF parsing, multi-region scale, external SSO/OIDC.
```

- [ ] **Step 8: Push and open the PR**

```bash
cd /home/nestinka/assistant/reconciliation-system
git push -u origin feat/phase7-polish-bundle
gh pr create --title "Phase 7 — Phase 5/6 polish: MT942 + counterparty fields + PATCH source + concurrent-appender test + audit page split" --body "$(cat <<'EOF'
## Summary

Five deferred Phase 5/6 polish items, landed together in commit-per-item form:

1. **Migration 0006** — `counterparty_bic` + `counterparty_account` (nullable, BIC shape CHECK) on `canonical_transactions`. Additive; no rewrite; safe rollback.
2. **Counterparty plumbing** — fields flow through `ParsedTxn` + `CanonicalTransaction` + store inserts + SELECTs. **Matching engine unchanged** (YAGNI).
3. **Parser extraction** — CSV via optional column-index mapping; CAMT.053 from `<RltdPties>`/`<RltdAgts>` (Cdtr/Dbtr branches); MT940 Subfielded `?32` → account, `?33` → BIC; MT940 Generic + MT942 Generic + BAI v2 leave the fields `None`.
4. **MT942 parser** — both Generic and Subfielded via the new `mt94x_shared` module (decoder, `parse_tag`, `parse_61`, `parse_subfielded_86`, `Mt94xDialect`). `:90D:`/`:90C:` totals are sanity-checked against parsed counts/sums; balance tags rejected as invalid.
5. **PATCH /sources/:id** — admin-only; `name` + `format_dialect`; double-`Option` semantics on dialect (absent vs `null` vs value); audited as `source.updated` with before/after diff inside the same tx.
6. **Frontend** — `EditSourceDialog` + admin-only "Edit" row action on sources page; MT942 in upload dialog (5 formats); audit notice extends to MT942.
7. **Concurrent-appender stress test** — 50 parallel appends on one tenant → unique seqs `1..=50`, chain verifies clean; 25+25 two-tenant interleaved → both chains independently valid.
8. **audit/page.tsx split** — 853 LOC → ~100 LOC shell + 5 focused components (`AuditFilterBar`, `AuditTable`, `VerifyChainDialog`, `AnchorNowButton`, `EventDetailDrawer`). Pure functional equivalence — existing tests are the regression gate.
9. **Counterparty display** — run-detail + exceptions transactions tables show conditional `Cpty BIC` / `Cpty account` columns (hidden when all rows are null).

## Test plan

- [x] Backend `cargo test --workspace` green (~209 tests)
- [x] Backend `cargo clippy --workspace --all-targets -- -D warnings` clean
- [x] Frontend `pnpm vitest run` green (~204 tests)
- [x] Frontend `pnpm typecheck` clean
- [x] Playwright 16/16 green
- [x] Manual UI smoke (sources edit dialog; upload MT942; audit refactor parity; counterparty columns)

## Migration safety

`0006_transactions_counterparty.sql` is additive (two nullable columns + CHECK). Metadata-only; no table rewrite. Safe rollback. Old binary continues to function against the migrated schema.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Then capture the PR number from the output.

- [ ] **Step 9: Final code review**

After all task commits are pushed, dispatch a final code reviewer subagent over the full diff (`git diff origin/master..HEAD`). Address any findings before requesting merge.

---

## Done

All 15 tasks complete → Phase 7 lands on master.
