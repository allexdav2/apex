<!-- status: DONE -->

# Full Fleet Review -- 2026-03-16

Review-only pass. No feature work. Assessed current state of all 17 crates.

## Summary

| Metric | Value |
|--------|-------|
| Total tests | 4,926 |
| Tests passing | 4,926 (100%) |
| Tests failing | 0 |
| Tests ignored | 3 |
| Clippy warnings | 0 (all crates clean with -D warnings) |
| Bugs found (>=80 confidence) | 4 |
| Long-tail items (<80 confidence) | 8 |

## Per-Crew Results

### foundation (apex-core: 196, apex-coverage: 79, apex-mir: 141 = 416 tests)
- Tests: 416 passed, 0 failed
- Clippy: clean
- Bugs: 0
- Notes: Solid foundation. DashMap usage in CoverageOracle is correct. Mutex poisoning handled via `unwrap_or_else`. Config validation clamps values properly.

### runtime (apex-lang: 370, apex-instrument: 325, apex-sandbox: 314, apex-index: 361, apex-reach: 103 = 1,473 tests)
- Tests: 1,473 passed, 0 failed
- Clippy: clean
- Bugs: 0 (see long_tail)
- Notes: ShmBitmap unsafe code is well-structured with proper Drop cleanup. SanCov runtime FFI is inherently unsafe but documented. `unsafe impl Send for ShmBitmap` has adequate SAFETY comment.

### intelligence (apex-agent: 382, apex-synth: 172 = 554 tests)
- Tests: 554 passed, 0 failed
- Clippy: clean
- Bugs: 0
- Notes: Orchestrator loop is clean with proper stall detection and deadline handling. Mutex usage consistently uses poison-recovery pattern. Driller tests verify mutex poisoning recovery.

### exploration (apex-fuzz: 310, apex-symbolic: 236, apex-concolic: 207 = 753 tests)
- Tests: 753 passed, 0 failed
- Clippy: clean
- Bugs: 1 (see below)

### security-detect (apex-detect: 1148, apex-cpg: 205 = 1,353 tests)
- Tests: 1,353 passed, 0 failed (1 ignored)
- Clippy: clean
- Bugs: 1 (see below)
- Notes: Largest test suite. Well-tested detector framework.

### platform (apex-cli: 225, apex-rpc: 114 = 339 tests)
- Tests: 339 passed, 0 failed
- Clippy: clean
- Bugs: 0
- Notes: CLI properly canonicalizes all target paths. Single process::exit() is in main.rs only (appropriate).

### mcp-integration (apex-cli/src/mcp.rs, integrations/)
- Tests: covered by apex-cli
- Clippy: clean
- Bugs: 1 (see below)

## Bugs Found (confidence >= 80)

### BUG-1: Mutex lock without poison recovery in production code
- **Severity:** WARNING
- **Confidence:** 85
- **File:** `crates/apex-concolic/src/python.rs:166`
- **Description:** `self.trace_cache.lock().unwrap()` in `get_trace()` -- a production method on `PythonConcolicStrategy`. Every other Mutex lock in the codebase (orchestrator, driller, ledger, ensemble, exchange, oracle, cache, interceptor, firecracker) uses `.unwrap_or_else(|e| e.into_inner())` for poison recovery. This one does not, meaning if any thread panics while holding the trace_cache lock, all subsequent concolic operations will panic.
- **Fix:** Change `.lock().unwrap()` to `.lock().unwrap_or_else(|e| e.into_inner())`.

### BUG-2: Known DFS cycle-detection bug in dep_graph
- **Severity:** WARNING
- **Confidence:** 90
- **File:** `crates/apex-detect/src/dep_graph.rs:159`
- **Description:** Documented as `TODO(Bug 14)`: the `visited` set causes DFS to skip nodes already seen from different paths, potentially missing cycles reachable only via those nodes. The comment itself acknowledges the bug and notes Tarjan's SCC is the proper fix.
- **Impact:** `apex audit` dependency graph cycle detection may produce false negatives (miss real circular dependencies).

