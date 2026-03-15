<!-- status: DONE --># Phase 2: Cross-Cutting Features

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build generational search (SAGE), stall detection with strategy escalation, Driller-style feedback loop, function summaries for Python stdlib, and MC/DC coverage tier.

**Architecture:** Five items spanning four crates. A.6 upgrades `SymbolicSession` to batch-negate all prefixes in one pass. C.1 adds a `CoverageMonitor` with sliding window growth detection. C.2 adds a `SeedExchange` for Driller-style fuzz↔concolic feedback. C.3 adds stdlib summaries to avoid tracing into known functions. D.3 extends `BranchId` and `CoverageOracle` for MC/DC coverage.

**Tech Stack:** Rust, async-trait, DashMap, proptest

**Spec:** `docs/superpowers/specs/2026-03-11-apex-research-implementation-design.md`
**Depends on:** Phase 1 (Solver trait A.1, Mutator trait B.1, SanCov D.2)

> **Scope note:** B.5 (RedQueen/CmpLog) is listed in the spec under Phase 2 but is deferred. CmpLog requires `__sanitizer_cov_trace_cmp` callbacks and I2S (input-to-state) replacement, which have no existing infrastructure. It will be addressed in a future plan once the SanCov callback layer (D.2) is validated end-to-end.
>
> C.2 (Driller loop) is partially covered: Task 5 creates `SeedExchange` and Task 6 wires `CoverageMonitor`. Full Driller integration (depositing frontier seeds from fuzzer → concolic → merge back) requires the orchestrator refactor in Phase 3's ensemble sync (C.4).

---

## Chunk 1: Generational Search + Function Summaries

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Modify | `crates/apex-symbolic/src/solver.rs` | Add `SymbolicSession::diverging_inputs_generational()` using `Solver::solve_batch()` |
| Create | `crates/apex-symbolic/src/summaries.rs` | Python stdlib function summaries as constraint substitutions |
| Modify | `crates/apex-symbolic/src/lib.rs` | Re-export `summaries` module |
| Modify | `crates/apex-concolic/src/python.rs` | Use generational search + summaries in `symbolic_seeds_from_trace()` |

---

### Task 1: Generational search on SymbolicSession

**Files:**
- Modify: `crates/apex-symbolic/src/solver.rs`

The current `diverging_inputs()` iterates linearly, calling `solve()` once per prefix. Generational search (SAGE-style) negates ALL prefixes in one `solve_batch()` pass.

- [ ] **Step 1: Write the failing test**

Add to `crates/apex-symbolic/src/solver.rs` `#[cfg(test)]` block:

```rust
#[test]
fn session_diverging_inputs_generational_empty() {
    let session = SymbolicSession::new();
    let inputs = session.diverging_inputs_generational().unwrap();
    assert!(inputs.is_empty());
}

#[test]
fn session_diverging_inputs_generational_with_constraints() {
    let mut session = SymbolicSession::new();
    session.push(make_constraint("(> x 0)"));
    session.push(make_constraint("(< y 5)"));
    session.push(make_constraint("(= z 3)"));
    // Without z3-solver, all return None — but method should not panic
    let inputs = session.diverging_inputs_generational().unwrap();
    // Without Z3, no seeds produced
    assert!(inputs.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-symbolic session_diverging_inputs_generational`
Expected: FAIL — method doesn't exist yet

- [ ] **Step 3: Implement `diverging_inputs_generational()`**

Add to `SymbolicSession` in `crates/apex-symbolic/src/solver.rs`:

```rust
/// Generate diverging inputs using generational search (SAGE pattern).
///
/// Instead of solving one prefix at a time (linear), builds ALL
/// prefix-negation constraint sets and solves them in a single batch.
/// For N constraints, generates N constraint sets in one `solve_batch()` call.
pub fn diverging_inputs_generational(&self) -> Result<Vec<InputSeed>> {
    if self.constraints.is_empty() {
        return Ok(Vec::new());
    }

    let smtlibs: Vec<String> =
        self.constraints.iter().map(|c| c.smtlib2.clone()).collect();

    // Build all prefix-negation sets at once.
    let mut constraint_sets: Vec<Vec<String>> = Vec::with_capacity(smtlibs.len());
    for i in 1..=smtlibs.len() {
        let mut set = smtlibs[..i - 1].to_vec();
        // Negate the i-th constraint.
        let negated = format!("(not {})", smtlibs[i - 1]);
        set.push(negated);
        constraint_sets.push(set);
    }

    // Batch solve — each set is independent.
    let results: Vec<Option<InputSeed>> = constraint_sets
        .into_iter()
        .map(|cs| match solve(cs, false) {
            Ok(seed) => seed,
            Err(e) => {
                tracing::debug!(error = %e, "generational solve failed for one prefix");
                None
            }
        })
        .collect();

    Ok(results.into_iter().flatten().collect())
}
```

