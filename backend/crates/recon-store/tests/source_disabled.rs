//! list_sources include_archived filter: disabled sources are hidden by default.

use recon_store::Store;

#[sqlx::test(migrations = "../../migrations")]
async fn list_sources_hides_disabled_unless_included(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    let tenant_id = format!("tenant-test-{}", uuid::Uuid::new_v4());
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,'T','t')")
        .bind(&tenant_id)
        .execute(&store.pool)
        .await
        .unwrap();

    let src = store
        .create_source(
            &tenant_id,
            recon_domain::SourceKind::Bank,
            "S",
            "GBP",
            "actor",
            None,
            None,
        )
        .await
        .unwrap();

    // Active source appears without include_archived.
    let active = store.list_sources(&tenant_id, false).await.unwrap();
    assert_eq!(active.len(), 1, "active source should be listed");

    // Disable the source via raw SQL.
    sqlx::query("UPDATE sources SET disabled=true WHERE id=$1")
        .bind(&src.id)
        .execute(&store.pool)
        .await
        .unwrap();

    // Disabled source hidden by default.
    let hidden = store.list_sources(&tenant_id, false).await.unwrap();
    assert_eq!(hidden.len(), 0, "disabled source should be hidden by default");

    // Disabled source visible with include_archived=true.
    let archived = store.list_sources(&tenant_id, true).await.unwrap();
    assert_eq!(archived.len(), 1, "disabled source should appear when include_archived=true");
    assert!(archived[0].source.disabled, "returned source should have disabled=true");
}
