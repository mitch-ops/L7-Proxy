use std::sync::Mutex;
use std::time::Instant;

/// Tracks the ratio of retries to total requests within a sliding window.
/// If retries exceed the configured percentage of total requests, further retries are denied.
pub struct RetryBudget {
    state: Mutex<BudgetState>,
    budget_percent: f64,
    window: std::time::Duration,
}

struct BudgetState {
    requests: Vec<Instant>,
    retries: Vec<Instant>,
}

impl RetryBudget {
    pub fn new(budget_percent: f64, window_secs: u64) -> Self {
        Self {
            state: Mutex::new(BudgetState {
                requests: Vec::new(),
                retries: Vec::new(),
            }),
            budget_percent,
            window: std::time::Duration::from_secs(window_secs),
        }
    }

    /// Record an incoming request (called once per original request).
    pub fn record_request(&self) {
        let mut state = self.state.lock().unwrap();
        state.requests.push(Instant::now());
    }

    /// Check if a retry is allowed, and if so, record it.
    pub fn allow_retry(&self) -> bool {
        let mut state = self.state.lock().unwrap();
        let cutoff = Instant::now() - self.window;

        state.requests.retain(|t| *t > cutoff);
        state.retries.retain(|t| *t > cutoff);

        let total = state.requests.len();
        if total == 0 {
            return true;
        }

        let max_retries = (total as f64 * self.budget_percent / 100.0).ceil() as usize;
        if state.retries.len() < max_retries {
            state.retries.push(Instant::now());
            true
        } else {
            false
        }
    }
}