Note: This uses `solve()` per set for now. When `Solver::solve_batch()` is available from Phase 1 Task 1, this can be optimized to a single batch call. The constraint sets are pre-built with negation already applied (so `negate_last = false`).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p apex-symbolic session_diverging_inputs_generational`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-symbolic/src/solver.rs
git commit -m "feat(symbolic): add generational search (SAGE) to SymbolicSession"
```

---

### Task 2: Function summaries module

**Files:**
- Create: `crates/apex-symbolic/src/summaries.rs`
- Modify: `crates/apex-symbolic/src/lib.rs`

Summaries substitute known constraint patterns for Python stdlib functions, avoiding the need to trace into function bodies.

- [ ] **Step 1: Write the failing test**

Create `crates/apex-symbolic/src/summaries.rs` with tests first:

```rust
//! Function summaries for Python stdlib.
//!
//! When the concolic tracer encounters a call to a known stdlib function,
//! we substitute a pre-defined constraint instead of tracing into the
//! function body. This avoids path explosion in well-understood code.

/// A function summary: given argument names, returns SMTLIB2 constraints
/// that model the function's behavior.
pub struct FunctionSummary {
    /// Fully qualified function name (e.g. "builtins.len").
    pub name: &'static str,
    /// Number of expected arguments.
    pub arity: usize,
    /// Generate constraints given argument variable names.
    /// Returns a list of SMTLIB2 constraint strings.
    pub generate: fn(args: &[&str]) -> Vec<String>,
}

/// Look up a summary by function name.
pub fn lookup(name: &str) -> Option<&'static FunctionSummary> {
    SUMMARIES.iter().find(|s| s.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_len_exists() {
        let s = lookup("builtins.len").unwrap();
        assert_eq!(s.arity, 1);
    }

    #[test]
    fn lookup_range_exists() {
        let s = lookup("builtins.range").unwrap();
        assert_eq!(s.arity, 1);
    }

    #[test]
    fn lookup_unknown_returns_none() {
        assert!(lookup("some.random.function").is_none());
    }

    #[test]
    fn len_summary_generates_constraint() {
        let s = lookup("builtins.len").unwrap();
        let constraints = (s.generate)(&["result"]);
        assert!(!constraints.is_empty());
        // len() result is always >= 0
        assert!(constraints[0].contains(">="));
    }

    #[test]
    fn int_summary_no_constraints() {
        let s = lookup("builtins.int").unwrap();
        let constraints = (s.generate)(&["result"]);
        // int() can produce any integer — no constraints
        assert!(constraints.is_empty());
    }

    #[test]
    fn max_summary_generates_gte() {
        let s = lookup("builtins.max").unwrap();
        let constraints = (s.generate)(&["result", "a", "b"]);
        // result >= a and result >= b
        assert_eq!(constraints.len(), 2);
    }

    #[test]
    fn min_summary_generates_lte() {
        let s = lookup("builtins.min").unwrap();
        let constraints = (s.generate)(&["result", "a", "b"]);
        assert_eq!(constraints.len(), 2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-symbolic summaries`
Expected: FAIL — module doesn't exist yet / `SUMMARIES` undefined

- [ ] **Step 3: Implement summaries**

Add the summary definitions above the tests in `crates/apex-symbolic/src/summaries.rs`:

