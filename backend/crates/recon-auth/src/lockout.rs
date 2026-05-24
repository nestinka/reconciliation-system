#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockoutDecision {
    pub locked_until_unix: Option<i64>,
    pub reset_attempts: bool,
}

pub const MAX_ATTEMPTS: i32 = 5;
const BASE_LOCK_SECS: i64 = 60;

/// Given the failed-attempt count AFTER incrementing, decide lock state.
pub fn on_failure(attempts_after: i32, now_unix: i64) -> LockoutDecision {
    if attempts_after < MAX_ATTEMPTS {
        return LockoutDecision { locked_until_unix: None, reset_attempts: false };
    }
    let over = (attempts_after - MAX_ATTEMPTS) as u32;
    let secs = BASE_LOCK_SECS.saturating_mul(2_i64.saturating_pow(over.min(10)));
    LockoutDecision { locked_until_unix: Some(now_unix + secs), reset_attempts: false }
}

/// True if the account is currently locked.
pub fn is_locked(locked_until_unix: Option<i64>, now_unix: i64) -> bool {
    matches!(locked_until_unix, Some(t) if now_unix < t)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_lock_below_threshold() {
        for a in 1..MAX_ATTEMPTS { assert_eq!(on_failure(a, 1000).locked_until_unix, None); }
    }
    #[test]
    fn locks_at_threshold_for_base() {
        assert_eq!(on_failure(MAX_ATTEMPTS, 1000).locked_until_unix, Some(1000 + 60));
    }
    #[test]
    fn backoff_doubles() {
        assert_eq!(on_failure(MAX_ATTEMPTS + 1, 1000).locked_until_unix, Some(1000 + 120));
        assert_eq!(on_failure(MAX_ATTEMPTS + 2, 1000).locked_until_unix, Some(1000 + 240));
    }
    #[test]
    fn is_locked_window() {
        assert!(is_locked(Some(2000), 1999));
        assert!(!is_locked(Some(2000), 2000));
        assert!(!is_locked(None, 2000));
    }
}
