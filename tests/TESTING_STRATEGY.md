# APEX CLI Testing Strategy: Branch Coverage for Rust Binaries

## Executive Summary

This document covers five approaches for integration-testing the `apex` CLI binary
while achieving LLVM source-based branch coverage. The recommendations are ordered
from "highest coverage yield" to "most ergonomic for regression suites."

---

## 1. Architectural Refactor: Extract `run_cli()` from `main()`

**This is the single highest-impact change.** Currently all logic lives in
`async fn main()` inside `main.rs`. Code inside `main()` of a binary crate is
only reachable via process-level tests (spawning the binary). By extracting the
core dispatch into a library-visible async function, you can call it directly from
`#[tokio::test]` and get full LLVM coverage instrumentation without process
boundaries.

### Current structure (simplified)

```rust
// crates/apex-cli/src/main.rs
#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let cli = Cli::parse();
    let cfg = load_config(&cli)?;
    init_tracing(&cli, &cfg);
    match cli.command {
        Commands::Run(args) => run(args, &cfg).await,
        Commands::Doctor => doctor::run_doctor().await,
        // ...18 more arms...
    }
}
```

### Proposed refactor

```rust
// crates/apex-cli/src/lib.rs  (new file -- makes this a mixed bin+lib crate)
pub mod doctor;
pub mod fuzz;

pub use cli::{Cli, Commands};

mod cli {
    use clap::{Parser, Subcommand};
    // ... Cli, Commands, RunArgs, etc. -- all pub(crate) or pub as needed
}

/// Core entry point, callable from tests without process spawn.
/// Takes pre-parsed CLI args so tests can construct them directly.
pub async fn run_cli(cli: Cli) -> color_eyre::Result<()> {
    let cfg = match &cli.config {
        Some(path) => ApexConfig::from_file(path)?,
        None => ApexConfig::discover(&std::env::current_dir().unwrap_or_default()),
    };
    let log_level = cli.log_level.as_deref().unwrap_or(&cfg.logging.level);
    // tracing init is idempotent or guarded
    match cli.command {
        Commands::Run(args) => run(args, &cfg).await,
        Commands::Doctor => doctor::run_doctor().await,
        // ...
    }
}
```

```rust
// crates/apex-cli/src/main.rs  (thin shim)
use apex_cli::run_cli;

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let cli = apex_cli::Cli::parse();
    run_cli(cli).await
}
```

### Testing directly

```rust
// crates/apex-cli/tests/integration.rs
use apex_cli::{Cli, run_cli};
use clap::Parser;

#[tokio::test]
async fn doctor_subcommand_succeeds() {
    let cli = Cli::parse_from(["apex", "doctor"]);
    let result = run_cli(cli).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn run_missing_target_errors() {
    let cli = Cli::parse_from(["apex", "run", "--target", "/nonexistent", "--lang", "python"]);
    let result = run_cli(cli).await;
    assert!(result.is_err());
}
```

### Why this matters for coverage

- `cargo llvm-cov --tests` instruments the library crate and runs integration
  tests in-process. Every branch in `run_cli()` and its callees is tracked.
- No `.profraw` merging headaches -- single instrumented process.
- Tests run 10-100x faster than process-spawn approaches.
- You can still use `Cli::parse_from` to exercise clap parsing logic (invalid
  args, missing required flags, value validation).

### Tradeoffs

| Pro | Con |
|-----|-----|
| Full branch coverage of dispatch logic | Requires moving structs to `pub` visibility |
| Fast test execution | `color_eyre::install()` can only be called once per process -- guard it |
| Can test with mock configs easily | Tracing subscriber init needs `try_init()` not `init()` |
| Clap parsing tested via `parse_from` | Not a true end-to-end test of the binary |

### Tracing guard pattern

```rust
use std::sync::Once;
static TRACING: Once = Once::new();

pub fn init_tracing_once(level: &str) {
    TRACING.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::new(level))
            .with_writer(std::io::stderr)
            .try_init()
            .ok();
    });
}
```

---

## 2. `assert_cmd` -- Process-Level Integration Tests

