use crate::{Store, StoreError};
use recon_domain::{Membership, Tenant, User, UserRole};

pub struct Credential {
    pub user_id: String,
    pub password_hash: String,
    pub failed_attempts: i32,
    pub locked_until: Option<i64>,
}

fn parse_role(s: &str) -> UserRole {
    match s {
        "approver" => UserRole::Approver,
        "admin" => UserRole::Admin,
        _ => UserRole::Operator,
    }
}
fn role_str(r: UserRole) -> &'static str {
    match r {
        UserRole::Operator => "operator",
        UserRole::Approver => "approver",
        UserRole::Admin => "admin",
    }
}

impl Store {
    /// User + credential by email (global identity). The returned User.role is a placeholder
    /// (Operator); callers resolve the real role per-tenant via `role_in_tenant`.
    pub async fn find_credential_by_email(
        &self,
        email: &str,
    ) -> Result<Option<(User, Credential)>, StoreError> {
        let row = sqlx::query_as::<
            _,
            (
                String,
                String,
                bool,
                String,
                i32,
                Option<time::OffsetDateTime>,
            ),
        >(
            "SELECT u.id, u.name, u.disabled, c.password_hash, c.failed_attempts, c.locked_until \
             FROM users u JOIN user_credentials c ON c.user_id = u.id WHERE u.email = $1",
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(row.map(|(id, name, disabled, hash, fa, lu)| {
            let user = User {
                id: id.clone(),
                name,
                email: email.to_string(),
                disabled,
                role: UserRole::Operator,
            };
            let cred = Credential {
                user_id: id,
                password_hash: hash,
                failed_attempts: fa,
                locked_until: lu.map(|t| t.unix_timestamp()),
            };
            (user, cred)
        }))
    }

    pub async fn memberships_for(&self, user_id: &str) -> Result<Vec<Membership>, StoreError> {
        let rows = sqlx::query_as::<_, (String, String, String)>(
            "SELECT m.tenant_id, t.name, m.role FROM memberships m JOIN tenants t ON t.id = m.tenant_id \
             WHERE m.user_id = $1 ORDER BY t.name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(rows
            .into_iter()
            .map(|(tid, tn, role)| Membership {
                tenant_id: tid,
                tenant_name: tn,
                role: parse_role(&role),
            })
            .collect())
    }

    pub async fn role_in_tenant(
        &self,
        user_id: &str,
        tenant_id: &str,
    ) -> Result<Option<UserRole>, StoreError> {
        let r = sqlx::query_scalar::<_, String>(
            "SELECT role FROM memberships WHERE user_id=$1 AND tenant_id=$2",
        )
        .bind(user_id)
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(r.map(|s| parse_role(&s)))
    }

    // --- lockout / credential mutations ---
    pub async fn record_login_failure(
        &self,
        user_id: &str,
        locked_until_unix: Option<i64>,
    ) -> Result<(), StoreError> {
        let lu = locked_until_unix
            .map(|u| time::OffsetDateTime::from_unix_timestamp(u).unwrap());
        sqlx::query(
            "UPDATE user_credentials SET failed_attempts = failed_attempts + 1, locked_until = $2 WHERE user_id = $1",
        )
        .bind(user_id)
        .bind(lu)
        .execute(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(())
    }

    pub async fn reset_login_failures(&self, user_id: &str) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE user_credentials SET failed_attempts = 0, locked_until = NULL WHERE user_id = $1",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(())
    }

    pub async fn current_failed_attempts(&self, user_id: &str) -> Result<i32, StoreError> {
        sqlx::query_scalar::<_, i32>(
            "SELECT failed_attempts FROM user_credentials WHERE user_id=$1",
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn set_password(&self, user_id: &str, password_hash: &str) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE user_credentials SET password_hash=$2, password_updated_at=now(), failed_attempts=0, locked_until=NULL WHERE user_id=$1",
        )
        .bind(user_id)
        .bind(password_hash)
        .execute(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(())
    }

    // --- refresh tokens ---
    pub async fn insert_refresh(
        &self,
        id: &str,
        user_id: &str,
        tenant_id: &str,
        token_hash: &str,
        expires_at_unix: i64,
        rotated_from: Option<&str>,
    ) -> Result<(), StoreError> {
        let exp = time::OffsetDateTime::from_unix_timestamp(expires_at_unix).unwrap();
        sqlx::query(
            "INSERT INTO refresh_tokens(id,user_id,tenant_id,token_hash,expires_at,rotated_from) VALUES ($1,$2,$3,$4,$5,$6)",
        )
        .bind(id)
        .bind(user_id)
        .bind(tenant_id)
        .bind(token_hash)
        .bind(exp)
        .bind(rotated_from)
        .execute(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(())
    }

    pub async fn find_live_refresh(
        &self,
        token_hash: &str,
        now_unix: i64,
    ) -> Result<Option<(String, String, String)>, StoreError> {
        let now = time::OffsetDateTime::from_unix_timestamp(now_unix).unwrap();
        let r = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id,user_id,tenant_id FROM refresh_tokens WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at > $2",
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(r)
    }

    pub async fn refresh_is_revoked(&self, token_hash: &str) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM refresh_tokens WHERE token_hash=$1 AND revoked_at IS NOT NULL",
        )
        .bind(token_hash)
        .fetch_one(&self.pool)
        .await
        .map_err(StoreError::from)?
            > 0)
    }

