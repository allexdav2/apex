//! GALS (Globally Asynchronous, Locally Synchronous) ensemble synchronization.
//!
//! Provides a thread-safe buffer for exchanging seeds between concurrent
//! solver/fuzzer agents at configurable intervals.

use std::sync::Mutex;

use apex_core::types::InputSeed;

/// Synchronization primitive for exchanging seeds between ensemble agents.
///
/// Agents deposit interesting seeds into the shared buffer. At each sync
/// interval the buffer is drained and seeds are redistributed.
pub struct EnsembleSync {
    buffer: Mutex<Vec<InputSeed>>,
    interval: u64,
    last_sync: Mutex<u64>,
}

impl EnsembleSync {
    /// Create a new ensemble sync with the given sync interval (in iterations).
    pub fn new(interval: u64) -> Self {
        EnsembleSync {
            buffer: Mutex::new(Vec::new()),
            interval,
            last_sync: Mutex::new(0),
        }
    }

    /// Deposit a seed into the shared buffer.
    pub fn deposit(&self, seed: InputSeed) {
        self.buffer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seed);
    }

    /// Check whether a sync should happen at the given iteration.
    ///
    /// Returns `false` if interval is zero (sync disabled).
    pub fn should_sync(&self, iteration: u64) -> bool {
        if self.interval == 0 {
            return false;
        }
        let last = *self.last_sync.lock().unwrap_or_else(|e| e.into_inner());
        iteration >= last + self.interval
    }

    /// Drain the buffer and reset the sync timer. Returns all pending seeds.
    pub fn sync(&self, iteration: u64) -> Vec<InputSeed> {
        let mut last = self.last_sync.lock().unwrap_or_else(|e| e.into_inner());
        *last = iteration;
        let mut buf = self.buffer.lock().unwrap_or_else(|e| e.into_inner());
        buf.drain(..).collect()
    }

    /// Number of seeds waiting in the buffer.
    pub fn pending_count(&self) -> usize {
        self.buffer.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

impl Default for EnsembleSync {
    fn default() -> Self {
        Self::new(20)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::SeedOrigin;

    fn make_seed() -> InputSeed {
        InputSeed::new(vec![0xAA, 0xBB], SeedOrigin::Fuzzer)
    }

    #[test]
    fn new_is_empty() {
        let sync = EnsembleSync::new(10);
        assert_eq!(sync.pending_count(), 0);
    }

    #[test]
    fn deposit_increments_count() {
        let sync = EnsembleSync::new(10);
        sync.deposit(make_seed());
        assert_eq!(sync.pending_count(), 1);
        sync.deposit(make_seed());
        assert_eq!(sync.pending_count(), 2);
    }

    #[test]
    fn should_sync_at_interval() {
        let sync = EnsembleSync::new(5);
        // last_sync starts at 0, so need iteration >= 0 + 5
        assert!(!sync.should_sync(0));
        assert!(!sync.should_sync(4));
        assert!(sync.should_sync(5));
        assert!(sync.should_sync(10));
    }

    #[test]
    fn sync_drains_buffer() {
        let sync = EnsembleSync::new(5);
        sync.deposit(make_seed());
        sync.deposit(make_seed());
        let seeds = sync.sync(5);
        assert_eq!(seeds.len(), 2);
        assert_eq!(sync.pending_count(), 0);
    }

    #[test]
    fn sync_resets_timer() {
        let sync = EnsembleSync::new(5);
        sync.sync(5);
        // After syncing at 5, next sync should be at 5 + 5 = 10
        assert!(!sync.should_sync(6));
        assert!(!sync.should_sync(9));
        assert!(sync.should_sync(10));
    }

    #[test]
    fn zero_interval_never_syncs() {
        let sync = EnsembleSync::new(0);
        assert!(!sync.should_sync(0));
        assert!(!sync.should_sync(100));
        assert!(!sync.should_sync(u64::MAX));
    }

    #[test]
    fn default_interval_is_20() {
        let sync = EnsembleSync::default();
        assert!(!sync.should_sync(19));
        assert!(sync.should_sync(20));
    }
}
