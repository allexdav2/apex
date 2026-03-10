# APEX SDLC Intelligence — Design Spec

## Overview

Extend APEX from a coverage-driven test generator into a full SDLC intelligence platform. Instrumented binaries produce runtime data (branch coverage, execution traces, timing) that powers tools across testing, code review, documentation, linting, security, and CI/CD.

## Architecture

Six packs, layered by dependency:

```
Pack F: Security Analysis ──────────────────┐
Pack E: Doc Generation ─────────────────────┤
Pack D: Behavioral Analysis ────────────────┤
Pack C: Source Intelligence ────────────────┤
Pack B: Test Intelligence ──────────────────┤
Pack A: Foundation (index + trace store) ───┘
```

All packs consume the **Branch Index** — a persistent per-test branch mapping built by `apex index`.

---

## Pack A: Foundation

### New Crate: `apex-index`

Persistent store mapping tests to the branches they exercise.

#### Core Types

```rust
/// A single test's branch footprint
pub struct TestTrace {
    pub test_name: String,
    pub branches: Vec<BranchId>,
    pub duration_ms: u64,
    pub status: ExecutionStatus,
}

/// Aggregate branch profile
pub struct BranchProfile {
    pub branch: BranchId,
    pub hit_count: u64,           // total hits across all tests
    pub test_count: usize,        // how many tests reach this branch
    pub test_names: Vec<String>,  // which tests
}

/// The full index
pub struct BranchIndex {
    pub traces: Vec<TestTrace>,
    pub profiles: HashMap<BranchId, BranchProfile>,
    pub file_paths: HashMap<u64, PathBuf>,
    pub total_branches: usize,
    pub covered_branches: usize,
    pub created_at: String,       // ISO 8601
    pub language: Language,
    pub target_root: PathBuf,
}
```

#### Building the Index

**Python**: Run `pytest --collect-only` to enumerate tests, then run each test individually under `coverage.py --branch`. Parse per-test coverage JSON.

**Rust**: Run `cargo test -- --list` to enumerate, then run each test under SanitizerCoverage. Parse per-test bitmap.

**General pattern**: enumerate tests → run each in isolation with instrumentation → collect branch sets → merge into index.

#### Persistence

Store as `.apex/index.json` in project root. JSON for debuggability. Includes a content hash of source files so stale indexes can be detected.

#### CLI

```
apex index --target . --lang python [--parallel N]
```

Builds the index. Required before any intelligence command. Other commands auto-detect and use the index.

---

## Pack B: Test Intelligence

### `apex test-optimize`

Find minimal test subset that maintains current coverage.

**Algorithm**: Greedy weighted set cover.
1. Load index
2. While uncovered branches remain:
   a. Pick test covering most uncovered branches (break ties by shortest duration)
   b. Add to result set, mark its branches covered
3. Output: minimal set + estimated speedup

**Output**:
```
Minimal covering set: 612 / 3000 tests
Estimated speedup: 4.7x
Redundant tests: 2388

Essential tests (cover unique branches):
  test_auth_edge_case — uniquely covers auth.py:42 false-branch
  ...
```

### `apex test-prioritize`

Order tests by relevance to changed files.

**Algorithm**:
1. Load index + `--changed-files` list
2. Compute changed branches: all branches in changed files
3. Score each test: |test.branches ∩ changed_branches|
4. Sort descending, output ordered list

**Output**: Ordered test list, suitable for piping to test runner.

### `apex flaky-detect`

Detect nondeterministic tests via path divergence.

**Algorithm**:
1. Run each test N times (default 5) with instrumentation
2. For each test, compare branch sets across runs
3. If sets differ → flaky. Report divergent branches.

**Output**:
```
Flaky tests (2 of 847):
  test_concurrent_write — branches diverge at db.py:142
    Run 1: {db.py:142 true}  Run 2: {db.py:142 false}
  test_timeout — branches diverge at net.py:67
```

---

## Pack C: Source Intelligence

