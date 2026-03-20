# Contributing to APEX

APEX eats its own cooking. Every change is validated by running APEX on itself before merge.

## Quick Start

```bash
git clone https://github.com/sahajamoth/apex.git && cd apex
cargo build --release
cargo nextest run --workspace       # 5,000+ tests, parallel
cargo clippy --workspace -- -D warnings
```

## The Golden Rule: APEX on APEX

Before opening a PR, run APEX on itself and include the output:

```bash
# Security audit — must show 0 new high findings
apex audit --target . --lang rust

# Full run — coverage must not regress below 93%
apex run --target . --lang rust
```

CI enforces this automatically. If your change introduces new high-severity findings or drops coverage, the PR will be blocked.

**Why?** APEX is a code analysis tool. If it can't analyze itself cleanly, it can't analyze anyone else's code. Dogfooding is not optional — it's the product.

## Pull Request Workflow

All changes go through pull requests. Direct pushes to `main` are blocked.

### 1. Fork and branch

```bash
# Fork on GitHub, then:
git clone https://github.com/YOUR-USERNAME/apex.git && cd apex
git remote add upstream https://github.com/sahajamoth/apex.git
git checkout -b feat/your-feature
```

### 2. Make your changes

- Write tests first (TDD encouraged)
- Follow existing patterns in the crate you're modifying
- Tests go in `#[cfg(test)] mod tests` inside each file
- Use `#[tokio::test]` for async tests

### 3. Validate

```bash
# Required — all must pass
cargo nextest run --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check

# Required — run APEX on itself
apex audit --target . --lang rust
apex run --target . --lang rust    # coverage must be >= 93%
```

### 4. Update changelog

Every PR must add an entry to `CHANGELOG.md` under `[Unreleased]`. Describe what changed and why.

### 5. Submit

```bash
git push origin feat/your-feature
# Open PR on GitHub — CI runs automatically
```

### CI Checks (all required)

| Check | What it validates |
|-------|-------------------|
| `cargo check` | Compilation |
| `cargo nextest run` | 5,000+ tests pass |
| `cargo clippy -D warnings` | Zero lint warnings |
| `cargo fmt --check` | Formatting |
| `apex audit --target .` | No new high findings |
| Changelog | `CHANGELOG.md` updated |

## Code Ownership (Fleet)

