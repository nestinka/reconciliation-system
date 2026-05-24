use std::collections::HashMap;
use std::sync::Mutex;

/// Simple in-memory per-key token bucket. Single-instance only.
pub struct IpLimiter {
    inner: Mutex<HashMap<String, (f64, i64)>>,
    capacity: f64,
    refill_per_sec: f64,
}

impl IpLimiter {
    pub fn new(capacity: f64, refill_per_sec: f64) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            capacity,
            refill_per_sec,
        }
    }

    pub fn check_at(&self, key: &str, now: i64) -> bool {
        let mut g = self.inner.lock().unwrap();
        let e = g.entry(key.to_string()).or_insert((self.capacity, now));
        let elapsed = (now - e.1).max(0) as f64;
        e.0 = (e.0 + elapsed * self.refill_per_sec).min(self.capacity);
        e.1 = now;
        if e.0 >= 1.0 {
            e.0 -= 1.0;
            true
        } else {
            false
        }
    }

    pub fn check(&self, key: &str) -> bool {
        self.check_at(key, time::OffsetDateTime::now_utc().unix_timestamp())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhausts_then_refills() {
        let l = IpLimiter::new(2.0, 1.0);
        assert!(l.check_at("ip", 0));
        assert!(l.check_at("ip", 0));
        assert!(!l.check_at("ip", 0));
        assert!(l.check_at("ip", 2));
    }
}
