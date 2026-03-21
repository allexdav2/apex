<!-- status: ACTIVE -->

# Toolchain Management and Coverage Fixes

**Date:** 2026-03-21
**Scope:** mise-based toolchain management (new crate), CPG crash fix, CPython venv fix, devcontainer/devbox detection, CI config parser, pipeline wiring, Apple Containers stub.
**Depends on:** v0.4.0 plan (adoption infrastructure complete)

---

## Crew Roster

| Crew | Role in This Plan |
|------|-------------------|
| **runtime** | apex-toolchain crate, mise integration, version detection from project files |
| **platform** | CLI wiring, devcontainer/devbox detection, CI config parser, pipeline orchestration |
| **security-detect** | CPG Go parser crash fix (builder.rs) |
| **foundation** | Pipeline orchestration (toolchain_setup step in run_analyze) |

---

## File Map

| Crew | Files (new or modified) |
|------|------------------------|
| runtime | `crates/apex-toolchain/` (new crate), `crates/apex-toolchain/Cargo.toml` (new), `crates/apex-toolchain/src/lib.rs` (new), `crates/apex-toolchain/src/detect.rs` (new), `crates/apex-toolchain/src/mise.rs` (new), `crates/apex-toolchain/src/devenv.rs` (new), `Cargo.toml` (workspace member) |
| platform | `crates/apex-cli/src/lib.rs`, `crates/apex-cli/src/doctor.rs`, `crates/apex-cli/Cargo.toml` |
| security-detect | `crates/apex-cpg/src/builder.rs` |
| foundation | `crates/apex-core/src/traits.rs`, `crates/apex-cli/src/lib.rs` |

---

## Wave 1 -- Fixes and Foundation (parallel, no deps)

Three independent tasks. All can run simultaneously.

### Task 1.1 -- security-detect crew: Fix K8s CPG crash on Go code

**Files:** `crates/apex-cpg/src/builder.rs`
**Effort:** 0.5 days
**Description:** The `InternalGoParser::parse_func()` method at line 798 panics on certain Go syntax patterns. The line-based parser uses unchecked string slicing (`after_func[..paren]`, `after_func[open + 1..close]`, `after_func[close..]`) that panics when:
- A `func` keyword appears without a following `(` on the same line (e.g., multi-line signatures)
- A receiver type contains nested parentheses (e.g., generic receivers `func (s *Set[T])`)
- An interface method declaration has no body (`Handle(w http.ResponseWriter, r *http.Request)`)

Similarly, `parse_go_call_expr()` (line 954) and `parse_go_assign()` (line 898) use unchecked slicing. The `body_indentation()` helper (line 477) can also read past the end of `lines` if `start >= lines.len()`.

**Acceptance criteria:**
- [ ] Write a test with Go source that triggers the current panic (multi-line func signature, generic receiver, interface method)
- [ ] Run test, confirm panic
- [ ] Add bounds checks to all string slice operations in `InternalGoParser`: guard `find()` results before slicing, return early on malformed lines
- [ ] Add bounds check to `body_indentation()`: return default indent when `start >= lines.len()`
- [ ] Run test, confirm no panic -- malformed Go degrades gracefully (skips function, does not crash)
- [ ] All existing CPG tests pass (`cargo nextest run -p apex-cpg`)
- [ ] Commit

### Task 1.2 -- runtime crew: Version detection module (apex-toolchain crate)

**Files:** `crates/apex-toolchain/Cargo.toml` (new), `crates/apex-toolchain/src/lib.rs` (new), `crates/apex-toolchain/src/detect.rs` (new), `Cargo.toml` (workspace member)
**Effort:** 1.5 days
**Description:** Create the `apex-toolchain` crate with version detection from project files. This module reads project configuration files and extracts required toolchain versions without installing anything. Supported files:

