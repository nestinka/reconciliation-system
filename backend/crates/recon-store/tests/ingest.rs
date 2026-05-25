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
