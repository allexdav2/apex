# Phase 3: Advanced Features

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Multi-solver portfolio, directed fuzzing (AFLGo), GALS ensemble sync, Python proxy symbolic engine (CrossHair-style), and Kani BMC unreachability proofs.

**Architecture:** Five independent workstreams touching different crates. A.7 adds `PortfolioSolver` wrapping multiple `Solver` backends. B.6 adds distance-guided energy to corpus entries. C.4 adds cross-strategy seed sharing. E.1-E.5 builds the Python proxy object symbolic engine with PyO3 bridge. F.1 adds Kani-based bounded model checking.

**Tech Stack:** Rust, Z3, PyO3 (feature-gated), Kani (feature-gated), async-trait, tokio

**Spec:** `docs/superpowers/specs/2026-03-11-apex-research-implementation-design.md`
**Depends on:** Phase 1 (A.1 Solver trait, B.1 Mutator trait, B.3 energy model), Phase 2 (C.1 CoverageMonitor, D.3 MC/DC)

> **Scope note:** E.2 (PyO3 bridge) and E.5 (concretization boundary) are deferred. E.2 requires a working Python proxy engine (E.1, E.3, E.4 built here) before the PyO3 bridge is useful. E.5 depends on E.2 to evaluate symbolic expressions at the Rust↔Python boundary. Both will be added as follow-up tasks once E.1-E.4 are validated.
>
> All Phase 3 tasks depend on Phase 1 types (`Solver`, `Mutator`, `PowerSchedule`, `CorpusEntry` with energy). If Phase 1 has not been implemented, these types will not exist and compilation will fail. Ensure Phase 1 is complete before starting.

---

## Chunk 1: Multi-Solver Portfolio + Directed Fuzzing + GALS

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/apex-symbolic/src/portfolio.rs` | `PortfolioSolver` running multiple backends with timeout |
| Modify | `crates/apex-symbolic/Cargo.toml` | Feature gates for `bitwuzla-solver`, `cvc5-solver` |
| Modify | `crates/apex-fuzz/src/corpus.rs` | Add `distance_to_target` field, directed sampling |
| Create | `crates/apex-fuzz/src/directed.rs` | `DirectedScheduler` with simulated annealing |
| Create | `crates/apex-agent/src/ensemble.rs` | `EnsembleSync` for GALS cross-strategy sharing |

---

### Task 1: PortfolioSolver

**Files:**
- Create: `crates/apex-symbolic/src/portfolio.rs`
- Modify: `crates/apex-symbolic/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Create `crates/apex-symbolic/src/portfolio.rs`:

```rust
//! Multi-solver portfolio: runs multiple `Solver` backends in parallel,
//! returns the first SAT result. Feature-gated per backend.

use crate::traits::{Solver, SolverLogic};
use apex_core::types::InputSeed;
use std::time::Duration;

/// Runs multiple solvers with a timeout, returns first SAT result.
pub struct PortfolioSolver {
    solvers: Vec<Box<dyn Solver>>,
    timeout: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Stub solver that always returns None.
    struct NullSolver;
    impl Solver for NullSolver {
        fn solve(&self, _constraints: &[String], _negate_last: bool) -> Option<InputSeed> {
            None
        }
        fn solve_batch(&self, sets: Vec<Vec<String>>) -> Vec<Option<InputSeed>> {
            sets.iter().map(|_| None).collect()
        }
        fn set_logic(&mut self, _logic: SolverLogic) {}
        fn name(&self) -> &str { "null" }
    }

    #[test]
    fn empty_portfolio_returns_none() {
        let p = PortfolioSolver::new(vec![], Duration::from_secs(5));
        assert!(p.solve(&["(> x 0)".into()], false).is_none());
    }

    #[test]
    fn single_null_solver_returns_none() {
        let p = PortfolioSolver::new(
            vec![Box::new(NullSolver)],
            Duration::from_secs(5),
        );
        assert!(p.solve(&["(> x 0)".into()], false).is_none());
    }

    #[test]
    fn portfolio_name() {
        let p = PortfolioSolver::new(vec![], Duration::from_secs(5));
        assert_eq!(p.name(), "portfolio");
    }

    #[test]
    fn portfolio_solve_batch_delegates() {
        let p = PortfolioSolver::new(
            vec![Box::new(NullSolver)],
            Duration::from_secs(5),
        );
        let results = p.solve_batch(vec![
            vec!["(> x 0)".into()],
            vec!["(< y 5)".into()],
        ]);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_none()));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-symbolic portfolio`
