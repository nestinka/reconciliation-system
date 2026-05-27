//! Concurrent-appender stress test for the audit chain.
//!
//! Proves that `append_audit`'s `FOR UPDATE` serialization preserves chain
//! integrity under parallel writers — sequences are unique and contiguous,
//! and the chain verifies clean. Two tenants writing in parallel keep their
//! chains independent.

use recon_audit::chain::VerifyStatus;
use recon_store::Store;
use std::sync::Arc;

/// Insert a tenant and a user (no membership needed — actor_id is just stored
/// as a free-text string in audit_events, not FK-validated).
async fn fresh_tenant(store: &Store) -> (String, String) {
    let tid = format!("tenant-stress-{}", uuid::Uuid::new_v4());
    let aid = format!("user-stress-{}", uuid::Uuid::new_v4());
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,'Stress','stress')")
        .bind(&tid)
        .execute(&store.pool)
        .await
        .unwrap();
    // users table (post-migration) has: id, name, email, disabled — no tenant_id.
    sqlx::query(
        "INSERT INTO users(id,name,email,disabled) VALUES ($1,'Stress',$2,false)",
    )
    .bind(&aid)
    .bind(format!("{aid}@s.test"))
    .execute(&store.pool)
    .await
    .unwrap();
    (tid, aid)
}

/// 50 tasks all append to ONE tenant concurrently.
/// Expected: sequences are exactly 1..=50, chain verifies clean.
#[sqlx::test(migrations = "../../migrations")]
async fn fifty_parallel_appends_have_unique_sequences_and_verify_clean(pool: sqlx::PgPool) {
    let store = Arc::new(Store::from_pool(pool));
    let (tid, aid) = fresh_tenant(&store).await;
    let n: i64 = 50;

    let handles: Vec<_> = (0..n)
        .map(|i| {
            let store = store.clone();
            let tid = tid.clone();
            let aid = aid.clone();
            tokio::spawn(async move {
                store
                    .append_audit_standalone(
                        &tid,
                        &aid,
                        recon_audit::AuditPayload::AdminUserCreated {
                            user_id: format!("u-{i}"),
                            email: format!("u{i}@s.test"),
                            role: "operator".into(),
                        },
                    )
                    .await
                    .expect("append should succeed")
            })
        })
        .collect();

    let mut seqs: Vec<i64> = Vec::with_capacity(n as usize);
    for h in handles {
        let entry = h.await.expect("task should not panic");
        seqs.push(entry.seq);
    }
    seqs.sort();
    assert_eq!(
        seqs,
        (1..=n).collect::<Vec<_>>(),
        "expected sequences to be exactly 1..={n}, got {seqs:?}"
    );

    let result = store.verify_audit(&tid, None, None, None).await.unwrap();
    assert_eq!(
        result.status,
        VerifyStatus::Valid,
        "chain verify reported invalid: {result:?}"
    );
    assert_eq!(result.checked, n, "all {n} rows must have been checked");
}

/// 25 + 25 tasks across TWO tenants, interleaved submission order.
/// Expected: both per-tenant chains independently valid, each with exactly 25 rows.
#[sqlx::test(migrations = "../../migrations")]
async fn two_tenants_interleaved_both_chains_valid(pool: sqlx::PgPool) {
    let store = Arc::new(Store::from_pool(pool));
    let (t1, a1) = fresh_tenant(&store).await;
    let (t2, a2) = fresh_tenant(&store).await;
    let n_each: i64 = 25;

    let mut handles = Vec::new();
    for i in 0..(2 * n_each) {
        let store = store.clone();
        let (tid, aid) = if i % 2 == 0 {
            (t1.clone(), a1.clone())
        } else {
            (t2.clone(), a2.clone())
        };
        handles.push(tokio::spawn(async move {
            store
                .append_audit_standalone(
                    &tid,
                    &aid,
                    recon_audit::AuditPayload::AdminUserCreated {
                        user_id: format!("u-{i}"),
                        email: format!("u{i}@s.test"),
                        role: "operator".into(),
                    },
                )
                .await
                .expect("append should succeed")
        }));
    }
    for h in handles {
        let _ = h.await.expect("task should not panic");
    }

    let r1 = store.verify_audit(&t1, None, None, None).await.unwrap();
    let r2 = store.verify_audit(&t2, None, None, None).await.unwrap();
    assert_eq!(r1.status, VerifyStatus::Valid, "tenant 1 chain invalid: {r1:?}");
    assert_eq!(r2.status, VerifyStatus::Valid, "tenant 2 chain invalid: {r2:?}");

    // Each tenant should have exactly n_each rows.
    let count_t1: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE tenant_id=$1")
            .bind(&t1)
            .fetch_one(&store.pool)
            .await
            .unwrap();
    let count_t2: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE tenant_id=$1")
            .bind(&t2)
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(count_t1, n_each, "tenant 1 should have {n_each} rows");
    assert_eq!(count_t2, n_each, "tenant 2 should have {n_each} rows");
}
