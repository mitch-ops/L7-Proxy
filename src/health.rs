use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct UpstreamHealth {
    consecutive_failures: u32,
    marked_unhealthy_at: Option<Instant>,
}

pub struct HealthTracker {
    state: Mutex<HashMap<String, UpstreamHealth>>,
    failure_threshold: u32,
    cooldown: Duration,
}

impl HealthTracker {
    pub fn new(failure_threshold: u32, cooldown: Duration) -> Self {
        Self {
            state: Mutex::new(HashMap::new()),
            failure_threshold,
            cooldown,
        }
    }

    pub fn record_failure(&self, upstream: &str) {
        let mut state = self.state.lock().unwrap();
        let entry = state.entry(upstream.to_string()).or_insert(UpstreamHealth {
            consecutive_failures: 0,
            marked_unhealthy_at: None,
        });

        entry.consecutive_failures += 1;

        if entry.consecutive_failures >= self.failure_threshold {
            entry.consecutive_failures = self.failure_threshold; // cap to prevent unbounded growth
            entry.marked_unhealthy_at = Some(Instant::now());
        }
    }

    pub fn record_success(&self, upstream: &str) {
        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.get_mut(upstream) {
            entry.consecutive_failures = 0;
            entry.marked_unhealthy_at = None;
        }
    }

    pub fn is_healthy(&self, upstream: &str) -> bool {
        let mut state = self.state.lock().unwrap();
        let entry = match state.get_mut(upstream) {
            Some(entry) => entry,
            None => return true,
        };

        match entry.marked_unhealthy_at {
            None => true,
            Some(marked_at) => {
                if marked_at.elapsed() >= self.cooldown {
                    // Cooldown expired — give upstream a fresh chance
                    entry.consecutive_failures = 0;
                    entry.marked_unhealthy_at = None;
                    true
                } else {
                    false
                }
            }
        }
    }
}
