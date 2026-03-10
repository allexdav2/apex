use apex_core::types::InputSeed;
use std::sync::Mutex;

/// Bidirectional seed exchange for fuzz <-> concolic feedback loop.
pub struct SeedExchange {
    fuzz_to_concolic: Mutex<Vec<InputSeed>>,
    concolic_to_fuzz: Mutex<Vec<InputSeed>>,
}

impl SeedExchange {
    pub fn new() -> Self {
        SeedExchange {
            fuzz_to_concolic: Mutex::new(Vec::new()),
            concolic_to_fuzz: Mutex::new(Vec::new()),
        }
    }

    pub fn deposit_for_concolic(&self, seed: InputSeed) {
        self.fuzz_to_concolic
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seed);
    }

    pub fn deposit_for_fuzz(&self, seed: InputSeed) {
        self.concolic_to_fuzz
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(seed);
    }

    pub fn take_for_concolic(&self) -> Vec<InputSeed> {
        std::mem::take(
            &mut *self
                .fuzz_to_concolic
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    }

    pub fn take_for_fuzz(&self) -> Vec<InputSeed> {
        std::mem::take(
            &mut *self
                .concolic_to_fuzz
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
        )
    }

    pub fn pending_for_concolic(&self) -> usize {
        self.fuzz_to_concolic
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub fn pending_for_fuzz(&self) -> usize {
        self.concolic_to_fuzz
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

impl Default for SeedExchange {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::SeedOrigin;

    fn make_seed(data: &[u8]) -> InputSeed {
        InputSeed::new(data.to_vec(), SeedOrigin::Fuzzer)
    }

    #[test]
    fn new_is_empty() {
        let ex = SeedExchange::new();
        assert_eq!(ex.pending_for_concolic(), 0);
        assert_eq!(ex.pending_for_fuzz(), 0);
    }

    #[test]
    fn deposit_and_take_fuzz_to_concolic() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(b"hello"));
        assert_eq!(ex.pending_for_concolic(), 1);

        let seeds = ex.take_for_concolic();
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].data.as_ref(), b"hello");
        assert_eq!(ex.pending_for_concolic(), 0);
    }

    #[test]
    fn deposit_and_take_concolic_to_fuzz() {
        let ex = SeedExchange::new();
        ex.deposit_for_fuzz(make_seed(b"world"));
        assert_eq!(ex.pending_for_fuzz(), 1);

        let seeds = ex.take_for_fuzz();
        assert_eq!(seeds.len(), 1);
        assert_eq!(seeds[0].data.as_ref(), b"world");
        assert_eq!(ex.pending_for_fuzz(), 0);
    }

    #[test]
    fn multiple_deposits_accumulate() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(b"a"));
        ex.deposit_for_concolic(make_seed(b"b"));
        ex.deposit_for_concolic(make_seed(b"c"));
        assert_eq!(ex.pending_for_concolic(), 3);

        let seeds = ex.take_for_concolic();
        assert_eq!(seeds.len(), 3);
    }

    #[test]
    fn pending_counts() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(make_seed(b"x"));
        ex.deposit_for_fuzz(make_seed(b"y"));
        ex.deposit_for_fuzz(make_seed(b"z"));
        assert_eq!(ex.pending_for_concolic(), 1);
        assert_eq!(ex.pending_for_fuzz(), 2);
    }
}
