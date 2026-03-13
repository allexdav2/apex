# Integration Coverage Gap Closure Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the 91.4% → ~99.5% branch coverage gap by refactoring integration-layer code for testability and writing integration tests.

**Architecture:** Extract `run_cli()` from `main()` into a library function testable via `Cli::parse_from()`. Introduce `FixtureRunner` (a deterministic `CommandRunner` impl) for subprocess-dependent code. Refactor index builders to accept generic `CommandRunner`. Add `assert_cmd` binary-level tests for CLI smoke coverage.

**Tech Stack:** Rust, tokio, clap (derive), assert_cmd, mockall, tempfile

---

## File Structure

| File | Responsibility |
|------|---------------|
| `crates/apex-cli/src/lib.rs` (create) | Public `run_cli(cli: Cli)` + re-exported types |
| `crates/apex-cli/src/main.rs` (modify) | Thin shim: parse → `run_cli()` |
| `crates/apex-core/src/fixture_runner.rs` (create) | `FixtureRunner` — deterministic `CommandRunner` for tests |
| `crates/apex-core/src/lib.rs` (modify) | Re-export `fixture_runner` under `#[cfg(test)]` or feature |
| `crates/apex-index/src/rust.rs` (modify) | `RustIndexBuilder<R: CommandRunner>` |
| `crates/apex-index/src/python.rs` (modify) | `PythonIndexBuilder<R: CommandRunner>` |
| `crates/apex-cli/tests/cli_integration.rs` (create) | `assert_cmd` binary-level tests |
| `crates/apex-cli/tests/subcommand_tests.rs` (create) | `#[tokio::test]` tests via `run_cli()` |

---

## Chunk 1: Foundation — Extract `run_cli()` and `FixtureRunner`

### Task 1: Extract `run_cli()` into `apex-cli/src/lib.rs`

**Files:**
- Create: `crates/apex-cli/src/lib.rs`
- Modify: `crates/apex-cli/src/main.rs`

This is the single highest-impact change. Moving all logic out of `main()` into a library function makes 2,171 branches reachable from `#[tokio::test]`.

- [ ] **Step 1: Create `crates/apex-cli/src/lib.rs` with public types and `run_cli()`**

Move `Cli`, `Commands`, all `*Args` structs, `OutputFormat`, `LangArg`, and the `From<LangArg>` impl into `lib.rs`. Make them `pub`. Then move the body of `main()` (config loading, tracing init, command dispatch) into:

```rust
pub async fn run_cli(cli: Cli) -> color_eyre::Result<()> {
    let cfg = match &cli.config {
        Some(path) => ApexConfig::from_file(path).map_err(|e| color_eyre::eyre::eyre!("{e}"))?,
        None => ApexConfig::discover(&std::env::current_dir().unwrap_or_default()),
    };

    let log_level = cli.log_level.as_deref().unwrap_or(&cfg.logging.level);
    // Note: tracing_subscriber::fmt().init() panics if called twice.
    // Use try_init() or move tracing init to main().

    match cli.command {
        Commands::Run(args) => run(args, &cfg).await,
        Commands::Ratchet(args) => ratchet(args, &cfg).await,
        // ... all 18 arms unchanged
    }
}
```

Keep all private handler functions (`run()`, `ratchet()`, `run_audit()`, etc.) in `lib.rs` as well. Also move `mod doctor;` and `mod fuzz;` declarations.

- [ ] **Step 2: Reduce `main.rs` to thin shim**

```rust
use clap::Parser;
use color_eyre::Result;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    // Init tracing here (only once, from binary entry point)
    let cli = apex_cli::Cli::parse();

    let log_level = cli.log_level.as_deref().unwrap_or("info");
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(log_level))
        .with_writer(std::io::stderr)
        .init();

    apex_cli::run_cli(cli).await
}
```