### What it is

`assert_cmd` wraps `std::process::Command` with ergonomic assertions. It spawns
your compiled binary as a child process and checks exit code, stdout, and stderr.

### Setup

```toml
# crates/apex-cli/Cargo.toml
[dev-dependencies]
assert_cmd = "2"
predicates = "3"
assert_fs = "1"   # optional, for temp file fixtures
```

### Basic patterns

```rust
// crates/apex-cli/tests/cli_smoke.rs
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn no_args_shows_help() {
    Command::cargo_bin("apex")
        .unwrap()
        .assert()
        .failure()
        .stderr(predicate::str::contains("Usage:"));
}

#[test]
fn doctor_subcommand() {
    Command::cargo_bin("apex")
        .unwrap()
        .arg("doctor")
        .assert()
        .success();
}

#[test]
fn run_requires_target_and_lang() {
    Command::cargo_bin("apex")
        .unwrap()
        .arg("run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--target"));
}

#[test]
fn run_with_nonexistent_target() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["run", "--target", "/does/not/exist", "--lang", "python"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No such file"));
}

#[test]
fn ratchet_below_threshold_fails() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["ratchet", "--target", ".", "--lang", "python", "--threshold", "1.0"])
        .assert()
        .failure();
}

#[test]
fn deploy_score_json_output() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["deploy-score", "--target", ".", "--lang", "python", "--format", "json"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("{"));
}
```

### Testing with environment variables and config files

```rust
use assert_fs::prelude::*;

#[test]
fn respects_config_file() {
    let config = assert_fs::NamedTempFile::new("apex.toml").unwrap();
    config.write_str(r#"
[coverage]
target = 0.5

[logging]
level = "debug"
format = "text"
"#).unwrap();

    Command::cargo_bin("apex")
        .unwrap()
        .args(["run", "--config", config.path().to_str().unwrap(),
               "--target", ".", "--lang", "python"])
        .assert()
        .success();
}

#[test]
fn log_level_override() {
    Command::cargo_bin("apex")
        .unwrap()
        .args(["--log-level", "trace", "doctor"])
        .env("RUST_LOG", "warn")  // CLI flag should win
        .assert()
        .success();
}
```

### Coverage implications

**Process-spawn tests do NOT contribute to `cargo llvm-cov --tests` by default.**
The spawned binary is a separate process with its own profraw. To capture coverage:

```bash
# Method A: cargo-llvm-cov with --include-build-script (doesn't help for bins)
# Method B: Set LLVM_PROFILE_FILE in the spawned process environment
LLVM_PROFILE_FILE="apex-%p-%m.profraw" cargo llvm-cov --tests

# Then merge all profraw files (cargo-llvm-cov does this automatically if
# the binary was built with coverage instrumentation)
```

Or use the `run` subcommand of cargo-llvm-cov:

```bash
# Build with instrumentation, then run tests that spawn the binary
cargo llvm-cov --no-report -- build  # instrument the binary
cargo llvm-cov --no-report -- test   # run tests (spawned binary writes profraw)
cargo llvm-cov --no-run report       # merge and report
```

### Tradeoffs

| Pro | Con |
|-----|-----|
| True end-to-end test (binary as shipped) | Slow: process spawn per test (~50-200ms overhead) |
| Tests exit codes, stdout/stderr exactly | Coverage requires extra profraw wrangling |
| No code changes to production code needed | Cannot mock internal dependencies |
| Catches linking/startup issues | Parallel tests may have port/file conflicts |

---

## 3. `trycmd` / `snapbox` -- Snapshot-Based CLI Testing

### What they are

- **`trycmd`**: Runs CLI commands defined in `.toml` or `.md` files and compares
  stdout/stderr/exit-code against expected snapshots. Ideal for maintaining a
  large corpus of CLI invocations.
- **`snapbox`**: Lower-level library (trycmd is built on it). Use when you need
  custom assertions or programmatic snapshot comparison.

### Setup for trycmd

```toml
# crates/apex-cli/Cargo.toml
[dev-dependencies]
trycmd = "0.15"
```

