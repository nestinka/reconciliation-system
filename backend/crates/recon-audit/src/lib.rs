pub mod chain;
pub mod controls;
pub mod events;

pub use chain::{verify, VerifyError, VerifyReason};
pub use controls::{Control, CONTROLS};
pub use events::{AuditKind, AuditPayload, LoginFailureReason};

/// One entry as it sits in the DB and on the wire (in-memory).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditEntry {
    pub tenant_id: String,
    pub seq: i64,
    pub at: String,
    pub actor_id: String,
    pub kind: AuditKind,
    pub payload: AuditPayload,
    pub prev_hash: [u8; 32],
    pub hash: [u8; 32],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_entry_is_constructible() {
        let e = AuditEntry {
            tenant_id: "tenant-acme".into(),
            seq: 1,
            at: "2026-05-26T10:00:00Z".into(),
            actor_id: "user-mia".into(),
            kind: AuditKind::AuthLogout,
            payload: AuditPayload::AuthLogout { user_id: "user-mia".into(), ip: None },
            prev_hash: [0u8; 32],
            hash: [0u8; 32],
        };
        assert_eq!(e.seq, 1);
        assert_eq!(e.kind.as_str(), "auth.logout");
    }
}
