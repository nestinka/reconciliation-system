use recon_store::Store;
use recon_domain::{NewCaseEvent, CaseEventBody, BreakStatus};

async fn seed_pending(store: &Store) {
    store.migrate().await.unwrap();
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t','T','t')").execute(&store.pool).await.unwrap();
    for (id, role) in [("user-mia","operator"),("user-theo","approver")] {
        sqlx::query("INSERT INTO users(id,tenant_id,name,role) VALUES ($1,'t',$1,$2)").bind(id).bind(role).execute(&store.pool).await.unwrap();
    }
    sqlx::query("INSERT INTO sources(id,tenant_id,kind,name,currency) VALUES ('s','t','bank','S','GBP')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO reconciliation_runs(id,tenant_id,name,source_a_id,source_b_id,status,started_at,config_version,stats) VALUES ('r','t','R','s','s','completed',now(),'v1','{}'::jsonb)").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO cases(id,tenant_id,break_id,assignee_id,status) VALUES ('case-pending','t','break-pending','user-mia','pending_approval')").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO breaks(id,tenant_id,run_id,case_id,type,status,value_minor,currency,assignee_id,txn_ids,opened_at) VALUES ('break-pending','t','r','case-pending','unmatched','pending_approval',125000,'GBP','user-mia','{}', now())").execute(&store.pool).await.unwrap();
    sqlx::query("INSERT INTO case_events(id,tenant_id,case_id,seq,kind,actor_id,at,payload) VALUES ('e1','t','case-pending',1,'approval_requested','user-mia',now(),'{\"resolution\":\"write_off\"}'::jsonb)").execute(&store.pool).await.unwrap();
}

#[sqlx::test]
async fn maker_approval_is_forbidden(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_pending(&store).await;
    let ev = NewCaseEvent { actor_id: "user-mia".into(), body: CaseEventBody::Approved {} };
    let r = store.append_case_event("t", "case-pending", ev).await;
    assert!(matches!(r, Err(recon_store::StoreError::Forbidden(_))));
    let c = store.load_case("t", "case-pending").await.unwrap();
    assert_eq!(c.status, BreakStatus::PendingApproval);
}

#[sqlx::test]
async fn different_approver_resolves(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_pending(&store).await;
    let ev = NewCaseEvent { actor_id: "user-theo".into(), body: CaseEventBody::Approved {} };
    let c = store.append_case_event("t", "case-pending", ev).await.unwrap();
    assert_eq!(c.status, BreakStatus::Resolved);
    assert!(c.events.iter().any(|e| matches!(e.body, CaseEventBody::Approved {})));
}

#[sqlx::test]
async fn comment_is_append_only_and_keeps_status(pool: sqlx::PgPool) {
    let store = Store::from_pool(pool);
    seed_pending(&store).await;
    let before = store.load_case("t", "case-pending").await.unwrap().events.len();
    let ev = NewCaseEvent { actor_id: "user-mia".into(), body: CaseEventBody::Comment { text: "hi".into() } };
    let c = store.append_case_event("t", "case-pending", ev).await.unwrap();
    assert_eq!(c.events.len(), before + 1);
    assert_eq!(c.status, BreakStatus::PendingApproval);
}
