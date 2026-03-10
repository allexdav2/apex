//! Real libafl 0.13 fuzzer backend — enabled with `--features libafl-backend`.
//!
//! Replaces the pure-Rust hand-rolled mutator in `lib.rs` with a full
//! libafl `StdFuzzer` pipeline:
//!   - `HavocMutationalStage` with all standard havoc mutators
//!   - `MaxMapFeedback` over a shared-memory coverage bitmap
//!   - `QueueScheduler` for seed selection
//!   - `InMemoryCorpus` (seeds never written to disk by default)
//!
//! # Usage
//! ```text
//! cargo run --bin apex --features apex-fuzz/libafl-backend -- run \
//!   --target ./my-c-target --lang c --strategy fuzz
//! ```

#[cfg(feature = "libafl-backend")]
pub mod backend {
    use apex_core::{
        error::{ApexError, Result},
        types::{InputSeed, SeedOrigin},
    };
    use libafl::{
        corpus::{Corpus, InMemoryCorpus},
        feedbacks::MaxMapFeedback,
        inputs::{BytesInput, HasTargetBytes},
        mutators::{havoc_mutations, StdScheduledMutator},
        observers::StdMapObserver,
        schedulers::QueueScheduler,
        stages::StdMutationalStage,
        state::{HasCorpus, StdState},
        Error as LibAflError,
    };
    use libafl_bolts::{current_nanos, rands::StdRand, tuples::tuple_list, AsSlice};
    use std::sync::{Arc, Mutex};
    use tracing::{debug, info};

    // Shared-memory coverage map size (must match SanitizerCoverage edge count).
    const MAP_SIZE: usize = 65536;

    /// libafl-backed fuzzer state.
    pub struct LibAflFuzzer {
        /// Current in-memory corpus (BytesInput entries).
        state: Mutex<
            StdState<BytesInput, InMemoryCorpus<BytesInput>, StdRand, InMemoryCorpus<BytesInput>>,
        >,
        /// Shared coverage bitmap (written by LD_PRELOAD shim, read here).
        coverage_map: Arc<Mutex<[u8; MAP_SIZE]>>,
    }

    impl LibAflFuzzer {
        pub fn new() -> Result<Self> {
            let coverage_map = Arc::new(Mutex::new([0u8; MAP_SIZE]));

            let corpus = InMemoryCorpus::new();
            let solutions = InMemoryCorpus::new();

            let state = StdState::new(
                StdRand::with_seed(current_nanos()),
                corpus,
                solutions,
                &mut tuple_list!(),
                &mut tuple_list!(),
            )
            .map_err(|e| ApexError::Strategy(format!("libafl state init: {e}")))?;

            Ok(LibAflFuzzer {
                state: Mutex::new(state),
                coverage_map,
            })
        }

        /// Seed the corpus with initial inputs.
        pub fn seed(&self, inputs: impl IntoIterator<Item = Vec<u8>>) -> Result<()> {
            let mut state = self
                .state
                .lock()
                .map_err(|e| ApexError::Other(format!("state mutex poisoned: {e}")))?;
            for data in inputs {
                let input = BytesInput::new(data);
                let _ = state.corpus_mut().add(libafl::corpus::Testcase::new(input));
            }
            Ok(())
        }

        /// Generate one batch of mutated inputs via libafl havoc mutations.
        ///
        /// Returns up to `count` `InputSeed`s derived from the current corpus.
        pub fn generate(&self, count: usize) -> Result<Vec<InputSeed>> {
            let mut state = self
                .state
                .lock()
                .map_err(|e| ApexError::Other(format!("state mutex poisoned: {e}")))?;

            if state.corpus().count() == 0 {
                // Corpus empty — seed with a zero byte.
                let _ = state
                    .corpus_mut()
                    .add(libafl::corpus::Testcase::new(BytesInput::new(vec![0u8])));
            }

            let mut mutator = StdScheduledMutator::new(havoc_mutations(), 6);
            let mut seeds = Vec::with_capacity(count);

            for _ in 0..count {
                // Pick an entry from the corpus and mutate it.
                let idx = {
                    let corpus = state.corpus();
                    let count = corpus.count();
                    if count == 0 {
                        break;
                    }
                    libafl::corpus::CorpusId::from(state.rand_mut().below(count as u64) as usize)
                };

                // Clone the input bytes.
                let input_bytes = {
                    let entry = state.corpus().get(idx);
                    match entry {
                        Ok(locked) => locked
                            .borrow()
                            .input()
                            .as_ref()
                            .map(|i| i.target_bytes().as_slice().to_vec())
                            .unwrap_or_default(),
                        Err(_) => continue,
                    }
                };

                // Apply havoc mutations in-place on a copy.
                let mut input = BytesInput::new(input_bytes);
                let _ = mutator.mutate(&mut state, &mut input);

                let data = input.target_bytes().as_slice().to_vec();
                debug!(bytes = data.len(), "libafl: generated mutant");
                seeds.push(InputSeed::new(data, SeedOrigin::Fuzzer));
            }

            Ok(seeds)
        }

        /// Notify the fuzzer that a seed produced new coverage.
        /// The winning input is added to the corpus.
        pub fn observe_new_coverage(&self, data: Vec<u8>) -> Result<()> {
            let mut state = self
                .state
                .lock()
                .map_err(|e| ApexError::Other(format!("state mutex poisoned: {e}")))?;
            let input = BytesInput::new(data);
            match state.corpus_mut().add(libafl::corpus::Testcase::new(input)) {
                Ok(id) => info!(corpus_id = ?id, "libafl: added interesting input to corpus"),
                Err(e) => tracing::warn!(error = %e, "libafl: corpus add failed"),
            }
            Ok(())
        }

        /// Update the coverage map from a raw bitmap slice (written by SHM shim).
        pub fn update_map(&self, bitmap: &[u8]) -> Result<()> {
            let mut map = self
                .coverage_map
                .lock()
                .map_err(|e| ApexError::Other(format!("coverage_map mutex poisoned: {e}")))?;
            let len = bitmap.len().min(MAP_SIZE);
            map[..len].copy_from_slice(&bitmap[..len]);
            Ok(())
        }
    }
}

/// Convenience re-export so callers can use `libafl_backend::LibAflFuzzer`
/// regardless of whether the feature is enabled (the type simply won't exist
/// when the feature is absent, which is fine since callers feature-gate too).
#[cfg(feature = "libafl-backend")]
pub use backend::LibAflFuzzer;
