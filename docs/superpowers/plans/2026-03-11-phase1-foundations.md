<!-- status: DONE --># Phase 1: Foundation Traits & Infrastructure

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract `Solver` and `Mutator` traits, optimize Z3, add adaptive mutation scheduling, energy-based sampling, corpus minimization, taint-guided branch filtering, and pure-Rust SanCov callbacks.

**Architecture:** Three independent streams (A, B, D) touching different crates — can run in parallel worktrees. Stream A refactors apex-symbolic around a `Solver` trait. Stream B refactors apex-fuzz around a `Mutator` trait with adaptive scheduling. Stream D adds taint tracking to apex-concolic and SanCov callbacks to apex-sandbox.

**Tech Stack:** Rust, Z3 (feature-gated), DashMap, proptest, tokio, async-trait

**Spec:** `docs/superpowers/specs/2026-03-11-apex-research-implementation-design.md`

---

## Chunk 1: Stream A — Solver Trait & Z3 Optimizations

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/apex-symbolic/src/traits.rs` | `Solver` trait + `SolverLogic` enum |
| Create | `crates/apex-symbolic/src/cache.rs` | `CachingSolver<S>` wrapper |
| Modify | `crates/apex-symbolic/src/solver.rs` | Extract `Z3Solver` struct implementing `Solver` trait; set logic per language |
| Modify | `crates/apex-symbolic/src/lib.rs` | Re-export new modules |

---

### Task 1: Define the Solver trait

**Files:**
- Create: `crates/apex-symbolic/src/traits.rs`
- Modify: `crates/apex-symbolic/src/lib.rs`

- [ ] **Step 1: Write the failing test**

In `crates/apex-symbolic/src/traits.rs`:

```rust
//! Solver trait abstraction for SMT backends.

use apex_core::{error::Result, types::InputSeed};

/// Which SMT logic to set on the solver. Guides solver heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverLogic {
    /// Quantifier-free linear integer arithmetic (Python targets).
    QfLia,
    /// Quantifier-free arrays + bitvectors (C/Rust compiled targets).
    QfAbv,
    /// Quantifier-free strings (JavaScript/web targets).
    QfS,
    /// Let the solver auto-detect (default).
    Auto,
}

/// Abstraction over SMT solver backends (Z3, Bitwuzla, CVC5, etc.).
pub trait Solver: Send + Sync {
    /// Solve a constraint set. If `negate_last` is true, negate the final constraint
    /// to find an input that takes the opposite branch.
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>>;

    /// Solve multiple constraint sets in one batch. Default implementation
    /// calls `solve()` for each set. Backends can override for efficiency
    /// (e.g., generational search producing N negations in one pass).
    fn solve_batch(&self, sets: &[Vec<String>], negate_last: bool) -> Vec<Result<Option<InputSeed>>> {
        sets.iter().map(|cs| self.solve(cs, negate_last)).collect()
    }

    /// Set the SMT logic for this solver instance.
    fn set_logic(&mut self, logic: SolverLogic);

    /// Human-readable name for logging.
    fn name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solver_logic_debug() {
        assert_eq!(format!("{:?}", SolverLogic::QfLia), "QfLia");
        assert_eq!(format!("{:?}", SolverLogic::QfAbv), "QfAbv");
        assert_eq!(format!("{:?}", SolverLogic::QfS), "QfS");
        assert_eq!(format!("{:?}", SolverLogic::Auto), "Auto");
    }

