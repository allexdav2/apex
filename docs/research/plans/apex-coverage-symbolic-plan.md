# Implementation Plan: 12 Research Techniques for APEX

Target crates: `apex-coverage`, `apex-index`, `apex-symbolic`, `apex-concolic`
Date: 2026-03-14

---

## Table of Contents

1. [Oracle Gap Metric](#1-oracle-gap-metric)
2. [Flaky Detection via Coverage Instability](#2-flaky-detection-via-coverage-instability)
3. [LLM-Based Flaky Test Repair](#3-llm-based-flaky-test-repair)
4. [Dead Code Detection + LLM Validation](#4-dead-code-detection--llm-validation)
5. [Rank Aggregation Test Prioritization](#5-rank-aggregation-test-prioritization)
6. [Slice-Based Change Impact Prioritization](#6-slice-based-change-impact-prioritization)
7. [Semantic Feedback Signals](#7-semantic-feedback-signals)
8. [Fitness Landscape Adaptation](#8-fitness-landscape-adaptation)
9. [Metamorphic Adequacy](#9-metamorphic-adequacy)
10. [LLM as Concolic Constraint Solver](#10-llm-as-concolic-constraint-solver)
11. [AutoBug Path Decomposition](#11-autobug-path-decomposition)
12. [Diverse SMT Solutions](#12-diverse-smt-solutions)

[Dependency Graph](#dependency-graph)
[Build Sequence](#build-sequence)

---

## 1. Oracle Gap Metric

**Paper:** Mind the Gap (arXiv:2309.02395)
**Concept:** Oracle gap = coverage% - mutation score%. High gap = code is executed but tests don't verify behavior.

### Target Crate(s)

- `apex-coverage` (primary) — new `mutation` module
- `apex-index` (secondary) — stores per-region mutation scores alongside coverage

### New Types / Traits

```rust
// apex-coverage/src/mutation.rs

/// A single mutation operator applied to source code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutationOperator {
    pub kind: MutationKind,
    pub file_id: u64,
    pub line: u32,
    pub col: u16,
    pub original: String,
    pub replacement: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MutationKind {
    NegateCondition,     // if x > 0 -> if x <= 0
    SwapOperator,        // + -> -, * -> /
    BoundaryShift,       // > -> >=, < -> <=
    ReturnDefault,       // return x -> return 0/None/""
    DeleteStatement,     // remove statement entirely
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MutantOutcome {
    Killed,   // at least one test failed
    Survived, // all tests still pass
    Timeout,  // test exceeded time limit
    CompileError, // mutation broke compilation
}

/// Per-region mutation adequacy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionMutationScore {
    pub file_id: u64,
    pub start_line: u32,
    pub end_line: u32,
    pub total_mutants: usize,
    pub killed: usize,
    pub survived: usize,
    pub timeout: usize,
    pub compile_error: usize,
    pub mutation_score: f64,    // killed / (killed + survived)
    pub oracle_gap: f64,        // coverage_pct - mutation_score
}

/// Trait for running mutation analysis on a target.
pub trait MutationRunner: Send + Sync {
    fn generate_mutants(&self, file_id: u64, lines: std::ops::Range<u32>)
        -> Vec<MutationOperator>;
    fn run_mutant(&self, mutant: &MutationOperator) -> MutantOutcome;
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-coverage/src/mutation.rs` | Types, operator set, `MutationRunner` trait |
| Create | `crates/apex-coverage/src/oracle_gap.rs` | `compute_oracle_gap()` — merges coverage data with mutation scores |
| Modify | `crates/apex-coverage/src/lib.rs` | Add `pub mod mutation; pub mod oracle_gap;` |
| Modify | `crates/apex-coverage/src/oracle.rs` | Add `oracle_gap_report()` method to `CoverageOracle` that returns `Vec<RegionMutationScore>` |
| Modify | `crates/apex-index/src/types.rs` | Add optional `mutation_scores: HashMap<String, RegionMutationScore>` field to `BranchIndex` |

### Integration Points

- `CoverageOracle::oracle_gap_report()` uses `uncovered_branches()` + coverage data to compute per-region coverage, then invokes `MutationRunner` on covered regions to get mutation scores.
- Gap reports in `apex-cli` already consume `CoverageOracle` — extend them to print oracle gap per file/region when `--mutation` flag is passed.
- `BranchIndex` gains a `mutation_scores` field for persistence across runs.

### Complexity Estimate

**Low-Medium** (~3-4 days). The types are straightforward. The main work is implementing `MutationRunner` for Python (source-level text transformations) and Rust (delegate to `cargo-mutants` or apply AST transforms). Start with Python-only (regex-based operator substitution).

### Test Strategy

- Unit: `MutationKind` operators produce correct source transformations on known inputs.
- Unit: `compute_oracle_gap()` returns 0.0 when mutation score == coverage%, positive when coverage > mutation.
- Integration: Run mutation analysis on a small Python fixture with tests that have known oracle gaps (e.g., a function covered but never asserted on).
- Property: Oracle gap is always in `[-100.0, 100.0]` and `mutation_score` is in `[0.0, 1.0]`.

---

## 2. Flaky Detection via Coverage Instability

**Paper:** FlaKat (arXiv:2403.01003)
**Concept:** Run index N times; tests whose branch sets differ across runs are flaky. Classify root cause by instability pattern.

### Target Crate(s)

- `apex-index` (primary) — already has `analysis::detect_flaky_tests()`

### New Types / Traits

```rust
// apex-index/src/analysis.rs (extend existing)

/// Root-cause classification for a flaky test.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum FlakyCause {
    /// Coverage varies randomly — likely async/timing.
    AsyncTiming,
    /// Coverage depends on test execution order.
    OrderDependent,
    /// Coverage leaks across tests — shared state.
    SharedState,
    /// Single branch flickers — conditional on external resource.
    ResourceDependent,
    /// Unknown pattern.
    Unknown,
}

/// Extended flaky report with root-cause classification.
#[derive(Debug, Clone, Serialize)]
pub struct FlakyReport {
    pub flaky_tests: Vec<FlakyTest>,       // existing type
    pub causes: HashMap<String, FlakyCause>, // test_name -> cause
    pub total_runs: usize,
    pub confidence: f64,  // 0..1 based on number of runs
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Modify | `crates/apex-index/src/analysis.rs` | Add `FlakyCause`, `FlakyReport`, `classify_flaky_cause()` function |
| Modify | `crates/apex-index/src/lib.rs` | Re-export new types |
| Create | `crates/apex-index/src/flaky_classifier.rs` | Classification heuristics: count divergent branches, check order correlation, detect shared-state patterns |

### Integration Points

- `detect_flaky_tests()` already exists and accepts `&[Vec<TestTrace>]`. The classifier takes its output and adds root-cause labels.
- CLI: `apex index --detect-flaky --runs N` runs N indexing passes, calls `detect_flaky_tests()`, then `classify_flaky_cause()`.
- Results feed into technique #3 (LLM flaky repair) as structured context.

### Complexity Estimate

**Low** (~2 days). The infrastructure already exists. Classification heuristics are pattern-matching on divergence shapes:
- Random divergence across many branches -> `AsyncTiming`
- Divergence only in later-ordered tests -> `OrderDependent`
- Same branch flickers in/out across all runs -> `ResourceDependent`

### Test Strategy

- Unit: Craft synthetic `TestTrace` vectors with known divergence patterns; assert correct `FlakyCause`.
- Unit: No divergence -> empty `FlakyReport`.
- Integration: Fixture Python project with a deliberately flaky test (reads current time) — run 5 times, confirm detection.

---

## 3. LLM-Based Flaky Test Repair

**Paper:** FlakyFix (arXiv:2307.00012)
**Concept:** Given a detected flaky test and its root cause, prompt an LLM to generate a repair.

### Target Crate(s)

- `apex-index` (primary) — repair logic lives near detection
- `apex-concolic` (secondary) — uses similar LLM prompting infrastructure

### New Types / Traits

```rust
// apex-index/src/flaky_repair.rs

/// A proposed repair for a flaky test.
#[derive(Debug, Clone, Serialize)]
pub struct FlakyRepair {
    pub test_name: String,
    pub cause: FlakyCause,
    pub original_source: String,
    pub repaired_source: String,
    pub repair_strategy: RepairStrategy,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub enum RepairStrategy {
    AddRetry,            // wrap in retry loop
    AddSleep,            // add wait for async resource
    IsolateState,        // reset shared state in setUp/tearDown
    MockExternalResource,// replace time/network with deterministic mock
    FixOrdering,         // add explicit ordering/synchronization
}

/// Trait for LLM interaction — shared with apex-concolic.
pub trait LlmClient: Send + Sync {
    fn prompt(&self, system: &str, user: &str) -> Result<String>;
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-index/src/flaky_repair.rs` | `FlakyRepair`, `RepairStrategy`, `generate_flaky_repair()` |
| Modify | `crates/apex-index/src/lib.rs` | Add `pub mod flaky_repair;` |
| Create | `crates/apex-core/src/llm.rs` | `LlmClient` trait (shared between crates) |
| Modify | `crates/apex-core/src/lib.rs` | Add `pub mod llm;` |

### Integration Points

- Depends on technique #2 output: `FlakyReport` with `FlakyCause` per test.
- `generate_flaky_repair()` takes `FlakyTest` + `FlakyCause` + source code + `&dyn LlmClient`.
- Prompt template: include the test source, the divergent branches, the classified cause, and ask for a minimal repair.
- Output can be piped to `apex-synth`'s test runner to validate the repair (rerun N times, confirm stability).

### Complexity Estimate

**Medium** (~4-5 days). The LLM prompting is straightforward. The harder part is validating repairs — need to rerun the repaired test N times and confirm no divergence. Also requires defining the `LlmClient` trait in `apex-core` for cross-crate sharing.

### Test Strategy

- Unit: Given a known `FlakyCause::AsyncTiming`, prompt construction includes retry/wait patterns.
- Unit: `RepairStrategy` selection logic maps each `FlakyCause` to expected strategies.
- Mock: Use a mock `LlmClient` that returns canned repair strings; verify parsing and validation.
- Integration: Repair a deliberately flaky Python test, rerun 10 times, confirm zero divergence.

---

## 4. Dead Code Detection + LLM Validation

**Paper:** DCE-LLM (arXiv:2506.11076)
**Concept:** Identify code never reached by any test (from index), then use CodeBERT/LLM to validate whether it's truly unreachable vs. just untested. >94% F1.

### Target Crate(s)

- `apex-index` (primary) — dead code identification from branch data
- `apex-coverage` (secondary) — `mark_unreachable()` integration

### New Types / Traits

```rust
// apex-index/src/dead_code.rs

/// A region of code suspected to be dead.
#[derive(Debug, Clone, Serialize)]
pub struct DeadCodeCandidate {
    pub file_id: u64,
    pub file_path: PathBuf,
    pub start_line: u32,
    pub end_line: u32,
    pub kind: DeadCodeKind,
    pub llm_confidence: Option<f64>,
    pub llm_verdict: Option<DeadCodeVerdict>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum DeadCodeKind {
    NeverCoveredBranch,   // branch in index with zero hits
    UnreachableFunction,  // entire function never entered
    DeadConditional,      // condition that can never be true
    DefensiveCode,        // error handling for impossible states
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum DeadCodeVerdict {
    TrulyDead,        // LLM confirms unreachable
    Undertested,      // reachable but no test exercises it
    DefensiveKeep,    // unreachable but intentional safety code
    Uncertain,        // LLM cannot determine
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-index/src/dead_code.rs` | Types, `detect_dead_code()`, `validate_with_llm()` |
| Modify | `crates/apex-index/src/lib.rs` | Add `pub mod dead_code;` |
| Modify | `crates/apex-index/src/types.rs` | Add `dead_code_candidates: Vec<DeadCodeCandidate>` to `BranchIndex` (optional, defaulting to empty) |
| Modify | `crates/apex-coverage/src/oracle.rs` | Use `DeadCodeVerdict::TrulyDead` to auto-call `mark_unreachable()` |

### Integration Points

- `detect_dead_code()` takes a `BranchIndex` + `&[BranchId]` (all known branches) and returns candidates by finding branches that appear in no trace.
- `validate_with_llm()` takes candidates + source code + `&dyn LlmClient` and adds verdicts.
- Candidates with `TrulyDead` verdict can be auto-suppressed in `CoverageOracle` via `mark_unreachable()`, removing them from gap reports.
- Candidates with `Undertested` verdict become high-priority targets for `apex-synth`.

### Complexity Estimate

**Medium** (~4 days). Dead code identification from index data is trivial (set difference). LLM validation prompt is: "Given this function and its call graph, is this branch reachable? Consider: 1) Is there any caller that passes values reaching this branch? 2) Is this defensive error handling?" Parsing LLM response into `DeadCodeVerdict` is the fiddliest part.

### Test Strategy

- Unit: `detect_dead_code()` correctly identifies branches absent from all traces.
- Unit: Mock LLM returning "truly dead" / "undertested" / "defensive" — verify parsing to `DeadCodeVerdict`.
- Integration: Python fixture with unreachable `else` branch — detect and validate.
- Regression: Ensure `mark_unreachable()` count matches `TrulyDead` count from validation.

---

## 5. Rank Aggregation Test Prioritization

**Paper:** arXiv:2412.00015
**Concept:** Combine multiple prioritization signals (coverage breadth, recency of failure, execution time, mutation score) via rank aggregation (Borda count or Kemeny-Young) to produce a single test ordering.

### Target Crate(s)

- `apex-index` (primary) — operates on `BranchIndex` data

### New Types / Traits

```rust
// apex-index/src/prioritize.rs

/// A single prioritization signal.
pub trait PrioritizationSignal: Send + Sync {
    fn name(&self) -> &str;
    fn rank(&self, index: &BranchIndex) -> Vec<(String, f64)>; // (test_name, score)
}

/// Built-in signals.
pub struct CoverageBreadthSignal;    // more unique branches = higher
pub struct ExecutionTimeSignal;       // faster = higher (for CI)
pub struct FailureRecencySignal;      // recently failed = higher
pub struct MutationKillSignal;        // kills more mutants = higher
pub struct ChangeCoverageSignal;      // covers changed files = higher

/// Rank aggregation methods.
#[derive(Debug, Clone, Copy)]
pub enum AggregationMethod {
    BordaCount,
    ReciprocalRank,  // RRF: 1/(k+rank), good default
    WeightedSum,     // user-provided weights
}

/// Prioritized test ordering.
#[derive(Debug, Clone, Serialize)]
pub struct PrioritizedSuite {
    pub ordered_tests: Vec<PrioritizedTest>,
    pub method: String,
    pub signals_used: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrioritizedTest {
    pub test_name: String,
    pub aggregate_score: f64,
    pub per_signal_ranks: HashMap<String, usize>,
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-index/src/prioritize.rs` | Trait, built-in signals, aggregation algorithms |
| Modify | `crates/apex-index/src/lib.rs` | Add `pub mod prioritize;` |

### Integration Points

- CLI: `apex index --prioritize [--method borda|rrf|weighted]` outputs ordered test list.
- `PrioritizationSignal` trait allows `apex-agent` to plug in additional signals (e.g., LLM-estimated relevance).
- `MutationKillSignal` depends on technique #9 (metamorphic adequacy) — degrades gracefully to 0 scores when mutation data is absent.
- `ChangeCoverageSignal` bridges to technique #6 (slice-based change impact).

### Complexity Estimate

**Low-Medium** (~3 days). Borda count and RRF are simple algorithms (~20 lines each). The real value is in the trait design allowing extensibility. Each built-in signal is a separate small struct implementing `rank()`.

### Test Strategy

- Unit: Borda count with 3 signals, 3 tests — verify known ordering.
- Unit: RRF with ties — verify tiebreaking is stable.
- Unit: Single signal degenerates to raw ranking.
- Property: Output always contains exactly the tests from the index (no drops, no duplicates).

---

## 6. Slice-Based Change Impact Prioritization

**Paper:** arXiv:2508.19056
**Concept:** Given a set of changed files/lines (from `git diff`), use the branch index to find tests that cover those regions, then prioritize them. More targeted than full re-prioritization.

### Target Crate(s)

- `apex-index` (primary) — leverages existing `BranchProfile.test_names`

### New Types / Traits

```rust
// apex-index/src/change_impact.rs

/// A change region from a VCS diff.
#[derive(Debug, Clone)]
pub struct ChangeRegion {
    pub file_path: PathBuf,
    pub file_id: u64,
    pub changed_lines: Vec<u32>,
}

/// Test impact for a set of changes.
#[derive(Debug, Clone, Serialize)]
pub struct ChangeImpactReport {
    pub changes: Vec<ChangeRegion>,
    pub impacted_tests: Vec<ImpactedTest>,
    pub unrelated_tests: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactedTest {
    pub test_name: String,
    /// Number of changed branches this test covers.
    pub impact_score: usize,
    /// The specific changed branches covered by this test.
    pub covered_changes: Vec<BranchId>,
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-index/src/change_impact.rs` | `ChangeRegion`, `ChangeImpactReport`, `compute_change_impact()` |
| Modify | `crates/apex-index/src/lib.rs` | Add `pub mod change_impact;` |

### Integration Points

- `compute_change_impact()` takes a `BranchIndex` + `Vec<ChangeRegion>` and cross-references `BranchProfile` entries.
- For each changed line, find branches at that `(file_id, line)` via the profile map.
- For each matching branch, collect `test_names` from its `BranchProfile` — those are the impacted tests.
- CLI: `apex index --impact` reads `git diff HEAD~1` to extract `ChangeRegion`s automatically.
- Feeds into technique #5 as `ChangeCoverageSignal`.

### Complexity Estimate

**Low** (~2 days). The branch index already maps branches to test names. This is a join operation: changed lines -> matching branches -> test names. Parsing `git diff` output into `ChangeRegion`s is the only new I/O work.

### Test Strategy

- Unit: Known index + known changes -> expected impacted tests.
- Unit: Changes outside any branch -> all tests in `unrelated_tests`.
- Unit: All branches changed -> all tests impacted.
- Integration: Python fixture with two files, change one, verify correct test subset.

---

## 7. Semantic Feedback Signals

**Paper:** arXiv:2511.03995
**Concept:** Beyond binary branch coverage, track richer feedback: exception types, output content patterns, memory usage, return value distributions. These signals guide test generation toward more diverse behaviors.

### Target Crate(s)

- `apex-coverage` (primary) — extends `DeltaCoverage` and `CoverageOracle`

### New Types / Traits

```rust
// apex-coverage/src/semantic.rs

/// Semantic feedback from a single execution, beyond branch coverage.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SemanticFeedback {
    /// Exception/error types raised during execution.
    pub exceptions: Vec<ExceptionInfo>,
    /// Distinct output content hashes (for output diversity).
    pub output_hashes: Vec<u64>,
    /// Return value categories (null, zero, positive, negative, string, etc.).
    pub return_categories: Vec<ReturnCategory>,
    /// Peak memory usage in bytes (if measurable).
    pub peak_memory: Option<u64>,
    /// Wall clock duration in microseconds.
    pub duration_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ExceptionInfo {
    pub exception_type: String,
    pub message_hash: u64,    // hash of message to detect new error messages
    pub stack_depth: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ReturnCategory {
    Null,
    Zero,
    PositiveInt,
    NegativeInt,
    EmptyString,
    NonEmptyString,
    EmptyCollection,
    NonEmptyCollection,
    Boolean(bool),
    Other,
}

/// Semantic coverage oracle — tracks unique semantic states.
pub struct SemanticOracle {
    seen_exceptions: DashSet<ExceptionInfo>,
    seen_output_hashes: DashSet<u64>,
    seen_return_categories: DashSet<ReturnCategory>,
}

/// Delta that captures new semantic behaviors discovered.
#[derive(Debug, Default)]
pub struct SemanticDelta {
    pub new_exceptions: Vec<ExceptionInfo>,
    pub new_output_hashes: Vec<u64>,
    pub new_return_categories: Vec<ReturnCategory>,
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-coverage/src/semantic.rs` | `SemanticFeedback`, `SemanticOracle`, `SemanticDelta` |
| Modify | `crates/apex-coverage/src/lib.rs` | Add `pub mod semantic;` |
| Modify | `crates/apex-coverage/src/oracle.rs` | Add optional `semantic: Option<SemanticOracle>` field to `CoverageOracle`, extend `merge_from_result()` to capture semantic data |
| Modify | `crates/apex-core/src/types.rs` | Add `semantic_feedback: Option<SemanticFeedback>` to `ExecutionResult` |

### Integration Points

- `ExecutionResult` gains an optional `SemanticFeedback` field. Language-specific runners populate it.
- `CoverageOracle::merge_from_result()` checks semantic novelty alongside branch novelty.
- `DeltaCoverage` is extended (or a parallel `SemanticDelta` is returned) to report new semantic behaviors.
- The agent loop in `apex-agent` treats semantic novelty as a reward signal alongside branch coverage, preventing test suites that achieve high coverage but low behavioral diversity.

### Complexity Estimate

**Medium** (~4 days). Type definitions are straightforward. The work is in modifying each language runner (Python concolic, Rust instrument) to extract semantic signals from execution output. Start with Python: parse stderr for exception types, hash stdout for output diversity.

### Test Strategy

- Unit: `SemanticOracle` correctly deduplicates exceptions by type+message_hash.
- Unit: `ReturnCategory` classification for edge cases (None, 0, "", [], True).
- Unit: `SemanticDelta` is empty when re-merging identical feedback.
- Integration: Python fixture that raises different exceptions on different inputs — verify all captured.

---

## 8. Fitness Landscape Adaptation

**Paper:** arXiv:2502.00169
**Concept:** Analyze the "fitness landscape" (how coverage changes as inputs vary) to determine which test generation technique to apply. Smooth landscapes suit gradient-based search; rugged landscapes need random search or LLM.

### Target Crate(s)

- `apex-coverage` (primary) — landscape analysis operates on heuristic data
- `apex-symbolic` (secondary) — solver selection informed by landscape

### New Types / Traits

```rust
// apex-coverage/src/landscape.rs

/// Characterization of the fitness landscape around a branch.
#[derive(Debug, Clone, Serialize)]
pub struct LandscapeProfile {
    pub branch_id: BranchId,
    pub smoothness: f64,        // 0=rugged, 1=smooth (gradient-friendly)
    pub neutrality: f64,        // fraction of neighbors with equal fitness
    pub gradient_magnitude: f64, // average fitness change per step
    pub recommended_technique: TechniqueClass,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum TechniqueClass {
    Gradient,    // smooth landscape, use GradientSolver
    Symbolic,    // complex constraints, use Z3/SMT
    Random,      // highly rugged, random mutation
    LlmGuided,  // structured input, use LLM solver
}

/// Analyze landscape around a set of branches using sampled heuristics.
pub fn analyze_landscape(
    oracle: &CoverageOracle,
    branches: &[BranchId],
    sample_count: usize,
) -> Vec<LandscapeProfile>;
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-coverage/src/landscape.rs` | `LandscapeProfile`, `TechniqueClass`, `analyze_landscape()` |
| Modify | `crates/apex-coverage/src/lib.rs` | Add `pub mod landscape;` |
| Modify | `crates/apex-symbolic/src/portfolio.rs` | `PortfolioSolver` uses `TechniqueClass` to reorder its solver chain per-query |

### Integration Points

- `analyze_landscape()` uses `CoverageOracle::heuristic_for()` data — it needs multiple heuristic samples per branch (from nearby inputs) to estimate smoothness.
- The `BranchHeuristic` already stores `score`, `operand_a`, `operand_b`. Landscape analysis perturbs operands and measures score deltas.
- `PortfolioSolver` gains a `set_technique_hint(TechniqueClass)` method. When set, it reorders its solver list (e.g., gradient first for `Smooth`, Z3 first for `Symbolic`).
- `apex-agent`'s strategy router replaces ad-hoc proximity thresholds with `LandscapeProfile.recommended_technique`.

### Complexity Estimate

**Medium** (~4-5 days). Landscape analysis requires sampling — either from stored heuristic history or by requesting new samples. The analysis itself (smoothness, neutrality) is simple statistics over score vectors. The architectural impact is larger: `PortfolioSolver` needs per-query technique hints.

### Test Strategy

- Unit: Perfectly smooth landscape (linear scores) -> `TechniqueClass::Gradient`.
- Unit: Constant landscape (all scores equal) -> high neutrality.
- Unit: Alternating scores -> rugged -> `TechniqueClass::Random`.
- Integration: `PortfolioSolver` with technique hint reorders solvers correctly.

---

## 9. Metamorphic Adequacy

**Paper:** Derived from Mind the Gap + Meta ACH convergence
**Concept:** Track mutation score as a first-class metric alongside coverage throughout the APEX pipeline. Mutation score becomes a quality gate, not just a post-hoc analysis.

### Target Crate(s)

- `apex-coverage` (primary) — builds on technique #1 (oracle gap)

### New Types / Traits

```rust
// apex-coverage/src/adequacy.rs

/// Combined adequacy metric: coverage + mutation score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdequacyMetric {
    pub coverage_percent: f64,
    pub mutation_score: f64,
    pub oracle_gap: f64,         // coverage - mutation_score
    pub effective_coverage: f64,  // min(coverage, mutation_score) — conservative
    pub region_scores: Vec<RegionMutationScore>, // from technique #1
}

/// Quality gate: does the test suite meet adequacy thresholds?
#[derive(Debug, Clone)]
pub struct AdequacyGate {
    pub min_coverage: f64,
    pub min_mutation_score: f64,
    pub max_oracle_gap: f64,
}

impl AdequacyGate {
    pub fn check(&self, metric: &AdequacyMetric) -> AdequacyResult {
        // ...
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AdequacyResult {
    pub passed: bool,
    pub violations: Vec<String>,
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-coverage/src/adequacy.rs` | `AdequacyMetric`, `AdequacyGate`, `AdequacyResult` |
| Modify | `crates/apex-coverage/src/lib.rs` | Add `pub mod adequacy;` |
| Modify | `crates/apex-cli/src/main.rs` | Extend `ratchet` subcommand to accept `--min-mutation-score` and `--max-oracle-gap` flags |

### Integration Points

- `AdequacyMetric` is computed by combining `CoverageOracle::coverage_percent()` with `oracle_gap_report()` from technique #1.
- `ratchet` subcommand already gates on coverage — extend to gate on mutation score and oracle gap.
- `apex-agent` loop termination: currently stops when coverage target is met. With metamorphic adequacy, also require mutation score target before declaring "done."

### Complexity Estimate

**Low** (~2 days). This is primarily a composition of technique #1 outputs into a unified metric plus CLI plumbing. The `AdequacyGate` is trivial threshold comparison.

### Dependencies

- Requires technique #1 (Oracle Gap Metric) to be implemented first.

### Test Strategy

- Unit: `AdequacyGate::check()` with known metrics — verify pass/fail cases.
- Unit: `effective_coverage` = min(coverage, mutation_score) — always <= either.
- Integration: `apex ratchet --min-mutation-score 0.8 --max-oracle-gap 20` with fixture data.

---

## 10. LLM as Concolic Constraint Solver

**Paper:** Cottontail (arXiv:2504.17542)
**Concept:** Replace Z3 with LLM for structured input constraints (JSON schemas, string formats, protocol buffers). Z3 handles numeric/boolean; LLM handles string/structural. 30-41% higher coverage on structured inputs.

### Target Crate(s)

- `apex-symbolic` (primary) — new `LlmSolver` backend
- `apex-concolic` (secondary) — routes structured constraints to LLM

### New Types / Traits

```rust
// apex-symbolic/src/llm_solver.rs

/// Solver backend that uses an LLM to satisfy constraints.
pub struct LlmSolver {
    client: Box<dyn LlmClient>,
    /// Constraint types this solver handles.
    handled_types: Vec<ConstraintDomain>,
    max_retries: usize,
}

/// Classifies what domain a constraint operates in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintDomain {
    Numeric,       // integer/float arithmetic
    Boolean,       // pure boolean logic
    String,        // string operations (length, contains, regex)
    Structural,    // JSON/XML/protobuf structure
    Mixed,         // combination
}

/// Classify a set of constraints by domain.
pub fn classify_constraints(constraints: &[String]) -> ConstraintDomain;

impl Solver for LlmSolver {
    fn solve(&self, constraints: &[String], negate_last: bool)
        -> Result<Option<InputSeed>>;
    // LLM prompt: "Given these constraints on program inputs:
    //   {constraints}
    // Generate a concrete input (as JSON) that satisfies all constraints
    // {if negate_last: except negate the last constraint}"
    // Parse LLM output as InputSeed bytes.

    fn set_logic(&mut self, _logic: SolverLogic) {}
    fn name(&self) -> &str { "llm" }
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-symbolic/src/llm_solver.rs` | `LlmSolver`, `ConstraintDomain`, `classify_constraints()` |
| Modify | `crates/apex-symbolic/src/lib.rs` | Add `pub mod llm_solver;` |
| Modify | `crates/apex-symbolic/src/portfolio.rs` | `PortfolioSolver::with_gradient_and_llm()` factory that adds `LlmSolver` as the last backend |
| Modify | `crates/apex-concolic/src/python.rs` | When path constraints are classified as `Structural`/`String`, route directly to `LlmSolver` (skip gradient + Z3) |

### Integration Points

- `PortfolioSolver` already chains solvers sequentially. Adding `LlmSolver` as the final backend means: gradient (fast, numeric) -> Z3 (precise, numeric/boolean) -> LLM (structured/string).
- `classify_constraints()` inspects constraint strings for patterns: SMT-LIB string ops (`str.contains`, `str.len`) -> `String`; JSON-like references -> `Structural`; arithmetic only -> `Numeric`.
- `PythonConcolicStrategy` can short-circuit: if all constraints are `String`/`Structural`, skip gradient and Z3 entirely (they'll return None anyway).
- `LlmClient` trait from technique #3 is reused here.

### Complexity Estimate

**High** (~6-7 days). The `LlmSolver` itself is ~200 lines. The complexity is in:
1. Constraint classification — needs robust parsing of SMT-LIB2 strings.
2. LLM output parsing — the LLM returns JSON or structured text that must be converted to `InputSeed` bytes.
3. Validation — after generating an input, we should ideally verify it satisfies the constraints (run through Z3 as a check, or re-execute).
4. Retry logic — LLMs produce invalid outputs; need N retries with error feedback.

### Test Strategy

- Unit: `classify_constraints()` correctly identifies `String` vs `Numeric` vs `Structural`.
- Unit: Mock `LlmClient` returning valid JSON -> correct `InputSeed` parsing.
- Unit: Mock `LlmClient` returning garbage -> `LlmSolver` returns `None` after retries.
- Integration: Portfolio with gradient + LLM; string constraint that gradient can't solve -> LLM finds solution.
- Integration: Python fixture with JSON-parsing function — concolic with LLM solver generates valid JSON inputs.

---

## 11. AutoBug Path Decomposition

**Paper:** arXiv:2505.13452
**Concept:** Instead of building a symbolic execution engine, decompose target paths into segments and use an LLM to generate inputs for each segment. The LLM "reasons" about each segment's constraints in natural language.

### Target Crate(s)

- `apex-symbolic` (primary) — alternative to `SymbolicSession`
- `apex-concolic` (secondary) — uses decomposition as fallback when concolic fails

### New Types / Traits

```rust
// apex-symbolic/src/path_decomposition.rs

/// A segment of a program path with its constraints described in natural language.
#[derive(Debug, Clone)]
pub struct PathSegment {
    pub segment_id: usize,
    pub source_lines: String,           // the code for this segment
    pub entry_constraints: Vec<String>, // what must hold at segment entry
    pub branch_condition: String,       // the branch we want to take
    pub variables_in_scope: Vec<(String, String)>, // (name, type)
}

/// A decomposed path from function entry to target branch.
#[derive(Debug, Clone)]
pub struct DecomposedPath {
    pub target_branch: BranchId,
    pub segments: Vec<PathSegment>,
    pub function_signature: String,
}

/// Solve a decomposed path by LLM reasoning over each segment.
pub struct PathDecompositionSolver {
    client: Box<dyn LlmClient>,
}

impl PathDecompositionSolver {
    /// Decompose the path from function entry to `target` using CFG/source.
    pub fn decompose(
        &self,
        source: &str,
        target: &BranchId,
        cfg_edges: &[(u32, u32)], // (from_line, to_line)
    ) -> Result<DecomposedPath>;

    /// Solve: prompt LLM with each segment sequentially, accumulating constraints.
    pub fn solve(&self, path: &DecomposedPath) -> Result<Option<InputSeed>>;
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-symbolic/src/path_decomposition.rs` | `PathSegment`, `DecomposedPath`, `PathDecompositionSolver` |
| Modify | `crates/apex-symbolic/src/lib.rs` | Add `pub mod path_decomposition;` |
| Modify | `crates/apex-concolic/src/python.rs` | Add fallback: when `SymbolicSession` + Z3 fail, try `PathDecompositionSolver` |

### Integration Points

- `PathDecompositionSolver::decompose()` uses CFG data. Currently `PythonConcolicStrategy` collects branch trace data that includes condition text and variables. Extend to also record basic block boundaries for CFG reconstruction.
- The solver prompts the LLM segment-by-segment: "Given that we enter this segment with {entry_constraints}, what input values cause the branch `{branch_condition}` to be True?"
- Each segment's solution constrains the next segment's entry constraints.
- This is a fallback, not a replacement — it fires only when traditional concolic (gradient -> Z3) fails.
- Especially valuable for languages without symbolic execution support (Ruby, JavaScript).

### Complexity Estimate

**High** (~7-8 days). Path decomposition from source + CFG is the hard part. The LLM prompting is straightforward but requires careful prompt engineering to chain segment constraints. Parsing LLM natural-language reasoning into concrete input values needs robust extraction.

### Test Strategy

- Unit: `decompose()` on a simple 3-branch function produces 3 segments with correct constraints.
- Unit: Mock LLM returning concrete values for each segment -> correct `InputSeed`.
- Unit: Mock LLM failing on segment 2 -> solver returns `None`.
- Integration: Python function with nested `if/elif/else` — decompose and solve for deepest branch.
- Comparison: Same target, compare `PathDecompositionSolver` vs `SymbolicSession` — verify equivalent coverage.

---

## 12. Diverse SMT Solutions

**Paper:** PanSampler (arXiv:2511.10326)
**Concept:** Instead of one satisfying assignment per constraint, generate N diverse solutions. Diversity is measured by Hamming distance over key variables. Needs 32-76% fewer test cases for the same fault detection.

### Target Crate(s)

- `apex-symbolic` (primary) — extends `Solver` trait and `PortfolioSolver`
- `apex-concolic` (secondary) — consumes diverse solutions for broader path exploration

### New Types / Traits

```rust
// apex-symbolic/src/diversity.rs

/// Configuration for diverse solution sampling.
#[derive(Debug, Clone)]
pub struct DiversityConfig {
    pub num_solutions: usize,     // how many diverse solutions to request (default: 5)
    pub diversity_metric: DiversityMetric,
    pub min_distance: f64,        // minimum distance between any two solutions
}

#[derive(Debug, Clone, Copy)]
pub enum DiversityMetric {
    Hamming,           // bitwise difference
    Euclidean,         // numeric distance on key variables
    VariableFlip,      // maximize number of variables that differ
}

/// A set of diverse solutions for the same constraint.
#[derive(Debug, Clone)]
pub struct DiverseSolutions {
    pub solutions: Vec<InputSeed>,
    pub pairwise_distances: Vec<f64>,   // flattened lower triangle
    pub average_diversity: f64,
}

/// Extension trait for solvers that support diverse solution generation.
pub trait DiverseSolver: Solver {
    fn solve_diverse(
        &self,
        constraints: &[String],
        negate_last: bool,
        config: &DiversityConfig,
    ) -> Result<DiverseSolutions>;
}

/// Wrapper that adds diversity to any Solver via iterative re-solving.
pub struct DiversityWrapper<S: Solver> {
    inner: S,
    config: DiversityConfig,
}
```

### Files to Create or Modify

| Action | Path | Description |
|--------|------|-------------|
| Create | `crates/apex-symbolic/src/diversity.rs` | `DiversityConfig`, `DiverseSolutions`, `DiverseSolver`, `DiversityWrapper` |
| Modify | `crates/apex-symbolic/src/lib.rs` | Add `pub mod diversity;` |
| Modify | `crates/apex-symbolic/src/portfolio.rs` | Add `solve_diverse()` to `PortfolioSolver` that delegates to `DiversityWrapper` |
| Modify | `crates/apex-symbolic/src/traits.rs` | Add default `solve_diverse()` method to `Solver` trait (default: call `solve()` once, return single-element `DiverseSolutions`) |
| Modify | `crates/apex-concolic/src/python.rs` | When generating test inputs for a branch, request diverse solutions to maximize coverage breadth |

### Integration Points

- `DiversityWrapper::solve_diverse()` algorithm:
  1. Call `inner.solve(constraints, negate_last)` to get first solution S1.
  2. For i in 2..=N: add anti-constraint "(not (and (= v1 s1_v1) (= v2 s1_v2) ...))" for all previous solutions, re-solve.
  3. Compute pairwise distances, return `DiverseSolutions`.
- Anti-constraints are constructed from `InputSeed` data — requires knowing variable names from the constraint set (parse SMT-LIB2 `declare-const` statements).
- `PythonConcolicStrategy` currently generates one test per solved branch. With diverse solutions, it generates N tests, maximizing the chance of covering downstream branches.
- The `PortfolioSolver` gains `solve_diverse()` that uses the first solver backend that returns SAT, then wraps it in `DiversityWrapper` for N-1 more solutions.

### Complexity Estimate

**Medium** (~5 days). The core algorithm (iterative re-solving with anti-constraints) is well-defined. The challenges:
1. Parsing variable names from SMT-LIB2 constraint strings.
2. Constructing anti-constraints in valid SMT-LIB2 syntax.
3. Handling the case where fewer than N diverse solutions exist (solver returns UNSAT before reaching N).
4. Efficient distance computation (Hamming over `InputSeed` bytes is trivial; Euclidean over decoded variables is harder).

### Test Strategy

- Unit: `DiversityWrapper` with mock solver — 5 requests produce 5 distinct `InputSeed` values.
- Unit: Anti-constraint generation produces valid SMT-LIB2.
- Unit: Distance computation: Hamming([0,0,0], [1,1,1]) = 3.
- Unit: When solver returns UNSAT after 3 solutions, `DiverseSolutions` contains 3 (not 5).
- Property: All solutions in `DiverseSolutions` satisfy the original constraints (verify via re-solving).
- Integration: Simple numeric constraint `x > 0 AND x < 100` — diverse solutions span the range, not cluster near one value.

---

## Dependency Graph

```
Technique #1 (Oracle Gap Metric)
    |
    v
Technique #9 (Metamorphic Adequacy) ----> Technique #5 (Rank Aggregation - MutationKillSignal)

Technique #2 (Flaky Detection)
    |
    v
Technique #3 (Flaky Repair) -----> LlmClient trait (apex-core)
                                         ^
                                         |
Technique #10 (LLM Solver) -------------+
    |                                    |
    v                                    |
Technique #11 (Path Decomposition) -----+

Technique #4 (Dead Code) ---------------+---> LlmClient trait
                                         |
Technique #6 (Change Impact) ------------> Technique #5 (Rank Aggregation - ChangeCoverageSignal)

Technique #7 (Semantic Feedback) ---------> standalone (extends ExecutionResult)

Technique #8 (Fitness Landscape) ---------> Technique #10 (LLM Solver routing)

Technique #12 (Diverse SMT) -------------> standalone (extends Solver trait)
```

**Shared dependency:** Techniques #3, #4, #10, #11 all require a shared `LlmClient` trait in `apex-core`. This trait should be implemented first.

---

## Build Sequence

### Phase 0 — Foundation (~1 day)
Create `LlmClient` trait in `apex-core/src/llm.rs`. This unblocks techniques #3, #4, #10, #11.

### Phase 1 — Coverage Enrichment (~1 week)
Independent of each other; can be parallelized.

| Order | Technique | Crate | Est. |
|-------|-----------|-------|------|
| 1a | #7 Semantic Feedback | apex-coverage | 4d |
| 1b | #1 Oracle Gap Metric | apex-coverage | 3d |
| 1c | #2 Flaky Detection (extend) | apex-index | 2d |
| 1d | #6 Change Impact | apex-index | 2d |

### Phase 2 — Index Intelligence (~1 week)
Some depend on Phase 1 outputs.

| Order | Technique | Depends On | Est. |
|-------|-----------|------------|------|
| 2a | #9 Metamorphic Adequacy | #1 | 2d |
| 2b | #5 Rank Aggregation | #6, #9 (partial) | 3d |
| 2c | #4 Dead Code + LLM | Phase 0 | 4d |
| 2d | #3 Flaky Repair | #2, Phase 0 | 4d |

### Phase 3 — Solver Upgrades (~2 weeks)
Deeper changes to the symbolic/concolic pipeline.

| Order | Technique | Depends On | Est. |
|-------|-----------|------------|------|
| 3a | #8 Fitness Landscape | #1 (heuristic data) | 4d |
| 3b | #12 Diverse SMT | standalone | 5d |
| 3c | #10 LLM Solver | Phase 0 | 6d |
| 3d | #11 Path Decomposition | Phase 0, #10 | 7d |

### Total estimated effort: ~6-7 weeks with parallelism, ~10 weeks serial.

---

## Summary Table

| # | Technique | Crate(s) | New Files | Complexity | Dependencies |
|---|-----------|----------|-----------|------------|-------------|
| 1 | Oracle Gap Metric | coverage | 2 | Low-Med | None |
| 2 | Flaky Detection | index | 1 | Low | None |
| 3 | Flaky Repair | index, core | 2 | Medium | #2, LlmClient |
| 4 | Dead Code + LLM | index, coverage | 1 | Medium | LlmClient |
| 5 | Rank Aggregation TCP | index | 1 | Low-Med | #6, #9 (partial) |
| 6 | Change Impact TCP | index | 1 | Low | None |
| 7 | Semantic Feedback | coverage, core | 1 | Medium | None |
| 8 | Fitness Landscape | coverage, symbolic | 1 | Medium | None |
| 9 | Metamorphic Adequacy | coverage | 1 | Low | #1 |
| 10 | LLM Solver | symbolic, concolic | 1 | High | LlmClient |
| 11 | Path Decomposition | symbolic, concolic | 1 | High | LlmClient |
| 12 | Diverse SMT | symbolic, concolic | 1 | Medium | None |
