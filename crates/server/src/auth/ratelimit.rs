//! Sign-in throttling: after a few failures, each further attempt for the same
//! username or client address must wait longer (1 s, 2 s, 4 s … up to 5 minutes).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const FREE_ATTEMPTS: u32 = 5;
const MAX_DELAY: Duration = Duration::from_secs(300);
const FORGET_AFTER: Duration = Duration::from_secs(3600);

#[derive(Clone, Copy)]
struct Entry {
    failures: u32,
    last: Instant,
}

#[derive(Default)]
pub struct RateLimiter {
    entries: Mutex<HashMap<String, Entry>>,
}

fn delay(failures: u32) -> Duration {
    if failures < FREE_ATTEMPTS {
        return Duration::ZERO;
    }
    let exp = (failures - FREE_ATTEMPTS).min(16);
    Duration::from_secs(1u64 << exp).min(MAX_DELAY)
}

impl RateLimiter {
    /// `Err(wait)` if any key must still wait before trying again.
    pub fn check(&self, keys: &[String], now: Instant) -> Result<(), Duration> {
        let map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let wait = keys
            .iter()
            .filter_map(|k| map.get(k))
            .map(|e| delay(e.failures).saturating_sub(now.saturating_duration_since(e.last)))
            .max()
            .unwrap_or_default();
        if wait.is_zero() { Ok(()) } else { Err(wait) }
    }

    pub fn record_failure(&self, keys: &[String], now: Instant) {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, e| now.saturating_duration_since(e.last) < FORGET_AFTER);
        for k in keys {
            let e = map.entry(k.clone()).or_insert(Entry {
                failures: 0,
                last: now,
            });
            e.failures += 1;
            e.last = now;
        }
    }

    pub fn record_success(&self, keys: &[String]) {
        let mut map = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        for k in keys {
            map.remove(k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backs_off_after_free_attempts() {
        let rl = RateLimiter::default();
        let keys = vec!["user:alice".to_string()];
        let t0 = Instant::now();
        for _ in 0..FREE_ATTEMPTS {
            assert!(rl.check(&keys, t0).is_ok());
            rl.record_failure(&keys, t0);
        }
        // Sixth failure recorded: 1 s wait.
        assert_eq!(rl.check(&keys, t0), Err(Duration::from_secs(1)));
        assert!(rl.check(&keys, t0 + Duration::from_secs(1)).is_ok());
        rl.record_failure(&keys, t0);
        assert_eq!(rl.check(&keys, t0), Err(Duration::from_secs(2)));
        assert!(rl.check(&["user:bob".to_string()], t0).is_ok());
        rl.record_success(&keys);
        assert!(rl.check(&keys, t0).is_ok());
        assert_eq!(delay(100), MAX_DELAY);
    }
}