### `apex dead-code`

Find semantically unreachable code.

**Algorithm**:
1. Load index
2. Branches with 0 hits across all tests + fuzz = dead candidates
3. Optionally run concolic solver to prove unreachability
4. Classify: "untested" vs "likely dead" vs "provably dead"

**Output**: File-annotated list of dead branches with confidence levels.

### `apex lint`

Overlay branch frequency on static analysis findings.

**Algorithm**:
1. Run existing detectors (apex-detect pipeline)
2. Load index
3. For each finding, look up branch frequency at that location
4. Re-prioritize: hot-path findings → critical, dead-path findings → low

**Output**: Findings sorted by runtime-informed severity.

### `apex complexity`

Exercised vs static complexity per function.

**Algorithm**:
1. Load index
2. Group branches by file + enclosing function (use `extract_enclosing_function`)
3. Static complexity = total branches in function
4. Exercised complexity = branches with hit_count > 0
5. Flag functions where exercised << static (over-engineered or under-tested)

---

## Pack D: Behavioral Analysis

### `apex diff --base <git-ref>`

Behavioral diff between two states.

**Algorithm**:
1. Build index on current HEAD
2. Build index on base ref (checkout in temp worktree)
3. For each test present in both:
   a. Compare branch sets
   b. Report: added paths, removed paths, unchanged
4. Report new tests (no comparison) and removed tests

**Output**:
```
Behavioral changes (3 of 847 tests):
  test_login: +1 branch (rate_limiter.py:23 true), -0 branches
  test_export: path diverged at csv.py:42 (true→false)
  ...

47 new branches introduced, 31 covered by existing tests.
```

### `apex regression-check --base <git-ref>`

CI gate: fail if unexpected behavioral changes.

Same as `apex diff` but returns exit code 1 on unexpected divergences. Supports `--allow <pattern>` for expected changes.

---

## Pack E: Documentation Generation

### `apex docs --target <path>`

Generate behavioral documentation from traces.

**Algorithm**:
1. Load index
2. For each function, enumerate distinct paths (unique branch sets)
3. For each path, find a representative test (shortest, simplest)
4. Extract input/output from test source
5. Generate markdown: function signature, paths, examples

### `apex contracts --target <path>`

Mine invariants from traces.

**Algorithm**:
1. Load index
2. For each branch, compute: "always taken", "never taken", "conditional"
3. For "always taken" branches: formulate as invariant
   e.g., "when input list is non-empty, branch at line 42 is always true"
4. Confidence = test_count / total_tests_reaching_function

---

## Pack F: Security Analysis

### `apex attack-surface --entry-points <pattern>`

Map reachable code from entry points.

**Algorithm**:
1. Load index
2. Filter tests matching entry-point pattern (e.g., "test_api_*")
3. Union of all branches reachable from entry-point tests = attack surface
4. Cross-reference with detector findings
5. Report: reachable findings (critical) vs unreachable (low priority)

### `apex verify-boundaries --auth-checks <pattern>`

Verify all paths from entry points pass through auth.

**Algorithm**:
1. Load index
2. For each entry-point test trace, check if auth-check branches are present
3. Report paths that reach sensitive operations without auth branches

---

## Implementation Order

1. **Pack A**: `apex-index` crate + `apex index` CLI command
2. **Pack B**: `test-optimize`, `test-prioritize`, `flaky-detect`
3. **Pack C**: `dead-code`, `lint`, `complexity`
4. **Pack D**: `diff`, `regression-check`
5. **Pack E**: `docs`, `contracts`
6. **Pack F**: `attack-surface`, `verify-boundaries`

Each pack is independently useful after Pack A ships.

---

## Non-Goals (for now)

- Production instrumentation (Pack 10 — monitoring/observability)
- AI-augmented review (requires LLM integration beyond current apex-agent)
- Performance profiling (requires sub-branch timing, not just per-execution)
- Mutation testing (requires mutation injection framework — large standalone effort)