Note: `run_cli()` must NOT call `tracing_subscriber::fmt().init()` — that stays in `main()` only. The library function should accept that tracing is already initialized.

- [ ] **Step 3: Update `Cargo.toml` to declare library target**

Add to `crates/apex-cli/Cargo.toml`:

```toml
[lib]
name = "apex_cli"
path = "src/lib.rs"
```

- [ ] **Step 4: Verify compilation**

Run: `cargo build -p apex-cli`
Expected: compiles cleanly

- [ ] **Step 5: Run full workspace tests**

Run: `cargo test --workspace`
Expected: all 2326+ tests pass, 0 failures

- [ ] **Step 6: Commit**

```bash
git add crates/apex-cli/src/lib.rs crates/apex-cli/src/main.rs crates/apex-cli/Cargo.toml
git commit -m "refactor: extract run_cli() into apex-cli library for testability"
```

---

### Task 2: Create `FixtureRunner` in apex-core

**Files:**
- Create: `crates/apex-core/src/fixture_runner.rs`
- Modify: `crates/apex-core/src/lib.rs`

A deterministic `CommandRunner` that maps command patterns to canned `CommandOutput` values. Unlike `MockCommandRunner` (which uses mockall's expect/returning), `FixtureRunner` is data-driven: you load it with `(pattern, output)` pairs and it matches against `CommandSpec.program` + args.

- [ ] **Step 1: Write test for `FixtureRunner`**

Create `crates/apex-core/src/fixture_runner.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, CommandSpec};

    #[tokio::test]
    async fn matches_exact_program() {
        let runner = FixtureRunner::new()
            .on("cargo", CommandOutput::success(b"ok".to_vec()));
        let spec = CommandSpec::new("cargo", "/tmp");
        let result = runner.run_command(&spec).await.unwrap();
        assert_eq!(result.stdout, b"ok");
    }

    #[tokio::test]
    async fn matches_program_with_args_substring() {
        let runner = FixtureRunner::new()
            .on_args("cargo", &["audit", "--json"], CommandOutput::success(b"{\"vulnerabilities\":{\"found\":0,\"list\":[]}}".to_vec()));
        let spec = CommandSpec::new("cargo", "/tmp").args(["audit", "--json"]);
        let result = runner.run_command(&spec).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn unmatched_command_returns_error() {
        let runner = FixtureRunner::new();
        let spec = CommandSpec::new("unknown", "/tmp");
        let result = runner.run_command(&spec).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Implement `FixtureRunner`**

```rust
use crate::command::{CommandOutput, CommandRunner, CommandSpec};
use crate::error::{ApexError, Result};
use async_trait::async_trait;

/// A deterministic `CommandRunner` for tests.
/// Maps (program, args) patterns to canned outputs.
pub struct FixtureRunner {
    fixtures: Vec<Fixture>,
}

struct Fixture {
    program: String,
    args: Option<Vec<String>>,
    output: CommandOutput,
}

impl FixtureRunner {
    pub fn new() -> Self {
        FixtureRunner { fixtures: Vec::new() }
    }

    /// Match any command with this program name.
    pub fn on(mut self, program: &str, output: CommandOutput) -> Self {
        self.fixtures.push(Fixture {
            program: program.into(),
            args: None,
            output,
        });
        self
    }

    /// Match commands with this program AND these args (prefix match).
    pub fn on_args(mut self, program: &str, args: &[&str], output: CommandOutput) -> Self {
        self.fixtures.push(Fixture {
            program: program.into(),
            args: Some(args.iter().map(|s| s.to_string()).collect()),
            output,
        });
        self
    }
}

#[async_trait]
impl CommandRunner for FixtureRunner {
    async fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput> {
        // Try most-specific match first (program + args), then program-only.
        for fixture in &self.fixtures {
            if fixture.program != spec.program {
                continue;
            }
            if let Some(ref expected_args) = fixture.args {
                if spec.args.len() >= expected_args.len()
                    && spec.args[..expected_args.len()] == **expected_args
                {
                    return Ok(fixture.output.clone());
                }
            } else {
                return Ok(fixture.output.clone());
            }
        }
        Err(ApexError::Detect(format!(
            "FixtureRunner: no fixture for `{} {}`",
            spec.program,
            spec.args.join(" ")
        )))
    }
}
```

- [ ] **Step 3: Re-export from `apex-core/src/lib.rs`**

Add to `crates/apex-core/src/lib.rs`:

```rust
pub mod fixture_runner;
```

This is always compiled (not `#[cfg(test)]`) so downstream test code can use it via `apex_core::fixture_runner::FixtureRunner`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-core`
Expected: all existing + 3 new `fixture_runner` tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/fixture_runner.rs crates/apex-core/src/lib.rs
git commit -m "feat: add FixtureRunner — deterministic CommandRunner for integration tests"
```

---

## Chunk 2: Refactor Index Builders + CLI Integration Tests

### Task 3: Refactor `apex-index/src/rust.rs` to `RustIndexBuilder<R>`

**Files:**
- Modify: `crates/apex-index/src/rust.rs`
- Modify: `crates/apex-index/src/lib.rs`

The current `build_rust_index()` uses direct `tokio::process::Command` calls (lines 84-92, and throughout). Refactor to a builder struct that accepts a generic `CommandRunner`.

- [ ] **Step 1: Add `apex-core` dependency to `apex-index/Cargo.toml`**

Check if it's already there. If not:
```toml
apex-core = { path = "../apex-core" }
```

- [ ] **Step 2: Create `RustIndexBuilder<R>` struct**

Wrap the existing `build_rust_index()` logic in a struct:

```rust
use apex_core::command::{CommandRunner, CommandSpec, CommandOutput};

pub struct RustIndexBuilder<R: CommandRunner> {
    runner: R,
}

impl<R: CommandRunner> RustIndexBuilder<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    pub async fn build(&self, target: &Path, parallel: usize) -> Result<BranchIndex, BoxErr> {
        // Same logic as current build_rust_index(), but replace all
        // tokio::process::Command calls with self.runner.run_command()
    }
}
```

Replace each `tokio::process::Command::new("cargo")...` with:
```rust
let spec = CommandSpec::new("cargo", &target)
    .args(["llvm-cov", "--no-report", "--workspace"])
    .env("LLVM_COV", &env.llvm_cov)
    .env("LLVM_PROFDATA", &env.llvm_profdata);
