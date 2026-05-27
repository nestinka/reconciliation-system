//! update_source semantics: each PATCH variant + audit emission.

use recon_store::Store;

async fn fixture_source(store: &Store) -> (String, String, String) {
    let tenant_id = format!("tenant-test-{}", uuid::Uuid::new_v4());
    let actor_id = format!("user-{}", uuid::Uuid::new_v4());
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,'T','t')")
        .bind(&tenant_id)
        .execute(&store.pool)
        .await
        .unwrap();
    let src = store
        .create_source(
            &tenant_id,
            recon_domain::SourceKind::Bank,
            "Original",
            "EUR",
            &actor_id,
            None,
        )
        .await
        .unwrap();
    (tenant_id, actor_id, src.id)
}

#[sqlx::test(migrations = "../../migrations")]
async fn rename_only_changes_name_and_keeps_dialect_null(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    let (t, a, sid) = fixture_source(&store).await;
    let updated = store
        .update_source(&t, &sid, &a, Some("Renamed"), None)
        .await
        .unwrap();
    assert_eq!(updated.name, "Renamed");
    assert!(updated.format_dialect.is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn set_dialect_only_keeps_name_and_sets_dialect(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    let (t, a, sid) = fixture_source(&store).await;
    let updated = store
        .update_source(&t, &sid, &a, None, Some(Some("subfielded")))
        .await
        .unwrap();
    assert_eq!(updated.name, "Original");
    assert_eq!(updated.format_dialect.as_deref(), Some("subfielded"));
}

#[sqlx::test(migrations = "../../migrations")]
async fn clear_dialect_sets_it_back_to_null(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    let (t, a, sid) = fixture_source(&store).await;
    let _ = store
        .update_source(&t, &sid, &a, None, Some(Some("subfielded")))
        .await
        .unwrap();
    let updated = store
        .update_source(&t, &sid, &a, None, Some(None))
        .await
        .unwrap();
    assert!(updated.format_dialect.is_none());
}

#[sqlx::test(migrations = "../../migrations")]
async fn empty_patch_no_changes_still_emits_audit_row(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    let (t, a, sid) = fixture_source(&store).await;
    let updated = store
        .update_source(&t, &sid, &a, None, None)
        .await
        .unwrap();
    assert_eq!(updated.name, "Original");
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM audit_events WHERE tenant_id=$1 AND kind='data.source.updated'",
    )
    .bind(&t)
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(count, 1);
}

#[sqlx::test(migrations = "../../migrations")]
async fn cross_tenant_update_returns_not_found(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    let (_, a, sid) = fixture_source(&store).await;
    let other = format!("tenant-other-{}", uuid::Uuid::new_v4());
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,'O','o')")
        .bind(&other)
        .execute(&store.pool)
        .await
        .unwrap();
    let err = store
        .update_source(&other, &sid, &a, Some("X"), None)
        .await
        .unwrap_err();
    assert!(matches!(err, recon_store::StoreError::NotFound));
}
