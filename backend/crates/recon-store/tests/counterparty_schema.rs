//! Migration 0006 schema invariants for canonical_transactions.counterparty_*.

use recon_store::Store;
use sqlx::Row;

async fn seed_tenant_and_source(
    pool: &sqlx::PgPool,
    tenant_id: &str,
    source_id: &str,
) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,'t','t')")
        .bind(tenant_id)
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ($1,$2,'bank','t','EUR')")
        .bind(source_id)
        .bind(tenant_id)
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn valid_8_char_bic_accepted(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_tenant_and_source(&store.pool, "tenant-1", "src-1").await;
    sqlx::query(
        "INSERT INTO canonical_transactions(\
            id, tenant_id, source_id, external_ref, value_date, posted_at, \
            amount_minor, currency, direction, counterparty, description, \
            counterparty_bic, counterparty_account) \
         VALUES ('txn-1','tenant-1','src-1','ref-1','2026-01-01'::date,'2026-01-01T00:00:00Z'::timestamptz,\
                 100,'EUR','credit',NULL,'','DEUTDEFF',NULL)",
    )
    .execute(&store.pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn valid_11_char_bic_accepted(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_tenant_and_source(&store.pool, "tenant-2", "src-2").await;
    sqlx::query(
        "INSERT INTO canonical_transactions(\
            id, tenant_id, source_id, external_ref, value_date, posted_at, \
            amount_minor, currency, direction, counterparty, description, \
            counterparty_bic, counterparty_account) \
         VALUES ('txn-2','tenant-2','src-2','ref-2','2026-01-01'::date,'2026-01-01T00:00:00Z'::timestamptz,\
                 100,'EUR','credit',NULL,'','DEUTDEFF500',NULL)",
    )
    .execute(&store.pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn null_bic_accepted(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_tenant_and_source(&store.pool, "tenant-3", "src-3").await;
    sqlx::query(
        "INSERT INTO canonical_transactions(\
            id, tenant_id, source_id, external_ref, value_date, posted_at, \
            amount_minor, currency, direction, counterparty, description, \
            counterparty_bic, counterparty_account) \
         VALUES ('txn-3','tenant-3','src-3','ref-3','2026-01-01'::date,'2026-01-01T00:00:00Z'::timestamptz,\
                 100,'EUR','credit',NULL,'',NULL,NULL)",
    )
    .execute(&store.pool)
    .await
    .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn lowercase_bic_rejected(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_tenant_and_source(&store.pool, "tenant-4", "src-4").await;
    let err = sqlx::query(
        "INSERT INTO canonical_transactions(\
            id, tenant_id, source_id, external_ref, value_date, posted_at, \
            amount_minor, currency, direction, counterparty, description, \
            counterparty_bic, counterparty_account) \
         VALUES ('txn-4','tenant-4','src-4','ref-4','2026-01-01'::date,'2026-01-01T00:00:00Z'::timestamptz,\
                 100,'EUR','credit',NULL,'','deutdeff',NULL)",
    )
    .execute(&store.pool)
    .await;
    let msg = format!("{:?}", err);
    assert!(err.is_err(), "lowercase BIC should be rejected");
    assert!(
        msg.contains("chk_counterparty_bic_shape"),
        "expected CHECK constraint error, got: {msg}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn wrong_length_bic_rejected(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_tenant_and_source(&store.pool, "tenant-5", "src-5").await;
    let err = sqlx::query(
        "INSERT INTO canonical_transactions(\
            id, tenant_id, source_id, external_ref, value_date, posted_at, \
            amount_minor, currency, direction, counterparty, description, \
            counterparty_bic, counterparty_account) \
         VALUES ('txn-5','tenant-5','src-5','ref-5','2026-01-01'::date,'2026-01-01T00:00:00Z'::timestamptz,\
                 100,'EUR','credit',NULL,'','DEUTDEF',NULL)",
    )
    .execute(&store.pool)
    .await;
    let msg = format!("{:?}", err);
    assert!(err.is_err(), "7-char BIC should be rejected");
    assert!(
        msg.contains("chk_counterparty_bic_shape"),
        "expected CHECK constraint error, got: {msg}"
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn account_round_trips(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_tenant_and_source(&store.pool, "tenant-6", "src-6").await;
    sqlx::query(
        "INSERT INTO canonical_transactions(\
            id, tenant_id, source_id, external_ref, value_date, posted_at, \
            amount_minor, currency, direction, counterparty, description, \
            counterparty_bic, counterparty_account) \
         VALUES ('txn-6','tenant-6','src-6','r1','2026-01-01'::date,'2026-01-01T00:00:00Z'::timestamptz,\
                 100,'EUR','credit',NULL,'','DEUTDEFF','DE89370400440532013000')",
    )
    .execute(&store.pool)
    .await
    .unwrap();
    let row = sqlx::query("SELECT counterparty_bic, counterparty_account FROM canonical_transactions WHERE id='txn-6'")
        .fetch_one(&store.pool)
        .await
        .unwrap();
    let bic: Option<String> = row.try_get("counterparty_bic").unwrap();
    let acc: Option<String> = row.try_get("counterparty_account").unwrap();
    assert_eq!(bic.as_deref(), Some("DEUTDEFF"));
    assert_eq!(acc.as_deref(), Some("DE89370400440532013000"));
}