    #[test]
    fn solver_logic_eq() {
        assert_eq!(SolverLogic::QfLia, SolverLogic::QfLia);
        assert_ne!(SolverLogic::QfLia, SolverLogic::QfAbv);
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/apex-symbolic/src/lib.rs`, add:

```rust
pub mod traits;
pub mod smtlib;
pub mod solver;

pub use solver::{solve, SymbolicSession};
pub use traits::{Solver, SolverLogic};
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-symbolic`
Expected: All existing tests pass + 2 new trait tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-symbolic/src/traits.rs crates/apex-symbolic/src/lib.rs
git commit -m "feat(apex-symbolic): add Solver trait and SolverLogic enum"
```

---

### Task 2: Extract Z3Solver struct implementing Solver trait

**Files:**
- Modify: `crates/apex-symbolic/src/solver.rs`

- [ ] **Step 1: Write the failing test**

Add to `solver.rs` tests:

```rust
#[test]
fn z3_solver_implements_trait() {
    // Z3Solver should implement the Solver trait
    let solver = Z3Solver::new(SolverLogic::Auto);
    assert_eq!(solver.name(), "z3");
}

#[test]
fn z3_solver_set_logic() {
    let mut solver = Z3Solver::new(SolverLogic::Auto);
    solver.set_logic(SolverLogic::QfLia);
    // Should not panic
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-symbolic z3_solver_implements_trait`
Expected: FAIL — `Z3Solver` not defined.

- [ ] **Step 3: Implement Z3Solver**

Refactor `solver.rs`. The `Z3Solver` struct wraps the existing logic. The free `solve()` function delegates to a default `Z3Solver` for backward compatibility:

```rust
use crate::traits::{Solver as SolverTrait, SolverLogic};

/// Z3-backed solver. Without `z3-solver` feature, all methods return None.
pub struct Z3Solver {
    logic: SolverLogic,
}

impl Z3Solver {
    pub fn new(logic: SolverLogic) -> Self {
        Z3Solver { logic }
    }

    /// Factory: pick logic based on target language.
    pub fn for_language(lang: apex_core::types::Language) -> Self {
        use apex_core::types::Language;
        let logic = match lang {
            Language::Python => SolverLogic::QfLia,
            Language::C | Language::Rust => SolverLogic::QfAbv,
            Language::JavaScript => SolverLogic::QfS,
            _ => SolverLogic::Auto,
        };
        Z3Solver::new(logic)
    }
}

impl SolverTrait for Z3Solver {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        if constraints.is_empty() {
            return Ok(None);
        }
        #[cfg(feature = "z3-solver")]
        {
            solve_z3(constraints, negate_last, self.logic)
        }
        #[cfg(not(feature = "z3-solver"))]
        {
            let _ = negate_last;
            warn!("z3-solver feature not enabled");
            Ok(None)
        }
    }

    fn set_logic(&mut self, logic: SolverLogic) {
        self.logic = logic;
    }

    fn name(&self) -> &str {
        "z3"
    }
}

/// Backward-compatible free function (delegates to Z3Solver with Auto logic).
pub fn solve(constraints: Vec<String>, negate_last: bool) -> Result<Option<InputSeed>> {
    let solver = Z3Solver::new(SolverLogic::Auto);
    SolverTrait::solve(&solver, &constraints, negate_last)
}
```

Update `solve_z3` signature to accept `SolverLogic` parameter (used in Task 3):

```rust
#[cfg(feature = "z3-solver")]
fn solve_z3(constraints: &[String], negate_last: bool, logic: SolverLogic) -> Result<Option<InputSeed>> {
    // ... existing body, with `logic` parameter added for Task 3 ...
}
```

**Note:** The existing code uses `ApexError::Symbolic(...)` but the error enum has `ApexError::Solver(...)`. When refactoring, use `ApexError::Solver(...)` consistently.

- [ ] **Step 4: Run all tests**

Run: `cargo test -p apex-symbolic`
Expected: All 12+ existing tests pass + 2 new tests pass. The free `solve()` function preserves backward compatibility.

- [ ] **Step 5: Commit**

```bash
git add crates/apex-symbolic/src/solver.rs
git commit -m "refactor(apex-symbolic): extract Z3Solver struct implementing Solver trait"
```

---

### Task 3: Set logic per target language

**Files:**
- Modify: `crates/apex-symbolic/src/solver.rs`

- [ ] **Step 1: Write tests for set logic**

```rust
#[test]
fn z3_solver_for_language_python() {
    let solver = Z3Solver::for_language(apex_core::types::Language::Python);
    assert_eq!(solver.name(), "z3");
    // Python uses QF_LIA
}

#[test]
fn z3_solver_for_language_rust() {
    let solver = Z3Solver::for_language(apex_core::types::Language::Rust);
    assert_eq!(solver.name(), "z3");
    // Rust/C uses QF_ABV
}

#[test]
fn z3_solver_for_language_js() {
    let solver = Z3Solver::for_language(apex_core::types::Language::JavaScript);
    assert_eq!(solver.name(), "z3");
    // JS uses QF_S
}

// Feature-gated test to verify logic actually changes solver behavior:
#[cfg(feature = "z3-solver")]
#[test]
fn z3_solver_with_logic_qf_lia() {
    let solver = Z3Solver::new(SolverLogic::QfLia);
    let result = solver.solve(&["(> x 0)".into()], false);
    assert!(result.is_ok());
}
```

**Note:** The non-feature-gated tests verify the factory method works without Z3. The `#[cfg(feature = "z3-solver")]` test verifies actual solving behavior. Run with `cargo test -p apex-symbolic --features z3-solver` to execute the gated test.

- [ ] **Step 2: Implement set_logic in solve_z3**

Inside `solve_z3`, after creating `Solver`, set the logic:

```rust
#[cfg(feature = "z3-solver")]
fn solve_z3(constraints: &[String], negate_last: bool, logic: SolverLogic) -> Result<Option<InputSeed>> {
    // ... existing code ...
    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let solver = Solver::new(&ctx);

    // Set logic if specified
    match logic {
        SolverLogic::QfLia => { solver.set_logic("QF_LIA"); }
        SolverLogic::QfAbv => { solver.set_logic("QF_ABV"); }
        SolverLogic::QfS => { solver.set_logic("QF_S"); }
        SolverLogic::Auto => {} // let Z3 decide
    }

    // ... rest of existing solve_z3 code unchanged ...
}
```

Note: True incremental solving (persistent Context across calls) requires `Z3Solver` to hold a `Context`. Since `z3::Context` is not `Send`, this needs `Mutex` wrapping. For now, set logic per-call. Full incremental (push/pop with persistent context) is deferred to Phase 2 A.6. Z3 tactics (A.4: `simplify`, `propagate-values`, `solve-eqs`) are also deferred to Phase 2 as they require the persistent Context.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-symbolic`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-symbolic/src/solver.rs
git commit -m "feat(apex-symbolic): set solver logic per target language"
```

---

### Task 4: Solver cache layer

**Files:**
- Create: `crates/apex-symbolic/src/cache.rs`
- Modify: `crates/apex-symbolic/src/lib.rs`

- [ ] **Step 1: Write the failing test**

In `crates/apex-symbolic/src/cache.rs`:

```rust
//! Caching wrapper for any Solver implementation.
//!
//! Hashes constraint sets and caches results. Implements the KLEE
//! counterexample cache pattern: if a cached SAT model satisfies a
//! new superset of constraints, return it without re-solving.

use crate::traits::{Solver, SolverLogic};
use apex_core::{error::Result, types::InputSeed};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

/// Wraps any `Solver`, caching results by constraint-set hash.
pub struct CachingSolver<S: Solver> {
    inner: S,
    cache: Mutex<HashMap<u64, Option<InputSeed>>>,
    hits: Mutex<u64>,
    misses: Mutex<u64>,
}

impl<S: Solver> CachingSolver<S> {
    pub fn new(inner: S) -> Self {
        CachingSolver {
            inner,
            cache: Mutex::new(HashMap::new()),
            hits: Mutex::new(0),
            misses: Mutex::new(0),
        }
    }

    pub fn hit_count(&self) -> u64 {
        *self.hits.lock().unwrap()
    }

    pub fn miss_count(&self) -> u64 {
        *self.misses.lock().unwrap()
    }

    fn cache_key(constraints: &[String], negate_last: bool) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        for c in constraints {
            c.hash(&mut hasher);
        }
        negate_last.hash(&mut hasher);
        hasher.finish()
    }
}

impl<S: Solver> Solver for CachingSolver<S> {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        let key = Self::cache_key(constraints, negate_last);

        // Check cache
        if let Some(cached) = self.cache.lock().unwrap().get(&key) {
            *self.hits.lock().unwrap() += 1;
            return Ok(cached.clone());
        }

        // Cache miss — delegate to inner solver
        *self.misses.lock().unwrap() += 1;
        let result = self.inner.solve(constraints, negate_last)?;
        self.cache.lock().unwrap().insert(key, result.clone());
        Ok(result)
    }

    fn set_logic(&mut self, logic: SolverLogic) {
        self.inner.set_logic(logic);
    }

    fn name(&self) -> &str {
        "caching"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stub solver that counts calls and returns None.
    struct CountingSolver {
        calls: Mutex<u64>,
    }

    impl CountingSolver {
        fn new() -> Self {
            CountingSolver { calls: Mutex::new(0) }
        }
        fn call_count(&self) -> u64 {
            *self.calls.lock().unwrap()
        }
    }

    impl Solver for CountingSolver {
        fn solve(&self, _constraints: &[String], _negate_last: bool) -> Result<Option<InputSeed>> {
            *self.calls.lock().unwrap() += 1;
            Ok(None)
        }
        fn set_logic(&mut self, _logic: SolverLogic) {}
        fn name(&self) -> &str { "counting" }
    }

    #[test]
    fn cache_hit_avoids_inner_call() {
        let inner = CountingSolver::new();
        let solver = CachingSolver::new(inner);

        let constraints = vec!["(> x 0)".into()];
        let _ = solver.solve(&constraints, false);
        let _ = solver.solve(&constraints, false);

        assert_eq!(solver.hit_count(), 1);
        assert_eq!(solver.miss_count(), 1);
        // Inner solver should only have been called once
        assert_eq!(solver.inner.call_count(), 1);
    }

    #[test]
    fn different_constraints_are_separate_keys() {
        let inner = CountingSolver::new();
        let solver = CachingSolver::new(inner);

        let _ = solver.solve(&["(> x 0)".into()], false);
        let _ = solver.solve(&["(< y 5)".into()], false);

        assert_eq!(solver.hit_count(), 0);
        assert_eq!(solver.miss_count(), 2);
    }

    #[test]
    fn negate_last_changes_key() {
        let inner = CountingSolver::new();
        let solver = CachingSolver::new(inner);

        let constraints = vec!["(> x 0)".into()];
        let _ = solver.solve(&constraints, false);
        let _ = solver.solve(&constraints, true);

        assert_eq!(solver.miss_count(), 2);
    }

    #[test]
    fn set_logic_delegates() {
        let inner = CountingSolver::new();
        let mut solver = CachingSolver::new(inner);
        solver.set_logic(SolverLogic::QfLia); // should not panic
    }

    #[test]
    fn name_returns_caching() {
        let inner = CountingSolver::new();
        let solver = CachingSolver::new(inner);
        assert_eq!(solver.name(), "caching");
    }
}
```

- [ ] **Step 2: Register module**

In `crates/apex-symbolic/src/lib.rs`:

```rust
pub mod cache;
pub mod traits;
pub mod smtlib;
pub mod solver;

pub use cache::CachingSolver;
pub use solver::{solve, SymbolicSession, Z3Solver};
pub use traits::{Solver, SolverLogic};
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-symbolic`
Expected: All existing + 5 new cache tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-symbolic/src/cache.rs crates/apex-symbolic/src/lib.rs
git commit -m "feat(apex-symbolic): add CachingSolver with constraint-set hashing"
```

---

### Task 5: Update SymbolicSession to use Solver trait

**Files:**
- Modify: `crates/apex-symbolic/src/solver.rs`

- [ ] **Step 1: Write test**

Add these imports at the top of the `#[cfg(test)] mod tests` block in `solver.rs`:

```rust
use crate::cache::CachingSolver;
use crate::traits::{Solver as SolverTrait, SolverLogic};
```

Then add the test:

```rust
#[test]
fn session_diverging_inputs_with_solver() {
    let solver = Z3Solver::new(SolverLogic::Auto);
    let cached = CachingSolver::new(solver);
    let mut session = SymbolicSession::new();
    session.push(make_constraint("(> x 0)"));
    // Should work with any Solver impl
    let inputs = session.diverging_inputs_with(&cached).unwrap();
    // Without z3-solver feature, returns empty
    let _ = inputs;
}
```

- [ ] **Step 2: Add `diverging_inputs_with` method**

```rust
impl SymbolicSession {
    /// Generate diverging inputs using a provided solver.
    /// Generational search: negates ALL prefixes in one pass.
    pub fn diverging_inputs_with(&self, solver: &dyn SolverTrait) -> Result<Vec<InputSeed>> {
        if self.constraints.is_empty() {
            return Ok(Vec::new());
        }

        let smtlibs: Vec<String> = self.constraints.iter().map(|c| c.smtlib2.clone()).collect();

        // Build all prefix sets for batch solving (SAGE generational search)
        let sets: Vec<Vec<String>> = (1..=smtlibs.len())
            .map(|i| smtlibs[..i].to_vec())
            .collect();

        let results = solver.solve_batch(&sets, true);
        let mut inputs = Vec::new();
        for result in results {
            match result {
                Ok(Some(seed)) => inputs.push(seed),
                Ok(None) => {}
                Err(e) => {
                    tracing::debug!(error = %e, "symbolic solve failed in batch");
                }
            }
        }
        Ok(inputs)
    }

    /// Original method preserved for backward compatibility.
    pub fn diverging_inputs(&self) -> Result<Vec<InputSeed>> {
        let solver = Z3Solver::new(SolverLogic::Auto);
        self.diverging_inputs_with(&solver)
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-symbolic`
Expected: All existing tests still pass (backward compat) + new test passes.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-symbolic/src/solver.rs
git commit -m "feat(apex-symbolic): SymbolicSession::diverging_inputs_with for pluggable solvers"
```

---

## Chunk 2: Stream B — Mutator Trait & Adaptive Scheduling

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/apex-fuzz/src/traits.rs` | `Mutator` trait |
| Create | `crates/apex-fuzz/src/scheduler.rs` | `MOptScheduler` — adaptive mutation scheduling |
| Modify | `crates/apex-fuzz/src/mutators.rs` | Wrap existing functions as `Mutator` trait impls |
| Modify | `crates/apex-fuzz/src/corpus.rs` | Add `energy` field, power schedule, `minimize()` |
| Modify | `crates/apex-fuzz/src/lib.rs` | Use `MOptScheduler` in `FuzzStrategy::mutate_one` |

---

### Task 6: Define the Mutator trait

**Files:**
- Create: `crates/apex-fuzz/src/traits.rs`
- Modify: `crates/apex-fuzz/src/lib.rs`

- [ ] **Step 1: Write the trait and tests**

In `crates/apex-fuzz/src/traits.rs`:

```rust
//! Mutator trait for pluggable mutation operators.

use rand::RngCore;

/// A single mutation operator that transforms input bytes.
pub trait Mutator: Send + Sync {
    /// Apply this mutation to `input`, returning a new byte vector.
    fn mutate(&self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8>;

    /// Human-readable name for logging and scheduling stats.
    fn name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct IdentityMutator;
    impl Mutator for IdentityMutator {
        fn mutate(&self, input: &[u8], _rng: &mut dyn RngCore) -> Vec<u8> {
            input.to_vec()
        }
        fn name(&self) -> &str { "identity" }
    }

    #[test]
    fn identity_mutator_preserves_input() {
        let m = IdentityMutator;
        let mut rng = rand::thread_rng();
        assert_eq!(m.mutate(b"hello", &mut rng), b"hello");
    }

    #[test]
    fn mutator_name() {
        let m = IdentityMutator;
        assert_eq!(m.name(), "identity");
    }

    #[test]
    fn mutator_is_object_safe() {
        // Verify trait object works
        let m: Box<dyn Mutator> = Box::new(IdentityMutator);
        let mut rng = rand::thread_rng();
        let _ = m.mutate(b"test", &mut rng);
    }
}
```

- [ ] **Step 2: Register module**

In `crates/apex-fuzz/src/lib.rs`, add `pub mod traits;` at the top.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-fuzz traits`
Expected: 3 new tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-fuzz/src/traits.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(apex-fuzz): add Mutator trait for pluggable mutation operators"
```

---

### Task 7: Wrap existing mutators as Mutator trait impls

**Files:**
- Modify: `crates/apex-fuzz/src/mutators.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn all_builtin_mutators_implement_trait() {
    use crate::traits::Mutator;
    let mutators: Vec<Box<dyn Mutator>> = builtin_mutators();
    assert_eq!(mutators.len(), 7);
    let names: Vec<&str> = mutators.iter().map(|m| m.name()).collect();
    assert!(names.contains(&"bit_flip"));
    assert!(names.contains(&"byte_flip"));
    assert!(names.contains(&"byte_arith"));
    assert!(names.contains(&"interesting_byte"));
    assert!(names.contains(&"insert_byte"));
    assert!(names.contains(&"delete_byte"));
    assert!(names.contains(&"duplicate_block"));
}
```

- [ ] **Step 2: Implement wrapper structs**

At the end of `mutators.rs`, add:

```rust
use crate::traits::Mutator;
use rand::RngCore;

macro_rules! mutator_struct {
    ($name:ident, $func:ident, $label:expr) => {
        pub struct $name;
        impl Mutator for $name {
            fn mutate(&self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8> {
                // RngCore is not Rng, but we can wrap it
                let mut wrapper = RngCoreWrapper(rng);
                $func(input, &mut wrapper)
            }
            fn name(&self) -> &str { $label }
        }
    };
}

/// Wrapper to use `&mut dyn RngCore` where `impl Rng` is expected.
struct RngCoreWrapper<'a>(&'a mut dyn RngCore);

impl rand::RngCore for RngCoreWrapper<'_> {
    fn next_u32(&mut self) -> u32 { self.0.next_u32() }
    fn next_u64(&mut self) -> u64 { self.0.next_u64() }
    fn fill_bytes(&mut self, dest: &mut [u8]) { self.0.fill_bytes(dest) }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> std::result::Result<(), rand::Error> {
        self.0.try_fill_bytes(dest)
    }
}

mutator_struct!(BitFlipMutator, bit_flip, "bit_flip");
mutator_struct!(ByteFlipMutator, byte_flip, "byte_flip");
mutator_struct!(ByteArithMutator, byte_arith, "byte_arith");
mutator_struct!(InterestingByteMutator, interesting_byte, "interesting_byte");
mutator_struct!(InsertByteMutator, insert_byte, "insert_byte");
mutator_struct!(DeleteByteMutator, delete_byte, "delete_byte");
mutator_struct!(DuplicateBlockMutator, duplicate_block, "duplicate_block");

/// All 7 built-in mutators as trait objects.
pub fn builtin_mutators() -> Vec<Box<dyn Mutator>> {
    vec![
        Box::new(BitFlipMutator),
        Box::new(ByteFlipMutator),
        Box::new(ByteArithMutator),
        Box::new(InterestingByteMutator),
        Box::new(InsertByteMutator),
        Box::new(DeleteByteMutator),
        Box::new(DuplicateBlockMutator),
    ]
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-fuzz`
Expected: All existing 20+ mutator tests pass + 1 new test passes.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-fuzz/src/mutators.rs
git commit -m "feat(apex-fuzz): wrap 7 built-in mutators as Mutator trait impls"
```

---

### Task 8: MOpt adaptive mutation scheduler

**Files:**
- Create: `crates/apex-fuzz/src/scheduler.rs`

- [ ] **Step 1: Write the full module with tests**

```rust
//! MOpt-style adaptive mutation scheduling.
//!
//! Tracks per-mutator success rates (coverage hits / applications) and
//! biases selection toward productive operators using an exponential
//! moving average.

use crate::traits::Mutator;
use rand::RngCore;

/// Per-mutator statistics.
struct MutatorStats {
    applications: u64,
    coverage_hits: u64,
    ema_yield: f64,
}

/// Adaptive scheduler that selects mutators proportional to their yield.
pub struct MOptScheduler {
    mutators: Vec<Box<dyn Mutator>>,
    stats: Vec<MutatorStats>,
    /// Minimum selection probability to prevent starvation.
    floor: f64,
    /// EMA decay factor (0.0–1.0). Higher = more responsive.
    alpha: f64,
}

impl MOptScheduler {
    pub fn new(mutators: Vec<Box<dyn Mutator>>) -> Self {
        let n = mutators.len();
        MOptScheduler {
            mutators,
            stats: (0..n).map(|_| MutatorStats {
                applications: 0,
                coverage_hits: 0,
                ema_yield: 1.0, // start uniform
            }).collect(),
            floor: 0.01,
            alpha: 0.1,
        }
    }

    /// Select a mutator index weighted by EMA yield.
    pub fn select(&self, rng: &mut dyn RngCore) -> usize {
        if self.mutators.is_empty() {
            return 0;
        }
        let weights: Vec<f64> = self.stats.iter()
            .map(|s| s.ema_yield.max(self.floor))
            .collect();
        let total: f64 = weights.iter().sum();
        let mut pick = (rng.next_u64() as f64 / u64::MAX as f64) * total;
        for (i, w) in weights.iter().enumerate() {
            pick -= w;
            if pick <= 0.0 {
                return i;
            }
        }
        self.mutators.len() - 1
    }

    /// Apply a selected mutator.
    pub fn mutate(&mut self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8> {
        let idx = self.select(rng);
        self.stats[idx].applications += 1;
        self.mutators[idx].mutate(input, rng)
    }

    /// Report that the most recently selected mutator produced coverage.
    pub fn report_hit(&mut self, mutator_idx: usize) {
        if mutator_idx >= self.stats.len() {
            return;
        }
        let s = &mut self.stats[mutator_idx];
        s.coverage_hits += 1;
        let yield_now = if s.applications > 0 {
            s.coverage_hits as f64 / s.applications as f64
        } else {
            0.0
        };
        s.ema_yield = self.alpha * yield_now + (1.0 - self.alpha) * s.ema_yield;
    }

    /// Report that the most recently selected mutator did NOT produce coverage.
    pub fn report_miss(&mut self, mutator_idx: usize) {
        if mutator_idx >= self.stats.len() {
            return;
        }
        let s = &mut self.stats[mutator_idx];
        let yield_now = if s.applications > 0 {
            s.coverage_hits as f64 / s.applications as f64
        } else {
            0.0
        };
        s.ema_yield = self.alpha * yield_now + (1.0 - self.alpha) * s.ema_yield;
    }

    pub fn len(&self) -> usize {
        self.mutators.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mutators.is_empty()
    }

    /// Get stats for logging: (name, applications, hits, ema_yield).
    pub fn stats_summary(&self) -> Vec<(&str, u64, u64, f64)> {
        self.mutators.iter().zip(self.stats.iter())
            .map(|(m, s)| (m.name(), s.applications, s.coverage_hits, s.ema_yield))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    struct ConstMutator { name: &'static str }
    impl Mutator for ConstMutator {
        fn mutate(&self, input: &[u8], _rng: &mut dyn RngCore) -> Vec<u8> {
            input.to_vec()
        }
        fn name(&self) -> &str { self.name }
    }

    fn make_scheduler(n: usize) -> MOptScheduler {
        let mutators: Vec<Box<dyn Mutator>> = (0..n)
            .map(|i| -> Box<dyn Mutator> {
                Box::new(ConstMutator { name: Box::leak(format!("m{i}").into_boxed_str()) })
            })
            .collect();
        MOptScheduler::new(mutators)
    }

    #[test]
    fn select_returns_valid_index() {
        let scheduler = make_scheduler(5);
        let mut rng = StdRng::seed_from_u64(42);
        for _ in 0..100 {
            let idx = scheduler.select(&mut rng);
            assert!(idx < 5);
        }
    }

    #[test]
    fn report_hit_increases_ema() {
        let mut scheduler = make_scheduler(3);
        let initial_ema = scheduler.stats[0].ema_yield;
        // Apply mutator 0 a bunch, then report a hit
        scheduler.stats[0].applications = 10;
        scheduler.report_hit(0);
        // EMA should increase since yield_now = 1/10 but was decayed from 1.0
        // The exact value depends on alpha, but it should change
        assert_ne!(scheduler.stats[0].ema_yield, initial_ema);
    }

    #[test]
    fn high_yield_mutator_selected_more() {
        let mut scheduler = make_scheduler(2);
        let mut rng = StdRng::seed_from_u64(0);

        // Make mutator 0 very productive
        scheduler.stats[0].applications = 100;
        scheduler.stats[0].coverage_hits = 90;
        scheduler.stats[0].ema_yield = 0.9;
        // Make mutator 1 unproductive
        scheduler.stats[1].applications = 100;
        scheduler.stats[1].coverage_hits = 1;
        scheduler.stats[1].ema_yield = 0.01; // at floor

        let mut count_0 = 0;
        for _ in 0..1000 {
            if scheduler.select(&mut rng) == 0 {
                count_0 += 1;
            }
        }
        // Mutator 0 should be selected ~99% of the time (0.9 / (0.9 + 0.01))
        assert!(count_0 > 800, "expected > 800, got {count_0}");
    }

    #[test]
    fn stats_summary_returns_all() {
        let scheduler = make_scheduler(3);
        let summary = scheduler.stats_summary();
        assert_eq!(summary.len(), 3);
        assert_eq!(summary[0].0, "m0");
    }

    #[test]
    fn len_and_is_empty() {
        let scheduler = make_scheduler(3);
        assert_eq!(scheduler.len(), 3);
        assert!(!scheduler.is_empty());

        let empty = make_scheduler(0);
        assert!(empty.is_empty());
    }

    #[test]
    fn report_hit_out_of_bounds_no_panic() {
        let mut scheduler = make_scheduler(2);
        scheduler.report_hit(99); // should not panic
    }
}
```

- [ ] **Step 2: Register module**

In `crates/apex-fuzz/src/lib.rs`, add `pub mod scheduler;`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-fuzz scheduler`
Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-fuzz/src/scheduler.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(apex-fuzz): add MOptScheduler for adaptive mutation selection"
```

---

### Task 9: Energy-based power schedule in Corpus

**Files:**
- Modify: `crates/apex-fuzz/src/corpus.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PowerSchedule {
    Explore,  // uniform energy
    Fast,     // inverse of fuzz_count * path_length
    Rare,     // proportional to rarity of covered edges
}

// In tests:
#[test]
fn energy_field_exists() {
    let mut c = Corpus::new(10);
    c.add(vec![1], 1);
    assert!(c.entries.front().unwrap().energy > 0.0);
}

#[test]
fn set_power_schedule() {
    let mut c = Corpus::new(10);
    c.set_power_schedule(PowerSchedule::Rare);
    // Should not panic
}
```

- [ ] **Step 2: Add energy field and power schedule**

Update `CorpusEntry`:

```rust
#[derive(Clone)]
pub struct CorpusEntry {
    pub data: Vec<u8>,
    pub coverage_gain: usize,
    pub energy: f64,
    pub fuzz_count: u64,
    pub covered_edges: Vec<u64>,  // edge hashes this entry covers
}
```

Update `Corpus`:

```rust
pub struct Corpus {
    entries: VecDeque<CorpusEntry>,
    max_size: usize,
    schedule: PowerSchedule,
}

impl Corpus {
    pub fn set_power_schedule(&mut self, schedule: PowerSchedule) {
        self.schedule = schedule;
        self.recalculate_energy();
    }

    fn recalculate_energy(&mut self) {
        match self.schedule {
            PowerSchedule::Explore => {
                for e in &mut self.entries {
                    e.energy = 1.0;
                }
            }
            PowerSchedule::Fast => {
                for e in &mut self.entries {
                    e.energy = 1.0 / ((e.fuzz_count.max(1) as f64) * (e.data.len().max(1) as f64));
                }
            }
            PowerSchedule::Rare => {
                // True edge-rarity: count how many entries cover each edge,
                // then assign higher energy to entries covering rare edges.
                let mut edge_counts: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
                for e in self.entries.iter() {
                    for &edge in &e.covered_edges {
                        *edge_counts.entry(edge).or_default() += 1;
                    }
                }
                for e in &mut self.entries {
                    if e.covered_edges.is_empty() {
                        e.energy = 1.0;
                    } else {
                        // Sum of (1/count) for each edge — rare edges contribute more
                        e.energy = e.covered_edges.iter()
                            .map(|edge| 1.0 / *edge_counts.get(edge).unwrap_or(&1) as f64)
                            .sum::<f64>();
                    }
                }
            }
        }
    }

    /// Update sample() to use energy instead of coverage_gain for weighting.
    pub fn sample(&mut self, rng: &mut impl Rng) -> Option<&CorpusEntry> {
        if self.entries.is_empty() {
            return None;
        }
        let total: f64 = self.entries.iter().map(|e| e.energy.max(0.001)).sum();
        let mut pick = rng.gen::<f64>() * total;
        for entry in &mut self.entries {
            let w = entry.energy.max(0.001);
            if pick < w {
                entry.fuzz_count += 1;
                return Some(entry);
            }
            pick -= w;
        }
        self.entries.back()
    }
}
```

Update `Corpus::new()` to initialize the `schedule` field:

```rust
impl Corpus {
    pub fn new(max_size: usize) -> Self {
        Corpus {
            entries: VecDeque::new(),
            max_size,
            schedule: PowerSchedule::Explore,
        }
    }
}
```

Update `add()` to initialize new fields:

```rust
pub fn add(&mut self, data: Vec<u8>, coverage_gain: usize) {
    if self.entries.len() >= self.max_size {
        self.entries.pop_front();
    }
    self.entries.push_back(CorpusEntry {
        data,
        coverage_gain,
        energy: coverage_gain.max(1) as f64,
        fuzz_count: 0,
        covered_edges: Vec::new(),
    });
}
```

- [ ] **Step 3: Fix existing tests**

Existing tests reference `coverage_gain` weighting — they should still work since `energy` is initialized from `coverage_gain`. The `sample()` signature change (now `&mut self`) may require updating call sites.

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-fuzz`
Expected: All existing tests pass + 2 new tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/apex-fuzz/src/corpus.rs
git commit -m "feat(apex-fuzz): add energy-based power schedules (Explore/Fast/Rare)"
```

---

### Task 10: Corpus minimization

**Files:**
- Modify: `crates/apex-fuzz/src/corpus.rs`

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn minimize_reduces_corpus() {
    let mut c = Corpus::new(100);
    // Add entries where entry 0 covers edges {A, B}, entry 1 covers {B, C}, entry 2 covers {A}
    let mut e0 = CorpusEntry { data: vec![0], coverage_gain: 2, energy: 2.0, fuzz_count: 0, covered_edges: vec![1, 2] };
    let mut e1 = CorpusEntry { data: vec![1], coverage_gain: 2, energy: 2.0, fuzz_count: 0, covered_edges: vec![2, 3] };
    let mut e2 = CorpusEntry { data: vec![2], coverage_gain: 1, energy: 1.0, fuzz_count: 0, covered_edges: vec![1] };
    c.entries.push_back(e0);
    c.entries.push_back(e1);
    c.entries.push_back(e2);

    let minimized = c.minimize();
    // e0 covers {1,2}, e1 covers {2,3} => together cover {1,2,3} => need only 2
    assert!(minimized.len() <= 2);
    // All edges should still be covered
}

#[test]
fn minimize_empty_corpus() {
    let c = Corpus::new(10);
    let minimized = c.minimize();
    assert!(minimized.is_empty());
}
```

- [ ] **Step 2: Implement minimize**

```rust
impl Corpus {
    /// Greedy set-cover minimization. Returns a new Corpus containing the
    /// smallest subset of entries that covers all edges.
    pub fn minimize(&self) -> Corpus {
        use std::collections::HashSet;

        let mut remaining: HashSet<u64> = self.entries.iter()
            .flat_map(|e| e.covered_edges.iter().copied())
            .collect();

        let mut selected = Vec::new();
        let mut used = vec![false; self.entries.len()];

        while !remaining.is_empty() {
            // Find entry covering most remaining edges
            let mut best_idx = None;
            let mut best_count = 0;
            for (i, entry) in self.entries.iter().enumerate() {
                if used[i] { continue; }
                let count = entry.covered_edges.iter()
                    .filter(|e| remaining.contains(e))
                    .count();
                if count > best_count {
                    best_count = count;
                    best_idx = Some(i);
                }
            }
            match best_idx {
                Some(idx) => {
                    used[idx] = true;
                    for edge in &self.entries[idx].covered_edges {
                        remaining.remove(edge);
                    }
                    selected.push(self.entries[idx].clone());
                }
                None => break, // no entry covers remaining edges (shouldn't happen)
            }
        }

        let mut result = Corpus::new(self.max_size);
        result.schedule = self.schedule;
        for entry in selected {
            result.entries.push_back(entry);
        }
        result
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-fuzz corpus`
Expected: All existing + 2 new tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-fuzz/src/corpus.rs
git commit -m "feat(apex-fuzz): add greedy set-cover corpus minimization"
```

---

## Chunk 3: Stream D — Taint Tracking & SanCov Callbacks

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/apex-concolic/src/taint.rs` | Taint propagation types + filter logic |
| Create | `crates/apex-sandbox/src/sancov_rt.rs` | Pure Rust SanCov callbacks |
| Create | `crates/apex-instrument/src/rustc_wrapper.rs` | RUSTC_WRAPPER script logic |
| Modify | `crates/apex-concolic/src/lib.rs` | Register taint module |
| Modify | `crates/apex-sandbox/src/lib.rs` | Register sancov_rt module |

---

### Task 11: Taint tracking types and filter

**Files:**
- Create: `crates/apex-concolic/src/taint.rs`
- Modify: `crates/apex-concolic/src/lib.rs`

- [ ] **Step 1: Write the module with tests**

```rust
//! Taint-guided branch filtering.
//!
//! Marks function parameters as tainted and propagates through assignments.
//! Only branches whose conditions depend on tainted variables are worth
//! solving symbolically. Reduces solver calls by 60-80% on typical code.

use apex_core::types::BranchId;
use std::collections::{HashMap, HashSet};

/// A branch whose condition depends on input-derived (tainted) variables.
#[derive(Debug, Clone)]
pub struct TaintedBranch {
    pub branch_id: BranchId,
    pub tainted_vars: Vec<String>,
    pub condition: String,
}

/// Propagate taint from function parameters through a set of assignments.
///
/// `params` — names of function parameters (initially tainted).
/// `assignments` — list of `(lhs, rhs_vars)` representing `lhs = expr(rhs_vars...)`.
///
/// Returns the set of all tainted variable names.
pub fn propagate_taint(
    params: &[String],
    assignments: &[(String, Vec<String>)],
) -> HashSet<String> {
    let mut tainted: HashSet<String> = params.iter().cloned().collect();
    // Fixed-point iteration: keep propagating until no new taints
    let mut changed = true;
    while changed {
        changed = false;
        for (lhs, rhs_vars) in assignments {
            if !tainted.contains(lhs) && rhs_vars.iter().any(|v| tainted.contains(v)) {
                tainted.insert(lhs.clone());
                changed = true;
            }
        }
    }
    tainted
}

/// Filter branches: only keep those whose condition references tainted variables.
pub fn filter_tainted_branches(
    branches: &[(BranchId, String, Vec<String>)], // (id, condition, vars_in_condition)
    tainted: &HashSet<String>,
) -> Vec<TaintedBranch> {
    branches.iter()
        .filter_map(|(id, condition, cond_vars)| {
            let tainted_vars: Vec<String> = cond_vars.iter()
                .filter(|v| tainted.contains(v.as_str()))
                .cloned()
                .collect();
            if tainted_vars.is_empty() {
                None
            } else {
                Some(TaintedBranch {
                    branch_id: id.clone(),
                    tainted_vars,
                    condition: condition.clone(),
                })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_param_is_tainted() {
        let tainted = propagate_taint(
            &["x".into()],
            &[],
        );
        assert!(tainted.contains("x"));
    }

    #[test]
    fn transitive_taint() {
        let tainted = propagate_taint(
            &["x".into()],
            &[("y".into(), vec!["x".into()])],
        );
        assert!(tainted.contains("x"));
        assert!(tainted.contains("y"));
    }

    #[test]
    fn multi_hop_taint() {
        let tainted = propagate_taint(
            &["x".into()],
            &[
                ("y".into(), vec!["x".into()]),
                ("z".into(), vec!["y".into()]),
            ],
        );
        assert!(tainted.contains("z"));
    }

    #[test]
    fn untainted_stays_clean() {
        let tainted = propagate_taint(
            &["x".into()],
            &[("y".into(), vec!["CONST".into()])],
        );
        assert!(tainted.contains("x"));
        assert!(!tainted.contains("y"));
    }

    #[test]
    fn filter_keeps_tainted_branches() {
        let tainted: HashSet<String> = ["x".into()].into();
        let branches = vec![
            (BranchId::new(1, 10, 0, 0), "x > 5".into(), vec!["x".into()]),
            (BranchId::new(1, 20, 0, 0), "CONST > 0".into(), vec!["CONST".into()]),
        ];
        let filtered = filter_tainted_branches(&branches, &tainted);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].branch_id.line, 10);
    }

    #[test]
    fn filter_empty_branches() {
        let tainted: HashSet<String> = ["x".into()].into();
        let filtered = filter_tainted_branches(&[], &tainted);
        assert!(filtered.is_empty());
    }

    #[test]
    fn filter_no_tainted_vars() {
        let tainted: HashSet<String> = HashSet::new();
        let branches = vec![
            (BranchId::new(1, 10, 0, 0), "y > 0".into(), vec!["y".into()]),
        ];
        let filtered = filter_tainted_branches(&branches, &tainted);
        assert!(filtered.is_empty());
    }

    #[test]
    fn mixed_taint_in_condition() {
        // Condition uses both tainted and clean vars
        let tainted: HashSet<String> = ["x".into()].into();
        let branches = vec![
            (BranchId::new(1, 10, 0, 0), "x + CONST > 5".into(), vec!["x".into(), "CONST".into()]),
        ];
        let filtered = filter_tainted_branches(&branches, &tainted);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].tainted_vars, vec!["x".to_string()]);
    }
}
```

- [ ] **Step 2: Register module**

In `crates/apex-concolic/src/lib.rs`, add `pub mod taint;`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-concolic taint`
Expected: 8 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-concolic/src/taint.rs crates/apex-concolic/src/lib.rs
git commit -m "feat(apex-concolic): add taint propagation and branch filtering"
```

---

### Task 12: Pure Rust SanCov callbacks

**Files:**
- Create: `crates/apex-sandbox/src/sancov_rt.rs`
- Modify: `crates/apex-sandbox/src/lib.rs`

- [ ] **Step 1: Write the module with tests**

```rust
//! Pure Rust SanCov runtime callbacks.
//!
//! When a Rust binary is compiled with `-C passes=sancov-module
//! -C llvm-args=-sanitizer-coverage-trace-pc-guard`, the compiler inserts
//! calls to `__sanitizer_cov_trace_pc_guard` at each edge. These callbacks
//! record edge coverage into a shared bitmap.
//!
//! This module replaces the C shim in `shim.rs` with a pure Rust implementation.

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// Maximum number of edges tracked. Matches the SHM bitmap size.
pub const MAX_EDGES: usize = 65536;

/// Edge hit counters. Each edge gets one byte (saturating).
/// Indexed by guard ID assigned during init.
static COUNTERS: [AtomicU8; MAX_EDGES] = {
    // const initialization of array of AtomicU8
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; MAX_EDGES]
};

/// Total number of guards (edges) registered.
static NUM_GUARDS: AtomicU32 = AtomicU32::new(0);

/// Called by the instrumented binary once at startup for each module.
/// Assigns sequential IDs to guard slots.
///
/// # Safety
/// Called by compiler-inserted code. `start` and `stop` point to a
/// contiguous array of u32 guard slots.
#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_pc_guard_init(
    start: *mut u32,
    stop: *mut u32,
) {
    if start == stop || start.is_null() {
        return;
    }
    let count = (stop as usize - start as usize) / std::mem::size_of::<u32>();
    let base = NUM_GUARDS.fetch_add(count as u32, Ordering::SeqCst);
    for i in 0..count {
        let guard = start.add(i);
        let id = base + i as u32;
        if (id as usize) < MAX_EDGES {
            *guard = id;
        }
    }
}

/// Called at each instrumented edge. Increments the counter for this guard.
///
/// # Safety
/// Called by compiler-inserted code. `guard` points to a valid u32.
#[no_mangle]
pub unsafe extern "C" fn __sanitizer_cov_trace_pc_guard(guard: *mut u32) {
    let idx = *guard as usize;
    if idx < MAX_EDGES {
        // Saturating increment (won't wrap from 255 to 0)
        let _ = COUNTERS[idx].fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| {
            if v < 255 { Some(v + 1) } else { None }
        });
    }
}

/// Read the current coverage bitmap. Returns a copy of all counters.
pub fn read_bitmap() -> [u8; MAX_EDGES] {
    let mut bitmap = [0u8; MAX_EDGES];
    for (i, counter) in COUNTERS.iter().enumerate() {
        bitmap[i] = counter.load(Ordering::Relaxed);
    }
    bitmap
}

/// Reset all counters to zero (between executions).
pub fn reset_bitmap() {
    for counter in COUNTERS.iter() {
        counter.store(0, Ordering::Relaxed);
    }
}

/// Number of registered guards (edges).
pub fn num_guards() -> u32 {
    NUM_GUARDS.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_bitmap_is_zero() {
        reset_bitmap();
        let bitmap = read_bitmap();
        assert!(bitmap.iter().all(|&b| b == 0));
    }

    #[test]
    fn reset_clears_bitmap() {
        // Set a counter manually
        COUNTERS[0].store(42, Ordering::Relaxed);
        reset_bitmap();
        assert_eq!(COUNTERS[0].load(Ordering::Relaxed), 0);
    }

    #[test]
    fn trace_pc_guard_increments() {
        reset_bitmap();
        let mut guard: u32 = 5;
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
        }
        assert_eq!(COUNTERS[5].load(Ordering::Relaxed), 2);
        reset_bitmap();
    }

    #[test]
    fn counter_saturates_at_255() {
        reset_bitmap();
        let mut guard: u32 = 10;
        COUNTERS[10].store(254, Ordering::Relaxed);
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
        }
        // Should saturate at 255, not wrap to 0
        assert_eq!(COUNTERS[10].load(Ordering::Relaxed), 255);
        reset_bitmap();
    }