Expected: FAIL

- [ ] **Step 3: Implement PortfolioSolver**

```rust
impl PortfolioSolver {
    pub fn new(solvers: Vec<Box<dyn Solver>>, timeout: Duration) -> Self {
        PortfolioSolver { solvers, timeout }
    }

    pub fn add_solver(&mut self, solver: Box<dyn Solver>) {
        self.solvers.push(solver);
    }
}

impl Solver for PortfolioSolver {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Option<InputSeed> {
        // Try each solver sequentially; return first SAT.
        // TODO: When async is available, run in parallel with timeout.
        for solver in &self.solvers {
            if let Some(seed) = solver.solve(constraints, negate_last) {
                return Some(seed);
            }
        }
        None
    }

    fn solve_batch(&self, sets: Vec<Vec<String>>) -> Vec<Option<InputSeed>> {
        // Delegate each set to the portfolio's solve()
        sets.into_iter()
            .map(|cs| self.solve(&cs, false))
            .collect()
    }

    fn set_logic(&mut self, logic: SolverLogic) {
        for solver in &mut self.solvers {
            solver.set_logic(logic);
        }
    }

    fn name(&self) -> &str {
        "portfolio"
    }
}
```

Update `crates/apex-symbolic/src/lib.rs`:
```rust
pub mod portfolio;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-symbolic portfolio`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-symbolic/src/portfolio.rs crates/apex-symbolic/src/lib.rs
git commit -m "feat(symbolic): add PortfolioSolver for multi-backend solving"
```

---

### Task 2: Directed sampling (AFLGo-style)

**Files:**
- Create: `crates/apex-fuzz/src/directed.rs`
- Modify: `crates/apex-fuzz/src/corpus.rs`
- Modify: `crates/apex-fuzz/src/lib.rs` (re-export)

- [ ] **Step 1: Write failing tests**

Create `crates/apex-fuzz/src/directed.rs`:

```rust
//! Directed fuzzing via simulated annealing (AFLGo pattern).
//!
//! Corpus entries with smaller `distance_to_target` get higher energy
//! as the temperature decreases over time.

/// Compute directed energy for a corpus entry.
///
/// Uses simulated annealing: at high temperature (early), energy is
/// uniform (exploration). At low temperature (late), energy is inversely
/// proportional to distance (exploitation).
pub fn directed_energy(distance: f64, temperature: f64) -> f64 {
    if distance <= 0.0 {
        return 1.0; // at target
    }
    if temperature <= 0.0 {
        // Cold: pure exploitation
        return 1.0 / distance;
    }
    // Annealing: blend exploration and exploitation
    let exploitation = 1.0 / distance;
    let exploration = 1.0;
    let alpha = (1.0 - temperature).max(0.0).min(1.0);
    exploration * (1.0 - alpha) + exploitation * alpha
}

