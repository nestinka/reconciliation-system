//! Deterministic canonical serialization + SHA-256 hashing for audit entries.
//!
//! Encoding: a sequence of length-prefixed binary fields. Each field is
//! `<u32-BE byte-length> <utf-8 bytes>`. Fields are appended in a fixed order:
//!     prev_hash (32 bytes, no length prefix),
//!     seq (u64-BE, no length prefix),
//!     tenant_id (length-prefixed UTF-8),
//!     at (length-prefixed UTF-8 RFC3339),
//!     actor_id (length-prefixed UTF-8),
//!     kind (length-prefixed ASCII, e.g. "data.ingest.completed"),
//!     payload (length-prefixed sorted-keys JSON bytes).
//!
//! Sorted-keys JSON is produced by `serde_json::to_value(&payload)` (which uses
//! a BTreeMap-backed Map when the `preserve_order` feature is OFF — the workspace
//! does NOT enable it) followed by `serde_json::to_vec(&value)`.

use crate::AuditEntry;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub seq: i64,
    pub reason: VerifyReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyReason {
    Tampered,
    WrongPrev,
    Missing,
    Reordered,
    WrongGenesis,
}

/// Build the canonical pre-image bytes for an entry. Pure: same inputs → same bytes.
pub fn canonical_bytes(
    prev_hash: &[u8; 32],
    seq: i64,
    tenant_id: &str,
    at: &str,
    actor_id: &str,
    kind_str: &str,
    payload_canonical_json: &[u8],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        32 + 8 + 4 + tenant_id.len() + 4 + at.len() + 4 + actor_id.len()
            + 4 + kind_str.len() + 4 + payload_canonical_json.len(),
    );
    out.extend_from_slice(prev_hash);
    out.extend_from_slice(&(seq as u64).to_be_bytes());
    push_lp(&mut out, tenant_id.as_bytes());
    push_lp(&mut out, at.as_bytes());
    push_lp(&mut out, actor_id.as_bytes());
    push_lp(&mut out, kind_str.as_bytes());
    push_lp(&mut out, payload_canonical_json);
    out
}