```rust
// crates/apex-cli/tests/cli_tests.rs
#[test]
fn cli_tests() {
    trycmd::TestCases::new()
        .case("tests/cmd/*.toml")
        .case("tests/cmd/*.md");
}
```

### Test case format (TOML)

```toml
# crates/apex-cli/tests/cmd/doctor.toml
bin.name = "apex"
args = ["doctor"]
status.code = 0
stdout = """
[doctor] Checking Python ... ok
[doctor] Checking Node.js ... ok
...
"""
```

```toml
# crates/apex-cli/tests/cmd/run_missing_target.toml
bin.name = "apex"
args = ["run", "--target", "/nonexistent", "--lang", "python"]
status.code = 1
stderr = "...No such file..."
```

The `...` syntax is a glob-like wildcard that matches any text, making snapshots
resilient to minor output changes.

### Test case format (Markdown)

```markdown
<!-- crates/apex-cli/tests/cmd/help.md -->
```console
$ apex --help
Autonomous Path EXploration -- drives any repository to 100% branch coverage
...
```
```

### Updating snapshots

```bash
# Review and accept all changes:
TRYCMD=overwrite cargo test cli_tests
# Or dump to a file for review:
TRYCMD=dump cargo test cli_tests
```

### Setup for snapbox (programmatic)

```toml
[dev-dependencies]
snapbox = { version = "0.6", features = ["cmd"] }
```

```rust
use snapbox::cmd::Command;

#[test]
fn doctor_output() {
    Command::new(snapbox::cmd::cargo_bin!("apex"))
        .arg("doctor")
        .assert()
        .success()
        .stdout_matches("...[doctor] Checking Python...ok\n...");
}
```

### Tradeoffs

| Pro | Con |
|-----|-----|
| Scales to hundreds of test cases with minimal Rust code | Output must be deterministic (timestamps, paths break snapshots) |
| Self-documenting (test files show exact CLI usage) | Same coverage limitation as assert_cmd (process spawn) |
| `TRYCMD=overwrite` makes updating easy | Less flexible assertions than predicates |
| Great for regression testing output format | Not suited for tests needing setup/teardown logic |

### When to use which

- **trycmd**: You have 20+ subcommands and want to test all their help text,
  error messages, and basic success paths without writing Rust code for each.
- **snapbox**: You need snapshot matching but also need programmatic setup (temp
  dirs, env vars, fixtures).
- **assert_cmd**: You need maximum assertion flexibility (regex, custom
  predicates, JSON structure validation).

---

## 4. Testing `#[tokio::main]` and Async Entry Points

### The problem

`#[tokio::main]` expands to:

```rust
fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async { /* your async main body */ })
}
```

You cannot call `main()` from a test. You need the extracted `run_cli()` pattern
from Section 1.

### Pattern: `#[tokio::test]` with the extracted function

```rust
#[tokio::test]
async fn test_run_subcommand() {
    let cli = Cli::parse_from([
        "apex", "run",
        "--target", "/tmp/test-repo",
        "--lang", "python",
        "--strategy", "baseline",
    ]);
    let result = run_cli(cli).await;
    // assert on result
}
```

### Pattern: Multi-threaded test runtime

Some code paths (e.g., spawning blocking tasks, parallel fuzzing) require
a multi-threaded runtime:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_fuzz_subcommand() {
    let cli = Cli::parse_from(["apex", "fuzz", "--target", ".", "--lang", "rust"]);
    let result = run_cli(cli).await;
    assert!(result.is_ok());
}
```

### Pattern: Testing with timeouts

```rust
#[tokio::test]
async fn run_does_not_hang() {
    let cli = Cli::parse_from(["apex", "run", "--target", ".", "--lang", "python"]);
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        run_cli(cli),
    ).await;
    assert!(result.is_ok(), "run_cli timed out");
}
```

### Pattern: Mocking async dependencies with mockall

Since apex-cli uses traits like `Instrumentor` and `LanguageRunner`:

```rust
// If run_cli accepted trait objects, you could inject mocks:
pub async fn run_cli_with<I: Instrumentor, R: LanguageRunner>(
    cli: Cli,
    instrumentor: I,
    runner: R,
) -> Result<()> { ... }

