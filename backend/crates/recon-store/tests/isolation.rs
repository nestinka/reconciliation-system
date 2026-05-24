use recon_store::Store;

#[sqlx::test]
async fn migrations_apply(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    store.migrate().await.expect("migrations apply");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM tenants").fetch_one(&store.pool).await.unwrap();
    assert_eq!(n, 0);
}
