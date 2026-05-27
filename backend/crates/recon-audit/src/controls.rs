//! ISO 27001 / SOC 2 / FCA control → audit event kind mapping.
//!
//! This is the authoritative registry. New controls or new audit kinds require
//! a code change. The frontend reads this through `GET /api/audit/controls`.

use crate::AuditKind;
use serde::Serialize;

// NOTE: Server-emitted only — &'static fields preclude Deserialize. The plan asked for it
// but serde cannot synthesize Deserialize for &'static references, so this is read-only
// from the wire's perspective. The frontend Control type is a structural mirror.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub id: &'static str,
    pub framework: &'static str,
    pub description: &'static str,
    pub event_kinds: &'static [AuditKind],
}

pub static CONTROLS: &[Control] = &[
    Control {
        id: "ISO27001:A.9.2.1",
        framework: "ISO 27001",
        description: "User registration and de-registration",
        event_kinds: &[
            AuditKind::AdminUserCreated,
            AuditKind::AdminUserDisabled,
            AuditKind::AdminUserEnabled,
            AuditKind::AdminUserRemoved,
        ],
    },
    Control {
        id: "ISO27001:A.9.2.3",
        framework: "ISO 27001",
        description: "Management of privileged access rights",
        event_kinds: &[
            AuditKind::AdminUserRoleChanged,
        ],
    },
    Control {
        id: "ISO27001:A.9.4.2",
        framework: "ISO 27001",
        description: "Secure log-on procedures",
        event_kinds: &[
            AuditKind::AuthLoginSuccess,
            AuditKind::AuthLoginFailure,
            AuditKind::AuthLockout,
        ],
    },
    Control {
        id: "ISO27001:A.9.4.3",
        framework: "ISO 27001",
        description: "Password management system",
        event_kinds: &[
            AuditKind::AuthPasswordChanged,
            AuditKind::AuthPasswordResetRequested,
            AuditKind::AuthPasswordResetCompleted,
        ],
    },
    Control {
        id: "ISO27001:A.12.4.1",
        framework: "ISO 27001",
        description: "Event logging",
        event_kinds: &[
            AuditKind::SystemAnchorCreated,
            AuditKind::DataIngestCompleted,
            AuditKind::DataRunCreated,
            AuditKind::DataSourceCreated,
        ],
    },
    Control {
        id: "SOC2:CC6.1",
        framework: "SOC 2",
        description: "Logical access security software, infrastructure, and architectures",
        event_kinds: &[
            AuditKind::AuthLoginSuccess,
            AuditKind::AuthLoginFailure,
            AuditKind::AuthLockout,
            AuditKind::AuthTenantSwitched,
            AuditKind::AuthRefreshReused,
        ],
    },
    Control {
        id: "SOC2:CC6.2",
        framework: "SOC 2",
        description: "Prior to issuing system credentials and granting access",
        event_kinds: &[
            AuditKind::AdminUserCreated,
            AuditKind::AdminUserRoleChanged,
        ],
    },
    Control {
        id: "SOC2:CC6.3",
        framework: "SOC 2",
        description: "Authorize, modify, or remove access to data, software, functions",
        event_kinds: &[
            AuditKind::AdminUserRoleChanged,
            AuditKind::AdminUserDisabled,
            AuditKind::AdminUserEnabled,
            AuditKind::AdminUserRemoved,
        ],
    },
    Control {
        id: "SOC2:CC7.2",
        framework: "SOC 2",
        description: "Monitors system components and operation",
        event_kinds: &[
            AuditKind::AuthRefreshReused,
            AuditKind::AuthLockout,
            AuditKind::SystemAnchorCreated,
        ],
    },
    Control {
        id: "FCA:SYSC9.1",
        framework: "FCA",
        description: "Record keeping — adequacy of records of business activities",
        event_kinds: &[
            AuditKind::DataIngestCompleted,
            AuditKind::DataRunCreated,
            AuditKind::CaseAssigned,
            AuditKind::CaseEventAppended,
        ],
    },
    Control {
        id: "FCA:SYSC4.1.10",
        framework: "FCA",
        description: "Four-eyes / segregation of duties on resolution decisions",
        event_kinds: &[
            AuditKind::CaseEventAppended,
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_control_has_a_framework_and_kinds() {
        for c in CONTROLS {
            assert!(!c.id.is_empty());
            assert!(!c.framework.is_empty());
            assert!(!c.description.is_empty());
            assert!(!c.event_kinds.is_empty(), "control {} has no event_kinds", c.id);
        }
    }

    #[test]
    fn control_ids_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in CONTROLS {
            assert!(seen.insert(c.id), "duplicate control id {}", c.id);
        }
    }

    #[test]
    fn serde_roundtrip() {
        // Confirm camelCase: `eventKinds` not `event_kinds`.
        let c = &CONTROLS[0];
        let s = serde_json::to_string(c).unwrap();
        assert!(s.contains("\"eventKinds\""), "expected camelCase: {s}");
    }

    #[test]
    fn event_kinds_serialize_as_dot_notation() {
        // Regression guard for the `f669bbc` bug: AuditKind must serialize as
        // dot-notation strings (e.g. "admin.user.created"), NOT PascalCase
        // variant names (e.g. "AdminUserCreated"). The /api/audit/controls
        // endpoint serializes Control directly through serde, so the kind
        // formatting has to come from the AuditKind Serialize impl itself.
        // CONTROLS[0] is ISO27001:A.9.2.1 (event_kinds starts with AdminUserCreated).
        let c = &CONTROLS[0];
        let s = serde_json::to_string(c).unwrap();
        assert!(
            s.contains("\"admin.user.created\""),
            "AuditKind must serialize as dot-notation; got: {s}"
        );
        assert!(
            !s.contains("\"AdminUserCreated\""),
            "AuditKind must NOT serialize as PascalCase variant name; got: {s}"
        );
    }
}
