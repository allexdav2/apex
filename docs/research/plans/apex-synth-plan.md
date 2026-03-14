# apex-synth Implementation Plan

Compiled: 2026-03-14
Scope: 8 research techniques targeting `crates/apex-synth/`

---

## Table of Contents

1. [Current State](#current-state)
2. [Architecture Overview](#architecture-overview)
3. [Core Abstractions](#core-abstractions)
4. [Technique 1: Code Elimination from Prompts](#technique-1-code-elimination-from-prompts)
5. [Technique 2: Counter-Example Feedback](#technique-2-counter-example-feedback)
6. [Technique 3: Method Slicing](#technique-3-method-slicing)
7. [Technique 4: Co-Evolutionary Generation/Repair](#technique-4-co-evolutionary-generationrepair)
8. [Technique 5: Path-Enumeration Prompting](#technique-5-path-enumeration-prompting)
9. [Technique 6: Natural-Language Constraint Fallback](#technique-6-natural-language-constraint-fallback)
10. [Technique 7: Pluggable Prompt Pipeline](#technique-7-pluggable-prompt-pipeline)
11. [Technique 8: Agentic File-Level Generation](#technique-8-agentic-file-level-generation)
12. [Build Sequence](#build-sequence)
13. [Data Flow](#data-flow)

---

## Current State

`apex-synth` today has:

- **`LlmSynthesizer`** — CoverUp-style closed-loop: `initial_prompt` -> LLM -> `run_test` -> feedback -> retry (up to `max_attempts`).
- **`CoverageGap`** — file path, target line, function name, source segment, uncovered lines.
- **`SynthAttempt`** — test code, coverage delta, error, attempt number.
- **`LlmMessage` / `LlmRole`** — simple conversation history.
- **`LlmConfig`** — max_attempts, model, temperature.
- **`CodeSegment` / `extract_segment`** — extracts code around a target line with context window.
- **`clean_error_output`** — strips pytest separator noise.
- **Template synthesizers** — `PytestSynthesizer`, `JestSynthesizer`, `JUnitSynthesizer`, `CargoTestSynthesizer` for boilerplate generation.

The `fill_gap()` method takes two callbacks (`llm_call`, `run_test`) making it fully testable without real LLM or execution. This pattern must be preserved.

---

## Architecture Overview

The plan introduces four core abstractions that sit between the existing `CoverageGap` input and `SynthAttempt` output:

```
                          +-----------------+
                          |  SynthPipeline  |  (orchestrator)
                          +--------+--------+
                                   |
                    +--------------+--------------+
                    |              |              |
              route by gap    track history   select strategy
                    |              |              |
            +-------+------+  +---+----+  +------+-------+
            | GapClassifier |  |GapHist.|  |PromptStrategy|
            +-------+------+  +---+----+  +------+-------+
                    |              |              |
                    v              v              v
            +-----------+  +-----------+  +-----------+
            | Coverup   |  | Eliminate |  | TELPA     |
            | (default) |  | Strategy  |  | Strategy  |
            +-----------+  +-----------+  +-----------+
            | Slice     |  | PathEnum  |  | NLConstr  |
            | Strategy  |  | Strategy  |  | Strategy  |
            +-----------+  +-----------+  +-----------+
            | CoEvo     |  | TestForge |
            | Strategy  |  | Strategy  |
            +-----------+  +-----------+
                    |
                    v
            +------+-------+
            | RepairPolicy |  (classifies errors, decides repair vs regen)
            +------+-------+
                    |
                    v
              SynthAttempt
```

---

## Core Abstractions

These are the shared types and traits that all 8 techniques depend on. They must be built first.

### File: `crates/apex-synth/src/strategy.rs` (new)

#### `PromptStrategy` trait

```rust
/// A strategy for constructing LLM prompts to cover a gap.
///
/// Each research technique implements this trait. The pipeline
/// selects a strategy per-gap based on gap characteristics and
/// prior history.
pub trait PromptStrategy: Send + Sync {
    /// Human-readable name for logging and metrics.
    fn name(&self) -> &str;

    /// Whether this strategy can handle the given gap.
    /// Used by the pipeline to filter candidate strategies.
    fn can_handle(&self, gap: &CoverageGap, history: &GapHistory) -> bool;

    /// Build the initial prompt messages for this gap.
    fn initial_prompt(
        &self,
        gap: &CoverageGap,
        history: &GapHistory,
        config: &LlmConfig,
    ) -> Vec<LlmMessage>;

    /// Build a feedback prompt after a failed attempt.
    /// Returns None if the strategy has no feedback mechanism
    /// (pipeline will fall through to the next strategy).
    fn feedback_prompt(
        &self,
        gap: &CoverageGap,
        attempt: &SynthAttempt,
        result: &TestResult,
        history: &GapHistory,
    ) -> Option<LlmMessage>;

    /// Maximum attempts this strategy should get before yielding
    /// to the next strategy in the pipeline.
    fn max_attempts(&self) -> u32 {
        3
    }

    /// Priority weight (higher = tried earlier). Default strategies
    /// like CoverUp get 100; specialized ones get higher values
    /// only when their `can_handle` prerequisites are met.
    fn priority(&self, gap: &CoverageGap, history: &GapHistory) -> u32 {
        100
    }
}
```

#### `RepairPolicy` trait

```rust
/// Classifies test failures and decides how to repair them.
///
/// Implements the co-evolutionary insight from TestART/YATE:
/// different error types need different repair strategies.
pub trait RepairPolicy: Send + Sync {
    /// Classify the error from a test attempt.
    fn classify(&self, error: &str, test_code: &str) -> ErrorClass;

    /// Build a repair-specific prompt for the classified error.
    fn repair_prompt(
        &self,
        error_class: &ErrorClass,
        attempt: &SynthAttempt,
        gap: &CoverageGap,
    ) -> LlmMessage;

    /// Whether to retry with repair or abandon this attempt.
    fn should_repair(&self, error_class: &ErrorClass, attempt_number: u32) -> bool;
}

/// Classification of test failure types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorClass {
    /// Compilation/parse error — fix syntax or imports.
    CompileError { message: String },
    /// Runtime error — fix logic, mocking, or setup.
    RuntimeError { message: String, exception_type: String },
    /// Assertion failure — test runs but asserts wrong values.
    /// This often means the test is *close* and the oracle needs adjustment.
    AssertionError { message: String },
    /// Timeout — test hangs, likely infinite loop or deadlock.
    Timeout,
    /// Unknown — cannot classify.
    Unknown(String),
}
```

#### `GapHistory` struct

```rust
/// Accumulated history for a single coverage gap across all
/// strategy attempts. Enables counter-example feedback (TELPA)
/// and co-evolutionary repair (TestART/YATE).
#[derive(Debug, Clone, Default)]
pub struct GapHistory {
    /// All prior attempts at this gap, across all strategies.
    pub attempts: Vec<HistoricalAttempt>,
    /// Number of consecutive rounds with no coverage progress.
    pub stall_count: u32,
    /// Strategies already tried (by name).
    pub tried_strategies: Vec<String>,
    /// Source segment as it evolves (for elimination strategy).
    pub current_segment: Option<String>,
    /// Covered line ranges (updated after each successful test).
    pub covered_lines: Vec<u32>,
    /// Inter-procedural dependency chain (populated lazily from CPG).
    pub dependency_chain: Option<String>,
    /// CFG paths through the target function (populated lazily).
    pub cfg_paths: Option<Vec<CfgPath>>,
    /// Formal constraints that solvers failed on (for NL fallback).
    pub failed_constraints: Vec<String>,
}

/// A single historical attempt, richer than SynthAttempt.
#[derive(Debug, Clone)]
pub struct HistoricalAttempt {
    pub strategy_name: String,
    pub attempt: SynthAttempt,
    pub result: TestResult,
    pub error_class: Option<ErrorClass>,
}

/// A single path through the CFG for path-enumeration prompting.
#[derive(Debug, Clone)]
pub struct CfgPath {
    /// Human-readable description of the path conditions.
    pub condition_summary: String,
    /// Line numbers traversed.
    pub lines: Vec<u32>,
    /// Whether this path has already been covered.
    pub covered: bool,
}
```

#### `SynthPipeline` orchestrator

```rust
/// Orchestrates gap-filling by routing each gap through a
/// sequence of PromptStrategy implementations.
///
/// The pipeline:
/// 1. Classifies the gap (simple, complex, hard-branch, path-dense).
/// 2. Selects candidate strategies based on classification + history.
/// 3. Tries strategies in priority order until one succeeds or all exhaust.
/// 4. Updates GapHistory after each attempt for future rounds.
pub struct SynthPipeline {
    strategies: Vec<Box<dyn PromptStrategy>>,
    repair_policy: Box<dyn RepairPolicy>,
    config: LlmConfig,
    /// Per-gap history, keyed by (file_path, target_line).
    histories: HashMap<(String, u32), GapHistory>,
}

impl SynthPipeline {
    pub fn new(config: LlmConfig) -> Self;

    /// Register a prompt strategy.
    pub fn add_strategy(&mut self, strategy: Box<dyn PromptStrategy>);

    /// Set the repair policy (default: ClassifiedRepairPolicy).
    pub fn set_repair_policy(&mut self, policy: Box<dyn RepairPolicy>);

    /// Fill a single gap. Tries strategies in priority order.
    /// This is the main entry point, replacing direct LlmSynthesizer::fill_gap calls.
    pub fn fill_gap<F, G>(
        &mut self,
        gap: &CoverageGap,
        llm_call: F,
        run_test: G,
    ) -> Result<Option<SynthAttempt>>
    where
        F: Fn(&[LlmMessage]) -> Result<String>,
        G: Fn(&str) -> TestResult;

    /// Fill multiple gaps with shared history tracking.
    pub fn fill_gaps<F, G>(
        &mut self,
        gaps: &[CoverageGap],
        llm_call: F,
        run_test: G,
    ) -> Result<Vec<(CoverageGap, Option<SynthAttempt>)>>
    where
        F: Fn(&[LlmMessage]) -> Result<String>,
        G: Fn(&str) -> TestResult;

    /// Get history for a gap (for inspection/testing).
    pub fn history(&self, file: &str, line: u32) -> Option<&GapHistory>;
}
```

### File: `crates/apex-synth/src/classify.rs` (new)

```rust
/// Gap classification for strategy routing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GapClass {
    /// Simple gap: few uncovered lines, shallow control flow.
    Simple,
    /// Complex method: many branches, deep nesting.
    ComplexMethod { branch_count: u32 },
    /// Hard branch: has been attempted before without success.
    HardBranch { stall_count: u32 },
    /// Path-dense: function has many distinct execution paths.
    PathDense { path_count: u32 },
    /// Constraint-blocked: formal solver failed on this branch.
    ConstraintBlocked,
}

/// Classifies a gap based on its characteristics and history.
pub fn classify_gap(gap: &CoverageGap, history: &GapHistory) -> GapClass;
```

---

## Technique 1: Code Elimination from Prompts

**Paper:** Xu et al. 2026, arXiv:2602.21997
**Effort:** Low (1-2 days)
**Insight:** Remove already-covered code from the prompt entirely, rather than highlighting uncovered code. Reduces prompt size and eliminates distraction.

### New types

```rust
/// Configuration for code elimination.
pub struct EliminationConfig {
    /// Minimum coverage % before switching to elimination.
    /// Below this, the full source is more useful for context.
    pub min_coverage_for_elimination: f64,  // default: 0.3
    /// Whether to keep function signatures of eliminated code as stubs.
    pub keep_stubs: bool,  // default: true
}
```

### Files to create/modify

| File | Action | Description |
|------|--------|-------------|
| `src/eliminate.rs` | Create | `EliminationStrategy` implementing `PromptStrategy`, plus `eliminate_covered_lines()` function |
| `src/segment.rs` | Modify | Add `strip_lines(source, covered_ranges) -> String` helper |
| `src/lib.rs` | Modify | Add `pub mod eliminate;` |

### Implementation

`eliminate_covered_lines(source: &str, covered: &[u32], keep_stubs: bool) -> String`

1. Parse source into lines.
2. For each contiguous range of covered lines, either remove entirely or replace with a stub comment (`// ... N covered lines ...`).
3. If `keep_stubs`, retain function/class signatures at the boundary of removed regions.
4. Return the reduced source.

`EliminationStrategy` implements `PromptStrategy`:
- `can_handle`: returns true when `history.covered_lines` covers > 30% of the segment.
- `initial_prompt`: calls `eliminate_covered_lines` on `gap.source_segment`, then builds the standard CoverUp prompt with the reduced source.
- `feedback_prompt`: same as CoverUp feedback but on reduced source.
- `priority`: 150 (tried before CoverUp when applicable).

### Integration points

- Reads `history.covered_lines` (populated by `SynthPipeline` after each round).
- Uses `extract_segment` from `segment.rs` as the base source.

### Test strategy

- Unit test: `eliminate_covered_lines` with various coverage patterns (no coverage, partial, all-but-one-line).
- Unit test: `EliminationStrategy::can_handle` threshold behavior.
- Unit test: prompt output contains only uncovered code plus stubs.
- Property test: eliminated prompt is always shorter than or equal to original.

---

## Technique 2: Counter-Example Feedback

**Paper:** TELPA, arXiv:2404.04966
**Effort:** Medium (3-4 days)
**Insight:** After N failed attempts, include the failed test code in the prompt as "counter-examples" — things that did not work. Also include backward/forward method-invocation context.

### New types

```rust
/// Configuration for counter-example feedback.
pub struct CounterExampleConfig {
    /// Number of stalled rounds before switching to counter-example mode.
    pub stall_threshold: u32,  // default: 2
    /// Maximum number of counter-examples to include in prompt.
    pub max_counter_examples: usize,  // default: 3
    /// Whether to include inter-procedural dependency chains.
    pub include_dependencies: bool,  // default: true
}
```

### Files to create/modify

| File | Action | Description |
|------|--------|-------------|
| `src/counter_example.rs` | Create | `CounterExampleStrategy` implementing `PromptStrategy` |
| `src/lib.rs` | Modify | Add `pub mod counter_example;` |

### Implementation

`CounterExampleStrategy` implements `PromptStrategy`:
- `can_handle`: returns true when `history.stall_count >= stall_threshold`.
- `initial_prompt`: builds a prompt containing:
  1. The source segment (possibly already eliminated).
  2. A "Previous Attempts" section listing up to `max_counter_examples` failed test codes with their error messages.
  3. An optional "Dependencies" section with the inter-procedural call chain from `history.dependency_chain`.
  4. Instruction: "The following test attempts did NOT cover the target. Study why they failed and try a different approach."
- `feedback_prompt`: appends the latest failure as another counter-example.
- `priority`: 200 (higher than elimination, only activates after stalls).

### Integration points

- Reads `history.attempts` for failed test code.
- Reads `history.dependency_chain` (populated by caller from `apex-cpg` data).
- The `SynthPipeline` must increment `history.stall_count` on `NoCoverageGain` results.

### Test strategy

- Unit test: `can_handle` returns false when stall_count < threshold, true when >=.
- Unit test: prompt includes exactly N counter-examples (not more than max).
- Unit test: prompt includes dependency chain when available, omits section when None.
- Unit test: counter-examples are ordered newest-first (most informative).

---

## Technique 3: Method Slicing

**Paper:** HITS, arXiv:2408.11324
**Effort:** Medium (4-5 days)
**Insight:** Decompose complex methods into logically coherent "slices" (basic block groups with single entry/exit). Test each slice independently. Union of per-slice tests covers the whole method.

### New types

```rust
/// A slice of a method — a subset of lines forming a coherent unit.
#[derive(Debug, Clone)]
pub struct MethodSlice {
    /// Descriptive name for the slice (e.g., "validation-block", "error-path-1").
    pub name: String,
    /// Lines belonging to this slice (1-based).
    pub lines: Vec<u32>,
    /// The source code of just this slice.
    pub source: String,
    /// Lines in this slice that are uncovered.
    pub uncovered_lines: Vec<u32>,
    /// Setup code needed to reach this slice (e.g., variable initialization).
    pub preamble: String,
}

/// Configuration for method slicing.
pub struct SlicingConfig {
    /// Minimum branch count in a method to trigger slicing.
    pub complexity_threshold: u32,  // default: 5
    /// Maximum lines per slice.
    pub max_slice_lines: u32,  // default: 30
}
```

### Files to create/modify

| File | Action | Description |
|------|--------|-------------|
| `src/slice.rs` | Create | `SlicingStrategy` implementing `PromptStrategy`, plus `decompose_method()` function |
| `src/lib.rs` | Modify | Add `pub mod slice;` |

### Implementation

`decompose_method(source: &str, uncovered: &[u32], cfg_paths: Option<&[CfgPath]>) -> Vec<MethodSlice>`

1. If CFG paths are available (from `apex-mir`), use them to identify slice boundaries at branch points.
2. Otherwise, fall back to heuristic slicing: split at blank lines between top-level blocks, or at branch statements (if/match/for).
3. For each slice, extract the minimal preamble (variable declarations used in the slice but defined outside it).
4. Mark which slices contain uncovered lines.
5. Return only slices with uncovered lines.

`SlicingStrategy` implements `PromptStrategy`:
- `can_handle`: returns true when the source segment has >= `complexity_threshold` branch points (counted by simple pattern matching on `if`/`match`/`for`/`while` keywords, or from CFG data).
- `initial_prompt`: decomposes into slices, then builds a prompt focused on a single slice:
  - "Focus on this section of the method. Here is the setup needed to reach it: {preamble}. Write a test that exercises lines {uncovered} in this slice."
- `feedback_prompt`: standard error/coverage feedback scoped to the slice.
- `max_attempts`: 2 per slice (then move to next slice).
- The `SynthPipeline` iterates over slices, calling `fill_gap` per slice with a sub-gap derived from the original.

### Integration points

- Optionally reads `history.cfg_paths` from `apex-mir` CFG analysis.
- Falls back to heuristic slicing when CFG data is unavailable.
- Produces multiple sub-gaps from one parent gap; `SynthPipeline` must handle this fan-out.

### Test strategy

- Unit test: `decompose_method` on a 30-line function with 3 if-branches produces 3 slices.
- Unit test: heuristic slicing works without CFG data.
- Unit test: preamble extraction captures variables used-but-not-defined in the slice.
- Unit test: slices with no uncovered lines are filtered out.
- Integration test: end-to-end with mock LLM, gap with complex method, verify sliced prompts.

---

## Technique 4: Co-Evolutionary Generation/Repair

**Papers:** TestART (arXiv:2408.03095), YATE (arXiv:2507.18316)
**Effort:** Medium (3-4 days)
**Insight:** Interleave generation and repair. When a test fails, classify the error and use a repair-specific prompt rather than generic "fix this" feedback. Repair contributes +32% line coverage.

### New types

The `RepairPolicy` trait and `ErrorClass` enum defined in [Core Abstractions](#core-abstractions) are the primary types.

```rust
/// Default repair policy implementing TestART/YATE error classification.
pub struct ClassifiedRepairPolicy {
    /// Maximum repair attempts before abandoning.
    pub max_repairs: u32,  // default: 2
}
```

### Files to create/modify

| File | Action | Description |
|------|--------|-------------|
| `src/repair.rs` | Create | `ClassifiedRepairPolicy` implementing `RepairPolicy`, error classification logic |
| `src/strategy.rs` | Modify | `CoEvolutionaryStrategy` implementing `PromptStrategy` that wraps any inner strategy with repair |
| `src/lib.rs` | Modify | Add `pub mod repair;` |

### Implementation

`ClassifiedRepairPolicy::classify(error, test_code) -> ErrorClass`:

1. Check for compilation markers: `SyntaxError`, `IndentationError`, `rustc error`, `tsc error` -> `CompileError`.
2. Check for assertion markers: `AssertionError`, `assert!`, `assertEqual` -> `AssertionError`.
3. Check for timeout markers: `TimeoutError`, `timed out` -> `Timeout`.
4. Check for runtime markers: `NameError`, `TypeError`, `NullPointerException`, `panic` -> `RuntimeError` (extract exception type).
5. Fallback -> `Unknown`.

`ClassifiedRepairPolicy::repair_prompt(error_class, attempt, gap) -> LlmMessage`:

- `CompileError`: "The test has a compilation error: {msg}. Fix the syntax/imports without changing the test logic."
- `RuntimeError`: "The test threw {exception_type}: {msg}. The setup or mocking is likely wrong. Fix the test setup."
- `AssertionError`: "The test assertion failed: {msg}. The test logic is correct but the expected value is wrong. Update the assertion."
- `Timeout`: "The test timed out. It likely creates an infinite loop or deadlock. Simplify the test."

`CoEvolutionaryStrategy` wraps an inner `PromptStrategy`:
- On `TestResult::Error`, classifies the error and uses `repair_prompt` instead of the inner strategy's `feedback_prompt`.
- Tracks repair attempts separately from generation attempts.
- After `max_repairs` repair failures, signals the pipeline to try the next strategy.

### Integration points

- `SynthPipeline` uses `RepairPolicy` in its main loop between receiving a `TestResult::Error` and building the next prompt.
- The `CoEvolutionaryStrategy` is a decorator: it wraps any other strategy, adding repair behavior.
- `clean_error_output` from `segment.rs` is used to sanitize error messages before classification.

### Test strategy

- Unit test: `classify` correctly categorizes each error type (one test per variant).
- Unit test: `repair_prompt` produces different text for each error class.
- Unit test: `should_repair` returns false after max_repairs exceeded.
- Unit test: `CoEvolutionaryStrategy` delegates to inner strategy for initial prompt.
- Unit test: repair -> success flow (error on attempt 1, repair prompt on attempt 2, success).
- Unit test: repair -> exhaust -> next strategy flow.

---

## Technique 5: Path-Enumeration Prompting

**Paper:** SymPrompt, arXiv:2402.00097
**Effort:** Medium-High (4-5 days)
**Insight:** Enumerate execution paths through the function via CFG analysis, then generate one prompt per uncovered path. 5x improvement on some models.

### New types

`CfgPath` is defined in [Core Abstractions](#core-abstractions) as part of `GapHistory`.

```rust
/// Configuration for path-enumeration.
pub struct PathEnumConfig {
    /// Maximum paths to enumerate per function.
    pub max_paths: usize,  // default: 10
    /// Whether to include path condition formulas in the prompt.
    pub include_conditions: bool,  // default: true
}
```

### Files to create/modify

| File | Action | Description |
|------|--------|-------------|
| `src/path_enum.rs` | Create | `PathEnumStrategy` implementing `PromptStrategy`, plus `enumerate_paths()` |
| `src/lib.rs` | Modify | Add `pub mod path_enum;` |

### Implementation

`enumerate_paths(source: &str, cfg_paths: Option<&[CfgPath]>) -> Vec<CfgPath>`:

1. If CFG paths are provided (from `apex-mir`), use directly.
2. Otherwise, heuristic enumeration:
   a. Parse the source for branch statements (if/elif/else, match arms, try/except).
   b. Enumerate combinations of taken/not-taken for each branch (up to `max_paths`).
   c. For each combination, record the condition summary and traversed lines.
3. Mark each path as covered/uncovered based on `gap.uncovered_lines`.
4. Return only uncovered paths.

`PathEnumStrategy` implements `PromptStrategy`:
- `can_handle`: returns true when the function has >= 2 branch points and CFG data is available (or heuristic detects branches).
- `initial_prompt`: selects one uncovered path and builds:
  - "This function has the following execution path: {condition_summary}. This path traverses lines {lines}. Write a test that follows this specific path."
- The pipeline calls this strategy once per uncovered path (fan-out similar to slicing).
- `feedback_prompt`: "The test did not follow the intended path. The conditions {conditions} must all be true/false as specified."
- `priority`: 180 (between elimination and counter-example).

### Integration points

- Reads `history.cfg_paths` (populated from `apex-mir` or heuristic).
- The `SynthPipeline` must handle path-level fan-out: one parent gap -> N path-specific sub-invocations.
- Works best when `apex-mir` CFG data is available; degrades gracefully to heuristic.

### Test strategy

- Unit test: heuristic `enumerate_paths` on a function with 2 if-statements produces 4 paths.
- Unit test: covered paths are filtered out.
- Unit test: prompt includes the path condition summary.
- Unit test: `can_handle` returns false for straight-line code.
- Unit test: `max_paths` limits output even with many branches.

---

## Technique 6: Natural-Language Constraint Fallback

**Paper:** PALM, arXiv:2506.19287
**Effort:** Medium (3-4 days)
**Insight:** When formal constraint solvers (Z3, gradient) fail on a branch, translate the constraint to natural language and include it in the LLM prompt. The LLM can often satisfy constraints that solvers cannot (string operations, format constraints, domain logic).

### New types

```rust
/// A formal constraint that a solver failed to satisfy.
#[derive(Debug, Clone)]
pub struct FailedConstraint {
    /// The original constraint expression (e.g., "x.startswith('http') && len(x) < 256").
    pub formal: String,
    /// Natural-language translation.
    pub natural_language: String,
    /// Which solver failed.
    pub solver: String,
    /// The branch this constraint guards.
    pub target_line: u32,
}

/// Configuration for NL constraint fallback.
pub struct NlConstraintConfig {
    /// Whether to attempt LLM-based constraint translation.
    pub enabled: bool,  // default: true
}
```

### Files to create/modify

| File | Action | Description |
|------|--------|-------------|
| `src/nl_constraint.rs` | Create | `NlConstraintStrategy` implementing `PromptStrategy`, plus `translate_constraint()` |
| `src/lib.rs` | Modify | Add `pub mod nl_constraint;` |

### Implementation

`translate_constraint(formal: &str) -> String`:
- Use simple rule-based translation for common patterns:
  - `x > 0` -> "x must be a positive number"
  - `x.startswith("http")` -> "x must be a string starting with 'http'"
  - `len(x) < N` -> "x must have fewer than N characters"
- For complex constraints, fall through to an LLM call (the prompt: "Translate this formal constraint to plain English: {formal}").

`NlConstraintStrategy` implements `PromptStrategy`:
- `can_handle`: returns true when `history.failed_constraints` is non-empty.
- `initial_prompt`: builds a prompt that includes:
  1. The source segment.
  2. A "Constraints" section: "To reach line {target_line}, the following conditions must hold: {natural_language_constraints}."
  3. Instruction: "Write a test where the input satisfies all of the above constraints."
- `feedback_prompt`: "The test did not satisfy the constraint: {constraint}. Adjust the input values."
- `priority`: 250 (highest — constraint knowledge is the strongest signal when available).

### Integration points

- `history.failed_constraints` is populated by `apex-concolic` or `apex-symbolic` when their solvers fail.
- The `SynthPipeline` checks for failed constraints before trying other strategies.
- `translate_constraint` may itself need an LLM call; the pipeline must provide the `llm_call` callback.

### Test strategy

- Unit test: rule-based `translate_constraint` handles common patterns.
- Unit test: `can_handle` activates only when failed_constraints is non-empty.
- Unit test: prompt includes natural-language constraints.
- Unit test: strategy falls through gracefully when no constraints are available.

---

## Technique 7: Pluggable Prompt Pipeline

**Effort:** Medium (3-4 days)
**Insight:** Architecture to select the right strategy per-gap. This is the `SynthPipeline` orchestrator itself plus the default strategy chain.

### Default Strategy Chain

The pipeline tries strategies in this order (unless overridden by priority/can_handle):

```
NL Constraint Fallback (250)   -- if solver failed, we have the strongest signal
    |
    v
Counter-Example (200)          -- if stalled, try what hasn't been tried
    |
    v
Path Enumeration (180)         -- if path data available, target specific paths
    |
    v
Code Elimination (150)         -- if partial coverage, focus the prompt
    |
    v
Method Slicing (120)           -- if complex method, break it down
    |
    v
CoverUp Default (100)          -- baseline strategy
    |
    v
TestForge Agentic (90)         -- file-level fallback for tough gaps
```

### Files to create/modify

| File | Action | Description |
|------|--------|-------------|
| `src/pipeline.rs` | Create | `SynthPipeline` struct and orchestration logic |
| `src/coverup.rs` | Create | Extract current `LlmSynthesizer` prompt logic into `CoverUpStrategy` implementing `PromptStrategy` |
| `src/llm.rs` | Modify | `LlmSynthesizer::fill_gap` becomes a thin wrapper around `SynthPipeline::fill_gap` for backward compatibility |
| `src/lib.rs` | Modify | Add `pub mod pipeline; pub mod coverup;`, re-export `SynthPipeline` |

### Implementation

`SynthPipeline::fill_gap(gap, llm_call, run_test)`:

```
1. key = (gap.file_path, gap.target_line)
2. history = self.histories.entry(key).or_default()
3. candidates = self.strategies
       .iter()
       .filter(|s| s.can_handle(gap, history))
       .sorted_by(|a, b| b.priority(gap, history).cmp(&a.priority(gap, history)))
4. for strategy in candidates:
   a. if strategy.name() in history.tried_strategies && history.stall_count < 2:
        continue  // don't retry a strategy that already failed unless stalled
   b. messages = strategy.initial_prompt(gap, history, &self.config)
   c. for attempt in 1..=strategy.max_attempts():
      i.   test_code = llm_call(&messages)?
      ii.  messages.push(assistant(test_code))
      iii. result = run_test(&test_code)
      iv.  record attempt in history
      v.   match result:
           Success => return Ok(Some(attempt))
           Error(e) =>
             error_class = self.repair_policy.classify(&e, &test_code)
             if self.repair_policy.should_repair(&error_class, attempt):
               messages.push(self.repair_policy.repair_prompt(&error_class, ...))
             else:
               messages.push(strategy.feedback_prompt(...) or break)
           NoCoverageGain =>
             history.stall_count += 1
             messages.push(strategy.feedback_prompt(...) or break)
   d. history.tried_strategies.push(strategy.name().to_string())
5. Ok(None)  // all strategies exhausted
```

### Backward compatibility

`LlmSynthesizer::fill_gap` is preserved as-is but internally creates a single-strategy `SynthPipeline` with `CoverUpStrategy`. Existing callers and tests continue to work without modification.

### Test strategy

- Unit test: pipeline tries strategies in priority order.
- Unit test: pipeline skips strategies where `can_handle` returns false.
- Unit test: pipeline falls through to next strategy after max_attempts exhausted.
- Unit test: history accumulates across multiple `fill_gap` calls for the same gap.
- Unit test: backward-compatible `LlmSynthesizer::fill_gap` still passes all existing tests.

---

## Technique 8: Agentic File-Level Generation

**Paper:** TestForge, arXiv:2503.14713
**Effort:** High (5-7 days)
**Insight:** Multi-agent approach: a planner agent analyzes the file and creates a test plan, a generator produces tests per plan item, a validator checks them. Better for "greenfield" test files where no tests exist yet.

### New types

```rust
/// A test plan produced by the planner agent.
#[derive(Debug, Clone)]
pub struct TestPlan {
    /// File under test.
    pub target_file: String,
    /// Individual test objectives.
    pub objectives: Vec<TestObjective>,
}

/// A single test objective from the planner.
#[derive(Debug, Clone)]
pub struct TestObjective {
    /// What this test should verify.
    pub description: String,
    /// Target lines/branches.
    pub target_lines: Vec<u32>,
    /// Suggested approach (e.g., "mock the database", "pass None for x").
    pub approach: String,
    /// Priority (higher = more valuable coverage).
    pub priority: u32,
}

/// Roles in the agentic pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRole {
    Planner,
    Generator,
    Validator,
}

/// Configuration for agentic generation.
pub struct AgenticConfig {
    /// Maximum objectives per plan.
    pub max_objectives: usize,  // default: 10
    /// Whether the validator agent re-checks generated tests.
    pub enable_validator: bool,  // default: true
}
```

### Files to create/modify

| File | Action | Description |
|------|--------|-------------|
| `src/agentic.rs` | Create | `AgenticStrategy` implementing `PromptStrategy`, plus planner/generator/validator prompts |
| `src/lib.rs` | Modify | Add `pub mod agentic;` |

### Implementation

`AgenticStrategy` implements `PromptStrategy`:
- `can_handle`: returns true when the gap represents a file with no existing tests (all lines uncovered), or when all other strategies have been tried.
- `initial_prompt`: runs a two-phase prompt:
  1. **Planner prompt**: "Analyze this source file and create a test plan. For each function, identify what inputs would exercise uncovered branches. Output a numbered list of test objectives."
  2. Parse the planner response into `TestPlan`.
  3. **Generator prompt**: For each objective, "Write a test that achieves: {objective.description}. Target lines: {lines}. Approach: {approach}."
- `feedback_prompt`: "The generated test for objective {N} failed: {error}. Revise the test."
- `priority`: 90 (fallback, tried last).

The agentic flow requires multiple LLM calls per gap (planner + N generators + optional validator). The `fill_gap` callback model supports this because each call is just another invocation of `llm_call`.

Implementation detail: the planner and generator use different system prompts but share the same `llm_call` callback. The strategy internally manages the multi-turn conversation.

### Integration points

- The `SynthPipeline` treats this as any other strategy. The multi-agent logic is encapsulated inside `AgenticStrategy`.
- Validator phase optionally calls `run_test` between generator and final output.
- Works best for file-level coverage gaps; less useful for single-branch gaps.

### Test strategy

- Unit test: planner prompt includes all uncovered lines from the file.
- Unit test: generator prompt references a specific objective.
- Unit test: `can_handle` returns true for file-level gaps (all lines uncovered).
- Unit test: `can_handle` returns true when `tried_strategies` contains all other strategy names.
- Unit test: multi-turn conversation accumulates planner -> generator -> validator messages.
- Unit test: validator feedback is incorporated before final output.

---

## Build Sequence

The techniques have dependency relationships. Build order:

```
Phase A: Core Abstractions (2-3 days)
  |
  |-- strategy.rs   (PromptStrategy, RepairPolicy, ErrorClass, GapHistory)
  |-- classify.rs   (GapClass, classify_gap)
  |-- coverup.rs    (extract existing CoverUp logic into CoverUpStrategy)
  |-- pipeline.rs   (SynthPipeline orchestrator)
  |
  +-- All existing tests must still pass at this point.
      LlmSynthesizer::fill_gap wraps SynthPipeline internally.

Phase B: Simple Strategies (3-4 days, parallelizable)
  |
  |-- eliminate.rs        (Technique 1 — no external deps)
  |-- repair.rs           (Technique 4 — no external deps)
  |
  +-- These two are independent and can be built in parallel.

Phase C: History-Dependent Strategies (4-5 days, parallelizable after Phase A)
  |
  |-- counter_example.rs  (Technique 2 — needs GapHistory)
  |-- nl_constraint.rs    (Technique 6 — needs GapHistory.failed_constraints)
  |
  +-- These need Phase A but not Phase B.

Phase D: CFG-Dependent Strategies (5-6 days, needs Phase A)
  |
  |-- slice.rs            (Technique 3 — needs CfgPath, heuristic fallback)
  |-- path_enum.rs        (Technique 5 — needs CfgPath, heuristic fallback)
  |
  +-- These share the CfgPath type. Build slice.rs first since path_enum
      builds on similar decomposition logic.

Phase E: Agentic + Integration (5-7 days, needs Phase A)
  |
  |-- agentic.rs          (Technique 8 — needs pipeline infrastructure)
  |-- Pipeline integration testing with all strategies registered
  |
  +-- This is the most complex single technique and benefits from
      having all other strategies available as reference.

Total estimated: 4-6 weeks (one developer), 2-3 weeks (two developers
working in parallel on Phases B/C/D).
```

### Dependency Graph

```
strategy.rs ──┬── coverup.rs ──── pipeline.rs
              |                      |
              ├── eliminate.rs ──────┤
              ├── repair.rs ────────┤
              ├── counter_example.rs┤
              ├── nl_constraint.rs──┤
              ├── slice.rs ─────────┤
              ├── path_enum.rs ─────┤
              └── agentic.rs ───────┘
```

---

## Data Flow

### Single Gap: End-to-End

```
CoverageGap                  External Data
    |                             |
    v                             v
SynthPipeline.fill_gap()     (coverage data, CFG, constraints)
    |                             |
    +-------- classify_gap -------+
    |              |
    |         GapClass::HardBranch
    |              |
    |         select strategies: [NlConstraint, CounterExample, Elimination, CoverUp]
    |              |
    |         filter by can_handle: [CounterExample, CoverUp]
    |              |
    |         sort by priority: [CounterExample(200), CoverUp(100)]
    |              |
    |         try CounterExampleStrategy:
    |              |
    |              +-- initial_prompt(gap, history) -> messages
    |              |       includes 2 failed attempts from history
    |              |       includes dependency chain
    |              |
    |              +-- llm_call(messages) -> test_code
    |              +-- run_test(test_code) -> Error("NameError")
    |              |
    |              +-- repair_policy.classify("NameError") -> RuntimeError
    |              +-- repair_policy.repair_prompt(RuntimeError) -> repair_msg
    |              +-- llm_call([...messages, repair_msg]) -> fixed_test
    |              +-- run_test(fixed_test) -> Success([branch_42])
    |              |
    |              +-- record in history, return SynthAttempt
    |
    v
SynthAttempt { test_code, coverage_delta: [branch_42], attempt: 2 }
```

### Multi-Gap Batch Flow

```
Vec<CoverageGap>
    |
    v
SynthPipeline.fill_gaps()
    |
    +-- for each gap:
    |       |
    |       +-- fill_gap(gap) with shared history map
    |       |       history persists across gaps in same file
    |       |
    |       +-- if gap has CfgPaths, may fan out:
    |       |       gap -> [slice_1, slice_2, slice_3]
    |       |       fill_gap(slice_1), fill_gap(slice_2), ...
    |       |       merge results
    |       |
    |       +-- update covered_lines in history
    |       |       next gap in same file benefits from elimination
    |
    v
Vec<(CoverageGap, Option<SynthAttempt>)>
```

### Strategy Escalation Flow

```
Round 1: CoverUp (baseline)
    |-- Success? -> done
    |-- NoCoverageGain -> stall_count = 1
    v
Round 2: CoverUp (with missing-coverage feedback)
    |-- Success? -> done
    |-- NoCoverageGain -> stall_count = 2
    v
Round 3: Elimination (reduce prompt, fresh start)
    |-- Success? -> done
    |-- NoCoverageGain -> stall_count = 3
    v
Round 4: CounterExample (include failed attempts)
    |-- Success? -> done
    |-- Error? -> repair_policy classifies, tries repair
    |-- NoCoverageGain -> stall_count = 4
    v
Round 5: Slicing or PathEnum (decompose the problem)
    |-- Fan out to sub-gaps
    |-- Any sub-gap success? -> partial win
    v
Round 6: Agentic (file-level replanning)
    |-- Full replanning from scratch
    v
Give up on this gap (for now)
```

---

## Cargo.toml Changes

No new external dependencies are required for the core implementation. The existing deps (`serde`, `tracing`, `apex-core`) suffice. If CFG data integration from `apex-mir` is added later, it would come through `apex-core` types, not a direct dependency.

```toml
# No changes needed for Phase A-E.
# Optional future addition for heuristic CFG parsing:
# tree-sitter = { version = "0.24", optional = true }
# tree-sitter-python = { version = "0.23", optional = true }
```

---

## Summary of New Files

| File | Lines (est.) | Phase | Technique(s) |
|------|-------------|-------|---------------|
| `src/strategy.rs` | 150 | A | Core traits: PromptStrategy, RepairPolicy, GapHistory |
| `src/classify.rs` | 60 | A | GapClass, classify_gap |
| `src/coverup.rs` | 120 | A | CoverUpStrategy (extracted from llm.rs) |
| `src/pipeline.rs` | 200 | A | SynthPipeline orchestrator |
| `src/eliminate.rs` | 120 | B | Code Elimination |
| `src/repair.rs` | 150 | B | ClassifiedRepairPolicy, CoEvolutionaryStrategy |
| `src/counter_example.rs` | 130 | C | CounterExampleStrategy |
| `src/nl_constraint.rs` | 130 | C | NlConstraintStrategy |
| `src/slice.rs` | 180 | D | SlicingStrategy, decompose_method |
| `src/path_enum.rs` | 160 | D | PathEnumStrategy, enumerate_paths |
| `src/agentic.rs` | 200 | E | AgenticStrategy, TestPlan, multi-agent flow |
| **Total** | **~1600** | | |

---

## Risk Mitigation

1. **Backward compatibility**: `LlmSynthesizer::fill_gap` signature is preserved. Existing callers and all 12 tests in `llm.rs` continue to pass unchanged throughout all phases.

2. **No CFG data yet**: Techniques 3 and 5 have heuristic fallbacks that work without `apex-mir` CFG data. They degrade gracefully rather than failing.

3. **No external LLM in tests**: All strategies are tested via the same callback pattern (`llm_call`, `run_test`) used today. No real LLM calls in unit tests.

4. **Incremental value**: Each phase delivers independently useful strategies. Phase A alone improves the architecture. Phase B adds value without requiring Phases C-E.

5. **Prompt size**: Elimination (Technique 1) directly counteracts prompt bloat from counter-examples (Technique 2) and path enumeration (Technique 5). The pipeline naturally balances these.
