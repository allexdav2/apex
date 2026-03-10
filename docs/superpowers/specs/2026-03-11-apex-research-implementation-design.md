# APEX Research Implementation — Design Spec

**Date**: 2026-03-11
**Source**: `docs/sancov-self-instrumentation-research.md` (654-line research document)
**Scope**: All 24 roadmap items across 6 dependency-driven work streams
**Priority**: Architectural foundations first (traits/abstractions unblock 60%+ of items)

---

## Work Stream Decomposition

Six streams organized by crate ownership:

| Stream | Name | Crates | Items | Foundation |
|--------|------|--------|-------|-----------|
| A | Solver & Symbolic | apex-symbolic, apex-concolic | 1,3,4,5,13,20 | `Solver` trait |
| B | Fuzzer Engine | apex-fuzz | 6,7,9,15,19,21 | `Mutator` trait + energy model |
| C | Orchestrator | apex-agent, apex-cli | 8,10,11,22 | Driller feedback loop |
| D | Instrumentation & Coverage | apex-instrument, apex-coverage, apex-sandbox | 2,14,17 | SanCov + MC/DC |
| E | Python Symbolic | apex-concolic (rewrite) | 16 | Proxy-object engine |
| F | Verification & Synthesis | apex-symbolic, apex-synth | 18,23,24 | Kani/Bolero integration |

**Dependency graph:**
```
A (Solver trait) ──┬──→ C (Orchestrator) ──→ C.driller (needs A + B)
                   ├──→ E (Python Symbolic, needs A via PyO3)
                   └──→ F (Verification, needs A + D)
B (Fuzzer Engine) ─┘
D (Coverage Model) ────→ F
```

Streams A, B, D are independent and start in parallel.

---

## Stream A — Solver & Symbolic Foundation

**Goal**: Extract `Solver` trait, optimize Z3, enable multi-solver portfolio, implement generational search.

### A.1 — Solver trait extraction

New file: `crates/apex-symbolic/src/traits.rs`

```rust
#[async_trait]
pub trait Solver: Send + Sync {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Option<InputSeed>;
    fn solve_batch(&self, constraint_sets: Vec<Vec<String>>) -> Vec<Option<InputSeed>>;
    fn set_logic(&mut self, logic: SolverLogic);
}

pub enum SolverLogic { QF_LIA, QF_ABV, QF_S, Auto }
```

Existing Z3 code becomes `Z3Solver` implementing this trait.

### A.2 — Incremental Z3 (push/pop)

Persistent `Context` + `Solver` across calls. Push/pop instead of recreating. Expected: 3-10x speedup on sequential prefix-sharing constraint sets.

### A.3 — Set logic explicitly

Factory selects by `Language`: Python→QF_LIA, C/Rust→QF_ABV, JS→QF_S.

### A.4 — Z3 tactics

Apply `(then simplify propagate-values solve-eqs)` before `check()`. Configurable chain.

### A.5 — Solver cache layer

`CachingSolver<S: Solver>` wrapper. Hash constraint set → cache. Counterexample reuse: if cached SAT model satisfies new superset, return without solving (KLEE pattern).

### A.6 — Generational search (SAGE)

Replace linear prefix negation in `diverging_inputs()`. Given path [C0..C3], generate ALL negations in one `solve_batch()` pass. Up to N seeds per symbolic run.

### A.7 — Multi-solver portfolio

Feature-gated: `BitwuzlaSolver` (`bitwuzla-solver`), `Cvc5Solver` (`cvc5-solver`). `PortfolioSolver` runs multiple with timeout, returns first SAT.

---

## Stream B — Fuzzer Engine

**Goal**: Extract `Mutator` trait, adaptive scheduling, energy sampling, comparison feedback, directed sampling, grammar mutations.

### B.1 — Mutator trait extraction

```rust
pub trait Mutator: Send + Sync {
    fn mutate(&self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8>;
    fn name(&self) -> &str;
}
```

Existing 7 operators become individual structs. `HavocMutator` chains N random selections.

### B.2 — MOpt mutation scheduling

`MOptScheduler` with per-mutator stats: `(applications, coverage_hits, ema_yield)`. Selection probability proportional to `max(ema_yield, floor)`. ~50 lines.

### B.3 — Energy-based power schedule

`energy: f64` on corpus entries. Three schedules: `Explore` (uniform), `Fast` (inverse of fuzz count × path length), `Rare` (proportional to edge rarity). ~30 lines.

### B.4 — Corpus minimization

`Corpus::minimize()`. Greedy set-cover: select entry covering most uncovered edges, repeat. O(n×m). ~60 lines.

### B.5 — RedQueen / CmpLog

