use recon_store::Store;
use recon_store::read::{BreakFilter, RunFilter};

#[sqlx::test]
async fn migrations_apply(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    store.migrate().await.expect("migrations apply");
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM tenants").fetch_one(&store.pool).await.unwrap();
    assert_eq!(n, 0);
}

async fn seed_two_tenants(store: &Store) {
    store.migrate().await.unwrap();
    for (t, name) in [("tenant-a", "Alpha"), ("tenant-b", "Beta")] {
        sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ($1,$2,$1)").bind(t).bind(name).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO users(id,tenant_id,name,role) VALUES ($1,$2,'U','operator')").bind(format!("u-{t}")).bind(t).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ($1,$2,'bank','S','GBP')").bind(format!("s-{t}")).bind(t).execute(&store.pool).await.unwrap();
        sqlx::query("INSERT INTO reconciliation_runs(id,tenant_id,name,source_a_id,source_b_id,status,started_at,config_version,stats) VALUES ($1,$2,'R',$3,$3,'completed', now(), 'v1', '{\"matched\":1,\"unmatched\":0,\"partial\":0,\"duplicate\":0,\"breakCount\":0,\"matchRatePct\":100.0,\"valueAtRiskMinor\":0}'::jsonb)")
            .bind(format!("run-{t}")).bind(t).bind(format!("s-{t}")).execute(&store.pool).await.unwrap();
    }
}

#[sqlx::test]
async fn tenant_isolation_on_runs(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_two_tenants(&store).await;
    let a = store.list_runs("tenant-a", &RunFilter::default()).await.unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].tenant_id, "tenant-a");
    let cross = store.get_run("tenant-a", "run-tenant-b").await;
    assert!(matches!(cross, Err(recon_store::StoreError::NotFound)));
}

#[sqlx::test]
async fn tenant_isolation_on_users(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_two_tenants(&store).await;
    let users = store.list_users("tenant-a").await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(store.list_breaks("tenant-a", &BreakFilter::default()).await.unwrap().len(), 0);
}

#[sqlx::test]
async fn dashboard_counts_open_breaks(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_two_tenants(&store).await;
    sqlx::query("INSERT INTO cases(id,tenant_id,break_id,status) VALUES ('c1','tenant-a','b1','open')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO breaks(id,tenant_id,run_id,case_id,type,status,value_minor,currency,txn_ids,opened_at) VALUES ('b1','tenant-a','run-tenant-a','c1','unmatched','open',1000,'GBP','{}', now())").execute(&store.pool).await.unwrap();
    let d = store.get_dashboard("tenant-a").await.unwrap();
    assert_eq!(d.open_breaks, 1);
    assert_eq!(d.value_at_risk_minor, 1000);
    assert_eq!(d.match_rate_pct, 100.0);
}