let output = self.runner.run_command(&spec).await
    .map_err(|e| format!("cargo llvm-cov: {e}"))?;
```

- [ ] **Step 3: Keep `build_rust_index()` as a convenience wrapper**

```rust
pub async fn build_rust_index(target: &Path, parallel: usize) -> Result<BranchIndex, BoxErr> {
    RustIndexBuilder::new(apex_core::command::RealCommandRunner)
        .build(target, parallel)
        .await
}
```

This preserves backward compatibility — existing callers don't change.

- [ ] **Step 4: Write tests with `FixtureRunner`**

Add tests in `rust.rs` `#[cfg(test)]` module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::fixture_runner::FixtureRunner;
    use apex_core::command::CommandOutput;

    #[tokio::test]
    async fn builder_handles_build_failure() {
        let runner = FixtureRunner::new()
            .on_args("cargo", &["llvm-cov", "--no-report"],
                CommandOutput::failure(1, b"compilation failed".to_vec()));
        let builder = RustIndexBuilder::new(runner);
        let result = builder.build(Path::new("/tmp/fake"), 1).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p apex-index`
Expected: all pass

- [ ] **Step 6: Commit**

```bash
git add crates/apex-index/src/rust.rs crates/apex-index/src/lib.rs crates/apex-index/Cargo.toml
git commit -m "refactor: RustIndexBuilder<R: CommandRunner> for testable indexing"
```

---

### Task 4: Refactor `apex-index/src/python.rs` to `PythonIndexBuilder<R>`

**Files:**
- Modify: `crates/apex-index/src/python.rs`

Same pattern as Task 3 but for Python index building. Replace direct subprocess calls with `CommandRunner`.

- [ ] **Step 1: Read `python.rs` to identify all subprocess calls**

Find all `tokio::process::Command` usages.

- [ ] **Step 2: Create `PythonIndexBuilder<R>` struct**

Same pattern: builder struct with generic `R: CommandRunner`, convenience wrapper `build_python_index()`.

- [ ] **Step 3: Write tests with `FixtureRunner`**

Test subprocess failure paths, empty output, valid output parsing.

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-index`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add crates/apex-index/src/python.rs
git commit -m "refactor: PythonIndexBuilder<R: CommandRunner> for testable indexing"
```

---

### Task 5: Add `assert_cmd` binary-level CLI tests

**Files:**
- Modify: `crates/apex-cli/Cargo.toml`
- Create: `crates/apex-cli/tests/cli_integration.rs`

Binary-level tests using `assert_cmd` to exercise the compiled `apex` binary. These cover clap parsing, help text, error messages, and the thin `main()` shim.

- [ ] **Step 1: Add dev-dependencies**

Add to `crates/apex-cli/Cargo.toml`:

```toml
[dev-dependencies]
assert_cmd = "2"
predicates = "3"
```

- [ ] **Step 2: Write integration tests**

Create `crates/apex-cli/tests/cli_integration.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn cli_help() {
    Command::cargo_bin("apex")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Autonomous Path EXploration"));
}

#[test]
fn cli_no_args_shows_help() {
    Command::cargo_bin("apex")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage"));
}

#[test]
fn cli_doctor_runs() {
    Command::cargo_bin("apex")
        .unwrap()
        .arg("doctor")
        .assert()
        .success();
}

#[test]
fn cli_run_missing_target() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["run", "--lang", "rust"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--target"));
}

#[test]
fn cli_ratchet_missing_args() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["ratchet"])
        .assert()
        .failure();
}