    /// Find the owning user of any refresh row by hash (used for reuse-detection revocation).
    pub async fn refresh_owner(&self, token_hash: &str) -> Result<Option<String>, StoreError> {
        sqlx::query_scalar::<_, String>(
            "SELECT user_id FROM refresh_tokens WHERE token_hash=$1 LIMIT 1",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)
    }

    pub async fn revoke_refresh(&self, id: &str) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at=now() WHERE id=$1 AND revoked_at IS NULL",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(())
    }

    /// Revoke by token hash (used by logout, which only has the cookie's hash).
    pub async fn revoke_refresh_by_hash(&self, token_hash: &str) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at=now() WHERE token_hash=$1 AND revoked_at IS NULL",
        )
        .bind(token_hash)
        .execute(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(())
    }

    pub async fn revoke_all_refresh(&self, user_id: &str) -> Result<(), StoreError> {
        sqlx::query(
            "UPDATE refresh_tokens SET revoked_at=now() WHERE user_id=$1 AND revoked_at IS NULL",
        )
        .bind(user_id)
        .execute(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(())
    }

    // --- password reset tokens ---
    pub async fn insert_reset_token(
        &self,
        id: &str,
        user_id: &str,
        token_hash: &str,
        expires_at_unix: i64,
    ) -> Result<(), StoreError> {
        let exp = time::OffsetDateTime::from_unix_timestamp(expires_at_unix).unwrap();
        sqlx::query(
            "INSERT INTO password_reset_tokens(id,user_id,token_hash,expires_at) VALUES ($1,$2,$3,$4)",
        )
        .bind(id)
        .bind(user_id)
        .bind(token_hash)
        .bind(exp)
        .execute(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(())
    }

    pub async fn consume_reset_token(
        &self,
        token_hash: &str,
        now_unix: i64,
    ) -> Result<Option<String>, StoreError> {
        let now = time::OffsetDateTime::from_unix_timestamp(now_unix).unwrap();
        let r = sqlx::query_scalar::<_, String>(
            "UPDATE password_reset_tokens SET used_at=now() WHERE token_hash=$1 AND used_at IS NULL AND expires_at > $2 RETURNING user_id",
        )
        .bind(token_hash)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(r)
    }

    // --- admin user management (scoped to a tenant) ---
    pub async fn list_users_in_tenant(&self, tenant_id: &str) -> Result<Vec<User>, StoreError> {
        let rows = sqlx::query_as::<_, (String, String, String, bool, String)>(
            "SELECT u.id,u.name,u.email,u.disabled,m.role FROM users u JOIN memberships m ON m.user_id=u.id \
             WHERE m.tenant_id=$1 ORDER BY u.name",
        )
        .bind(tenant_id)
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(rows
            .into_iter()
            .map(|(id, name, email, disabled, role)| User {
                id,
                name,
                email,
                disabled,
                role: parse_role(&role),
            })
            .collect())
    }

    pub async fn create_user_with_membership(
        &self,
        id: &str,
        name: &str,
        email: &str,
        password_hash: &str,
        tenant_id: &str,
        role: UserRole,
    ) -> Result<(), StoreError> {
        let mut tx = self.pool.begin().await.map_err(StoreError::from)?;
        sqlx::query("INSERT INTO users(id,name,email,disabled) VALUES ($1,$2,$3,false)")
            .bind(id)
            .bind(name)
            .bind(email)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::from)?;
        sqlx::query("INSERT INTO user_credentials(user_id,password_hash) VALUES ($1,$2)")
            .bind(id)
            .bind(password_hash)
            .execute(&mut *tx)
            .await
            .map_err(StoreError::from)?;
        sqlx::query("INSERT INTO memberships(user_id,tenant_id,role) VALUES ($1,$2,$3)")
            .bind(id)
            .bind(tenant_id)
            .bind(role_str(role))
            .execute(&mut *tx)
            .await
            .map_err(StoreError::from)?;
        tx.commit().await.map_err(StoreError::from)?;
        Ok(())
    }

    pub async fn update_membership_role(
        &self,
        user_id: &str,
        tenant_id: &str,
        role: UserRole,
    ) -> Result<u64, StoreError> {
        let r =
            sqlx::query("UPDATE memberships SET role=$3 WHERE user_id=$1 AND tenant_id=$2")
                .bind(user_id)
                .bind(tenant_id)
                .bind(role_str(role))
                .execute(&self.pool)
                .await
                .map_err(StoreError::from)?;
        Ok(r.rows_affected())
    }

    pub async fn set_user_disabled(
        &self,
        user_id: &str,
        disabled: bool,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE users SET disabled=$2 WHERE id=$1")
            .bind(user_id)
            .bind(disabled)
            .execute(&self.pool)
            .await
            .map_err(StoreError::from)?;
        Ok(())
    }

    pub async fn remove_membership(
        &self,
        user_id: &str,
        tenant_id: &str,
    ) -> Result<u64, StoreError> {
        let r =
            sqlx::query("DELETE FROM memberships WHERE user_id=$1 AND tenant_id=$2")
                .bind(user_id)
                .bind(tenant_id)
                .execute(&self.pool)
                .await
                .map_err(StoreError::from)?;
        Ok(r.rows_affected())
    }

    /// Fetch a single tenant by id.
    pub async fn get_tenant(&self, tenant_id: &str) -> Result<Option<Tenant>, StoreError> {
        let row = sqlx::query_as::<_, (String, String, String)>(
            "SELECT id, name, slug FROM tenants WHERE id = $1",
        )
        .bind(tenant_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::from)?;
        Ok(row.map(|(id, name, slug)| Tenant { id, name, slug }))
    }
}