```rust
static SUMMARIES: &[FunctionSummary] = &[
    FunctionSummary {
        name: "builtins.len",
        arity: 1,
        generate: |args| {
            // len(x) >= 0
            if let Some(result) = args.first() {
                vec![format!("(>= {} 0)", result)]
            } else {
                vec![]
            }
        },
    },
    FunctionSummary {
        name: "builtins.range",
        arity: 1,
        generate: |args| {
            // range(n) produces values in [0, n)
            if let Some(result) = args.first() {
                vec![
                    format!("(>= {} 0)", result),
                ]
            } else {
                vec![]
            }
        },
    },
    FunctionSummary {
        name: "builtins.int",
        arity: 1,
        generate: |_args| {
            // int() can produce any integer
            vec![]
        },
    },
    FunctionSummary {
        name: "builtins.str",
        arity: 1,
        generate: |_args| {
            // str() — no integer constraints
            vec![]
        },
    },
    FunctionSummary {
        name: "builtins.max",
        arity: 2,
        generate: |args| {
            // max(a, b): result >= a, result >= b
            if args.len() >= 3 {
                vec![
                    format!("(>= {} {})", args[0], args[1]),
                    format!("(>= {} {})", args[0], args[2]),
                ]
            } else {
                vec![]
            }
        },
    },
    FunctionSummary {
        name: "builtins.min",
        arity: 2,
        generate: |args| {
            // min(a, b): result <= a, result <= b
            if args.len() >= 3 {
                vec![
                    format!("(<= {} {})", args[0], args[1]),
                    format!("(<= {} {})", args[0], args[2]),
                ]
            } else {
                vec![]
            }
        },
    },
    FunctionSummary {
        name: "builtins.abs",
        arity: 1,
        generate: |args| {
            // abs(x) >= 0
            if let Some(result) = args.first() {
                vec![format!("(>= {} 0)", result)]
            } else {
                vec![]
            }
        },
    },
    FunctionSummary {
        name: "str.split",
        arity: 1,
        generate: |_args| {
            // Returns a list — no simple integer constraint
            vec![]
        },
    },
    FunctionSummary {
        name: "dict.get",
        arity: 2,
        generate: |_args| {
            // Returns value or default — no constraint
            vec![]
        },
    },
];
```

