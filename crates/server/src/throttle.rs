//! Limits on expensive work (PDFs and page previews), so one account or a
//! runaway browser tab can't keep the server busy.
//!
//! * At most [`RENDER_SLOTS`] renders run at once; others wait their turn.
//! * Each user may start at most [`RENDERS_PER_MINUTE`] a minute; more get
//!   "429 Too Many Requests".

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, SemaphorePermit};

pub const RENDER_SLOTS: usize = 2;
pub const RENDERS_PER_MINUTE: usize = 60;
const WINDOW: Duration = Duration::from_secs(60);

pub struct RenderGate {
    slots: Semaphore,
    recent: Mutex<HashMap<i64, VecDeque<Instant>>>,
    per_minute: usize,
}

impl Default for RenderGate {
    fn default() -> Self {
        Self::new(RENDERS_PER_MINUTE)
    }
}

impl RenderGate {
    pub fn new(per_minute: usize) -> Self {
        Self {
            slots: Semaphore::new(RENDER_SLOTS),
            recent: Mutex::new(HashMap::new()),
            per_minute,
        }
    }

    /// Count one render for `user`; `false` if they've used up this minute.
    pub fn allow(&self, user: i64, now: Instant) -> bool {
        let mut map = self.recent.lock().unwrap_or_else(|e| e.into_inner());
        map.retain(|_, q| {
            while q.front().is_some_and(|t| now.duration_since(*t) >= WINDOW) {
                q.pop_front();
            }
            !q.is_empty()
        });
        let q = map.entry(user).or_default();
        if q.len() >= self.per_minute {
            return false;
        }
        q.push_back(now);
        true
    }

    /// Wait for a free render slot (held until the permit is dropped).
    pub async fn slot(&self) -> anyhow::Result<SemaphorePermit<'_>> {
        Ok(self.slots.acquire().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_each_user_per_minute() {
        let g = RenderGate::new(3);
        let t0 = Instant::now();
        assert!((0..3).all(|_| g.allow(1, t0)));
        assert!(!g.allow(1, t0), "fourth in the same minute");
        assert!(g.allow(2, t0), "other users are separate");
        assert!(g.allow(1, t0 + WINDOW), "a minute later");
    }
}