/// Compute temperature based on elapsed iterations and cooling schedule.
///
/// Linear cooling from 1.0 to 0.0 over `total_iterations`.
pub fn temperature(current_iteration: u64, total_iterations: u64) -> f64 {
    if total_iterations == 0 {
        return 0.0;
    }
    1.0 - (current_iteration as f64 / total_iterations as f64).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_at_target_is_one() {
        assert_eq!(directed_energy(0.0, 0.5), 1.0);
    }

    #[test]
    fn energy_far_from_target_low_temp() {
        let e = directed_energy(10.0, 0.0);
        assert!((e - 0.1).abs() < 0.01); // 1/10
    }

    #[test]
    fn energy_high_temp_is_uniform() {
        let e1 = directed_energy(1.0, 1.0);
        let e2 = directed_energy(100.0, 1.0);
        // At max temp, both should be close to 1.0 (exploration)
        assert!((e1 - 1.0).abs() < 0.01);
        assert!((e2 - 1.0).abs() < 0.01);
    }

    #[test]
    fn temperature_starts_at_one() {
        assert_eq!(temperature(0, 100), 1.0);
    }

    #[test]
    fn temperature_ends_at_zero() {
        assert_eq!(temperature(100, 100), 0.0);
    }

    #[test]
    fn temperature_midpoint() {
        let t = temperature(50, 100);
        assert!((t - 0.5).abs() < 0.01);
    }

    #[test]
    fn temperature_zero_total() {
        assert_eq!(temperature(0, 0), 0.0);
    }

    #[test]
    fn temperature_past_total_clamped() {
        assert_eq!(temperature(200, 100), 0.0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-fuzz directed`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Implement and wire**

The code is already in the test file above. Add the module export to `crates/apex-fuzz/src/lib.rs`:

```rust
pub mod directed;
```

Add `distance_to_target: Option<f64>` to `CorpusEntry` in `crates/apex-fuzz/src/corpus.rs`. The `energy`, `fuzz_count`, and `covered_edges` fields were added by Phase 1 Task B.3 (PowerSchedule). If they exist, add after them; if Phase 1 B.3 is not yet complete, add after `coverage_gain`:

```rust
// In CorpusEntry, add this field:
pub distance_to_target: Option<f64>,
```

Update `Corpus::add()` (line ~32 of corpus.rs) where `CorpusEntry` is constructed inline — add `distance_to_target: None` to the struct literal.

- [ ] **Step 4: Run all fuzz tests**

Run: `cargo test -p apex-fuzz`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-fuzz/src/directed.rs crates/apex-fuzz/src/corpus.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(fuzz): add directed sampling with simulated annealing (AFLGo)"
```

---

### Task 3: GALS ensemble synchronization

**Files:**
- Create: `crates/apex-agent/src/ensemble.rs`
- Modify: `crates/apex-agent/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Create `crates/apex-agent/src/ensemble.rs`:

```rust
//! GALS ensemble synchronization.
//!
//! Each strategy maintains its own corpus. Every N iterations, interesting
//! seeds are broadcast to all strategies via a shared sync buffer.

use apex_core::types::InputSeed;
use std::sync::Mutex;

/// Collects interesting seeds from all strategies and redistributes them.
pub struct EnsembleSync {
    /// Seeds pending broadcast.
    buffer: Mutex<Vec<InputSeed>>,
    /// Sync interval in iterations.
    interval: u64,
    /// Counter for last sync iteration.
    last_sync: Mutex<u64>,
}

impl EnsembleSync {
    pub fn new(interval: u64) -> Self {
        EnsembleSync {
            buffer: Mutex::new(Vec::new()),
            interval,
            last_sync: Mutex::new(0),
        }
    }

    /// Deposit an interesting seed for broadcast.
    pub fn deposit(&self, seed: InputSeed) {
        self.buffer.lock().unwrap().push(seed);
    }

    /// Check if sync is due at this iteration.
    pub fn should_sync(&self, iteration: u64) -> bool {
        if self.interval == 0 {
            return false;
        }
        let last = *self.last_sync.lock().unwrap();
        iteration >= last + self.interval
    }

    /// Take all buffered seeds and reset the sync timer.
    pub fn sync(&self, iteration: u64) -> Vec<InputSeed> {
        *self.last_sync.lock().unwrap() = iteration;
        std::mem::take(&mut *self.buffer.lock().unwrap())
    }

    pub fn pending_count(&self) -> usize {
        self.buffer.lock().unwrap().len()
    }
}

impl Default for EnsembleSync {
    fn default() -> Self {
        Self::new(20) // default: sync every 20 iterations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::SeedOrigin;

    fn make_seed() -> InputSeed {
        InputSeed::new(vec![1, 2, 3], SeedOrigin::Fuzzer)
    }

    #[test]
    fn new_is_empty() {
        let sync = EnsembleSync::new(20);
        assert_eq!(sync.pending_count(), 0);
    }

    #[test]
    fn deposit_increments_count() {
        let sync = EnsembleSync::new(20);
        sync.deposit(make_seed());
        sync.deposit(make_seed());
        assert_eq!(sync.pending_count(), 2);
    }

    #[test]
    fn should_sync_at_interval() {
        let sync = EnsembleSync::new(5);
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
        assert!(sync.should_sync(5));
        sync.sync(5);
        assert!(!sync.should_sync(6));
        assert!(sync.should_sync(10));
    }

    #[test]
    fn zero_interval_never_syncs() {
        let sync = EnsembleSync::new(0);
        assert!(!sync.should_sync(0));
        assert!(!sync.should_sync(100));
    }

    #[test]
    fn default_interval_is_20() {
        let sync = EnsembleSync::default();
        assert!(!sync.should_sync(19));
        assert!(sync.should_sync(20));
    }
}
```

- [ ] **Step 2: Register module**

Add to `crates/apex-agent/src/lib.rs`:
```rust
pub mod ensemble;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-agent ensemble`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-agent/src/ensemble.rs crates/apex-agent/src/lib.rs
git commit -m "feat(agent): add GALS ensemble synchronization"
```

---

## Chunk 2: Python Proxy Symbolic Engine (Stream E)

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/apex-concolic/python/apex_symbolic/__init__.py` | Package init |
| Create | `crates/apex-concolic/python/apex_symbolic/proxy.py` | Proxy types (SymbolicInt, SymbolicStr, etc.) |
| Create | `crates/apex-concolic/python/apex_symbolic/engine.py` | Re-execution engine with constraint accumulation |
| Create | `crates/apex-concolic/python/apex_symbolic/inference.py` | Type inference for proxy construction |
| Create | `crates/apex-concolic/python/tests/test_proxy.py` | Unit tests for proxy objects |
| Create | `crates/apex-concolic/python/tests/test_engine.py` | Tests for execution engine |

This is a Python package that builds Z3 AST through dunder methods on proxy objects (CrossHair pattern). The PyO3 bridge to Rust (E.2) is feature-gated and connects via `crates/apex-concolic/src/pyo3_bridge.rs`.

---

### Task 4: Proxy object library — SymbolicInt

**Files:**
- Create: `crates/apex-concolic/python/apex_symbolic/__init__.py`
- Create: `crates/apex-concolic/python/apex_symbolic/proxy.py`
- Create: `crates/apex-concolic/python/tests/test_proxy.py`

- [ ] **Step 1: Write the failing test**

Create `crates/apex-concolic/python/tests/test_proxy.py`:

```python
"""Tests for symbolic proxy objects."""
import sys
import os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from apex_symbolic.proxy import SymbolicInt, ConstraintCollector

def test_symbolic_int_creation():
    collector = ConstraintCollector()
    x = SymbolicInt("x", collector)
    assert x.name == "x"

def test_symbolic_int_gt():
    collector = ConstraintCollector()
    x = SymbolicInt("x", collector)
    result = x > 0
    assert len(collector.constraints) == 1
    assert "(> x 0)" in collector.constraints[0]

def test_symbolic_int_lt():
    collector = ConstraintCollector()
    x = SymbolicInt("x", collector)
    result = x < 5
    assert "(< x 5)" in collector.constraints[0]

def test_symbolic_int_eq():
    collector = ConstraintCollector()
    x = SymbolicInt("x", collector)
    result = x == 3
    assert "(= x 3)" in collector.constraints[0]

def test_symbolic_int_ne():
    collector = ConstraintCollector()
    x = SymbolicInt("x", collector)
    result = x != 0
    assert "(not (= x 0))" in collector.constraints[0]

def test_symbolic_int_add():
    collector = ConstraintCollector()
    x = SymbolicInt("x", collector)
    y = x + 1
    assert isinstance(y, SymbolicInt)
    assert y.expr == "(+ x 1)"

def test_symbolic_int_sub():
    collector = ConstraintCollector()
    x = SymbolicInt("x", collector)
    y = x - 1
    assert y.expr == "(- x 1)"

def test_symbolic_int_mul():
    collector = ConstraintCollector()
    x = SymbolicInt("x", collector)
    y = x * 2
    assert y.expr == "(* x 2)"

def test_symbolic_int_bool_queries_solver():
    """When __bool__ is called at a branch, it queries the solver."""
    collector = ConstraintCollector()
    x = SymbolicInt("x", collector)
    cmp = x > 0
    # __bool__ on a SymbolicBool should record a branch constraint
    # and return a concrete value (True or False)
    val = bool(cmp)
    assert isinstance(val, bool)

def test_constraint_collector_clear():
    collector = ConstraintCollector()
    x = SymbolicInt("x", collector)
    _ = x > 0
    assert len(collector.constraints) > 0
    collector.clear()
    assert len(collector.constraints) == 0
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/ad/prj/bcov && python3 -m pytest crates/apex-concolic/python/tests/test_proxy.py -v`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Implement proxy objects**

Create `crates/apex-concolic/python/apex_symbolic/__init__.py`:
```python
"""CrossHair-style symbolic proxy objects for Python concolic execution."""
from .proxy import SymbolicInt, SymbolicBool, ConstraintCollector
```

Create `crates/apex-concolic/python/apex_symbolic/proxy.py`:
```python
"""Symbolic proxy types that build SMTLIB2 AST through dunder methods."""

class ConstraintCollector:
    """Accumulates SMTLIB2 constraints from symbolic comparisons."""

    def __init__(self):
        self.constraints = []
        self.branch_directions = []

    def add_constraint(self, smtlib2: str, direction: bool):
        self.constraints.append(smtlib2)
        self.branch_directions.append(direction)

    def clear(self):
        self.constraints.clear()
        self.branch_directions.clear()


class SymbolicBool:
    """Result of a symbolic comparison. Records constraint on __bool__."""

    def __init__(self, expr: str, collector: ConstraintCollector):
        self.expr = expr
        self._collector = collector

    def __bool__(self):
        # At a branch point: record the constraint and return True.
        # The solver will later negate to explore the other path.
        self._collector.add_constraint(self.expr, True)
        return True

    def __and__(self, other):
        if isinstance(other, SymbolicBool):
            return SymbolicBool(f"(and {self.expr} {other.expr})", self._collector)
        return NotImplemented

    def __or__(self, other):
        if isinstance(other, SymbolicBool):
            return SymbolicBool(f"(or {self.expr} {other.expr})", self._collector)
        return NotImplemented

    def __invert__(self):
        return SymbolicBool(f"(not {self.expr})", self._collector)


class SymbolicInt:
    """Symbolic integer that builds SMTLIB2 AST through operations."""

    def __init__(self, name: str, collector: ConstraintCollector, expr: str = None):
        self.name = name
        self.expr = expr or name
        self._collector = collector

    def _binop(self, op: str, other) -> 'SymbolicInt':
        if isinstance(other, SymbolicInt):
            return SymbolicInt(self.name, self._collector, f"({op} {self.expr} {other.expr})")
        elif isinstance(other, (int, float)):
            return SymbolicInt(self.name, self._collector, f"({op} {self.expr} {other})")
        return NotImplemented

    def _cmpop(self, op: str, other) -> SymbolicBool:
        if isinstance(other, SymbolicInt):
            return SymbolicBool(f"({op} {self.expr} {other.expr})", self._collector)
        elif isinstance(other, (int, float)):
            return SymbolicBool(f"({op} {self.expr} {other})", self._collector)
        return NotImplemented

    def __add__(self, other): return self._binop("+", other)
    def __sub__(self, other): return self._binop("-", other)
    def __mul__(self, other): return self._binop("*", other)
    def __floordiv__(self, other): return self._binop("div", other)
    def __mod__(self, other): return self._binop("mod", other)
    def __neg__(self): return SymbolicInt(self.name, self._collector, f"(- {self.expr})")

    def __radd__(self, other): return self._binop("+", other)
    def __rsub__(self, other):
        if isinstance(other, (int, float)):
            return SymbolicInt(self.name, self._collector, f"(- {other} {self.expr})")
        return NotImplemented
    def __rmul__(self, other): return self._binop("*", other)

    def __gt__(self, other): return self._cmpop(">", other)
    def __ge__(self, other): return self._cmpop(">=", other)
    def __lt__(self, other): return self._cmpop("<", other)
    def __le__(self, other): return self._cmpop("<=", other)
    def __eq__(self, other): return self._cmpop("=", other)
    def __ne__(self, other):
        if isinstance(other, SymbolicInt):
            return SymbolicBool(f"(not (= {self.expr} {other.expr}))", self._collector)
        elif isinstance(other, (int, float)):
            return SymbolicBool(f"(not (= {self.expr} {other}))", self._collector)
        return NotImplemented

    def __int__(self):
        """Concretize: return 0 as default concrete value."""
        return 0

    def __repr__(self):
        return f"SymbolicInt({self.expr})"
```

- [ ] **Step 4: Run tests**

Run: `cd /Users/ad/prj/bcov && python3 -m pytest crates/apex-concolic/python/tests/test_proxy.py -v`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-concolic/python/
git commit -m "feat(concolic): add CrossHair-style Python proxy objects for symbolic execution"
```

---

### Task 5: Type inference for proxy construction

**Files:**
- Create: `crates/apex-concolic/python/apex_symbolic/inference.py`
- Create: `crates/apex-concolic/python/tests/test_inference.py`

- [ ] **Step 1: Write failing tests**

Create `crates/apex-concolic/python/tests/test_inference.py`:

```python
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from apex_symbolic.inference import infer_proxy_args
from apex_symbolic.proxy import ConstraintCollector, SymbolicInt

def sample_func(x: int, y: int) -> bool:
    return x > y

def untyped_func(a, b):
    return a + b

def test_infer_typed_function():
    collector = ConstraintCollector()
    args = infer_proxy_args(sample_func, collector)
    assert "x" in args
    assert "y" in args
    assert isinstance(args["x"], SymbolicInt)
    assert isinstance(args["y"], SymbolicInt)

def test_infer_untyped_defaults_to_int():
    collector = ConstraintCollector()
    args = infer_proxy_args(untyped_func, collector)
    assert "a" in args
    # Default proxy for unknown types is SymbolicInt
    assert isinstance(args["a"], SymbolicInt)

def test_infer_skips_self():
    class Foo:
        def method(self, x: int):
            pass
    collector = ConstraintCollector()
    args = infer_proxy_args(Foo.method, collector)
    assert "self" not in args
    assert "x" in args
```

- [ ] **Step 2: Implement inference**

Create `crates/apex-concolic/python/apex_symbolic/inference.py`:

```python
"""Type inference for symbolic proxy construction."""
import inspect
from .proxy import SymbolicInt, ConstraintCollector


def infer_proxy_args(func, collector: ConstraintCollector) -> dict:
    """Inspect function signature and create symbolic proxies for each parameter.

    Strategy:
    1. inspect.signature() + type hints → proxy types
    2. Fallback: default to SymbolicInt for unknown types
    """
    sig = inspect.signature(func)
    args = {}

    for name, param in sig.parameters.items():
        if name == "self" or name == "cls":
            continue

        annotation = param.annotation
        if annotation is int or annotation is inspect.Parameter.empty:
            args[name] = SymbolicInt(name, collector)
        elif annotation is float:
            args[name] = SymbolicInt(name, collector)  # approximate float as int for now
        elif annotation is bool:
            args[name] = SymbolicInt(name, collector)  # bool is subclass of int
        else:
            # Default: SymbolicInt
            args[name] = SymbolicInt(name, collector)

    return args
```

- [ ] **Step 3: Run tests**

Run: `cd /Users/ad/prj/bcov && python3 -m pytest crates/apex-concolic/python/tests/test_inference.py -v`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-concolic/python/apex_symbolic/inference.py crates/apex-concolic/python/tests/test_inference.py
git commit -m "feat(concolic): add type inference for Python symbolic proxy construction"
```

---

### Task 6: Execution engine with constraint accumulation

**Files:**
- Create: `crates/apex-concolic/python/apex_symbolic/engine.py`
- Create: `crates/apex-concolic/python/tests/test_engine.py`

- [ ] **Step 1: Write failing tests**

Create `crates/apex-concolic/python/tests/test_engine.py`:

```python
import sys, os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..'))

from apex_symbolic.engine import SymbolicExecutor
from apex_symbolic.proxy import ConstraintCollector

def simple_branch(x: int) -> str:
    if x > 0:
        return "positive"
    else:
        return "non-positive"

def test_executor_runs_function():
    executor = SymbolicExecutor()
    results = executor.explore(simple_branch)
    # Should produce at least one constraint
    assert len(results.constraints) >= 1

def test_executor_returns_path_constraints():
    executor = SymbolicExecutor()
    results = executor.explore(simple_branch)
    # Constraints should be SMTLIB2 strings
    for c in results.constraints:
        assert isinstance(c, str)
        assert "x" in c  # variable name should appear

def test_executor_result_has_concrete_output():
    executor = SymbolicExecutor()
    results = executor.explore(simple_branch)
    assert results.return_value is not None
```

- [ ] **Step 2: Implement engine**

Create `crates/apex-concolic/python/apex_symbolic/engine.py`:

```python
"""Symbolic execution engine using proxy object re-execution."""
from dataclasses import dataclass, field
from typing import Any, Callable, List
from .proxy import ConstraintCollector
from .inference import infer_proxy_args


@dataclass
class ExecutionResult:
    """Result of one symbolic execution."""
    constraints: List[str] = field(default_factory=list)
    branch_directions: List[bool] = field(default_factory=list)
    return_value: Any = None
    exception: Exception = None


class SymbolicExecutor:
    """Re-executes a function with symbolic proxy arguments."""

    def explore(self, func: Callable) -> ExecutionResult:
        """Run the function once with symbolic arguments, collecting path constraints."""
        collector = ConstraintCollector()
        proxy_args = infer_proxy_args(func, collector)

        result = ExecutionResult()
        try:
            ret = func(**proxy_args)
            result.return_value = ret
        except Exception as e:
            result.exception = e

        result.constraints = list(collector.constraints)
        result.branch_directions = list(collector.branch_directions)
        return result
```

- [ ] **Step 3: Run tests**

Run: `cd /Users/ad/prj/bcov && python3 -m pytest crates/apex-concolic/python/tests/test_engine.py -v`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-concolic/python/apex_symbolic/engine.py crates/apex-concolic/python/tests/test_engine.py
git commit -m "feat(concolic): add symbolic execution engine with constraint accumulation"
```

---

## Chunk 3: Kani BMC Unreachability Proofs (Stream F.1)

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/apex-symbolic/src/bmc.rs` | `KaniProver` struct, harness generation, result parsing |
| Modify | `crates/apex-symbolic/Cargo.toml` | `kani-prover` feature gate |
| Modify | `crates/apex-symbolic/src/lib.rs` | Conditional re-export |

---

### Task 7: KaniProver stub with reachability check

**Files:**
- Create: `crates/apex-symbolic/src/bmc.rs`

- [ ] **Step 1: Write failing tests**

Create `crates/apex-symbolic/src/bmc.rs`:

```rust
//! Kani BMC unreachability proofs.
//!
//! After exploration exhausts fuzzing and concolic strategies, batch
//! remaining uncovered branches through Kani bounded model checking.
//! Feature-gated: `kani-prover`.

use apex_core::types::BranchId;
use std::path::PathBuf;

/// Result of a Kani reachability check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachabilityResult {
    /// Branch is reachable — Kani found a counterexample input.
    Reachable(String),
    /// Branch is provably unreachable within the BMC bound.
    Unreachable,
    /// Kani could not determine (timeout, unsupported feature, etc.).
    Unknown(String),
}

/// Generates Kani harnesses and checks branch reachability.
pub struct KaniProver {
    target_root: PathBuf,
}

impl KaniProver {
    pub fn new(target_root: PathBuf) -> Self {
        KaniProver { target_root }
    }

    /// Generate a Kani harness that asserts reachability of the given branch.
    pub fn generate_harness(&self, branch: &BranchId, function_name: &str) -> String {
        format!(
            r#"#[cfg(kani)]
#[kani::proof]
fn check_reachability_{file_id}_{line}_{dir}() {{
    // Kani will explore all possible inputs
    let result = {func}(kani::any());
    // If this assertion is reachable, the branch is reachable
    kani::cover!(true, "branch {file_id}:{line}:{dir} reachable");
}}"#,
            file_id = branch.file_id,
            line = branch.line,
            dir = branch.direction,
            func = function_name,
        )
    }

    /// Check reachability of a branch.
    ///
    /// Without the `kani-prover` feature, always returns `Unknown`.
    pub fn check_reachability(
        &self,
        _branch: &BranchId,
        _function_name: &str,
    ) -> ReachabilityResult {
        #[cfg(feature = "kani-prover")]
        {
            // TODO: Run `cargo kani` and parse output
            ReachabilityResult::Unknown("kani execution not yet implemented".into())
        }

        #[cfg(not(feature = "kani-prover"))]
        {
            ReachabilityResult::Unknown("kani-prover feature not enabled".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_generation() {
        let prover = KaniProver::new(PathBuf::from("/tmp/project"));
        let branch = BranchId::new(42, 10, 0, 0);
        let harness = prover.generate_harness(&branch, "check_value");
        assert!(harness.contains("#[kani::proof]"));
        assert!(harness.contains("check_reachability_42_10_0"));
        assert!(harness.contains("check_value"));
    }

    #[test]
    fn check_without_feature_returns_unknown() {
        let prover = KaniProver::new(PathBuf::from("/tmp"));
        let branch = BranchId::new(1, 5, 0, 0);
        let result = prover.check_reachability(&branch, "foo");
        assert!(matches!(result, ReachabilityResult::Unknown(_)));
    }

    #[test]
    fn reachability_result_variants() {
        let r = ReachabilityResult::Reachable("input: x=5".into());
        assert!(matches!(r, ReachabilityResult::Reachable(_)));

        let u = ReachabilityResult::Unreachable;
        assert_eq!(u, ReachabilityResult::Unreachable);

        let k = ReachabilityResult::Unknown("timeout".into());
        assert!(matches!(k, ReachabilityResult::Unknown(_)));
    }
}
```

- [ ] **Step 2: Register module and feature**

Add to `crates/apex-symbolic/Cargo.toml` under `[features]`:
```toml
kani-prover = []
```

Add to `crates/apex-symbolic/src/lib.rs`:
```rust
pub mod bmc;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-symbolic bmc`
Expected: PASS

- [ ] **Step 4: Run all symbolic tests**

Run: `cargo test -p apex-symbolic`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-symbolic/src/bmc.rs crates/apex-symbolic/Cargo.toml crates/apex-symbolic/src/lib.rs
git commit -m "feat(symbolic): add KaniProver stub for BMC unreachability proofs"
```

---

### Task 8: Integration verification

- [ ] **Step 1: Run full workspace tests**

```bash
cargo test --workspace
```

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 3: Run Python tests**

```bash
cd /Users/ad/prj/bcov && python3 -m pytest crates/apex-concolic/python/tests/ -v
```

- [ ] **Step 4: Commit any fixes**

```bash
git add -u crates/
git commit -m "fix: address Phase 3 integration issues"
```