APEX uses [Fleet](https://github.com/fleet-plugins/fleet) for code ownership. Each crate (or group of crates) has a **crew** that owns it.

### Layer Crews (shared infrastructure)

| Crew | Owns | What it does |
|------|------|-------------|
| `foundation` | apex-core, apex-coverage, apex-mir | Shared types, traits, coverage model |
| `security-detect` | apex-detect, apex-cpg | Security patterns, taint analysis |
| `exploration` | apex-fuzz, apex-symbolic, apex-concolic | Fuzzing, symbolic/concolic execution |
| `runtime` | apex-lang, apex-instrument, apex-sandbox | Language runners, coverage instrumentation |
| `intelligence` | apex-agent, apex-synth | AI-driven test generation, synthesis |
| `platform` | apex-cli, apex-rpc | CLI binary, RPC coordination |

### Language Crews (end-to-end pipeline per language)

| Crew | Owns | Pipeline |
|------|------|----------|
| `lang-python` | python.rs across 7 crates | instrument (coverage.py) -> runner (pytest) -> sandbox -> index -> synth -> concolic |
| `lang-js` | javascript.rs across 6 crates | instrument (istanbul/V8) -> runner (jest) -> sandbox -> index -> synth -> concolic |
| `lang-rust` | rust_cov.rs, rust_lang.rs, etc. | instrument (cargo-llvm-cov) -> runner (cargo test) -> sandbox -> index -> synth -> concolic |
| `lang-go` | go.rs across 6 crates | instrument (go cover) -> runner (go test) -> index -> synth -> concolic |
| `lang-jvm` | java.rs, kotlin.rs | instrument (JaCoCo) -> runner (JUnit/Gradle) -> index -> synth -> concolic |
| `lang-c-cpp` | c_coverage.rs, c.rs, cpp.rs | instrument (gcov/sancov) -> runner (make/cmake) -> index -> synth -> concolic |
| `lang-dotnet` | csharp.rs | instrument (coverlet) -> runner (dotnet test) -> index -> synth -> concolic |
| `lang-swift` | swift.rs | instrument (xccov) -> runner (swift test) -> index -> synth -> concolic |
| `lang-ruby` | ruby.rs | instrument (SimpleCov) -> runner (RSpec) -> sandbox -> index -> synth -> concolic |

### Using Fleet (recommended for multi-crate changes)

If you use [Claude Code](https://claude.com/claude-code) with the Fleet plugin:

```bash
# See which crew owns the files you're changing
/fleet crew list

# Dispatch a crew agent to implement your change
/fleet plan create "Add X to Y"

# The captain coordinates multi-crew work automatically
```

Fleet is not required — you can contribute with any editor. But for changes that span multiple crates, Fleet agents understand the ownership boundaries and coordinate automatically.

## Cross-Repo Coordination (Federation)

APEX participates in Fleet Federation for cross-repo feature requests.

### When to use federation

- You're working on APEX and need a change in the Fleet plugin (or vice versa)
- Your feature requires coordinated changes across multiple repos
- You want to propose a protocol extension that affects multiple projects

### How to file a federation request

```bash
# From the APEX repo:
/fleet federation request fleet "Support preflight_check() in crew protocol"

# Or create a GitHub issue with the `fleet:request` label
```

### Incoming requests

If you maintain a partner repo and receive a `fleet:request` issue:

```bash
/fleet federation inbox          # Check incoming requests
/fleet federation review 42      # Assess feasibility
/fleet federation implement 42   # Accept and implement
```

## Architecture

```
apex-core          Foundation — types, traits, config, error handling
  |
  +-- apex-coverage    Coverage oracle, bitmap tracking
  +-- apex-mir         MIR parsing, control-flow analysis
  |
  +-- apex-instrument  Multi-language coverage instrumentation
  +-- apex-lang        Language-specific test runners
  +-- apex-sandbox     Process isolation for test execution
  |
  +-- apex-detect      Security pattern detectors (40+)
  +-- apex-cpg         Code Property Graph, taint analysis
  |
  +-- apex-agent       AI-driven test generation, priority scheduler
  +-- apex-synth       Test synthesis (12 language templates)
  |
  +-- apex-fuzz        Coverage-guided fuzzing (MOpt scheduler)
  +-- apex-symbolic    SMT-LIB2 constraint solving
  +-- apex-concolic    Concolic execution (8 language parsers)
  |
  +-- apex-index       Per-test branch indexing, SDLC analysis
  +-- apex-rpc         gRPC distributed coordination
  +-- apex-cli         CLI binary — 20 subcommands + MCP server
```

Key principles:

- **apex-core** is the foundation — all other crates depend on it
- Heavy external dependencies (Z3, LibAFL, pyo3, LLVM) are always feature-gated
- Each language has files across 6-7 crates (instrument, lang, sandbox, index, synth, concolic)
- The CLI orchestrates everything through trait objects
- `preflight_check()` reviews the target project before instrumentation

## Building with Optional Features

Heavy dependencies are feature-gated and not compiled by default:

```bash
# Z3 SMT solver + LibAFL fuzzer
cargo build --release --features "apex-symbolic/z3-solver,apex-fuzz/libafl-backend"
```

## Adding a New Language

To add language support (e.g., Zig), you need files in 6 crates:

1. `crates/apex-instrument/src/zig.rs` — coverage instrumentation
2. `crates/apex-lang/src/zig.rs` — test runner + `preflight_check()`
3. `crates/apex-index/src/zig.rs` — per-test branch indexer
4. `crates/apex-synth/src/zig.rs` — test synthesis templates
5. `crates/apex-concolic/src/zig_conditions.rs` — condition parser
6. `crates/apex-detect/src/detectors/security_pattern.rs` — add `ZIG_SECURITY_PATTERNS`

Plus: add `Zig` to the `Language` enum in `apex-core/src/types.rs` and wire dispatch in `apex-cli/src/lib.rs`.

Use an existing language (e.g., Go) as a reference. Each file follows the same trait pattern.

## Adding a New Detector

Security detectors go in `crates/apex-detect/src/detectors/`. Each implements:

```rust
#[async_trait]
impl Detector for MyDetector {
    fn name(&self) -> &str { "my-detector" }
    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        // Read ctx.source_cache, match patterns, return findings
    }
}
```

Register in `crates/apex-detect/src/pipeline.rs`.

## Reporting Issues

- **Bug reports and feature requests:** [GitHub Issues](https://github.com/sahajamoth/apex/issues)
- **Security vulnerabilities:** [GitHub Security Advisories](https://github.com/sahajamoth/apex/security/advisories) (private)
- **Cross-repo requests:** Create an issue with the `fleet:request` label

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