    #[test]
    fn out_of_bounds_guard_ignored() {
        reset_bitmap();
        let mut guard: u32 = MAX_EDGES as u32 + 100;
        unsafe {
            __sanitizer_cov_trace_pc_guard(&mut guard as *mut u32);
        }
        // Should not panic or corrupt memory
    }

    #[test]
    fn read_bitmap_reflects_counters() {
        reset_bitmap();
        COUNTERS[42].store(7, Ordering::Relaxed);
        let bitmap = read_bitmap();
        assert_eq!(bitmap[42], 7);
        reset_bitmap();
    }
}
```

- [ ] **Step 2: Register module**

In `crates/apex-sandbox/src/lib.rs`, add `pub mod sancov_rt;`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-sandbox sancov_rt -- --test-threads=1`
Expected: 6 tests pass.

> **Note:** `--test-threads=1` is required because these tests mutate global `COUNTERS` state and would race under parallel execution.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-sandbox/src/sancov_rt.rs crates/apex-sandbox/src/lib.rs
git commit -m "feat(apex-sandbox): pure Rust SanCov trace-pc-guard callbacks"
```

---

### Task 13: RUSTC_WRAPPER for instrumented builds

**Files:**
- Create: `crates/apex-instrument/src/rustc_wrapper.rs`
- Modify: `crates/apex-instrument/src/lib.rs`

- [ ] **Step 1: Write module with tests**

```rust
//! RUSTC_WRAPPER logic for SanCov-instrumented builds.
//!
//! Generates the rustc flags needed to enable SanCov instrumentation.
//! Used by APEX when building target code with coverage callbacks.