| File | Language/Tool | Version extraction |
|------|--------------|-------------------|
| `go.mod` | Go | `go X.Y` directive on line 3+ |
| `pyproject.toml` | Python | `requires-python`, `[tool.poetry.dependencies].python` |
| `.python-version` | Python | bare version string |
| `.ruby-version` | Ruby | bare version string |
| `Gemfile` | Ruby | `ruby "X.Y.Z"` directive |
| `.node-version` | Node.js | bare version string |
| `.nvmrc` | Node.js | bare version string or `lts/*` |
| `package.json` | Node.js | `engines.node` field |
| `global.json` | .NET | `sdk.version` field |
| `.tool-versions` | Any (asdf/mise) | `<tool> <version>` per line |
| `.mise.toml` | Any (mise) | `[tools].<name> = "<version>"` |
| `rust-toolchain.toml` | Rust | `[toolchain].channel` |
| `rust-toolchain` | Rust | bare version/channel string |

Data model:
```rust
pub struct ToolchainRequirement {
    pub tool: String,        // "python", "go", "node", "ruby", "dotnet", "rust"
    pub version: String,     // "3.12", "1.22", ">=20", etc.
    pub source: String,      // "pyproject.toml", "go.mod", ".tool-versions", etc.
    pub constraint: VersionConstraint,  // Exact, MinVersion, Range
}

pub enum VersionConstraint {
    Exact(String),
    MinVersion(String),
    Range { min: String, max: String },
}

pub fn detect_requirements(project_root: &Path) -> Vec<ToolchainRequirement>;
```

**Acceptance criteria:**
- [ ] Write tests for each file type (12 parsers, minimum 2 tests each = 24 tests)
- [ ] Run tests, confirm failure (crate does not exist yet)
- [ ] Create `crates/apex-toolchain/` with `Cargo.toml`, add to workspace members in root `Cargo.toml`
- [ ] Implement `detect_requirements()` that scans project root for all supported files
- [ ] Each parser handles missing files gracefully (returns empty, no error)
- [ ] `.tool-versions` and `.mise.toml` parsers extract all tools, not just one language
- [ ] Conflicting versions from multiple files: return all, caller resolves (last-wins or user choice)
- [ ] Run tests, confirm pass
- [ ] `cargo clippy -p apex-toolchain -- -D warnings` clean
- [ ] Commit

### Task 1.3 -- platform crew: Commit CPython venv fix

**Files:** `crates/apex-instrument/src/python.rs` (already modified)
**Effort:** 0.25 days (15 minutes)
**Description:** The CPython venv fix has already been applied -- venvs are now created at `$TMPDIR/apex-venvs/<hash>/` instead of inside the target directory. This avoids polluting the target project and fixes issues with read-only source trees. The change just needs to be committed and verified.

**Acceptance criteria:**
- [ ] Verify the change in `crates/apex-instrument/src/python.rs` uses external venv path
- [ ] Run Python instrumentation tests: `cargo nextest run -p apex-instrument -- python`
- [ ] Confirm no regressions
- [ ] Commit with message describing the venv relocation

---

## Wave 2 -- Mise Integration and Environment Detection (depends on Wave 1)

Depends on Task 1.2 (version detection module must exist). Tasks 2.1, 2.2, and 2.3 are parallel within this wave.

### Task 2.1 -- runtime crew: mise install integration

**Files:** `crates/apex-toolchain/src/mise.rs` (new), `crates/apex-toolchain/src/lib.rs`
**Effort:** 1 day
**Description:** Implement the mise installation backend. Given a list of `ToolchainRequirement`s from Task 1.2, install missing tools via `mise install <tool>@<version>`. Graceful fallback when mise is not installed.

Logic:
1. Check if `mise` binary exists (`which mise`)
2. If not, return `MiseNotAvailable` (not an error -- caller proceeds without toolchain management)
3. For each requirement, check if tool+version is already installed (`mise ls --json`)
4. Install missing: `mise install <tool>@<version>` with timeout
5. Activate: `mise exec --tools <tool>@<version> -- <command>` or set `MISE_EXPERIMENTAL=1` env

Data model:
```rust
pub enum MiseResult {
    Installed { tool: String, version: String },
    AlreadyPresent { tool: String, version: String },
    Failed { tool: String, error: String },
    MiseNotAvailable,
}

pub async fn ensure_tools(requirements: &[ToolchainRequirement]) -> Vec<MiseResult>;
pub fn mise_available() -> bool;
```