#[test]
fn cli_unknown_subcommand() {
    Command::cargo_bin("apex")
        .unwrap()
        .arg("nonexistent")
        .assert()
        .failure();
}

#[test]
fn cli_audit_missing_target() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["audit", "--lang", "rust"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--target"));
}
```

- [ ] **Step 3: Run integration tests**

Run: `cargo test -p apex-cli --test cli_integration`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add crates/apex-cli/Cargo.toml crates/apex-cli/tests/cli_integration.rs
git commit -m "test: add assert_cmd CLI integration tests for argument validation"
```

---

### Task 6: Write `run_cli()` subcommand handler tests

**Files:**
- Create: `crates/apex-cli/tests/subcommand_tests.rs`

Now that `run_cli()` is a library function, we can call it from `#[tokio::test]` with `Cli::parse_from()`. These tests exercise each subcommand's handler with controlled inputs (tempdir targets, mock configs).

- [ ] **Step 1: Write tests for simpler subcommands**

Start with subcommands that don't need real repos: `doctor`, `test-optimize` (empty index), `dead-code`, `complexity`, `deploy-score`.

```rust
use apex_cli::{Cli, run_cli};
use clap::Parser;
use tempfile::TempDir;

#[tokio::test]
async fn run_cli_doctor() {
    let cli = Cli::parse_from(["apex", "doctor"]);
    let result = run_cli(cli).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_deploy_score() {
    let tmp = TempDir::new().unwrap();
    // Create minimal .apex/index.json
    let apex_dir = tmp.path().join(".apex");
    std::fs::create_dir_all(&apex_dir).unwrap();
    std::fs::write(
        apex_dir.join("index.json"),
        r#"{"traces":[],"profiles":{},"file_paths":{},"total_branches":100,"covered_branches":90,"created_at":"now","language":"Rust","source_hash":"abc"}"#,
    ).unwrap();

    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from(["apex", "deploy-score", "--target", target]);
    let result = run_cli(cli).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_cli_test_optimize_empty_index() {
    let tmp = TempDir::new().unwrap();
    let apex_dir = tmp.path().join(".apex");
    std::fs::create_dir_all(&apex_dir).unwrap();
    std::fs::write(
        apex_dir.join("index.json"),
        r#"{"traces":[],"profiles":{},"file_paths":{},"total_branches":0,"covered_branches":0,"created_at":"now","language":"Rust","source_hash":"abc"}"#,
    ).unwrap();

    let target = tmp.path().to_str().unwrap();
    let cli = Cli::parse_from(["apex", "test-optimize", "--target", target]);
    let result = run_cli(cli).await;
    assert!(result.is_ok());
}
```

