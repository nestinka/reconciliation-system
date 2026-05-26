//! Static controls registry. Real bodies in A5.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Control {
    pub id: &'static str,
    pub framework: &'static str,
    pub description: &'static str,
    pub event_kinds: &'static [crate::AuditKind],
}

pub static CONTROLS: &[Control] = &[];
