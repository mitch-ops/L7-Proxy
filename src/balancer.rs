/*
Don't depend on ordering, just need uniqueness
*/

use std::sync::atomic::{AtomicUsize, Ordering};

pub struct RoundRobin {
    counter: AtomicUsize,
}

impl RoundRobin {
    pub fn new() -> Self {
        Self {
            counter: AtomicUsize::new(0),
        }
    }

    pub fn next_index(&self, pool_size: usize) -> usize {
        let current = self.counter.fetch_add(1, Ordering::Relaxed);
        current % pool_size
    }
}