**Acceptance criteria:**
- [ ] Write test: `mise_available()` returns false when binary not found (mock PATH)
- [ ] Write test: `ensure_tools()` with mock mise binary returns `AlreadyPresent` for installed tools
- [ ] Write test: `ensure_tools()` with empty requirements returns empty vec
- [ ] Run tests, confirm failure
- [ ] Implement `mise.rs` with process spawning via `CommandSpec` (reuse existing pattern from `apex-cli/src/doctor.rs`)
- [ ] Timeout: 120 seconds per install, 300 seconds total
- [ ] Stderr output from mise forwarded to tracing::debug (not swallowed, not shown to user by default)
- [ ] Run tests, confirm pass
- [ ] Integration test (gated behind `#[ignore]`): actually run `mise install python@3.12` if mise is available
- [ ] Commit

### Task 2.2 -- platform crew: CI config parser (.github/workflows)

**Files:** `crates/apex-toolchain/src/ci.rs` (new), `crates/apex-toolchain/src/lib.rs`
**Effort:** 1 day
**Description:** Parse GitHub Actions workflow YAML files to extract toolchain versions from `actions/setup-*` steps. This is the most reliable version source for CI-oriented projects -- if the CI sets up Python 3.12, that is the version the project needs.

Supported actions:
| Action | Extracted field |
|--------|----------------|
| `actions/setup-python` | `with.python-version` |
| `actions/setup-node` | `with.node-version` |
| `actions/setup-go` | `with.go-version` |
| `actions/setup-java` | `with.java-version` + `with.distribution` |
| `actions/setup-dotnet` | `with.dotnet-version` |
| `ruby/setup-ruby` | `with.ruby-version` |

Parser approach: lightweight YAML line scanning (no full YAML parser dependency). Look for `uses: actions/setup-*` lines, then extract `with.<field>:` values from subsequent indented lines. Matrix expansions (`${{ matrix.python-version }}`) are detected but not resolved (marked as `VersionConstraint::Matrix`).

**Acceptance criteria:**
- [ ] Write test: parse a workflow file with `actions/setup-python` and `python-version: "3.12"` -- extracts `ToolchainRequirement { tool: "python", version: "3.12", source: ".github/workflows/ci.yml" }`
- [ ] Write test: parse workflow with multiple setup actions (Python + Node) -- extracts both
- [ ] Write test: matrix version (`${{ matrix.python-version }}`) detected as `VersionConstraint::Matrix`
- [ ] Write test: no `.github/workflows/` directory -- returns empty (no error)
- [ ] Run tests, confirm failure
- [ ] Implement `pub fn detect_ci_requirements(project_root: &Path) -> Vec<ToolchainRequirement>` scanning all `.yml`/`.yaml` files in `.github/workflows/`
- [ ] Run tests, confirm pass
- [ ] Minimum 10 tests covering each supported action + edge cases
- [ ] Commit

### Task 2.3 -- platform crew: Devcontainer and devbox detection

**Files:** `crates/apex-toolchain/src/devenv.rs` (new), `crates/apex-toolchain/src/lib.rs`
**Effort:** 0.5 days
**Description:** Detect `devcontainer.json` and `devbox.json` in the project root (or `.devcontainer/` directory). When present, these override toolchain guessing -- the project has declared its environment. APEX should report this to the user and optionally defer to the container/devbox environment.

Data model:
```rust
pub enum DevEnvironment {
    Devcontainer { path: PathBuf, image: Option<String> },
    Devbox { path: PathBuf, packages: Vec<String> },
    None,
}

pub fn detect_dev_environment(project_root: &Path) -> DevEnvironment;
```

Parsing:
- `devcontainer.json` or `.devcontainer/devcontainer.json`: extract `image` field (lightweight JSON scan, no serde_json dep in this crate)
- `devbox.json`: extract `packages` array (lightweight JSON scan)

**Acceptance criteria:**
- [ ] Write test: project with `devcontainer.json` at root -> `DevEnvironment::Devcontainer`
- [ ] Write test: project with `.devcontainer/devcontainer.json` -> `DevEnvironment::Devcontainer`
- [ ] Write test: project with `devbox.json` -> `DevEnvironment::Devbox` with packages extracted
- [ ] Write test: project with neither -> `DevEnvironment::None`
- [ ] Run tests, confirm failure
- [ ] Implement `detect_dev_environment()`
- [ ] Run tests, confirm pass
- [ ] Commit