/// SanCov instrumentation mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SanCovMode {
    /// trace-pc-guard: function call per edge. Most flexible.
    TracePcGuard,
    /// inline-8bit-counters: one `inc` instruction per edge. 2-5x faster.
    Inline8BitCounters,
    /// inline-bool-flag: one store per edge. Fastest, binary only (no hit counts).
    InlineBoolFlag,
}

/// Generate rustc flags for SanCov instrumentation.
pub fn sancov_rustc_flags(mode: SanCovMode, trace_compares: bool) -> Vec<String> {
    let mut flags = vec![
        "-C".into(), "passes=sancov-module".into(),
        "-C".into(), "llvm-args=-sanitizer-coverage-level=3".into(),
        "-C".into(), "codegen-units=1".into(),
    ];

    match mode {
        SanCovMode::TracePcGuard => {
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-trace-pc-guard".into());
        }
        SanCovMode::Inline8BitCounters => {
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-inline-8bit-counters".into());
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-pc-table".into());
        }
        SanCovMode::InlineBoolFlag => {
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-inline-bool-flag".into());
        }
    }

    if trace_compares {
        flags.push("-C".into());
        flags.push("llvm-args=-sanitizer-coverage-trace-compares".into());
    }

    flags
}

/// Generate a complete RUSTC_WRAPPER shell command string.
pub fn wrapper_command(rustc_path: &str, mode: SanCovMode, trace_compares: bool) -> String {
    let flags = sancov_rustc_flags(mode, trace_compares);
    let flag_str = flags.join(" ");
    format!("{rustc_path} {flag_str} \"$@\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_pc_guard_flags() {
        let flags = sancov_rustc_flags(SanCovMode::TracePcGuard, false);
        assert!(flags.contains(&"passes=sancov-module".to_string()));
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-trace-pc-guard".to_string()));
        assert!(!flags.iter().any(|f| f.contains("trace-compares")));
    }

    #[test]
    fn inline_8bit_counters_flags() {
        let flags = sancov_rustc_flags(SanCovMode::Inline8BitCounters, false);
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-inline-8bit-counters".to_string()));
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-pc-table".to_string()));
    }

    #[test]
    fn inline_bool_flag_flags() {
        let flags = sancov_rustc_flags(SanCovMode::InlineBoolFlag, false);
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-inline-bool-flag".to_string()));
    }

    #[test]
    fn trace_compares_added() {
        let flags = sancov_rustc_flags(SanCovMode::TracePcGuard, true);
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-trace-compares".to_string()));
    }

    #[test]
    fn codegen_units_1() {
        let flags = sancov_rustc_flags(SanCovMode::TracePcGuard, false);
        assert!(flags.contains(&"codegen-units=1".to_string()));
    }

    #[test]
    fn wrapper_command_format() {
        let cmd = wrapper_command("rustc", SanCovMode::TracePcGuard, false);
        assert!(cmd.starts_with("rustc"));
        assert!(cmd.contains("sancov-module"));
        assert!(cmd.ends_with("\"$@\""));
    }
}
```

- [ ] **Step 2: Register module**

In `crates/apex-instrument/src/lib.rs`, add `pub mod rustc_wrapper;`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-instrument rustc_wrapper`
Expected: 6 tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/apex-instrument/src/rustc_wrapper.rs crates/apex-instrument/src/lib.rs
git commit -m "feat(apex-instrument): RUSTC_WRAPPER flag generation for SanCov modes"
```

---

### Task 14: Wire FuzzStrategy to use MOptScheduler

**Files:**
- Modify: `crates/apex-fuzz/src/lib.rs`

- [ ] **Step 1: Write test**

```rust
#[tokio::test]
async fn fuzz_strategy_uses_scheduler() {
    use apex_core::types::{ExplorationContext, Target, Language, BranchId};
    use std::path::PathBuf;

    let oracle = Arc::new(CoverageOracle::new());
    let strategy = FuzzStrategy::new(oracle);

    // Seed the corpus so suggest_inputs has something to mutate
    strategy.seed_corpus(vec![b"test".to_vec()]);

    let ctx = ExplorationContext {
        target: Target {
            root: PathBuf::from("/tmp/test"),
            language: Language::Rust,
            test_command: vec!["cargo".into(), "test".into()],
        },
        uncovered_branches: vec![BranchId::new(1, 1, 0, 0)],
        iteration: 0,
    };
    let inputs = strategy.suggest_inputs(&ctx).await.unwrap();
    // Should produce inputs (verifies scheduler is wired correctly)
    assert!(!inputs.is_empty());
}
```

> **Depends on:** Tasks 8 (Mutator trait) and 9 (MOptScheduler) must be completed first.

- [ ] **Step 2: Add import and replace struct + methods**

Add the import at the top of `crates/apex-fuzz/src/lib.rs`:

```rust
use crate::scheduler::MOptScheduler;
```

Then replace the `FuzzStrategy` struct definition and its `new()` + `mutate_one()` methods (lines ~26-64 of the current file). The `seed_corpus()`, `splice_two()`, and the `Strategy` impl remain unchanged.

```rust
#[allow(dead_code)]
pub struct FuzzStrategy {
    oracle: Arc<CoverageOracle>,
    corpus: Mutex<Corpus>,
    rng: Mutex<StdRng>,
    scheduler: Mutex<MOptScheduler>,
}