### BUG-3: MCP deploy-score handler ignores `lang` parameter
- **Severity:** LOW
- **Confidence:** 82
- **File:** `crates/apex-cli/src/mcp.rs:269-277`
- **Description:** The `DeployScoreParams` struct accepts a `lang` field, but `apex_deploy_score()` never passes it to `run_apex_command()`. The CLI's `deploy-score` subcommand doesn't require `--lang`, so the MCP tool accepts a parameter it silently ignores. External AI clients may believe they're scoping the analysis to a specific language when they're not.
- **Fix:** Either remove `lang` from `DeployScoreParams` or pass it through if deploy-score gains language-specific behavior.

### BUG-4: MCP apex_reach tool invokes wrong CLI command
- **Severity:** WARNING
- **Confidence:** 90
- **File:** `crates/apex-cli/src/mcp.rs:215-221`
- **Description:** The `apex_reach` MCP tool calls `run_apex_command(&["attack-surface", ...])` but should call `run_apex_command(&["reach", ...])`. The `ReachParams` struct describes `target` as "file:line" which matches the `reach` subcommand's expected input format, but `attack-surface` expects a directory path via `--target`. An AI client calling this tool with "src/auth.py:42" would get an error or wrong results because `attack-surface` would try to canonicalize "src/auth.py:42" as a directory.
- **Fix:** Change `"attack-surface"` to `"reach"` on line 216.

## Long Tail (confidence < 80)

### LT-1: Sandbox python.rs uses raw Command instead of CommandRunner
- **Confidence:** 70
- **File:** `crates/apex-sandbox/src/python.rs:120`
- **Description:** `TODO(security)` comment notes this should use CommandRunner trait for sandboxing/auditing. Currently spawns `python3` directly via `tokio::process::Command` without going through the abstraction layer.

### LT-2: as u64 casts on Duration::as_millis() truncation
- **Confidence:** 55
- **File:** Multiple files in apex-lang/ and apex-index/
- **Description:** `start.elapsed().as_millis() as u64` -- `as_millis()` returns u128, cast to u64. Only a problem if a single operation takes >584 million years. Theoretical, not practical.

### LT-3: Firecracker sandbox is stubbed out
- **Confidence:** 60
- **File:** `crates/apex-sandbox/src/firecracker.rs:408-474`
- **Description:** Multiple TODOs indicate the Firecracker integration is not fully wired -- `spawn`, `inject target`, `vsock I/O` are all placeholder comments. Tests verify the data structures but not actual VM operations.

### LT-4: TOCTOU race in RPC port binding
- **Confidence:** 65
- **File:** `crates/apex-rpc/src/coordinator.rs:1352`, `crates/apex-rpc/src/worker.rs:210,546,822,970`
- **Description:** Self-documented TOCTOU race -- port may be grabbed between drop and re-bind. Only affects tests (ephemeral port binding), not production.

### LT-5: JavaScript V8 coverage parsing not implemented
- **Confidence:** 60
- **File:** `crates/apex-instrument/src/javascript.rs:440`
- **Description:** `TODO: Parse V8 coverage from stdout` -- coverage data collection path for JavaScript is incomplete.

### LT-6: CPG builder only supports Python
- **Confidence:** 50
- **File:** `crates/apex-cli/src/lib.rs:820,1590,2109`
- **Description:** Three identical comments: "Build CPG for Python projects (other languages: TODO)". The code property graph (used for taint analysis) only works for Python targets.

### LT-7: 4,298-line lib.rs in apex-cli
- **Confidence:** 40
- **File:** `crates/apex-cli/src/lib.rs`
- **Description:** Single file with 4,298 lines containing all command handlers. While functional, this makes navigation and maintenance harder. Could benefit from splitting into per-command modules.

### LT-8: Orchestrator silently drops strategy errors
- **Confidence:** 60
- **File:** `crates/apex-agent/src/orchestrator.rs:122-123`
- **Description:** `.filter_map(|r| r.ok())` on strategy suggestions and sandbox results -- failed strategies are silently dropped without logging. The sandbox result filtering at line 134 similarly swallows errors.