---

## Wave 3 -- Pipeline Integration (depends on Wave 2)

All Wave 2 modules must be complete. This wave wires everything together.

### Task 3.1 -- foundation crew: Wire toolchain_setup into run_analyze pipeline

**Files:** `crates/apex-cli/src/lib.rs`, `crates/apex-cli/Cargo.toml`
**Effort:** 1 day
**Description:** Insert a `toolchain_setup` step into the `run_analyze()` pipeline between `preflight_check()` and `install_deps()`. The new pipeline order:

```
preflight_check() -> toolchain_setup() -> install_deps() -> instrument() -> detect() -> report()
```

The `toolchain_setup()` function:
1. Calls `detect_dev_environment()` -- if devcontainer/devbox found, log it and skip mise (user manages their env)
2. Calls `detect_requirements()` + `detect_ci_requirements()` to find all version requirements
3. Calls `preflight.missing_tools` to identify what is actually missing
4. For each missing tool with a known version requirement, calls `ensure_tools()` (mise)
5. Logs results: what was installed, what was already present, what failed
6. If mise is not available and tools are missing, emit a hint: `"Install mise (https://mise.jdx.dev) to auto-install missing tools"`

Also update `preflight_check()` to include detected toolchain versions in its output (add `detected_versions: Vec<ToolchainRequirement>` to `PreflightInfo`).

**Acceptance criteria:**
- [ ] Write test: `toolchain_setup()` with devcontainer present skips mise installation
- [ ] Write test: `toolchain_setup()` with missing Python and valid `pyproject.toml` calls `ensure_tools(["python@3.12"])`
- [ ] Write test: `toolchain_setup()` with mise unavailable emits hint message (not error)
- [ ] Write test: `toolchain_setup()` with all tools present is a no-op
- [ ] Run tests, confirm failure
- [ ] Add `apex-toolchain` dependency to `apex-cli/Cargo.toml`
- [ ] Implement `toolchain_setup()` in `crates/apex-cli/src/lib.rs`
- [ ] Wire into `run_analyze()` after preflight, before install_deps
- [ ] Wire into `run_deterministic_coverage()` path as well
- [ ] Add `detected_versions` field to `PreflightInfo` (in `apex-core/src/traits.rs`)
- [ ] Run tests, confirm pass
- [ ] Full workspace check: `cargo check --workspace && cargo nextest run --workspace`
- [ ] Commit

### Task 3.2 -- platform crew: Update `apex doctor` with toolchain info

**Files:** `crates/apex-cli/src/doctor.rs`
**Effort:** 0.5 days
**Description:** Enhance `apex doctor` to show detected toolchain requirements alongside installed versions. Use the `detect_requirements()` function from `apex-toolchain` to show what the project expects vs. what is installed. This extends the existing mise checks in `doctor.rs` (lines 169-218).

**Acceptance criteria:**
- [ ] `apex doctor` shows a "Toolchain Requirements" section listing detected versions from project files
- [ ] Each requirement shows: tool name, required version, source file, installed version (if found), status (ok/missing/mismatch)
- [ ] Devcontainer/devbox detection shown as informational note
- [ ] Existing doctor output unchanged for projects with no toolchain files
- [ ] Tests for new doctor output sections
- [ ] Commit

### Task 3.3 -- platform crew: Apple Containers stub (feature-gated)

**Files:** `crates/apex-toolchain/src/containers.rs` (new), `crates/apex-toolchain/src/lib.rs`, `crates/apex-toolchain/Cargo.toml`
**Effort:** 0.25 days (15 minutes)
**Description:** Create a stub module for Apple Containers support (macOS 26+). Feature-gated behind `apple-containers` cargo feature. The stub defines the interface but returns `Err(Unsupported)` for all operations. This reserves the API surface for future implementation.

```rust
#[cfg(feature = "apple-containers")]
pub mod containers {
    pub struct ContainerConfig { ... }
    pub async fn run_in_container(config: &ContainerConfig, command: &[String]) -> Result<Output> {
        Err(ApexError::Unsupported("Apple Containers require macOS 26+".into()))
    }
}
```

