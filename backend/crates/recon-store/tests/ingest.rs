use recon_domain::SourceKind;
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
