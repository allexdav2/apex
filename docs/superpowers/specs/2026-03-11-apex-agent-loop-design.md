# APEX Agent Loop + Visualization Redesign

**Date:** 2026-03-11
**Status:** Approved

## Problem

APEX's internal agent orchestrator (`AgentCluster`) uses byte-level fuzz/driller strategies that produce raw byte mutations. `RustTestSandbox` expects valid Rust source code. These are fundamentally incompatible — the internal loop cannot improve coverage for source-level targets (Rust, Python, JS).

The agent loop should be Claude Code, not an internal Rust loop.

## Architecture

**APEX binary = measurement + strategy execution.** Each CLI strategy does one thing:
- `--strategy baseline` — run existing tests, report coverage
- `--strategy agent` — run baseline + output prioritized JSON gap report for external agent
- `--strategy fuzz` — coverage-guided byte-level fuzzing (C/Rust binary targets)
- `--strategy driller` — SMT-driven path exploration using concolic solver
- `--strategy concolic` — Python concolic execution with taint tracking
- `--strategy all` — combined fuzz + concolic

**Claude Code's `/apex-run` skill = the agent loop.** It orchestrates across rounds, mixing strategies:
1. **Source-level gaps** → Claude Code reads JSON gap report, writes targeted test files
2. **Binary-level gaps** → Claude Code invokes `--strategy fuzz` or `--strategy driller` for branches that need byte-level exploration
3. **Python constraint paths** → Claude Code invokes `--strategy concolic` for Python targets with complex conditionals

The agent decides per-round which strategy to use based on the gap report's `difficulty` and `suggested_approach` fields. Easy/medium gaps get source-level tests. Hard gaps with binary entry points get fuzz/driller. Python constraint paths get concolic.

**Removed:** `AgentCluster` internal orchestrator loop, `trait Agent`. The strategies themselves (`FuzzStrategy`, `DrillerStrategy`, `PythonConcolicStrategy`) stay — they're invoked via CLI, not via an internal loop.

## JSON Gap Report (`--strategy agent --output-format json`)

Each run produces:

```json
{
  "summary": {
    "total_branches": 25812,
    "covered_branches": 24340,
    "coverage_pct": 0.943,
    "files_total": 47,
    "files_fully_covered": 12
  },
  "gaps": [
    {
      "file": "crates/apex-cli/src/main.rs",
      "function": "run_agent_strategy",
      "branch_line": 248,
      "branch_condition": "match target.language { Language::Rust => ...",
      "source_context": ["line 244..258 source"],
      "uncovered_branches": 3,
      "coverage_pct": 0.82,
      "closest_existing_test": "tests/cli_basic.rs::test_run_baseline",
      "bang_for_buck": 0.87,
      "difficulty": "medium",
      "difficulty_reason": "needs mock sandbox for language dispatch",
      "suggested_approach": "Test each Language match arm with mock sandbox"
    }
  ],
  "blocked": [
    {
      "file": "crates/apex-rpc/src/worker.rs",
      "uncovered_branches": 315,
      "reason": "gRPC server required — needs integration harness"
    }
  ]
}
```

### Field definitions

