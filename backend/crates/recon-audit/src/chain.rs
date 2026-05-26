//! Canonical serialization + SHA-256 hashing + chain verification. Filled in A3/A4.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub seq: i64,
    pub reason: VerifyReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyReason { Tampered, WrongPrev, Missing, Reordered, WrongGenesis }

pub fn verify(
    _entries: &[crate::AuditEntry],
    _expected_prev_hash: Option<[u8; 32]>,
) -> Result<(), VerifyError> {
    // Stub — real implementation in A4.
    Ok(())
}
