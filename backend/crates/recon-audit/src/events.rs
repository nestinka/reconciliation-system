//! AuditKind + AuditPayload enums. Real bodies in A2.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginFailureReason { BadCredentials, Locked, RateLimited }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditKind { AuthLogout }

impl AuditKind {
    pub fn as_str(&self) -> &'static str {
        match self { AuditKind::AuthLogout => "auth.logout" }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum AuditPayload {
    AuthLogout { user_id: String, ip: Option<String> },
}
