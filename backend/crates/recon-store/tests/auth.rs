use recon_domain::UserRole;
use recon_store::Store;

async fn seed_user(pool: &sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t1','Acme','acme')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ('u1','Mia','mia@acme.test',false)")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ('u1','t1','approver')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES ('u1','$argon2id$dummy')")
        .execute(pool)
        .await
        .unwrap();
}

#[sqlx::test(migrations = "../../migrations")]
async fn find_credential_and_roles(pool: sqlx::PgPool) {
    seed_user(&pool).await;
    let store = Store::from_pool(pool);
    let (user, cred) = store
        .find_credential_by_email("mia@acme.test")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(user.id, "u1");
    assert_eq!(cred.password_hash, "$argon2id$dummy");
    assert_eq!(
        store.role_in_tenant("u1", "t1").await.unwrap(),
        Some(UserRole::Approver)
    );
    assert_eq!(store.role_in_tenant("u1", "nope").await.unwrap(), None);
    let ms = store.memberships_for("u1").await.unwrap();
    assert_eq!(ms.len(), 1);
    assert_eq!(ms[0].tenant_name, "Acme");
}

#[sqlx::test(migrations = "../../migrations")]
async fn lockout_counters(pool: sqlx::PgPool) {
    seed_user(&pool).await;
    let store = Store::from_pool(pool);
    store.record_login_failure("u1", None).await.unwrap();
    store
        .record_login_failure("u1", Some(9_999_999_999))
        .await
        .unwrap();
    assert_eq!(store.current_failed_attempts("u1").await.unwrap(), 2);
    let (_, cred) = store
        .find_credential_by_email("mia@acme.test")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cred.locked_until, Some(9_999_999_999));
    store.reset_login_failures("u1").await.unwrap();
    assert_eq!(store.current_failed_attempts("u1").await.unwrap(), 0);
}

#[sqlx::test(migrations = "../../migrations")]
async fn refresh_rotation_and_reuse(pool: sqlx::PgPool) {
    seed_user(&pool).await;
    let store = Store::from_pool(pool);
    store
        .insert_refresh("r1", "u1", "t1", "hash1", 9_999_999_999, None)
        .await
        .unwrap();
    assert!(store
        .find_live_refresh("hash1", 1000)
        .await
        .unwrap()
        .is_some());
    store.revoke_refresh("r1").await.unwrap();
    assert!(store
        .find_live_refresh("hash1", 1000)
        .await
        .unwrap()
        .is_none());
    assert!(store.refresh_is_revoked("hash1").await.unwrap());
}

#[sqlx::test(migrations = "../../migrations")]
async fn reset_token_single_use(pool: sqlx::PgPool) {
    seed_user(&pool).await;
    let store = Store::from_pool(pool);
    store
        .insert_reset_token("rt1", "u1", "rhash", 9_999_999_999)
        .await
        .unwrap();
    assert_eq!(
        store.consume_reset_token("rhash", 1000).await.unwrap(),
        Some("u1".into())
    );
    assert_eq!(
        store.consume_reset_token("rhash", 1000).await.unwrap(),
        None
    );
}

#[sqlx::test(migrations = "../../migrations")]
async fn create_and_list_users(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t1','Acme','acme')")
        .execute(&pool)
        .await
        .unwrap();
    let store = Store::from_pool(pool);
    store
        .create_user_with_membership(
            "u9",
            "New Op",
            "op@acme.test",
            "$argon2id$x",
            "t1",
            UserRole::Operator,
        )
        .await
        .unwrap();
    let users = store.list_users_in_tenant("t1").await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].email, "op@acme.test");
    assert_eq!(
        store
            .update_membership_role("u9", "t1", UserRole::Approver)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        store.role_in_tenant("u9", "t1").await.unwrap(),
        Some(UserRole::Approver)
    );
}

/// Fix 3: creating two users with the same email returns StoreError::Conflict.
#[sqlx::test(migrations = "../../migrations")]
async fn duplicate_email_returns_conflict(pool: sqlx::PgPool) {
    sqlx::query("INSERT INTO tenants(id,name,slug) VALUES ('t1','Acme','acme')")
        .execute(&pool)
        .await
        .unwrap();
    let store = Store::from_pool(pool);
    store
        .create_user_with_membership(
            "u-dup-1",
            "First",
            "shared@acme.test",
            "$argon2id$x",
            "t1",
            UserRole::Operator,
        )
        .await
        .unwrap();
    let err = store
        .create_user_with_membership(
            "u-dup-2",
            "Second",
            "shared@acme.test",
            "$argon2id$x",
            "t1",
            UserRole::Operator,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, recon_store::StoreError::Conflict(_)),
        "expected Conflict, got: {err:?}"
    );
}