**Acceptance criteria:**
- [ ] Module compiles with `--features apple-containers`
- [ ] Module is absent from default build
- [ ] API surface defined: `ContainerConfig`, `run_in_container()`, `container_available()`
- [ ] All functions return `Err(Unsupported)` with descriptive message
- [ ] Commit

---

## Effort Summary

| Wave | Crew | Tasks | Total Effort |
|------|------|-------|-------------|
| Wave 1 (parallel) | security-detect, runtime, platform | 3 tasks | 2.25 days |
| Wave 2 (parallel) | runtime, platform | 3 tasks | 2.5 days |
| Wave 3 (sequential from Wave 2) | foundation, platform | 3 tasks | 1.75 days |
| **Total** | | **9 tasks** | **6.5 days** |

---

## Dependency Graph

```
Wave 1 (no deps, all parallel) ─────────────────────────────────────────
  1.1 CPG Go crash fix          (security-detect)   [independent]
  1.2 Version detection module  (runtime)           ─┐
  1.3 CPython venv commit       (platform)           │ [independent]
                                                     │
Wave 2 (depends on 1.2) ─────────────────────────────┤
  2.1 mise install integration  (runtime)       ─────┤
  2.2 CI config parser          (platform)      ─────┤
  2.3 devcontainer/devbox       (platform)      ─────┤
                                                     │
Wave 3 (depends on 2.1 + 2.2 + 2.3) ────────────────┘
  3.1 Wire into run_analyze     (foundation)
  3.2 Update apex doctor        (platform)
  3.3 Apple Containers stub     (platform)       [independent]
```

---

## Risk Register

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| mise binary not available on CI runners | High | Low | Graceful fallback -- emit hint, do not fail |
| Version string formats vary wildly (>=3.12, ~3.12, 3.12.*, ^3.12) | Medium | Medium | Parse common formats, pass through unknown constraints as-is |
| YAML line-scanning misses complex workflow syntax | Medium | Low | Cover common patterns (90%+ of real workflows); full YAML parser is a future upgrade |
| Adding apex-toolchain crate increases compile time | Low | Low | Minimal deps (no serde, no tokio -- pure sync IO + string parsing) |
| Go CPG fix changes parse behavior for valid Go code | Low | High | Test against Go stdlib fixtures before and after; diff node counts |
| devcontainer.json has JSON5/JSONC syntax (comments, trailing commas) | Medium | Low | Lightweight scanner tolerates comments; warn on parse failure |

---

## Verification Protocol

Every task follows TDD:
1. Write failing test
2. Run test, confirm failure
3. Implement
4. Run test, confirm pass
5. `cargo clippy -p <crate> -- -D warnings`
6. `cargo fmt --check`
7. Commit with descriptive message

Full workspace verification at wave boundaries:
```bash
cargo check --workspace
cargo nextest run --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

---

## Testing Strategy

### apex-toolchain crate tests

The crate is pure sync IO + string parsing. Tests use `tempdir` with fixture files:

```rust
#[test]
fn detect_python_from_pyproject_toml() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("pyproject.toml"), r#"
[project]
requires-python = ">=3.12"
"#).unwrap();
    let reqs = detect_requirements(dir.path());
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].tool, "python");
    assert_eq!(reqs[0].version, "3.12");
}
```

### CPG crash test

```rust
#[test]
fn go_multiline_func_signature_no_panic() {
    let source = "package main\n\nfunc (s *Set[T]) Add(\n    item T,\n) bool {\n    return true\n}\n";
    let cpg = build_cpg("test.go", source);
    // Should not panic -- may produce incomplete nodes, but no crash
    assert!(cpg.node_count() > 0);
}
```

### Pipeline integration test

The `toolchain_setup()` integration is tested by mocking the `CommandRunner` trait (already used in `doctor.rs` tests). No real mise installation required for unit tests.

---

## Out of Scope

- Full YAML parser for workflow files (using line scanning instead)
- mise plugin development (using built-in mise core tools only)
- Nix flake.nix parsing (future work)
- Docker/Dockerfile version extraction (future work)
- Automatic mise installation if not present (user installs mise themselves)
- Apple Containers implementation (stub only in this plan)
