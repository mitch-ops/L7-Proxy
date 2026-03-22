use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct TokenBucket {
    tokens: f64,
    capacity: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: f64, refill_rate: f64) -> Self {
        Self {
            tokens: capacity, // start full
            capacity,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    fn try_acquire(&mut self) -> bool {
        self.refill();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity);
        self.last_refill = now;
    }
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<IpAddr, TokenBucket>>,
    capacity: f64,
    refill_rate: f64,
    enabled: bool,
    cleanup_counter: AtomicU64,
}

impl RateLimiter {
    pub fn new(requests_per_second: f64, burst_size: u32) -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            capacity: burst_size as f64,
            refill_rate: requests_per_second,
            enabled: true,
            cleanup_counter: AtomicU64::new(0),
        }
    }

    pub fn disabled() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
            capacity: 0.0,
            refill_rate: 0.0,
            enabled: false,
            cleanup_counter: AtomicU64::new(0),
        }
    }

    pub fn check(&self, ip: IpAddr) -> bool {
        if !self.enabled {
            return true;
        }

        // Periodically clean up stale entries (every 1000 checks)
        let count = self.cleanup_counter.fetch_add(1, Ordering::Relaxed);
        if count % 1000 == 0 {
            self.cleanup();
        }

        let mut buckets = self.buckets.lock().unwrap();
        let bucket = buckets
            .entry(ip)
            .or_insert_with(|| TokenBucket::new(self.capacity, self.refill_rate));
        bucket.try_acquire()
    }

    fn cleanup(&self) {
        let mut buckets = self.buckets.lock().unwrap();
        let cutoff = Instant::now() - Duration::from_secs(300);
        buckets.retain(|_, bucket| bucket.last_refill > cutoff);
    }
}