// In tests:
#[tokio::test]
async fn run_with_mock_instrumentor() {
    let mut mock = MockInstrumentor::new();
    mock.expect_instrument()
        .returning(|_| Ok(InstrumentResult::default()));
    // ...
}
```

### Tradeoffs for async testing

| Approach | Coverage | Speed | Fidelity |
|----------|----------|-------|----------|
| `#[tokio::test]` + extracted fn | Full LLVM coverage | Fast | High (same code paths) |
| `assert_cmd` process spawn | Requires profraw merge | Slow | Highest (true binary) |
| `block_on` in sync test | Full LLVM coverage | Fast | Medium (different runtime config) |

---

## 5. LLVM Source-Based Coverage: Complete Workflow

### Tool: `cargo-llvm-cov`

```bash
cargo install cargo-llvm-cov
```

### Basic coverage run

```bash
# Line coverage (default)
cargo llvm-cov --workspace --html

# Branch coverage (requires nightly)
cargo +nightly llvm-cov --workspace --branch --html

# With specific test binary
cargo llvm-cov --package apex-cli --tests --html
```

### Combining library tests + integration tests + binary tests

```bash
# Step 1: Clean previous runs
cargo llvm-cov clean --workspace

# Step 2: Build everything with instrumentation (no report yet)
cargo llvm-cov --no-report --workspace

# Step 3: Run unit tests
cargo llvm-cov --no-report -- test --workspace

# Step 4: Run integration tests that spawn the binary
# The binary was built with instrumentation in step 2, so spawned
# processes will write profraw files
cargo llvm-cov --no-report -- test --package apex-cli --test cli_smoke

# Step 5: Generate combined report
cargo llvm-cov report --html --ignore-filename-regex='/.cargo/registry'
```

### Branch coverage specifics

Branch coverage in Rust via LLVM is still maturing. As of nightly-2024-03-16+:

```bash
# Enable branch coverage instrumentation
RUSTFLAGS="-C instrument-coverage -Z coverage-options=branch" \
    cargo +nightly test --workspace

# Or with cargo-llvm-cov
cargo +nightly llvm-cov --branch --workspace --html
```

Branch coverage tracks:
- `if`/`else` branches
- `match` arms (each arm is a branch)
- `&&` and `||` short-circuit evaluation
- `?` operator (Ok vs Err paths)

### MC/DC coverage (nightly, experimental)

```bash
# Modified Condition/Decision Coverage -- even more granular
cargo +nightly llvm-cov --mcdc --workspace --html
```

### CI integration pattern

```bash
# .github/workflows/coverage.yml
- name: Coverage
  run: |
    cargo +nightly llvm-cov --workspace --branch \
      --ignore-filename-regex='/.cargo/registry' \
      --fail-under-lines 80 \
      --fail-under-branches 60 \
      --lcov --output-path lcov.info
```

### Coverage of the binary's `main()` via process tests

For `assert_cmd` / `trycmd` tests to contribute coverage, the spawned binary
must have been built with `-C instrument-coverage` and must write profraw files.
`cargo-llvm-cov` handles this when you use it consistently:

```bash
# This builds the binary with coverage, then runs tests.
# Spawned processes inherit the instrumentation.
cargo llvm-cov --workspace --tests --html
```

The key is that `Command::cargo_bin("apex")` resolves to the binary in
`target/debug/`, and if that binary was built by `cargo llvm-cov`, it is
instrumented. The spawned process writes its own profraw, which `cargo llvm-cov`
auto-discovers and merges.

**Gotcha**: If you build with `cargo build` and then run `cargo llvm-cov --tests`,
the binary in target/debug is NOT instrumented. Always let `cargo llvm-cov` do
the building.

---

## 6. Additional Crates and Techniques

### `insta` -- Snapshot testing for structured output

```toml
[dev-dependencies]
insta = { version = "1", features = ["json", "yaml"] }
```