impl FuzzStrategy {
    pub fn new(oracle: Arc<CoverageOracle>) -> Self {
        FuzzStrategy {
            oracle,
            corpus: Mutex::new(Corpus::new(CORPUS_MAX)),
            rng: Mutex::new(StdRng::from_entropy()),
            scheduler: Mutex::new(MOptScheduler::new(mutators::builtin_mutators())),
        }
    }

    /// Seed the corpus with known-good inputs (e.g. existing test vectors).
    pub fn seed_corpus(&self, data: impl IntoIterator<Item = Vec<u8>>) {
        let mut corpus = self.corpus.lock().unwrap();
        for d in data {
            corpus.add(d, 1);
        }
    }

    fn mutate_one(&self, input: &[u8]) -> Vec<u8> {
        let mut rng = self.rng.lock().unwrap();
        let mut scheduler = self.scheduler.lock().unwrap();
        scheduler.mutate(input, &mut *rng)
    }

    fn splice_two(&self) -> Option<Vec<u8>> {
        let corpus = self.corpus.lock().unwrap();
        let mut rng = self.rng.lock().unwrap();
        let pair = corpus.sample_pair(&mut *rng)?;
        Some(mutators::splice(&pair.0.data, &pair.1.data, &mut *rng))
    }
}
```

- [ ] **Step 3: Run all tests**

Run: `cargo test -p apex-fuzz`
Expected: All tests pass.

- [ ] **Step 4: Run workspace tests**

Run: `cargo test --workspace`
Expected: All 846+ tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/apex-fuzz/src/lib.rs
git commit -m "feat(apex-fuzz): wire FuzzStrategy to MOptScheduler"
```

---

### Task 15: Final integration verification

- [ ] **Step 1: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All tests pass, no regressions.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Verify coverage doesn't regress**

Run:
```bash
LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov \
LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata \
cargo llvm-cov --json 2>&1 | tail -1
```
Expected: Coverage % >= baseline (86.5%).

- [ ] **Step 4: Commit any fixes**

```bash
git add -u crates/
git commit -m "chore: fix clippy warnings from Phase 1 implementation"
```

> **Note:** `git add -u` stages only tracked, modified files — it won't pick up untracked artifacts.