- [ ] **Step 2: Write tests for index-dependent subcommands with fixture data**

For `test-prioritize`, `dead-code`, `hotpaths`, `contracts`, `risk`, `complexity` — create a tempdir with a realistic `.apex/index.json` fixture.

```rust
fn write_fixture_index(dir: &std::path::Path) {
    let apex_dir = dir.join(".apex");
    std::fs::create_dir_all(&apex_dir).unwrap();
    // Minimal but valid index with 1 test, 2 branches
    std::fs::write(
        apex_dir.join("index.json"),
        include_str!("fixtures/sample_index.json"),
    ).unwrap();
}
```

Create `crates/apex-cli/tests/fixtures/sample_index.json` with a minimal valid index.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-cli`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add crates/apex-cli/tests/subcommand_tests.rs crates/apex-cli/tests/fixtures/
git commit -m "test: add run_cli() integration tests for all index-based subcommands"
```

---

## Chunk 3: Detector Refactoring + Agent Orchestrator Tests

### Task 7: Refactor subprocess-dependent detectors to accept `CommandRunner`

**Files:**
- Modify: `crates/apex-detect/src/detectors/unsafe_reach.rs`
- Modify: `crates/apex-detect/src/detectors/dep_audit.rs`
- Modify: `crates/apex-detect/src/lib.rs` (Detector trait or pipeline)

Currently `UnsafeReachabilityDetector::analyze()` and `DependencyAuditDetector::analyze()` use direct `tokio::process::Command`. The pure parsing functions (`parse_geiger_output`, `parse_cargo_audit_output`) are already well-tested. The gap is the `analyze()` method itself.

- [ ] **Step 1: Add `CommandRunner` parameter to `AnalysisContext`**

Modify `crates/apex-detect/src/context.rs`:

```rust
pub struct AnalysisContext {
    // ... existing fields ...
    pub runner: Arc<dyn CommandRunner>,
}
```

This is a breaking change within the workspace — update all constructor sites.

- [ ] **Step 2: Refactor `UnsafeReachabilityDetector::analyze()` to use `ctx.runner`**

```rust
async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
    let spec = CommandSpec::new("cargo", &ctx.target_root)
        .args(["geiger", "--output-format", "json", "--all-features"]);
    let output = ctx.runner.run_command(&spec).await
        .map_err(|e| ApexError::Detect(format!("cargo-geiger: {e}")))?;
    // ... rest unchanged, use output.stdout/stderr/exit_code
}
```

- [ ] **Step 3: Refactor `DependencyAuditDetector::analyze()` similarly**

- [ ] **Step 4: Write `analyze()` tests with `FixtureRunner`**

```rust
#[tokio::test]
async fn analyze_with_geiger_not_installed() {
    let runner = FixtureRunner::new()
        .on_args("cargo", &["geiger"],
            CommandOutput::failure(1, b"error: no such command: `geiger`".to_vec()));
    let ctx = make_test_context(runner);
    let detector = UnsafeReachabilityDetector;
    let findings = detector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty()); // gracefully skips
}
```

