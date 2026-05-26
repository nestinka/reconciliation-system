//! Closed set of audit event kinds + their typed payloads.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginFailureReason { BadCredentials, Locked, RateLimited }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum AuditKind {
    AuthLoginSuccess, AuthLoginFailure, AuthLockout, AuthLogout,
    AuthPasswordChanged, AuthPasswordResetRequested, AuthPasswordResetCompleted,
    AuthRefreshReused, AuthTenantSwitched,
    AdminUserCreated, AdminUserRoleChanged, AdminUserDisabled, AdminUserEnabled, AdminUserRemoved,
    DataSourceCreated, DataIngestCompleted, DataRunCreated,
    CaseAssigned, CaseEventAppended,
    SystemAnchorCreated,
}

// `from_str` returns `Option<Self>`, not `Result`, by deliberate plan choice — the api
// layer wraps the None in its own error envelope. Silence clippy's FromStr-trait warning.
#[allow(clippy::should_implement_trait)]
impl AuditKind {
    /// Stable string identifier used in DB rows and on the wire.
    pub fn as_str(&self) -> &'static str {
        match self {
            AuditKind::AuthLoginSuccess => "auth.login.success",
            AuditKind::AuthLoginFailure => "auth.login.failure",
            AuditKind::AuthLockout => "auth.lockout",
            AuditKind::AuthLogout => "auth.logout",
            AuditKind::AuthPasswordChanged => "auth.password.changed",
            AuditKind::AuthPasswordResetRequested => "auth.password.reset_requested",
            AuditKind::AuthPasswordResetCompleted => "auth.password.reset_completed",
            AuditKind::AuthRefreshReused => "auth.refresh.reused",
            AuditKind::AuthTenantSwitched => "auth.tenant.switched",
            AuditKind::AdminUserCreated => "admin.user.created",
            AuditKind::AdminUserRoleChanged => "admin.user.role_changed",
            AuditKind::AdminUserDisabled => "admin.user.disabled",
            AuditKind::AdminUserEnabled => "admin.user.enabled",
            AuditKind::AdminUserRemoved => "admin.user.removed",
            AuditKind::DataSourceCreated => "data.source.created",
            AuditKind::DataIngestCompleted => "data.ingest.completed",
            AuditKind::DataRunCreated => "data.run.created",
            AuditKind::CaseAssigned => "case.assigned",
            AuditKind::CaseEventAppended => "case.event_appended",
            AuditKind::SystemAnchorCreated => "system.anchor.created",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "auth.login.success" => AuditKind::AuthLoginSuccess,
            "auth.login.failure" => AuditKind::AuthLoginFailure,
            "auth.lockout" => AuditKind::AuthLockout,
            "auth.logout" => AuditKind::AuthLogout,
            "auth.password.changed" => AuditKind::AuthPasswordChanged,
            "auth.password.reset_requested" => AuditKind::AuthPasswordResetRequested,
            "auth.password.reset_completed" => AuditKind::AuthPasswordResetCompleted,
            "auth.refresh.reused" => AuditKind::AuthRefreshReused,
            "auth.tenant.switched" => AuditKind::AuthTenantSwitched,
            "admin.user.created" => AuditKind::AdminUserCreated,
            "admin.user.role_changed" => AuditKind::AdminUserRoleChanged,
            "admin.user.disabled" => AuditKind::AdminUserDisabled,
            "admin.user.enabled" => AuditKind::AdminUserEnabled,
            "admin.user.removed" => AuditKind::AdminUserRemoved,
            "data.source.created" => AuditKind::DataSourceCreated,
            "data.ingest.completed" => AuditKind::DataIngestCompleted,
            "data.run.created" => AuditKind::DataRunCreated,
            "case.assigned" => AuditKind::CaseAssigned,
            "case.event_appended" => AuditKind::CaseEventAppended,
            "system.anchor.created" => AuditKind::SystemAnchorCreated,
            _ => return None,
        })
    }
}

