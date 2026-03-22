<!-- status: DONE -->

# Test Harness — Breaking the 93.6% Ceiling

Date: 2026-03-17
Goal: Cover the 5,300 uncovered lines in integration-boundary code (lib.rs, worker.rs, orchestrator.rs, mcp.rs)

## 3 Deep Digs — Bugs Hiding in Uncovered Code

### Dig 1: Silent Failure in Orchestrator Inner Loop (WRONG)

**Location:** `orchestrator.rs:117-134`

The exploration loop has two `filter_map(|r| r.ok())` calls that silently swallow errors:

```rust
// Line 118-124: strategy errors silently dropped
let suggestions: Vec<_> = join_all(self.strategies.iter().map(|s| s.suggest_inputs(&ctx)))
    .await.into_iter()
    .filter_map(|r| r.ok())    // ← silent drop
    .flatten().collect();

// Line 129-135: sandbox execution errors silently dropped
let results: Vec<_> = join_all(suggestions.iter().map(|seed| self.sandbox.run(seed)))
    .await.into_iter()
    .filter_map(|r| r.ok())    // ← silent drop
    .collect();
```

**Bug:** If the sandbox returns `Err` for EVERY execution (e.g., binary missing, permission denied), `results` is empty, `new_coverage` stays false, `stall_count` increments — but there's **zero diagnostic output**. The orchestrator silently grinds through `stall_threshold` (default 10) iterations with no indication that executions are failing. The same applies when all strategies fail (API key expired, SMT solver crashed).

**Impact:** A user running `apex run --strategy agent` with an expired ANTHROPIC_API_KEY gets a silent 10-iteration stall then "coverage stalled" with no hint that the API calls were failing.

**Fix:** Log a warning when `filter_map` drops errors. Count consecutive all-error rounds and break early with a specific message.

### Dig 2: Nonsensical Deadline Calculation (WRONG)

**Location:** `lib.rs:1036`

```rust
deadline_secs: Some(cfg.sandbox.process_timeout_ms * fuzz_iters as u64 / 1000),
```

With defaults (`process_timeout_ms = 10_000`, `fuzz_iters = 10_000`), this computes:
`10,000 × 10,000 / 1,000 = 100,000 seconds = 27.8 hours`

**Bug:** The deadline is computed as "total time if every iteration hits the timeout" — but this is not a meaningful wall-clock limit. The orchestrator should have a *maximum run time*, not a sum of worst-case per-iteration times. With custom config (`process_timeout_ms = 5000`, `fuzz_iters = 50_000`): `5,000 × 50,000 / 1,000 = 250,000 seconds = 2.9 days`.

**Impact:** The deadline never triggers in practice — it's always longer than stall detection. It's dead code masquerading as a safety net. A real deadline (e.g., 5 minutes for CI, 30 minutes for deep analysis) would be more useful.

**Fix:** Use a dedicated `deadline_secs` config field (or `max_run_secs`) instead of deriving it from timeout × iterations.

### Dig 3: Worker Test TOCTOU Race + Sleep Flake (STYLE)

**Location:** `worker.rs:208-225`

```rust
// Bind to get a free port, then release it
let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
let addr = listener.local_addr().unwrap();
drop(listener);                    // ← port released here

let (service, _handle) = CoordinatorServer::start_with_service(addr, oracle).await?;

// Give the server time to bind the port
tokio::time::sleep(Duration::from_millis(200)).await;   // ← race condition
```

**Bug 1 (TOCTOU):** Between `drop(listener)` and `start_with_service(addr)`, another process can claim the port. The TODO comment acknowledges this.

**Bug 2 (Flake):** The 200ms sleep assumes the server binds within that time. Under load (parallel test execution with nextest), this can fail. And in sandbox mode, TCP binding is blocked entirely — all 614 lines of test code are dead.

**Impact:** These tests never run in CI sandbox environments. 12% of the RPC crate's test value is lost.

**Fix:** Use Unix domain sockets (UDS). The `connect_uds()` method already exists at line 50-71. UDS doesn't require TCP and can't be port-raced.

---

## Test Harness Architecture

### Layer 1: Configurable Mocks (covers orchestrator inner loop)

**New file:** `crates/apex-agent/src/test_harness.rs`

```rust
/// Sandbox that returns prescribed results in sequence.
pub struct ScriptedSandbox {
    results: Mutex<VecDeque<Result<ExecutionResult>>>,
    fallback: ExecutionResult,
}

/// Strategy that returns prescribed seed batches.
pub struct ScriptedStrategy {
    batches: Mutex<VecDeque<Vec<InputSeed>>>,
}
```