- **`bang_for_buck`** (0.0–1.0): Estimates how many sibling branches a single test unlocks. Computed from branch dependency graph in the instrumented CFG.
- **`difficulty`**: `easy` (simple conditional), `medium` (needs mocks/setup), `hard` (external deps/integration), `blocked` (can't unit-test). Derived from whether the branch touches I/O, FFI, network, or external process calls.
- **`suggested_approach`**: One-line hint based on branch type and surrounding code structure.
- **`closest_existing_test`**: The test that covers the most nearby branches — helps Claude Code write incremental tests.

## Agent Loop (`/apex-run` skill)

### Flow per round

1. Run `apex run --strategy agent --output-format json --target <path> --lang <lang>`
2. Parse JSON gap report
3. Sort gaps by `bang_for_buck` descending
4. **Strategy selection per gap:**
   - `difficulty: easy/medium` → Claude Code writes a targeted test file (source-level)
   - `difficulty: hard` + `suggested_approach` mentions binary/fuzz → run `apex run --strategy fuzz --target <path>`
   - `difficulty: hard` + `suggested_approach` mentions constraints/SMT → run `apex run --strategy driller --target <path>`
   - Python target + constraint path → run `apex run --strategy concolic --target <path>`
   - `difficulty: blocked` → skip, report in blocked section
5. Re-run APEX with `--strategy agent` to measure improvement
6. Print the combined round report (shows which strategy was used per file)
7. Check breakpoints:
   - **Stall**: 0% improvement → pause, show which gaps were attempted and which strategies were used, ask user
   - **Regression**: coverage dropped → pause, show which new tests/strategy runs caused it, ask user
   - **Compile failure**: test didn't compile → auto-retry once with error message, then pause if still failing
   - **Strategy failure**: fuzz/driller/concolic crashes or times out → log, skip that gap, continue
8. If no breakpoint hit → continue to next round

### Strategy mixing example

```
Round 1: 12 easy gaps → write 12 test files (+8% coverage)
Round 2: 5 medium gaps → write 5 test files (+3% coverage)
Round 3: 3 hard gaps → run fuzz on 2, driller on 1 (+1.5% coverage)
Round 4: 2 remaining hard gaps → fuzz finds 1 new path (+0.3%)
Round 5: stall — 0% improvement, pause
```

### Termination

Coverage target reached, max rounds hit, or user stops.

## Round Report (Markdown in conversation)

After each round, Claude Code prints:

```
## Round 2/5 — Coverage: 92.0% → 94.3% (+2.3%)

███████████████████████████████████████████░░░░ 94.3%
+591 branches covered  |  1,472 remaining  |  3 tests written

### This round
+27 apex-cli/main.rs  +43 apex-agent/orchestrator.rs  +14 apex-sandbox/shim.rs

### File coverage
  ██████████ 100%  apex-core/types.rs, oracle.rs, config.rs (12 files)
  █████████░  95%  apex-fuzz/mutators.rs, corpus.rs (4 files)
  ████████░░  85%  apex-agent/orchestrator.rs ↑14%
  ████░░░░░░  82%  apex-cli/main.rs ↑7%
  ██░░░░░░░░  23%  apex-cli/fuzz.rs (needs binary target)
  █░░░░░░░░░  12%  apex-rpc/worker.rs (gRPC integration)

### Blocked files (can't unit-test — need integration harness)
  apex-rpc/worker.rs (315) — gRPC server required
  apex-cli/fuzz.rs (163) — needs compiled fuzz target binary
```

Files grouped by coverage tier. Deltas shown for files improved this round. Blocked files called out with reason and uncovered count.

## Code Changes

### Remove

- **`crates/apex-agent/src/orchestrator.rs`** — `AgentCluster`, `OrchestratorConfig`, `run_agent_cycle()`
- **`crates/apex-core/src/traits.rs`** — `trait Agent`
- **`crates/apex-cli/src/main.rs`** — `run_agent_strategy()` function and `"agent"` match arm

### Keep

- `trait Strategy` — used by fuzz/driller/concolic CLI strategies
- `trait Sandbox` — used by baseline measurement and strategy execution
- `FuzzStrategy` — invoked via `--strategy fuzz` CLI path
- `DrillerStrategy` — invoked via `--strategy driller` CLI path
- `PythonConcolicStrategy` — invoked via `--strategy concolic` CLI path
- `CoverageOracle` — core measurement infrastructure
- `apex-agent/src/source.rs` — `extract_source_contexts()` used by new JSON report

### New

- **`print_agent_json_report()`** in `main.rs` — produces the rich JSON gap report with bang-for-buck ranking, difficulty estimates, source context, and suggested approaches
- **Updated `/apex-run` skill** in `.claude/commands/apex-run.md` — becomes the agent loop (measure → analyze → write tests → re-measure → report → repeat)
- **Report formatting logic** — markdown round reports with progress bar, file heatmap, and blocked file callouts

## Design Decisions

| Decision | Choice | Alternatives considered |
|----------|--------|------------------------|
| Audience | Developer at terminal | CI pipeline |
| Architecture | APEX = measurement, Claude Code = agent loop | APEX owns loop; Cooperative (watch mode) |
| Output | Markdown-native in conversation | Browser dashboard; TUI |
| Report format | Combined (progress bar + heatmap + blocked) | Compact only; Table only |
| Existing agent code | Repurpose `--strategy agent` for JSON output | Strip entirely; Keep both paths |
| Strategy access | Agent can invoke fuzz/driller/concolic via CLI | Strategies only available standalone; Agent writes tests only |
| Loop autonomy | Autonomous with breakpoints | Fully autonomous; Confirm per round |
| JSON richness | Prioritized rich (context + ranking + hints) | Minimal; Rich without ranking |
