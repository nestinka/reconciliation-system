use recon_audit::{AuditKind, AuditPayload};
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

#[sqlx::test(migrations = "../../migrations")]
async fn append_audit_chains_per_tenant(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t'),('u','U','u')")
        .execute(&store.pool).await.unwrap();

    let mut tx = store.pool.begin().await.unwrap();
    let e1 = store.append_audit(&mut tx, "t", "system",
        AuditPayload::AuthLogout { user_id: "user-1".into(), ip: None }).await.unwrap();
    let e2 = store.append_audit(&mut tx, "t", "system",
        AuditPayload::AuthLogout { user_id: "user-2".into(), ip: None }).await.unwrap();
    let f1 = store.append_audit(&mut tx, "u", "system",
        AuditPayload::AuthLogout { user_id: "user-3".into(), ip: None }).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(e1.seq, 1);
    assert_eq!(e2.seq, 2);
    assert_eq!(e2.prev_hash, e1.hash, "chain links inside a tenant");
    assert_eq!(f1.seq, 1, "the other tenant has its own seq=1");
    assert_eq!(f1.prev_hash, [0u8; 32]);
    assert_eq!(e1.kind, AuditKind::AuthLogout);
}