**What it tests:**
- [x] Inner loop: suggestions → sandbox → merge → observe cycle (lines 117-164)
- [x] Bug recording: crash/wrong-result detection (lines 148-156)
- [x] Stall detection: stall_count increment and threshold break (lines 126-172)
- [x] Coverage target: early exit when target reached (lines 94-98)
- [x] Deadline: early exit on time limit (lines 99-104)
- [x] All-error handling: strategy/sandbox returning Err (Dig 1 bug)

**Estimated coverage gain:** ~200 lines in orchestrator.rs

### Layer 2: UDS-Based RPC Tests (covers worker + coordinator)

**Modify:** `crates/apex-rpc/src/worker.rs` test module

Replace `setup_worker()` TCP approach with Unix domain sockets:

```rust
async fn setup_worker_uds() -> (WorkerClient, Arc<CoordinatorService>, Arc<CoverageOracle>) {
    let tmpdir = tempfile::tempdir().unwrap();
    let uds_path = tmpdir.path().join("apex-test.sock");
    let (service, _handle) = CoordinatorServer::start_uds(&uds_path, oracle).await.unwrap();
    let worker = WorkerClient::connect_uds(&uds_path, "python".into()).await.unwrap();
    (worker, service, oracle)
}
```

**Prerequisite:** Add `CoordinatorServer::start_uds()` method (mirrors `start_with_service` but binds UDS instead of TCP).

**What it tests:**
- [x] register, heartbeat, get_seeds, submit_results, get_coverage, pull_once
- [x] All currently-skipped tests become active

**Estimated coverage gain:** ~550 lines in worker.rs + ~70 lines in coordinator.rs

### Layer 3: Fixture Project Integration Tests (covers lib.rs end-to-end)

**New file:** `crates/apex-cli/tests/integration_harness.rs`

Create minimal fixture projects:

```
tests/fixtures/
├── tiny-python/          # 3 .py files, 5 branches, pytest.ini
│   ├── main.py
│   ├── test_main.py
│   └── pytest.ini
└── tiny-rust/            # 1 lib.rs, 3 branches, Cargo.toml
    ├── Cargo.toml
    └── src/lib.rs
```

Test pattern:
```rust
#[tokio::test]
async fn run_python_baseline_produces_json_report() {
    let fixture = fixtures::tiny_python();
    let cli = Cli::parse_from(["apex", "run", "--target", fixture.path(), "--lang", "python", "--output-format", "json"]);
    let cfg = ApexConfig::default();
    let result = run_cli(cli, &cfg).await;
    assert!(result.is_ok());
    // Capture stdout, parse JSON, verify fields
}
```

**What it tests:**
- [x] install_deps dispatch (lines 1077-1140)
- [x] instrument dispatch (lines 1142-1206)
- [x] run() full pipeline (lines 723-965)
- [x] Output formatting: JSON and text (lines 908-962)
- [x] Strategy dispatch: baseline, fuzz, concolic (lines 760-815)

**Estimated coverage gain:** ~800-1000 lines in lib.rs

---

## Crew Assignments

| Wave | Task | Crew | Files | Est. Coverage Gain |
|------|------|------|-------|-------------------|
| 1 | Scripted mocks + orchestrator tests | intelligence | `orchestrator.rs`, new `test_harness.rs` | +200 lines |
| 1 | UDS RPC test infrastructure | platform | `worker.rs`, `coordinator.rs` | +620 lines |
| 2 | Fixture projects + integration tests | platform | `tests/integration_harness.rs`, `tests/fixtures/` | +800 lines |
| 2 | Fix Dig 1 (silent error) + Dig 2 (deadline) | intelligence | `orchestrator.rs`, `lib.rs` | +50 lines |

**Wave 1** is prerequisite-free — both tasks are independent.
**Wave 2** depends on Wave 1 only for the fixture tests (which use the real orchestrator).

## Expected Outcome

| Metric | Before | After |
|--------|--------|-------|
| Line coverage | 93.6% | ~96-97% |
| Uncovered lines | 5,300 | ~2,000-2,500 |
| Tests | 5,281 | ~5,400-5,450 |

The remaining ~2,000 lines after this harness would be:
- Language-specific runner branches for languages not installed (Java, Kotlin, Swift, C#)
- Error paths that require specific OS conditions (SIGKILL, OOM)
- MCP handler bodies (need MCP client test infrastructure — separate effort)