fn push_lp(out: &mut Vec<u8>, bytes: &[u8]) {
    // Length-prefix is u32 BE, so each field is bounded at 4 GiB. Audit fields are
    // all tiny in practice (IDs, RFC3339 strings, JSON payloads <2KB); the assert
    // turns a silent truncation footgun into a panic if anyone ever tries.
    debug_assert!(bytes.len() <= u32::MAX as usize, "length-prefix overflow");
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// Serialize an `AuditPayload` to sorted-keys, no-whitespace JSON bytes. Determinism
/// relies on serde_json's `preserve_order` feature being OFF in the workspace.
pub fn payload_canonical_json(payload: &crate::AuditPayload) -> Vec<u8> {
    // to_value emits a `Map<String, Value>` which is `BTreeMap`-backed without the
    // `preserve_order` feature; to_vec then yields sorted-keys JSON.
    let v = serde_json::to_value(payload).expect("AuditPayload is always serializable");
    serde_json::to_vec(&v).expect("Value is always serializable")
}

/// Compute the SHA-256 hash of an entry given prev_hash + the entry's fields.
pub fn compute_hash(
    prev_hash: &[u8; 32],
    seq: i64,
    tenant_id: &str,
    at: &str,
    actor_id: &str,
    kind: crate::AuditKind,
    payload: &crate::AuditPayload,
) -> [u8; 32] {
    let pcj = payload_canonical_json(payload);
    let bytes = canonical_bytes(prev_hash, seq, tenant_id, at, actor_id, kind.as_str(), &pcj);
    let mut h = Sha256::new();
    h.update(&bytes);
    h.finalize().into()
}

/// Verify a contiguous slice of entries (in seq order).
///
/// If `entries[0].seq == 1`, the genesis prev_hash must be all-zero. Otherwise the
/// caller can supply `expected_prev_hash` to verify a slice mid-chain.
pub fn verify(
    entries: &[AuditEntry],
    expected_prev_hash: Option<[u8; 32]>,
) -> Result<(), VerifyError> {
    if entries.is_empty() {
        return Ok(());
    }
    // Genesis check.
    if entries[0].seq == 1 {
        if entries[0].prev_hash != [0u8; 32] {
            return Err(VerifyError { seq: entries[0].seq, reason: VerifyReason::WrongGenesis });
        }
    } else if let Some(expected) = expected_prev_hash {
        if entries[0].prev_hash != expected {
            return Err(VerifyError { seq: entries[0].seq, reason: VerifyReason::WrongGenesis });
        }
    }

    let mut prev_seq: Option<i64> = None;
    let mut prev_hash: Option<[u8; 32]> = None;
    for e in entries {
        if let Some(ps) = prev_seq {
            if e.seq < ps + 1 {
                return Err(VerifyError { seq: e.seq, reason: VerifyReason::Reordered });
            }
            if e.seq > ps + 1 {
                return Err(VerifyError { seq: e.seq, reason: VerifyReason::Missing });
            }
        }
        if let Some(ph) = prev_hash {
            if e.prev_hash != ph {
                return Err(VerifyError { seq: e.seq, reason: VerifyReason::WrongPrev });
            }
        }
        let recomputed = compute_hash(&e.prev_hash, e.seq, &e.tenant_id, &e.at, &e.actor_id, e.kind, &e.payload);
        if recomputed != e.hash {
            return Err(VerifyError { seq: e.seq, reason: VerifyReason::Tampered });
        }
        prev_seq = Some(e.seq);
        prev_hash = Some(e.hash);
    }
    Ok(())
}

/// API-shape outcome of running `verify` on a stored range.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyOutcome {
    pub status: VerifyStatus,
    pub checked: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_broken_seq: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<VerifyReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifyStatus { Valid, Invalid }

impl VerifyOutcome {
    pub fn valid(checked: i64) -> Self {
        Self { status: VerifyStatus::Valid, checked, first_broken_seq: None, reason: None }
    }
    pub fn invalid(checked: i64, e: VerifyError) -> Self {
        Self { status: VerifyStatus::Invalid, checked, first_broken_seq: Some(e.seq), reason: Some(e.reason) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuditEntry, AuditKind, AuditPayload};

    fn mk(seq: i64, prev: [u8; 32], payload: AuditPayload) -> AuditEntry {
        let kind = payload.kind();
        let at = "2026-05-26T10:00:00Z".to_string();
        let hash = compute_hash(&prev, seq, "tenant-acme", &at, "user-mia", kind, &payload);
        AuditEntry {
            tenant_id: "tenant-acme".into(),
            seq,
            at,
            actor_id: "user-mia".into(),
            kind,
            payload,
            prev_hash: prev,
            hash,
        }
    }

    #[test]
    fn canonical_bytes_is_deterministic() {
        let p = AuditPayload::AuthLogout { user_id: "u".into(), ip: None };
        let pcj1 = payload_canonical_json(&p);
        let pcj2 = payload_canonical_json(&p);
        assert_eq!(pcj1, pcj2);
        let b1 = canonical_bytes(&[0u8; 32], 1, "t", "at", "a", "auth.logout", &pcj1);
        let b2 = canonical_bytes(&[0u8; 32], 1, "t", "at", "a", "auth.logout", &pcj2);
        assert_eq!(b1, b2);
    }

    #[test]
    fn canonical_json_is_sorted_keys() {
        // For a struct variant, serde_json emits keys in BTreeMap order without preserve_order.
        let p = AuditPayload::AuthLoginSuccess {
            user_id: "u".into(), email: "e".into(), ip: Some("1.2.3.4".into()),
        };
        let s = String::from_utf8(payload_canonical_json(&p)).unwrap();
        // External tag + content shape: {"kind":"auth_login_success","data":{...sorted...}}
        // Verify the inner data object keys are sorted alphabetically (email < ip < user_id).
        let i_email = s.find("\"email\"").unwrap();
        let i_ip = s.find("\"ip\"").unwrap();
        let i_user_id = s.find("\"user_id\"").unwrap();
        assert!(i_email < i_ip && i_ip < i_user_id, "keys must be sorted: {s}");
    }

    #[test]
    fn verify_accepts_valid_chain_of_three() {
        let e1 = mk(1, [0u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let e2 = mk(2, e1.hash, AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let e3 = mk(3, e2.hash, AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        assert!(verify(&[e1, e2, e3], None).is_ok());
    }

    #[test]
    fn verify_detects_tamper() {
        let e1 = mk(1, [0u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let mut e2 = mk(2, e1.hash, AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        // Tamper: change payload but keep hash stale.
        e2.payload = AuditPayload::AuthLogout { user_id: "evil".into(), ip: None };
        let err = verify(&[e1, e2], None).unwrap_err();
        assert_eq!(err.seq, 2);
        assert_eq!(err.reason, VerifyReason::Tampered);
    }

    #[test]
    fn verify_detects_missing() {
        let e1 = mk(1, [0u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let e3 = mk(3, e1.hash, AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let err = verify(&[e1, e3], None).unwrap_err();
        assert_eq!(err.seq, 3);
        assert_eq!(err.reason, VerifyReason::Missing);
    }

    #[test]
    fn verify_detects_wrong_prev() {
        let e1 = mk(1, [0u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let e2 = mk(2, [9u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let err = verify(&[e1, e2], None).unwrap_err();
        assert_eq!(err.seq, 2);
        assert_eq!(err.reason, VerifyReason::WrongPrev);
    }

    #[test]
    fn verify_rejects_non_zero_genesis() {
        let e1 = mk(1, [9u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let err = verify(&[e1], None).unwrap_err();
        assert_eq!(err.reason, VerifyReason::WrongGenesis);
    }

    #[test]
    fn verify_partial_range_with_expected_prev() {
        let e1 = mk(1, [0u8; 32], AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        let e2 = mk(2, e1.hash, AuditPayload::AuthLogout { user_id: "u".into(), ip: None });
        // Verify just e2 with expected_prev_hash = e1.hash.
        assert!(verify(&[e2.clone()], Some(e1.hash)).is_ok());
        // Wrong expected_prev fails.
        let err = verify(&[e2], Some([7u8; 32])).unwrap_err();
        assert_eq!(err.reason, VerifyReason::WrongGenesis);
    }

    #[test]
    fn golden_vector_for_logout_genesis_entry() {
        // A specific entry whose hash is locked in. If this test ever flips, the
        // canonical encoding has changed and existing chains become unverifiable —
        // require a deliberate migration.
        let p = AuditPayload::AuthLogout { user_id: "user-mia".into(), ip: None };
        let h = compute_hash(&[0u8; 32], 1, "tenant-acme", "2026-05-26T10:00:00Z", "user-mia", AuditKind::AuthLogout, &p);
        let actual = hex::encode(h);
        // The expected value is computed once during initial implementation; replace
        // with the value printed by this test on first run.
        let expected = "4a87d4d47141fa543819b566627d327eb62ae91439c74224c3275bb9660866c7";
        if expected == "REPLACE_WITH_INITIAL_HASH" {
            // First-run helper: print the hash so the developer can paste it in.
            // After replacing, this branch is never taken again.
            panic!("first run: replace expected with {actual}");
        }
        assert_eq!(actual, expected, "canonical encoding changed");
    }
}
