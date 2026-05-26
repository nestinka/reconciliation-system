//! Static controls registry. Real bodies in A5.

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
    pub event_kinds: &'static [crate::AuditKind],
}

pub static CONTROLS: &[Control] = &[];
