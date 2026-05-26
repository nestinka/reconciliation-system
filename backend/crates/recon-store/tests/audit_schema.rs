use recon_audit::chain::VerifyStatus;
use recon_audit::{AuditKind, AuditPayload};
use recon_store::audit::AuditFilter;
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

#[sqlx::test(migrations = "../../migrations")]
async fn list_and_verify_round_trip(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    for i in 0..5 {
        store.append_audit(&mut tx, "t", "system",
            AuditPayload::AuthLogout { user_id: format!("user-{i}"), ip: None }).await.unwrap();
    }
    tx.commit().await.unwrap();

    let page = store.list_audit("t", &AuditFilter { limit: 100, ..Default::default() }).await.unwrap();
    assert_eq!(page.items.len(), 5);
    assert_eq!(page.items.first().unwrap().seq, 5, "descending");
    assert_eq!(page.items.last().unwrap().seq, 1);
    assert!(page.next_cursor.is_none());

    let outcome = store.verify_audit("t", None, None, None).await.unwrap();
    assert_eq!(outcome.status, VerifyStatus::Valid);
    assert_eq!(outcome.checked, 5);
}

#[sqlx::test(migrations = "../../migrations")]
async fn verify_detects_payload_tamper(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')")
        .execute(&store.pool).await.unwrap();
    let mut tx = store.pool.begin().await.unwrap();
    store.append_audit(&mut tx, "t", "system",
        AuditPayload::AuthLogout { user_id: "u1".into(), ip: None }).await.unwrap();
    store.append_audit(&mut tx, "t", "system",
        AuditPayload::AuthLogout { user_id: "u2".into(), ip: None }).await.unwrap();
    tx.commit().await.unwrap();

    // Manually tamper with row seq=2.
    sqlx::query(
        "UPDATE audit_events SET payload = jsonb_set(payload, '{data,user_id}', '\"evil\"') \
         WHERE tenant_id='t' AND seq=2",
    )
    .execute(&store.pool).await.unwrap();

    let outcome = store.verify_audit("t", None, None, None).await.unwrap();
    assert_eq!(outcome.status, VerifyStatus::Invalid);
    assert_eq!(outcome.first_broken_seq, Some(2));
    assert_eq!(outcome.reason, Some(recon_audit::chain::VerifyReason::Tampered));
}