Update `crates/apex-symbolic/src/lib.rs` to add:
```rust
pub mod summaries;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p apex-symbolic summaries`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-symbolic/src/summaries.rs crates/apex-symbolic/src/lib.rs
git commit -m "feat(symbolic): add Python stdlib function summaries"
```

---

### Task 3: Wire generational search into PythonConcolicStrategy

**Files:**
- Modify: `crates/apex-concolic/src/python.rs`

- [ ] **Step 1: Write the failing test**

Add to the test module in `crates/apex-concolic/src/python.rs`:

```rust
#[test]
fn symbolic_seeds_uses_generational() {
    let s = make_strategy();
    let trace = vec![
        make_trace_entry("f.py", 1, 0, "x > 0", "f", "m", vec![], HashMap::new()),
        make_trace_entry("f.py", 2, 0, "y < 5", "f", "m", vec![], HashMap::new()),
        make_trace_entry("f.py", 3, 0, "z == 3", "f", "m", vec![], HashMap::new()),
    ];
    // Without z3-solver, should still not panic
    let seeds = s.symbolic_seeds_from_trace(&trace);
    assert!(seeds.is_empty()); // no Z3 → no seeds, but no panic
}
```

- [ ] **Step 2: Run test — should pass already (smoke test)**

Run: `cargo test -p apex-concolic symbolic_seeds_uses_generational`
Expected: PASS (this validates the existing code path)

- [ ] **Step 3: Switch to generational search**

In `crates/apex-concolic/src/python.rs`, update `symbolic_seeds_from_trace()` to use generational:

Change:
```rust
match session.diverging_inputs() {
```
to:
```rust
match session.diverging_inputs_generational() {
```

- [ ] **Step 4: Run all concolic tests**

Run: `cargo test -p apex-concolic`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-concolic/src/python.rs
git commit -m "feat(concolic): switch to generational search (SAGE) for symbolic seeds"
```

---

## Chunk 2: Stall Detection + Driller Feedback Loop

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/apex-agent/src/monitor.rs` | `CoverageMonitor` sliding window growth rate detector |
| Create | `crates/apex-agent/src/exchange.rs` | `SeedExchange` for Driller-style fuzz↔concolic seeds |
| Modify | `crates/apex-agent/src/orchestrator.rs` | Replace binary stall counter with `CoverageMonitor`; add Driller loop |
| Modify | `crates/apex-agent/src/lib.rs` | Re-export new modules |

---

### Task 4: CoverageMonitor with growth rate detection

**Files:**
- Create: `crates/apex-agent/src/monitor.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/apex-agent/src/monitor.rs`:

```rust
use std::collections::VecDeque;

/// Sliding-window coverage growth monitor.
///
/// Tracks (iteration, coverage_count) pairs and computes growth rate.
/// Returns an `Action` that escalates: Normal → SwitchStrategy → AgentCycle → Stop.
pub struct CoverageMonitor {
    window: VecDeque<(u64, usize)>,
    window_size: usize,
    /// Persistent stall counter — increments each time `record()` observes zero growth,
    /// resets to 0 when growth resumes. Not capped by window_size.
    stall_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonitorAction {
    /// Coverage is progressing — continue normally.
    Normal,
    /// Coverage stalled briefly — try switching strategy.
    SwitchStrategy,
    /// Deep stall — trigger agent cycle.
    AgentCycle,
    /// Extended stall — stop exploration.
    Stop,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_empty_window() {
        let mon = CoverageMonitor::new(10);
        assert_eq!(mon.growth_rate(), 0.0);
    }

    #[test]
    fn record_single_sample() {
        let mut mon = CoverageMonitor::new(10);
        mon.record(0, 100);
        assert_eq!(mon.growth_rate(), 0.0); // need ≥2 samples
    }

    #[test]
    fn record_growing_coverage() {
        let mut mon = CoverageMonitor::new(5);
        mon.record(0, 100);
        mon.record(1, 110);
        mon.record(2, 120);
        assert!(mon.growth_rate() > 0.0);
        assert_eq!(mon.action(), MonitorAction::Normal);
    }

    #[test]
    fn stalled_coverage_escalates() {
        let mut mon = CoverageMonitor::new(3);
        for i in 0..10 {
            mon.record(i, 100); // no growth
        }
        assert_eq!(mon.growth_rate(), 0.0);
        // After enough stalled samples, should escalate
        assert_ne!(mon.action(), MonitorAction::Normal);
    }

    #[test]
    fn window_evicts_old_entries() {
        let mut mon = CoverageMonitor::new(3);
        mon.record(0, 100);
        mon.record(1, 110);
        mon.record(2, 120);
        mon.record(3, 120); // stall
        mon.record(4, 120); // stall
        // Window is [120, 120, 120] — growth = 0
        assert_eq!(mon.growth_rate(), 0.0);
    }

    #[test]
    fn action_escalation_levels() {
        let mut mon = CoverageMonitor::new(3);
        // Fill with stalled data
        for i in 0..3 {
            mon.record(i, 100);
        }
        let a1 = mon.action();
        assert_eq!(a1, MonitorAction::SwitchStrategy);

        // More stalling
        for i in 3..6 {
            mon.record(i, 100);
        }
        let a2 = mon.action();
        assert_eq!(a2, MonitorAction::AgentCycle);

        // Even more stalling
        for i in 6..12 {
            mon.record(i, 100);
        }
        let a3 = mon.action();
        assert_eq!(a3, MonitorAction::Stop);
    }

    #[test]
    fn recovery_resets_escalation() {
        let mut mon = CoverageMonitor::new(3);
        for i in 0..6 {
            mon.record(i, 100); // stall
        }
        assert_ne!(mon.action(), MonitorAction::Normal);

        // Coverage resumes
        mon.record(6, 110);
        mon.record(7, 120);
        mon.record(8, 130);
        assert_eq!(mon.action(), MonitorAction::Normal);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-agent monitor`
Expected: FAIL — `CoverageMonitor` methods don't exist

- [ ] **Step 3: Implement CoverageMonitor**

Add implementation above the tests in `crates/apex-agent/src/monitor.rs`:

```rust
impl CoverageMonitor {
    pub fn new(window_size: usize) -> Self {
        CoverageMonitor {
            window: VecDeque::with_capacity(window_size + 1),
            window_size,
            stall_count: 0,
        }
    }

    /// Record a (iteration, covered_count) sample.
    ///
    /// Updates the persistent `stall_count`: increments when coverage
    /// hasn't grown since the previous sample, resets to 0 on growth.
    pub fn record(&mut self, iteration: u64, covered: usize) {
        let grew = self
            .window
            .back()
            .map_or(false, |(_, prev)| covered > *prev);

        self.window.push_back((iteration, covered));
        while self.window.len() > self.window_size {
            self.window.pop_front();
        }

        if grew {
            self.stall_count = 0;
        } else if self.window.len() >= 2 {
            // Only count stalls once we have ≥2 samples
            self.stall_count += 1;
        }
    }

    /// Compute growth rate as (newest - oldest) / window_size.
    pub fn growth_rate(&self) -> f64 {
        if self.window.len() < 2 {
            return 0.0;
        }
        let oldest = self.window.front().unwrap().1;
        let newest = self.window.back().unwrap().1;
        (newest as f64 - oldest as f64) / self.window.len() as f64
    }

    /// Determine the recommended action based on stall duration.
    ///
    /// Uses the persistent `stall_count` (not the window length) so that
    /// escalation thresholds beyond `window_size` are reachable.
    pub fn action(&self) -> MonitorAction {
        if self.stall_count == 0 {
            return MonitorAction::Normal;
        }

        if self.stall_count >= self.window_size * 4 {
            MonitorAction::Stop
        } else if self.stall_count >= self.window_size * 2 {
            MonitorAction::AgentCycle
        } else {
            MonitorAction::SwitchStrategy
        }
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-agent monitor`
Expected: PASS

- [ ] **Step 5: Update lib.rs and commit**

Add to `crates/apex-agent/src/lib.rs`:
```rust
pub mod monitor;
```

```bash
git add crates/apex-agent/src/monitor.rs crates/apex-agent/src/lib.rs
git commit -m "feat(agent): add CoverageMonitor with sliding-window stall detection"
```

---

### Task 5: SeedExchange for Driller feedback

**Files:**
- Create: `crates/apex-agent/src/exchange.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/apex-agent/src/exchange.rs`:

```rust
use apex_core::types::InputSeed;
use std::sync::Mutex;

/// Bidirectional seed exchange for Driller-style fuzz↔concolic feedback.
///
/// The fuzzer deposits "frontier" seeds (high branch count but blocked)
/// for the concolic engine, and the concolic engine deposits solved seeds
/// back for the fuzzer.
pub struct SeedExchange {
    fuzz_to_concolic: Mutex<Vec<InputSeed>>,
    concolic_to_fuzz: Mutex<Vec<InputSeed>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::SeedOrigin;

    fn make_seed(origin: SeedOrigin) -> InputSeed {
        InputSeed::new(vec![1, 2, 3], origin)
    }

    #[test]
    fn new_is_empty() {
        let ex = SeedExchange::new();
        assert!(ex.take_for_concolic().is_empty());
        assert!(ex.take_for_fuzz().is_empty());
    }

    #[test]
    fn deposit_and_take_fuzz_to_concolic() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(vec![
            make_seed(SeedOrigin::Fuzzer),
            make_seed(SeedOrigin::Fuzzer),
        ]);
        let taken = ex.take_for_concolic();
        assert_eq!(taken.len(), 2);
        // After take, queue is empty
        assert!(ex.take_for_concolic().is_empty());
    }

    #[test]
    fn deposit_and_take_concolic_to_fuzz() {
        let ex = SeedExchange::new();
        ex.deposit_for_fuzz(vec![make_seed(SeedOrigin::Concolic)]);
        let taken = ex.take_for_fuzz();
        assert_eq!(taken.len(), 1);
        assert!(ex.take_for_fuzz().is_empty());
    }

    #[test]
    fn multiple_deposits_accumulate() {
        let ex = SeedExchange::new();
        ex.deposit_for_concolic(vec![make_seed(SeedOrigin::Fuzzer)]);
        ex.deposit_for_concolic(vec![make_seed(SeedOrigin::Fuzzer)]);
        assert_eq!(ex.take_for_concolic().len(), 2);
    }

    #[test]
    fn pending_counts() {
        let ex = SeedExchange::new();
        assert_eq!(ex.pending_for_concolic(), 0);
        assert_eq!(ex.pending_for_fuzz(), 0);
        ex.deposit_for_concolic(vec![make_seed(SeedOrigin::Fuzzer)]);
        assert_eq!(ex.pending_for_concolic(), 1);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-agent exchange`
Expected: FAIL — methods not implemented

- [ ] **Step 3: Implement SeedExchange**

Add above tests in `crates/apex-agent/src/exchange.rs`:

```rust
impl SeedExchange {
    pub fn new() -> Self {
        SeedExchange {
            fuzz_to_concolic: Mutex::new(Vec::new()),
            concolic_to_fuzz: Mutex::new(Vec::new()),
        }
    }

    /// Fuzzer deposits frontier seeds for concolic engine.
    pub fn deposit_for_concolic(&self, seeds: Vec<InputSeed>) {
        self.fuzz_to_concolic.lock().unwrap().extend(seeds);
    }

    /// Concolic engine deposits solved seeds for fuzzer.
    pub fn deposit_for_fuzz(&self, seeds: Vec<InputSeed>) {
        self.concolic_to_fuzz.lock().unwrap().extend(seeds);
    }

    /// Concolic engine takes all pending seeds from fuzzer.
    pub fn take_for_concolic(&self) -> Vec<InputSeed> {
        std::mem::take(&mut *self.fuzz_to_concolic.lock().unwrap())
    }

    /// Fuzzer takes all pending seeds from concolic engine.
    pub fn take_for_fuzz(&self) -> Vec<InputSeed> {
        std::mem::take(&mut *self.concolic_to_fuzz.lock().unwrap())
    }

    pub fn pending_for_concolic(&self) -> usize {
        self.fuzz_to_concolic.lock().unwrap().len()
    }

    pub fn pending_for_fuzz(&self) -> usize {
        self.concolic_to_fuzz.lock().unwrap().len()
    }
}

impl Default for SeedExchange {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-agent exchange`
Expected: PASS

- [ ] **Step 5: Update lib.rs and commit**

Add to `crates/apex-agent/src/lib.rs`:
```rust
pub mod exchange;
```

```bash
git add crates/apex-agent/src/exchange.rs crates/apex-agent/src/lib.rs
git commit -m "feat(agent): add SeedExchange for Driller-style fuzz↔concolic feedback"
```

---

### Task 6: Wire CoverageMonitor into orchestrator

**Files:**
- Modify: `crates/apex-agent/src/orchestrator.rs`

- [ ] **Step 1: Write the failing test**

Add to the orchestrator tests:

```rust
#[test]
fn orchestrator_has_monitor() {
    let oracle = Arc::new(CoverageOracle::new());
    let target = Target {
        root: PathBuf::from("/tmp"),
        language: Language::Rust,
        test_command: vec![],
    };
    let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), target);
    // Monitor should exist and return Normal initially
    assert_eq!(cluster.monitor_action(), crate::monitor::MonitorAction::Normal);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-agent orchestrator_has_monitor`
Expected: FAIL — `monitor_action()` doesn't exist

- [ ] **Step 3: Add CoverageMonitor to AgentCluster**

In `crates/apex-agent/src/orchestrator.rs`:

1. Add imports at the top of the file:
```rust
use crate::monitor::{CoverageMonitor, MonitorAction};
use std::sync::Mutex;
```
2. Add `monitor: Mutex<CoverageMonitor>` field to `AgentCluster`
3. Initialize in `new()` with `monitor: Mutex::new(CoverageMonitor::new(10))`
4. Add `pub fn monitor_action(&self) -> MonitorAction`:
```rust
pub fn monitor_action(&self) -> MonitorAction {
    self.monitor.lock().unwrap().action()
}
```
5. In the `run()` loop, replace the binary stall counter with:
```rust
// Record coverage sample
self.monitor.lock().unwrap().record(iteration, self.oracle.covered_count());

// Check monitor action
match self.monitor.lock().unwrap().action() {
    MonitorAction::Normal => {},
    MonitorAction::SwitchStrategy => {
        info!("coverage growth slowing — consider strategy switch");
    }
    MonitorAction::AgentCycle => {
        if self.agent.is_some() {
            info!("stalled — triggering agent cycle");
            if let Err(e) = self.run_agent_cycle().await {
                warn!(error = %e, "agent cycle failed");
            }
        }
    }
    MonitorAction::Stop => {
        info!("extended stall — stopping exploration");
        break;
    }
}
```

- [ ] **Step 4: Run all orchestrator tests**

Run: `cargo test -p apex-agent`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-agent/src/orchestrator.rs
git commit -m "feat(agent): replace binary stall counter with CoverageMonitor"
```

---

## Chunk 3: MC/DC Coverage Tier

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Modify | `crates/apex-core/src/types.rs` | Add `condition_index` to `BranchId`, `CoverageLevel` enum |
| Modify | `crates/apex-coverage/src/oracle.rs` | Add `CoverageLevel`, `mcdc_independence_pairs()` |

---

### Task 7: Extend BranchId for MC/DC

**Files:**
- Modify: `crates/apex-core/src/types.rs`

- [ ] **Step 1: Write the failing test**

Add to tests in `crates/apex-core/src/types.rs`:

```rust
#[test]
fn branch_id_with_condition_index() {
    let b = BranchId::new_mcdc(1, 10, 0, 0, Some(2));
    assert_eq!(b.condition_index, Some(2));
}

#[test]
fn branch_id_new_has_no_condition_index() {
    let b = BranchId::new(1, 10, 0, 0);
    assert_eq!(b.condition_index, None);
}

#[test]
fn coverage_level_display() {
    assert_eq!(CoverageLevel::Statement.to_string(), "statement");
    assert_eq!(CoverageLevel::Branch.to_string(), "branch");
    assert_eq!(CoverageLevel::Mcdc.to_string(), "mcdc");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-core branch_id_with_condition_index`
Expected: FAIL — `condition_index` field doesn't exist

- [ ] **Step 3: Implement changes**

In `crates/apex-core/src/types.rs`:

1. Add field to `BranchId`:
```rust
pub struct BranchId {
    pub file_id: u64,
    pub line: u32,
    pub col: u16,
    pub direction: u8,
    pub discriminator: u16,
    /// For MC/DC: identifies which sub-condition within a compound condition.
    /// `None` for simple branch coverage.
    #[serde(default)]
    pub condition_index: Option<u8>,
}
```

2. Update `BranchId::new()` to set `condition_index: None`.

3. Add `BranchId::new_mcdc()`:
```rust
pub fn new_mcdc(file_id: u64, line: u32, col: u16, direction: u8, condition_index: Option<u8>) -> Self {
    BranchId {
        file_id,
        line,
        col,
        direction,
        discriminator: 0,
        condition_index,
    }
}
```

4. Add `CoverageLevel` enum:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CoverageLevel {
    Statement,
    Branch,
    Mcdc,
}

impl std::fmt::Display for CoverageLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CoverageLevel::Statement => write!(f, "statement"),
            CoverageLevel::Branch => write!(f, "branch"),
            CoverageLevel::Mcdc => write!(f, "mcdc"),
        }
    }
}
```

- [ ] **Step 4: Run all core tests**

Run: `cargo test -p apex-core`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/types.rs
git commit -m "feat(core): add condition_index to BranchId + CoverageLevel enum for MC/DC"
```

---

### Task 8: MC/DC on CoverageOracle

**Files:**
- Modify: `crates/apex-coverage/src/oracle.rs`

- [ ] **Step 1: Write the failing test**

Add to tests in `crates/apex-coverage/src/oracle.rs`:

```rust
#[test]
fn mcdc_independence_pairs_simple_compound() {
    let oracle = CoverageOracle::new();

    // Compound condition: `a && b` at line 10
    // MC/DC requires independence pairs: changing one condition
    // independently affects the outcome.
    let b_a_true = BranchId::new_mcdc(1, 10, 0, 0, Some(0));
    let b_a_false = BranchId::new_mcdc(1, 10, 0, 1, Some(0));
    let b_b_true = BranchId::new_mcdc(1, 10, 0, 0, Some(1));
    let b_b_false = BranchId::new_mcdc(1, 10, 0, 1, Some(1));

    oracle.register_branches([
        b_a_true.clone(),
        b_a_false.clone(),
        b_b_true.clone(),
        b_b_false.clone(),
    ]);

    // Cover a=true, b=true, a=false
    let seed = SeedId::new();
    oracle.mark_covered(&b_a_true, seed);
    oracle.mark_covered(&b_b_true, seed);
    oracle.mark_covered(&b_a_false, seed);

    let pairs = oracle.mcdc_independence_pairs(1, 10);
    // Should identify that b_b_false is needed for MC/DC
    assert!(!pairs.is_empty());
}

#[test]
fn mcdc_independence_pairs_no_mcdc_branches() {
    let oracle = CoverageOracle::new();
    let b = make_branch(10, 0); // no condition_index
    oracle.register_branches([b]);
    let pairs = oracle.mcdc_independence_pairs(1, 10);
    assert!(pairs.is_empty());
}

#[test]
fn coverage_level_filtering() {
    let oracle = CoverageOracle::new();
    oracle.set_coverage_level(apex_core::types::CoverageLevel::Branch);
    assert_eq!(oracle.coverage_level(), apex_core::types::CoverageLevel::Branch);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-coverage mcdc`
Expected: FAIL — methods don't exist

- [ ] **Step 3: Implement MC/DC on oracle**

In `crates/apex-coverage/src/oracle.rs`:

1. Add import at the top:
```rust
use apex_core::types::CoverageLevel;
use std::sync::Mutex;
```
2. Add field to `CoverageOracle`:
```rust
pub struct CoverageOracle {
    // ... existing fields ...
    level: Mutex<CoverageLevel>,
}
```
3. Update `CoverageOracle::new()` to initialize:
```rust
level: Mutex::new(CoverageLevel::Branch),
```
4. Add setter/getter:
```rust
pub fn set_coverage_level(&self, level: CoverageLevel) {
    *self.level.lock().unwrap() = level;
}

pub fn coverage_level(&self) -> CoverageLevel {
    *self.level.lock().unwrap()
}
```
4. Add `mcdc_independence_pairs()`:
```rust
/// For a compound condition at (file_id, line), return the MC/DC
/// independence pairs that are still missing coverage.
///
/// An independence pair is two BranchIds that differ only in one
/// sub-condition's direction but produce different overall outcomes.
pub fn mcdc_independence_pairs(&self, file_id: u64, line: u32) -> Vec<(BranchId, BranchId)> {
    // Collect all MCDC branches at this location
    let mcdc_branches: Vec<BranchId> = self
        .branches
        .iter()
        .filter(|r| {
            let b = r.key();
            b.file_id == file_id && b.line == line && b.condition_index.is_some()
        })
        .map(|r| r.key().clone())
        .collect();

    if mcdc_branches.is_empty() {
        return Vec::new();
    }

    // Find pairs that differ in exactly one condition_index's direction
    let mut pairs = Vec::new();
    for (i, a) in mcdc_branches.iter().enumerate() {
        for b in mcdc_branches.iter().skip(i + 1) {
            if a.condition_index == b.condition_index && a.direction != b.direction {
                // Check if one is covered and the other isn't
                let a_covered = !matches!(
                    self.state_of(a),
                    Some(BranchState::Uncovered) | None
                );
                let b_covered = !matches!(
                    self.state_of(b),
                    Some(BranchState::Uncovered) | None
                );
                if a_covered != b_covered {
                    pairs.push((a.clone(), b.clone()));
                }
            }
        }
    }
    pairs
}
```

- [ ] **Step 4: Run all coverage tests**

Run: `cargo test -p apex-coverage`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-coverage/src/oracle.rs
git commit -m "feat(coverage): add MC/DC independence pairs + CoverageLevel"
```

---

### Task 9: Integration verification

**Files:** None new — verification only

- [ ] **Step 1: Run full workspace test suite**

```bash
cargo test --workspace
```

Expected: All tests pass

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace -- -D warnings
```

Expected: No warnings

- [ ] **Step 3: Commit any fixes if needed**

```bash
git add -u crates/
git commit -m "fix: address clippy warnings from Phase 2"
```

> **Note:** `git add -u` stages only tracked, modified files — won't pick up untracked artifacts.
