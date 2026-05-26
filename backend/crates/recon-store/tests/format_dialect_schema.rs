use recon_store::Store;

#[sqlx::test(migrations = "../../migrations")]
async fn format_dialect_column_accepts_valid_values(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    // generic + subfielded + NULL all accepted.
    for (id, dialect) in [("s1", Some("generic")), ("s2", Some("subfielded")), ("s3", None)] {
        sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency,format_dialect) VALUES ($1,'t','bank','S','GBP',$2)")
            .bind(id).bind(dialect).execute(&store.pool).await.unwrap();
    }
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM sources WHERE tenant_id='t'")
        .fetch_one(&store.pool).await.unwrap();
    assert_eq!(n, 3);
}

#[sqlx::test(migrations = "../../migrations")]
async fn format_dialect_check_constraint_rejects_invalid(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    let err = sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency,format_dialect) VALUES ('s','t','bank','S','GBP','wat')")
        .execute(&store.pool).await;
    assert!(err.is_err(), "CHECK constraint must reject 'wat'");
}
