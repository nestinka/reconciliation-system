//! Background scheduler for periodic audit-chain anchoring.
//!
//! `spawn_anchor_loop` reads `AUDIT_ANCHOR_INTERVAL_SECS` (default 3600 = 1 hour)
//! and spawns a tokio task that calls `Store::anchor_now` at that cadence. The
//! same endpoint is also exposed manually as `POST /api/audit/anchor` for ops
//! who want to anchor on demand (or for tests that don't want to wait an hour).
//!
//! Errors are logged via `tracing` and do not stop the loop — the next interval
//! tick still tries, and a failed anchor is recoverable (the chain itself isn't
//! affected, only the anchor row is missing for that interval).

use recon_store::Store;
use std::time::Duration;

pub fn spawn_anchor_loop(store: Store) {
    let secs: u64 = std::env::var("AUDIT_ANCHOR_INTERVAL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);
    let interval = Duration::from_secs(secs);
    tracing::info!(interval_secs = secs, "audit anchor scheduler starting");
    tokio::spawn(async move {
        // Wait one interval before the first anchor so app startup isn't blocked.
        tokio::time::sleep(interval).await;
        loop {
            match store.anchor_now().await {
                Ok(a) => tracing::info!(anchor_seq = a.anchor_seq, "audit anchor written"),
                Err(e) => tracing::error!(error = %e, "audit anchor failed"),
            }
            tokio::time::sleep(interval).await;
        }
    });
}