- [ ] **Step 5: Update all `AnalysisContext` construction sites to include `runner`**

Search workspace for `AnalysisContext {` and add `runner: Arc::new(RealCommandRunner)` at each site.

- [ ] **Step 6: Run tests**

Run: `cargo test --workspace`
Expected: all pass

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "refactor: detectors use CommandRunner via AnalysisContext for testability"
```

---

### Task 8: Add orchestrator tests with stub sandbox

**Files:**
- Modify: `crates/apex-agent/src/orchestrator.rs`

The `AgentCluster::run()` method (19 uncovered branches) needs a stub `Sandbox` and `Strategy` to test the orchestration loop.

- [ ] **Step 1: Create test helpers**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{BranchId, ExplorationContext, SeedId, Target};

    struct StubSandbox;

    #[async_trait::async_trait]
    impl Sandbox for StubSandbox {
        async fn execute(&self, _ctx: &ExplorationContext) -> apex_core::error::Result<Vec<BranchId>> {
            Ok(vec![])
        }
    }

    struct CoverNStrategy {
        branches: Vec<BranchId>,
    }

    #[async_trait::async_trait]
    impl Strategy for CoverNStrategy {
        fn name(&self) -> &str { "stub" }
        async fn next_input(&self, _ctx: &ExplorationContext) -> apex_core::error::Result<Option<Vec<u8>>> {
            Ok(Some(vec![0]))
        }
    }
}
```

- [ ] **Step 2: Write orchestrator tests**

```rust
#[tokio::test]
async fn orchestrator_terminates_at_coverage_target() {
    let oracle = Arc::new(CoverageOracle::new());
    let branch = BranchId { file_id: 1, line: 1, col: 0, direction: 0 };
    oracle.register_branches(std::iter::once(branch.clone()));
    // Pre-cover the only branch → 100%
    oracle.mark_covered(&branch, SeedId::new());

    let target = Target { root: PathBuf::from("/tmp"), language: Language::Rust, test_command: vec![] };
    let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), target)
        .with_config(OrchestratorConfig { coverage_target: 1.0, ..Default::default() });

    let result = cluster.run().await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn orchestrator_stalls_when_no_progress() {
    let oracle = Arc::new(CoverageOracle::new());
    let branch = BranchId { file_id: 1, line: 1, col: 0, direction: 0 };
    oracle.register_branches(std::iter::once(branch));
    // Don't cover it — stuck at 0%

    let target = Target { root: PathBuf::from("/tmp"), language: Language::Rust, test_command: vec![] };
    let cluster = AgentCluster::new(oracle, Arc::new(StubSandbox), target)
        .with_config(OrchestratorConfig {
            coverage_target: 1.0,
            stall_threshold: 2,
            deadline_secs: Some(1),
        });

    let result = cluster.run().await;
    assert!(result.is_ok()); // should terminate gracefully on stall/deadline
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-agent`
Expected: all pass

- [ ] **Step 4: Commit**

```bash
git add crates/apex-agent/src/orchestrator.rs
git commit -m "test: add orchestrator tests with stub sandbox and strategy"
```

---

## Chunk 4: Final Verification

### Task 9: Full workspace verification and coverage measurement

**Files:** None (verification only)

- [ ] **Step 1: Run full test suite**

```bash
cargo test --workspace
```
Expected: all tests pass, 0 failures

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace -- -D warnings
```
Expected: no warnings

- [ ] **Step 3: Measure coverage**

```bash
cargo run --bin apex --manifest-path /Users/ad/prj/bcov/Cargo.toml -- \
  run --target /Users/ad/prj/bcov --lang rust --strategy agent \
  --output-format json 2>/dev/null | jq '.summary'
```

Expected: coverage significantly above 91.4%, target ~95-99%

- [ ] **Step 4: Commit any final fixes**

If tests or clippy found issues, fix and commit.