/// Typed payload variants. No serde_json::Value escape hatch — sensitive material is
/// impossible to add by accident. Variant names mirror AuditKind exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum AuditPayload {
    AuthLoginSuccess { user_id: String, email: String, ip: Option<String> },
    AuthLoginFailure { email: String, ip: Option<String>, reason: LoginFailureReason },
    AuthLockout { user_id: String, email: String, locked_until: String },
    AuthLogout { user_id: String, ip: Option<String> },
    AuthPasswordChanged { user_id: String, ip: Option<String> },
    AuthPasswordResetRequested { email: String, ip: Option<String> },
    AuthPasswordResetCompleted { user_id: String, ip: Option<String> },
    AuthRefreshReused { user_id: String, token_id: String, ip: Option<String> },
    AuthTenantSwitched { user_id: String, from_tenant: String, to_tenant: String },
    AdminUserCreated { user_id: String, email: String, role: String },
    AdminUserRoleChanged { user_id: String, from: String, to: String },
    AdminUserDisabled { user_id: String },
    AdminUserEnabled { user_id: String },
    AdminUserRemoved { user_id: String },
    DataSourceCreated { source_id: String, kind: String, currency: String, name: String },
    DataIngestCompleted { source_id: String, format: String, file_sha256: String, bytes: i64, ingested: i64 },
    DataRunCreated { run_id: String, source_a_id: String, source_b_id: String, from: String, to: String, matched: i64, unmatched: i64 },
    CaseAssigned { case_id: String, break_id: String, assignee_id: String },
    CaseEventAppended { case_id: String, break_id: String, event_kind: String },
    SystemAnchorCreated { anchor_seq: i64, tenant_count: i64 },
}

impl AuditPayload {
    /// The kind tag that matches this payload (used for asserting consistency).
    pub fn kind(&self) -> AuditKind {
        match self {
            AuditPayload::AuthLoginSuccess { .. } => AuditKind::AuthLoginSuccess,
            AuditPayload::AuthLoginFailure { .. } => AuditKind::AuthLoginFailure,
            AuditPayload::AuthLockout { .. } => AuditKind::AuthLockout,
            AuditPayload::AuthLogout { .. } => AuditKind::AuthLogout,
            AuditPayload::AuthPasswordChanged { .. } => AuditKind::AuthPasswordChanged,
            AuditPayload::AuthPasswordResetRequested { .. } => AuditKind::AuthPasswordResetRequested,
            AuditPayload::AuthPasswordResetCompleted { .. } => AuditKind::AuthPasswordResetCompleted,
            AuditPayload::AuthRefreshReused { .. } => AuditKind::AuthRefreshReused,
            AuditPayload::AuthTenantSwitched { .. } => AuditKind::AuthTenantSwitched,
            AuditPayload::AdminUserCreated { .. } => AuditKind::AdminUserCreated,
            AuditPayload::AdminUserRoleChanged { .. } => AuditKind::AdminUserRoleChanged,
            AuditPayload::AdminUserDisabled { .. } => AuditKind::AdminUserDisabled,
            AuditPayload::AdminUserEnabled { .. } => AuditKind::AdminUserEnabled,
            AuditPayload::AdminUserRemoved { .. } => AuditKind::AdminUserRemoved,
            AuditPayload::DataSourceCreated { .. } => AuditKind::DataSourceCreated,
            AuditPayload::DataIngestCompleted { .. } => AuditKind::DataIngestCompleted,
            AuditPayload::DataRunCreated { .. } => AuditKind::DataRunCreated,
            AuditPayload::CaseAssigned { .. } => AuditKind::CaseAssigned,
            AuditPayload::CaseEventAppended { .. } => AuditKind::CaseEventAppended,
            AuditPayload::SystemAnchorCreated { .. } => AuditKind::SystemAnchorCreated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_strings_are_stable_dot_notation() {
        assert_eq!(AuditKind::AuthLoginSuccess.as_str(), "auth.login.success");
        assert_eq!(AuditKind::DataIngestCompleted.as_str(), "data.ingest.completed");
        assert_eq!(AuditKind::SystemAnchorCreated.as_str(), "system.anchor.created");
    }

    #[test]
    fn kind_string_roundtrip() {
        for k in [
            AuditKind::AuthLoginSuccess, AuditKind::AuthLockout, AuditKind::DataRunCreated,
            AuditKind::CaseAssigned, AuditKind::SystemAnchorCreated,
        ] {
            assert_eq!(AuditKind::from_str(k.as_str()), Some(k));
        }
        assert_eq!(AuditKind::from_str("nope"), None);
    }

    #[test]
    fn payload_kind_matches_variant() {
        let p = AuditPayload::AuthLogout { user_id: "u".into(), ip: None };
        assert_eq!(p.kind(), AuditKind::AuthLogout);
    }
}
