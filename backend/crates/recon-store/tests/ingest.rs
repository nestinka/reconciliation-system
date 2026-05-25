use recon_domain::{CanonicalTransaction, Direction, SourceKind};
use recon_store::Store;

#[sqlx::test(migrations = "../../migrations")]
async fn unique_constraint_blocks_duplicate_ref(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    // minimal tenant + source
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ('s','t','bank','Bank','GBP')")
        .execute(&store.pool).await.unwrap();
    let insert_dup = |r: String| {
        let pool = store.pool.clone();
        async move {
            sqlx::query(
                "INSERT INTO canonical_transactions(id,tenant_id,source_id,external_ref,value_date,posted_at,amount_minor,currency,direction,description) \
                 VALUES ($1,'t','s',$2,'2026-05-10','2026-05-10T00:00:00Z'::timestamptz,100,'GBP','debit','x')")
                .bind(format!("txn-{r}")).bind(r).execute(&pool).await
        }
    };
    insert_dup("DUP".to_string()).await.unwrap();
    let second = insert_dup("DUP".to_string()).await;
    assert!(second.is_err(), "second insert of same (source,ref) must violate the unique constraint");
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_and_list_sources(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    let s = store.create_source("t", SourceKind::Bank, "Acme Bank", "GBP").await.unwrap();
    assert!(s.id.starts_with("src-"));
    let got = store.get_source("t", &s.id).await.unwrap();
    assert_eq!(got.name, "Acme Bank");
    let list = store.list_sources("t").await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].txn_count, 0);
    // cross-tenant get is NotFound
    assert!(store.get_source("other", &s.id).await.is_err());
}

fn txn(id: &str, eref: &str) -> CanonicalTransaction {
    CanonicalTransaction {
        id: id.into(),
        tenant_id: "t".into(),
        source_id: "s".into(),
        external_ref: eref.into(),
        value_date: "2026-05-10".into(),
        posted_at: "2026-05-10T00:00:00Z".into(),
        amount_minor: 100,
        currency: "GBP".into(),
        direction: Direction::Debit,
        counterparty: None,
        description: "x".into(),
    }
}

async fn seed_source(store: &Store) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ('s','t','bank','Bank','GBP')").execute(&store.pool).await.unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_happy_path(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_source(&store).await;
    let n = store.ingest_transactions("t", "s", &[txn("txn-1", "R1"), txn("txn-2", "R2")]).await.unwrap();
    assert_eq!(n, 2);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM canonical_transactions WHERE source_id='s'")
        .fetch_one(&store.pool).await.unwrap();
    assert_eq!(count, 2);
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_rejects_within_batch_dup(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_source(&store).await;
    let err = store.ingest_transactions("t", "s", &[txn("txn-1", "R1"), txn("txn-2", "R1")]).await.unwrap_err();
    match err {
        recon_store::StoreError::DuplicateRefs(refs) => assert_eq!(refs, vec!["R1".to_string()]),
        other => panic!("expected DuplicateRefs, got {other:?}"),
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM canonical_transactions WHERE source_id='s'")
        .fetch_one(&store.pool).await.unwrap();
    assert_eq!(count, 0, "nothing stored on rejection");
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_rejects_existing_ref(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_source(&store).await;
    store.ingest_transactions("t", "s", &[txn("txn-1", "R1")]).await.unwrap();
    let err = store.ingest_transactions("t", "s", &[txn("txn-2", "R1")]).await.unwrap_err();
    assert!(matches!(err, recon_store::StoreError::DuplicateRefs(_)));
}

#[sqlx::test(migrations = "../../migrations")]
async fn ingest_into_foreign_source_is_not_found(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_source(&store).await;
    let err = store.ingest_transactions("other", "s", &[txn("txn-1", "R1")]).await.unwrap_err();
    assert!(matches!(err, recon_store::StoreError::NotFound));
}