New module: `crates/apex-fuzz/src/cmplog.rs`. Captures comparison operands per-language (Python: AST hook `__eq__`/`__ne__`; Rust/C: `-sanitizer-coverage-trace-compares`). `RedQueenMutator` substitutes expected values at matching positions.

### B.6 — Directed sampling (AFLGo)

`distance_to_target: Option<f64>` on corpus entries. CFG distances computed at instrumentation. Simulated annealing: temperature decreases, shifting from exploration to exploitation.

### B.7 — Grammar-based mutations

New module: `crates/apex-fuzz/src/grammar.rs`. `GrammarMutator` takes CFG definition, parses inputs to ASTs, mutates at grammar level. Optional Grimoire-style rule inference.

### B.8 — Custom mutator plugin API

`Corpus::register_mutator(Box<dyn Mutator>)`. Also support `dlopen` loading (AFL++ `AFL_CUSTOM_MUTATOR_LIBRARY` compatible).

---

## Stream C — Orchestrator & Strategy Coordination

**Goal**: Smart stall detection, Driller feedback loop, function summaries, GALS ensemble sync.

### C.1 — Stall detection with strategy switching

```rust
struct CoverageMonitor {
    window: VecDeque<(u64, usize)>,
    window_size: usize,
}
```

Growth rate monitor. Actions escalate: normal → switch strategy → agent cycle → stop. Replaces binary stall counter. ~40 lines.

### C.2 — Driller-style feedback loop

On stall: extract frontier seeds (highest branch count but blocked) → feed to concolic engine → concolic solves hard branches → results merged back to fuzz corpus.

```rust
struct SeedExchange {
    fuzz_to_concolic: Vec<InputSeed>,
    concolic_to_fuzz: Vec<InputSeed>,
}
```

Depends on Stream A (Solver) and Stream B (corpus API).

### C.3 — Function summaries for Python stdlib

`crates/apex-symbolic/src/summaries.rs`. Initial set: `len()`, `range()`, `str.split()`, `dict.get()`, `list.append()`, `int()`, `str()`, `max()`, `min()`. Substitute summary constraints instead of tracing into function body.

### C.4 — GALS ensemble synchronization

Each strategy has own corpus. Every N iterations (default 20), broadcast interesting seeds to all strategies. `InputSeed::origin` tracks provenance for attribution.

---

## Stream D — Instrumentation & Coverage Model

**Goal**: Taint-guided filtering, SanCov pure Rust callbacks, MC/DC coverage.

### D.1 — Taint-guided branch filtering

Modify `apex_tracer.py` to propagate taint from function parameters. On Rust side: `TaintedBranch { branch_id, tainted_vars, condition }`. Concolic engine filters: only solve branches with tainted conditions. Expected: 60-80% fewer solver calls.

### D.2 — SanCov self-instrumentation

**D.2a — Pure Rust SanCov callbacks** (`crates/apex-sandbox/src/sancov_rt.rs`). `#[no_mangle] extern "C"` functions replacing C shim. No FFI, no LD_PRELOAD.

**D.2b — RUSTC_WRAPPER** (`crates/apex-instrument/src/rustc_wrapper.rs`). Injects `-C passes=sancov-module` + related flags. Stable Rust compatible.

**D.2c — Inline-8bit-counters mode**. Alternative to trace-pc-guard: one `inc` per edge, 2-5x faster. BSS section read via shared memory.

### D.3 — MC/DC coverage tier

Extend `BranchId` with `condition_index: Option<u8>`. `CoverageOracle` gains `CoverageLevel` enum (Statement/Branch/Mcdc) and `mcdc_independence_pairs()`. Solver generates independence pairs for compound conditions.

---

## Stream E — Python Symbolic Revolution

**Goal**: CrossHair-style proxy objects building Z3 AST through dunder methods. PyO3 bridge to Rust solver (Option B).

### E.1 — Proxy object library

Python package: `crates/apex-concolic/python/apex_symbolic/`. Proxy types: `SymbolicInt`, `SymbolicFloat`, `SymbolicStr`, `SymbolicBool`, `SymbolicList`, `SymbolicDict`, `SymbolicOptional`. Each dunder method builds Z3 AST.

### E.2 — PyO3 bridge to Rust Z3

Reuses Stream A's `Solver` trait. Python proxy objects call into Rust solver via PyO3 FFI. Feature-gated: `pyo3` + `z3-solver`.

### E.3 — Execution engine

Re-execution with constraint accumulation. `SymbolicBool.__bool__()` queries solver at branches. Combined with generational search (A.6) for maximum yield per execution.

### E.4 — Type inference for proxy construction

`inspect.signature()` + type hints → proxy types. Fallback: docstrings → runtime tracing → default `SymbolicInt`.

### E.5 — Concretization boundary

S2E pattern: when symbolic value escapes analysis scope, concretize and add assignment constraint. Handles C extensions, I/O, uninstrumented code.

