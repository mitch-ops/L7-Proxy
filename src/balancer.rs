use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::config::BalancerStrategy;

/// Trait for load balancing strategies.
pub trait LoadBalancer: Send + Sync {
    /// Pick the next upstream index to use.
    fn next_index(&self, pool_size: usize) -> usize;

    /// Pick the next upstream index based on a request key (e.g., path).
    /// Defaults to ignoring the key and calling `next_index`.
    fn next_index_for_key(&self, pool_size: usize, _key: &str) -> usize {
        self.next_index(pool_size)
    }

    /// Record that a request started on this upstream (for connection tracking).
    fn record_start(&self, _index: usize) {}

    /// Record that a request finished on this upstream (for connection tracking).
    fn record_done(&self, _index: usize) {}
}

// --- Round Robin ---

pub struct RoundRobin {
    counter: AtomicUsize,
}

impl RoundRobin {
    pub fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
        }
    }
}

impl LoadBalancer for RoundRobin {
    fn next_index(&self, pool_size: usize) -> usize {
        let current = self.counter.fetch_add(1, Ordering::Relaxed);
        current % pool_size
    }
}

// --- Least Connections ---

pub struct LeastConnections {
    active: Vec<AtomicUsize>,
}

impl LeastConnections {
    pub fn new(pool_size: usize) -> Self {
        let active = (0..pool_size).map(|_| AtomicUsize::new(0)).collect();
        Self { active }
    }
}

impl LoadBalancer for LeastConnections {
    fn next_index(&self, pool_size: usize) -> usize {
        let len = pool_size.min(self.active.len());
        let mut min_idx = 0;
        let mut min_val = self.active[0].load(Ordering::Relaxed);
        for i in 1..len {
            let val = self.active[i].load(Ordering::Relaxed);
            if val < min_val {
                min_val = val;
                min_idx = i;
            }
        }
        min_idx
    }

    fn record_start(&self, index: usize) {
        if index < self.active.len() {
            self.active[index].fetch_add(1, Ordering::Relaxed);
        }
    }

    fn record_done(&self, index: usize) {
        if index < self.active.len() {
            self.active[index].fetch_sub(1, Ordering::Relaxed);
        }
    }
}

// --- Weighted Round Robin ---

pub struct WeightedRoundRobin {
    counter: AtomicUsize,
    /// Expanded schedule: weights [3, 1] → [0, 0, 0, 1]
    schedule: Vec<usize>,
}

impl WeightedRoundRobin {
    pub fn new(weights: Vec<u32>) -> Self {
        let mut schedule = Vec::new();
        for (i, &w) in weights.iter().enumerate() {
            for _ in 0..w {
                schedule.push(i);
            }
        }
        Self {
            counter: AtomicUsize::new(0),
            schedule,
        }
    }
}

impl LoadBalancer for WeightedRoundRobin {
    fn next_index(&self, _pool_size: usize) -> usize {
        let current = self.counter.fetch_add(1, Ordering::Relaxed);
        self.schedule[current % self.schedule.len()]
    }
}

// --- Consistent Hashing ---

fn hash_key(key: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

pub struct ConsistentHash {
    /// Sorted ring of (hash_value, upstream_index) pairs.
    ring: Vec<(u64, usize)>,
}

impl ConsistentHash {
    pub fn new(pool_size: usize, replicas: usize) -> Self {
        let mut ring = Vec::with_capacity(pool_size * replicas);
        for i in 0..pool_size {
            for r in 0..replicas {
                let vnode_key = format!("{}:{}", i, r);
                let hash = hash_key(&vnode_key);
                ring.push((hash, i));
            }
        }
        ring.sort_by_key(|&(h, _)| h);
        Self { ring }
    }
}

impl LoadBalancer for ConsistentHash {
    fn next_index(&self, _pool_size: usize) -> usize {
        // Fallback for callers that don't provide a key
        0
    }

    fn next_index_for_key(&self, _pool_size: usize, key: &str) -> usize {
        let hash = hash_key(key);
        // Binary search for the first ring entry >= hash
        match self.ring.binary_search_by_key(&hash, |&(h, _)| h) {
            Ok(pos) => self.ring[pos].1,
            Err(pos) => {
                if pos >= self.ring.len() {
                    self.ring[0].1 // Wrap around
                } else {
                    self.ring[pos].1
                }
            }
        }
    }
}

// --- Factory ---

pub fn create_balancer(
    strategy: &BalancerStrategy,
    pool_size: usize,
    weights: Option<&[u32]>,
) -> Box<dyn LoadBalancer> {
    match strategy {
        BalancerStrategy::RoundRobin => Box::new(RoundRobin::new()),
        BalancerStrategy::LeastConnections => Box::new(LeastConnections::new(pool_size)),
        BalancerStrategy::WeightedRoundRobin => {
            let default_weights: Vec<u32> = vec![1; pool_size];
            let w = weights.unwrap_or(&default_weights);
            Box::new(WeightedRoundRobin::new(w.to_vec()))
        }
        BalancerStrategy::ConsistentHash => Box::new(ConsistentHash::new(pool_size, 150)),
    }
}
