use proptest::prelude::*;
use recon_audit::chain::compute_hash;
use recon_audit::{verify, AuditEntry, AuditKind, AuditPayload};

fn mk(seq: i64, prev: [u8; 32], user: String) -> AuditEntry {
    let payload = AuditPayload::AuthLogout { user_id: user, ip: None };
    let at = "2026-05-26T10:00:00Z".to_string();
    let hash = compute_hash(&prev, seq, "tenant-acme", &at, "actor", AuditKind::AuthLogout, &payload);
    AuditEntry {
        tenant_id: "tenant-acme".into(),
        seq,
        at,
        actor_id: "actor".into(),
        kind: AuditKind::AuthLogout,
        payload,
        prev_hash: prev,
        hash,
    }
}

proptest! {
    /// A generated chain of N valid entries always verifies.
    #[test]
    fn valid_chains_always_verify(n in 1usize..50) {
        let mut entries = Vec::with_capacity(n);
        let mut prev = [0u8; 32];
        for i in 0..n {
            let e = mk(i as i64 + 1, prev, format!("user-{i}"));
            prev = e.hash;
            entries.push(e);
        }
        prop_assert!(verify(&entries, None).is_ok());
    }

    /// Tampering with a single byte of any entry's payload always breaks verify.
    #[test]
    fn tampering_breaks_verify(n in 2usize..20, tamper_at in 0usize..20) {
        let mut entries = Vec::with_capacity(n);
        let mut prev = [0u8; 32];
        for i in 0..n {
            let e = mk(i as i64 + 1, prev, format!("user-{i}"));
            prev = e.hash;
            entries.push(e);
        }
        let target = tamper_at % n;
        if let AuditPayload::AuthLogout { user_id, .. } = &mut entries[target].payload {
            user_id.push('!');
        }
        // After tampering, the recomputed hash won't match the stored hash anywhere
        // from the tamper point onward.
        prop_assert!(verify(&entries, None).is_err());
    }
}
