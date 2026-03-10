# Phase 2 Gap Closure: RedQueen/CmpLog + Driller Strategy

## B.5 — RedQueen/CmpLog

### Goal

Add input-to-state comparison feedback to the fuzzer so it can discover "magic bytes" (specific values required by comparisons) without brute-forcing them.

### Architecture

Two-layer CMP feedback system:

1. **Instrumentation layer** (`sancov_rt.rs`): Extend with `__sanitizer_cov_trace_cmp{1,2,4,8}` callbacks that write `(pc, arg1, arg2, size)` tuples to a static ring buffer. Works for C/Rust targets compiled with `-fsanitize-coverage=trace-cmp`.

2. **Output parsing fallback** (`cmplog.rs`): `parse_cmp_hints_from_output()` extracts comparison values from test stderr/stdout — assertion messages like `expected X, got Y`, error strings with numeric/string literals. Works for all languages without instrumentation.

Both layers feed into a `CmpLog` struct that stores deduplicated `CmpEntry { pc: u64, arg1: Vec<u8>, arg2: Vec<u8>, size: u8 }` tuples.

### CmpLog Mutator

`CmpLogMutator: Mutator` implements input-to-state replacement:
- Scan input bytes for subsequences matching `arg1`
- Replace with `arg2` (and vice versa)
- Try all logged comparisons, prioritizing recent/frequent ones

Registered in `MutatorRegistry`, scheduled by `MOptScheduler` like any other mutator. The scheduler's EMA tracking naturally up-weights CmpLog when it produces coverage hits.

### Files

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/apex-fuzz/src/cmplog.rs` | `CmpEntry`, `CmpLog`, `CmpLogMutator`, `parse_cmp_hints_from_output()` |
| Modify | `crates/apex-sandbox/src/sancov_rt.rs` | Add CMP trace callbacks + `read_cmp_log()`, `reset_cmp_log()` |
| Modify | `crates/apex-fuzz/src/lib.rs` | Add `pub mod cmplog` |

### Key Types

```rust
/// A single comparison observation.
pub struct CmpEntry {
    pub pc: u64,
    pub arg1: Vec<u8>,
    pub arg2: Vec<u8>,
    pub size: u8,
}

/// Deduplicated log of comparison observations from one execution.
pub struct CmpLog {
    entries: Vec<CmpEntry>,
}

/// Mutator that replaces input bytes matching one CMP operand with the other.
pub struct CmpLogMutator {
    log: CmpLog,
}
```

---

## C.1-C.2 — Stall Detection + Driller Strategy

### Goal

When the fuzzer hits a coverage plateau, automatically switch to symbolic execution to generate inputs that pass hard-to-fuzz branch conditions.

### Architecture

**Stall detection (C.1)** already exists: `CoverageMonitor` in `apex-agent/src/monitor.rs` tracks coverage growth and returns `MonitorAction::SwitchStrategy` when stalled. No new code needed for detection.

**Driller strategy (C.2)**: `DrillerStrategy: Strategy` registered alongside `FuzzStrategy` in the orchestrator's strategy list. The orchestrator's existing rotation mechanism switches to Driller when the monitor signals `SwitchStrategy`.

When `suggest_inputs()` is called:
1. Pick the highest-energy corpus entry from the stalled fuzzer
2. Collect its path constraints from `ExecutionTrace`
3. Filter via `filter_tainted_branches()` to reduce solver calls (60-80% reduction)
4. Use `SymbolicSession::diverging_inputs()` to negate frontier branches
5. Return solver-generated seeds with `SeedOrigin::Symbolic`

The orchestrator rotates back to `FuzzStrategy` once Driller seeds produce new coverage (the monitor exits stall state).

### Files

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/apex-agent/src/driller.rs` | `DrillerStrategy` implementing `Strategy` trait |
| Modify | `crates/apex-agent/src/lib.rs` | Add `pub mod driller` |

### Key Types

```rust
/// Driller-style symbolic execution strategy.
/// Activated by the orchestrator when the fuzzer stalls.
pub struct DrillerStrategy {
    solver: Arc<dyn SolverTrait>,
    oracle: Arc<CoverageOracle>,
    max_constraints: usize,
}
```

### Integration with Orchestrator

The orchestrator already supports multiple strategies and rotates on stall. `DrillerStrategy` is registered as a strategy — no orchestrator changes needed. The rotation flow:

```
FuzzStrategy (normal) → stall detected → SwitchStrategy → DrillerStrategy
DrillerStrategy → new coverage found → monitor resets → FuzzStrategy
```

---

## Dependencies

- B.5 depends on: `apex-fuzz` traits (Mutator), `apex-sandbox` sancov_rt
- C.1-C.2 depends on: `apex-agent` orchestrator, `apex-symbolic` solver, `apex-concolic` taint, `apex-coverage` oracle
- B.5 and C.1-C.2 are independent of each other