---

## Stream F — Verification & Synthesis

**Goal**: Kani BMC proofs, Bolero harness emission, MIR symbolic foundation.

### F.1 — Kani BMC unreachability proofs

`crates/apex-symbolic/src/bmc.rs`. `KaniProver::check_reachability()` generates harness, runs `cargo kani`, returns `Reachable(counterexample)` | `Unreachable` | `Unknown`. Feature-gated: `kani-prover`. After final exploration round, batch all remaining uncovered branches.

### F.2 — Bolero harness emission

Extend `apex-synth` with `HarnessFormat::Bolero`. Emits `bolero::check!()` harnesses that work as unit test + fuzz target + Kani proof. CLI flag `--harness-format bolero`.

### F.3 — MIR symbolic foundation

New crate: `crates/apex-mir` (feature-gated `mir-symbolic`).

- Phase 1: MIR extraction via `rustc -Zunpretty=mir` → typed CFG (`MirFunction`, `BasicBlock`, `Terminator`)
- Phase 2: Symbolic interpretation walking CFG with symbolic values, forking at `SwitchInt`
- Phase 3: Integration with `Solver` trait

Phase 1 is the deliverable. Phases 2-3 are incremental extensions.

---

## Execution Order

### Phase 1 — Foundations (parallel, ~1-2 weeks)

| Stream | Items | Effort |
|--------|-------|--------|
| A.1-A.5 | Solver trait + Z3 optimizations | 3-4 days |
| B.1-B.4 | Mutator trait + adaptive engine | 3-4 days |
| D.1-D.2 | Taint + SanCov callbacks | 3-4 days |

### Phase 2 — Cross-cutting (~2 weeks, depends on Phase 1)

| Stream | Items | Depends on | Effort |
|--------|-------|-----------|--------|
| A.6 | Generational search | A.1 | 1-2 days |
| B.5 | RedQueen/CmpLog | B.1, D.2 | 3-4 days |
| C.1-C.2 | Stall detection, Driller loop | A.1, B.1 | 3-4 days |
| C.3 | Function summaries | A.1 | 2-3 days |
| D.3 | MC/DC | D.2 | 2-3 days |

### Phase 3 — Advanced (~2-3 weeks, depends on Phase 2)

| Stream | Items | Depends on | Effort |
|--------|-------|-----------|--------|
| A.7 | Multi-solver portfolio | A.1 | 3-5 days |
| B.6 | Directed sampling | B.3, D.2 | 2-3 days |
| C.4 | GALS ensemble | C.1, B.1 | 2-3 days |
| E.1-E.5 | Python proxy symbolic | A.1 (PyO3) | 5-7 days |
| F.1 | Kani BMC | A.1, D.3 | 3-4 days |

### Phase 4 — Research-grade (~2 weeks, depends on Phase 3)

| Stream | Items | Depends on | Effort |
|--------|-------|-----------|--------|
| B.7 | Grammar mutations | B.1 | 4-5 days |
| B.8 | Custom mutator plugins | B.1 | 1-2 days |
| F.2 | Bolero harnesses | F.1 | 2-3 days |
| F.3 | MIR symbolic foundation | A.1, E | 5-7 days |

### Parallelism

```
Week 1-2:   [A.1-A.5] | [B.1-B.4] | [D.1-D.2]
Week 2-3:   [A.6] [C.3] | [B.5] [C.1-C.2] | [D.3]
Week 3-5:   [A.7] | [B.6] [C.4] | [E.1-E.5] | [F.1]
Week 5-7:   [B.7] [B.8] | [F.2] [F.3]
```

~7 weeks with 2-3 parallel worktrees.

### Verification checkpoints

After each phase:
```bash
cargo test --workspace
cargo llvm-cov  # no regression
```

Phase-specific benchmarks:
- Phase 1: Solver 3-10x faster, mutator scheduling converges, SanCov bitmaps valid
- Phase 2: RedQueen cracks magic bytes, Driller gains coverage beyond fuzzer plateau
- Phase 3: Portfolio auto-selects solver, Python proxy handles compound conditions, Kani proves unreachable
- Phase 4: Grammar mutations penetrate parsers, MIR CFG extracts correctly

---

## Key Decisions

1. **E.2 — PyO3 bridge (Option B)**: Python proxy objects call Rust solver via PyO3, reusing Stream A's `Solver` trait and caching infrastructure
2. **Priority: foundations first**: Trait abstractions (Solver, Mutator) before features that depend on them
3. **Feature-gated heavy deps**: z3-solver, bitwuzla-solver, cvc5-solver, kani-prover, mir-symbolic, pyo3 all optional
4. **Incremental value**: Each phase ships independently testable improvements; never "all or nothing"