```rust
#[test]
fn deploy_score_output_shape() {
    let output = Command::cargo_bin("apex").unwrap()
        .args(["deploy-score", "--target", ".", "--lang", "python", "--format", "json"])
        .output().unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    insta::assert_json_snapshot!(json);
}
```

Update snapshots: `cargo insta review`.

### `escargot` -- Lightweight binary resolution

```toml
[dev-dependencies]
escargot = "0.5"
```

```rust
let bin = escargot::CargoBuild::new()
    .bin("apex")
    .current_release()
    .run()
    .unwrap();
let output = bin.command().arg("doctor").output().unwrap();
```

Useful when you want process-level tests but `assert_cmd` is too heavy.

### `libtest-mimic` -- Custom test harness

For generating test cases dynamically (e.g., one test per subcommand):

```rust
// tests/subcommands.rs
use libtest_mimic::{Trial, run};

fn main() {
    let subcommands = vec!["doctor", "run", "ratchet", "index", /* ... */];
    let tests: Vec<Trial> = subcommands.into_iter().map(|cmd| {
        Trial::test(format!("help_{cmd}"), move || {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_apex"))
                .args([cmd, "--help"])
                .output()
                .map_err(|e| format!("{e}"))?;
            if output.status.success() { Ok(()) }
            else { Err(format!("{cmd} --help failed").into()) }
        })
    }).collect();
    run(&libtest_mimic::Arguments::from_args(), tests).exit();
}
```

### Clap's built-in `debug_assert!` testing

```rust
#[test]
fn verify_cli() {
    // Clap's own validation: catches conflicts, missing fields, etc.
    use clap::CommandFactory;
    Cli::command().debug_assert();
}
```

This is free coverage of clap's internal validation logic and catches
configuration bugs at test time.

---

## 7. Recommended Strategy for APEX

Given the codebase structure (18+ subcommands, tokio async, color-eyre), here is
the recommended layered approach:

### Layer 1: Refactor (highest ROI)

Extract `run_cli(cli: Cli) -> Result<()>` into `lib.rs`. This unlocks in-process
testing with full LLVM coverage. Estimated effort: 1-2 hours.

### Layer 2: Unit-style integration tests via `#[tokio::test]`

Test each subcommand's happy path and primary error path by constructing `Cli`
via `parse_from`. These run fast and give branch coverage of dispatch logic,
config loading, and error handling.

### Layer 3: `assert_cmd` smoke tests

A small suite (10-15 tests) that spawn the real binary. Covers:
- No-args help output
- Each subcommand's `--help`
- Invalid argument combinations
- Exit codes

### Layer 4: `trycmd` for output regression

Once output format stabilizes, add `.toml` test cases for each subcommand's
output. Low maintenance, catches unintended output changes.

### Layer 5: Clap validation test

One test: `Cli::command().debug_assert()`. Free, catches clap config errors.

### Coverage command

```bash
# Full coverage with branch tracking (nightly)
cargo +nightly llvm-cov --workspace --branch --html \
    --ignore-filename-regex='/.cargo/registry' \
    --open

# Stable toolchain (line coverage only)
cargo llvm-cov --workspace --tests --html --open
```

### Expected coverage targets

| Layer | Coverage contribution | Effort |
|-------|---------------------|--------|
| Existing unit tests (apex-coverage) | ~15% of workspace | Already done |
| Layer 1+2 (refactor + tokio tests) | +30-40% of apex-cli | 1-2 days |
| Layer 3 (assert_cmd smoke) | +5-10% (main, startup) | Half day |
| Layer 4 (trycmd snapshots) | ~0% new (regression guard) | Half day |
| Layer 5 (clap debug_assert) | ~1% | 5 minutes |

---

## Appendix: Complete dev-dependencies for apex-cli

```toml
[dev-dependencies]
# Existing
mockall = { workspace = true }
tempfile = "3"

# Add for CLI testing
assert_cmd = "2"
predicates = "3"
assert_fs = "1"
trycmd = "0.15"
insta = { version = "1", features = ["json"] }
tokio = { version = "1", features = ["full", "test-util"] }
```
