use recon_store::Store;

#[sqlx::test(migrations = "../../migrations")]
async fn audit_tables_exist(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    // Inserting a no-op tenant + a single row exercises the schema.
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    sqlx::query(
        "INSERT INTO audit_events(tenant_id,seq,at,actor_id,kind,payload,prev_hash,hash) \
         VALUES ('t',1, now(),'system','auth.logout','{}'::jsonb, $1, $2)",
    )
    .bind(vec![0u8; 32])
    .bind(vec![1u8; 32])
    .execute(&store.pool).await.unwrap();
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_events WHERE tenant_id='t'")
        .fetch_one(&store.pool).await.unwrap();
    assert_eq!(n, 1);
    // Composite PK rejects a duplicate seq for the same tenant.
    let err = sqlx::query(
        "INSERT INTO audit_events(tenant_id,seq,at,actor_id,kind,payload,prev_hash,hash) \
         VALUES ('t',1, now(),'system','auth.logout','{}'::jsonb, $1, $2)",
    )
    .bind(vec![0u8; 32])
    .bind(vec![2u8; 32])
    .execute(&store.pool).await;
    assert!(err.is_err(), "duplicate (tenant_id,seq) must violate the PK");
}
