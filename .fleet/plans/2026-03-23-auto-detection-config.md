<!-- status: IN_PROGRESS -->

# Agent-Level Environment Detection + Auto Config Generation

## Problem

APEX requires manual setup for each project:
- Users must have coverage.py, pytest, cargo-llvm-cov, etc. installed correctly
- Venv paths, tool paths, and language versions are not auto-detected into config
- `apex.toml` must be manually written
- When running in CI, containers, or devcontainers, paths are different
- APEX agents have no way to discover the environment before dispatching work

## Prior Art in This Codebase

Significant detection already exists but is **scattered and disconnected**:

| Existing capability | Location | Gap |
|---|---|---|
| Per-language `preflight_check()` | `crates/apex-lang/src/*.rs` (all 12 runners) | Returns `PreflightInfo` but never persisted or used in config generation |
| Toolchain version detection | `crates/apex-cli/src/toolchain.rs` | Detects from `.tool-versions`, `.python-version`, go.mod, CI configs; not connected to config |
| `apex doctor` | `crates/apex-cli/src/doctor.rs` | Checks tool availability; output is ephemeral (printed, not stored) |
| Python venv detection | `PythonRunner::find_venv_python()`, `resolve_python_for()` | Works but not surfaced in config |
| JS environment detection | `crates/apex-lang/src/js_env.rs` (`JsEnvironment::detect()`) | Rich detection (runtime, pkg_manager, test_runner, module_system, monorepo) but not persisted |
| Config discovery | `ApexConfig::discover()` in `crates/apex-core/src/config.rs` | Walks parents for `apex.toml` but has no `[environment]` section |
| mise/devcontainer/devbox detection | `crates/apex-cli/src/toolchain.rs` (`EnvironmentConfig` enum) | Type exists but not connected to config |

The plan is to **unify** these scattered detections into a single `EnvironmentProbe`, persist results, and generate `apex.toml` automatically.

## File Map

| Crew | Files |
|------|-------|
| foundation | `crates/apex-core/src/config.rs`, `crates/apex-core/src/probe.rs` (new), `crates/apex-core/src/lib.rs` |
| runtime | `crates/apex-lang/src/python.rs`, `crates/apex-lang/src/javascript.rs`, `crates/apex-lang/src/go.rs`, `crates/apex-lang/src/rust_lang.rs`, `crates/apex-lang/src/java.rs`, `crates/apex-lang/src/c.rs`, `crates/apex-lang/src/ruby.rs`, `crates/apex-lang/src/swift.rs`, `crates/apex-lang/src/csharp.rs` |
| platform | `crates/apex-cli/src/lib.rs`, `crates/apex-cli/src/init.rs` (new), `crates/apex-cli/src/doctor.rs` |

---

## Wave 1 (no dependencies)

### Task 1.1 -- foundation crew
**Files:** `crates/apex-core/src/probe.rs` (new), `crates/apex-core/src/lib.rs`, `crates/apex-core/src/config.rs`

Define the `EnvironmentProbe` type hierarchy and the `[environment]` config section.

- [ ] Create `crates/apex-core/src/probe.rs` with `EnvironmentProbe` struct:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct EnvironmentProbe {
      pub detected_at: String,           // ISO 8601 timestamp
      pub languages: Vec<LanguageProbe>, // one per detected language
      pub toolchain_manager: Option<String>, // "mise", "asdf", "devbox", "devcontainer", "none"
      pub ci_environment: Option<CiEnvironment>, // detected CI provider
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct LanguageProbe {
      pub language: Language,
      pub interpreter: Option<String>,     // resolved binary path
      pub version: Option<String>,         // detected version string
      pub package_manager: Option<String>, // uv, npm, cargo, etc.
      pub test_runner: Option<String>,     // pytest, jest, cargo-nextest, etc.
      pub coverage_tool: Option<String>,   // coverage.py, v8, llvm-cov, etc.
      pub build_system: Option<String>,    // gradle, cmake, cargo, etc.
      pub venv_path: Option<PathBuf>,      // Python venv, Go GOPATH, etc.
      pub deps_installed: bool,
      pub warnings: Vec<String>,
      pub extra: HashMap<String, String>,  // language-specific details
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct CiEnvironment {
      pub provider: String,   // "github-actions", "gitlab-ci", "circleci", etc.
      pub is_container: bool,
  }
  ```
- [ ] Add `EnvironmentProbe::save(path)` to write `.apex/environment.json`
- [ ] Add `EnvironmentProbe::load(path)` to read cached probe results
- [ ] Add `EnvironmentProbe::is_stale(max_age: Duration)` to check if re-probe needed
- [ ] Add `[environment]` section to `ApexConfig` in `config.rs`:
  ```rust
  #[derive(Debug, Clone, Default, Deserialize, Serialize)]
  #[serde(default)]
  pub struct EnvironmentConfig {
      /// Path to cached environment probe. Default: ".apex/environment.json"
      pub cache_path: Option<String>,
      /// Max age in seconds before re-probing. Default: 3600 (1 hour).
      pub max_cache_age_secs: u64,
      /// Skip auto-detection entirely if true. Default: false.
      pub skip_detection: bool,
  }
  ```
- [ ] Add `pub mod probe;` to `crates/apex-core/src/lib.rs`
- [ ] Write tests: serialize/deserialize round-trip, stale detection, load from empty/missing file
- [ ] Run `cargo test -p apex-core`
- [ ] Commit

### Task 1.2 -- foundation crew
**Files:** `crates/apex-core/src/probe.rs`

Add the `ProbeCollector` trait that language runners will implement.

- [ ] Define trait:
  ```rust
  pub trait ProbeCollector {
      /// Probe the target directory and return language-specific results.
      fn probe(&self, target: &Path) -> Option<LanguageProbe>;
  }
  ```
- [ ] Add `EnvironmentProbe::from_collectors(target, collectors)` that runs all probes and merges results
- [ ] Add CI environment detection (check `GITHUB_ACTIONS`, `GITLAB_CI`, `CIRCLECI`, `JENKINS_URL` env vars)
- [ ] Add toolchain manager detection (reuse logic from `crates/apex-cli/src/toolchain.rs` `EnvironmentConfig`)
- [ ] Write tests: CI detection with mock env vars, toolchain manager detection with temp dirs
- [ ] Run `cargo test -p apex-core`
- [ ] Commit

---

## Wave 2 (depends on Wave 1: needs EnvironmentProbe + ProbeCollector types)

### Task 2.1 -- runtime crew
**Files:** `crates/apex-lang/src/python.rs`

Implement `ProbeCollector` for `PythonRunner`.

- [ ] Implement `ProbeCollector for PythonRunner`:
  - Resolve interpreter via `resolve_python_for(target)`
  - Get version via `python3 --version`
  - Detect package manager via existing `detect_package_manager()`
  - Detect test runner (pytest/unittest/nose) from existing preflight logic
  - Detect coverage.py version via `coverage --version`
  - Detect venv path via existing `find_venv_python()`
  - Check PEP 668 via existing `is_externally_managed()`
  - Populate `extra` with `pep668: bool`, `pytest_version`, `coverage_version`
- [ ] Write tests with mock filesystem (venv detection, package manager detection)
- [ ] Run `cargo test -p apex-lang -- python`
- [ ] Commit

### Task 2.2 -- runtime crew
**Files:** `crates/apex-lang/src/javascript.rs`, `crates/apex-lang/src/js_env.rs`

Implement `ProbeCollector` for `JavaScriptRunner`.

- [ ] Implement `ProbeCollector for JavaScriptRunner`:
  - Detect runtime (node/bun/deno) via existing `js_env::detect_runtime()`
  - Get version via `node --version` / `bun --version` / `deno --version`
  - Detect package manager via existing `JsEnvironment::detect()`
  - Detect test runner via existing `detect_test_runner()`
  - Detect coverage tool (v8/istanbul/c8) from devDependencies
  - Check TypeScript via existing `detect_typescript()`
  - Detect monorepo via existing `detect_monorepo()`
  - Populate `extra` with `typescript: bool`, `module_system`, `monorepo_kind`
- [ ] Write tests
- [ ] Run `cargo test -p apex-lang -- javascript`
- [ ] Commit

### Task 2.3 -- runtime crew
**Files:** `crates/apex-lang/src/rust_lang.rs`

Implement `ProbeCollector` for `RustRunner`.

- [ ] Implement `ProbeCollector for RustRunner`:
  - Get toolchain via `rustc --version`
  - Detect cargo-llvm-cov via `cargo llvm-cov --version`
  - Detect cargo-nextest via `cargo nextest --version`
  - Detect workspace vs single crate from Cargo.toml
  - Populate `extra` with `workspace: bool`, `edition`, `target_triple`
- [ ] Write tests
- [ ] Run `cargo test -p apex-lang -- rust`
- [ ] Commit

### Task 2.4 -- runtime crew
**Files:** `crates/apex-lang/src/go.rs`, `crates/apex-lang/src/java.rs`, `crates/apex-lang/src/c.rs`, `crates/apex-lang/src/ruby.rs`, `crates/apex-lang/src/swift.rs`, `crates/apex-lang/src/csharp.rs`

Implement `ProbeCollector` for remaining language runners.

- [ ] Implement for GoRunner: go version, go.mod module path, build system
- [ ] Implement for JavaRunner: java version, gradle/maven, JaCoCo detection
- [ ] Implement for CRunner/CppRunner: compiler, build system (cmake/make/meson/xmake), gcov/llvm-cov
- [ ] Implement for RubyRunner: ruby version, bundler, rspec/minitest, simplecov
- [ ] Implement for SwiftRunner: swift version, Xcode/CLT, codecov path
- [ ] Implement for CSharpRunner: dotnet version, solution structure, coverlet
- [ ] Write tests for each
- [ ] Run `cargo test -p apex-lang`
- [ ] Commit

---

## Wave 3 (depends on Wave 2: needs all ProbeCollector implementations)

### Task 3.1 -- platform crew
**Files:** `crates/apex-cli/src/init.rs` (new), `crates/apex-cli/src/lib.rs`

Create `apex init` subcommand.

- [ ] Add `Init` variant to `Commands` enum in `crates/apex-cli/src/lib.rs`:
  ```rust
  /// Auto-detect environment and generate apex.toml.
  Init(InitArgs),
  ```
  with `InitArgs`:
  ```rust
  pub struct InitArgs {
      /// Target directory (default: current directory).
      #[arg(long, default_value = ".")]
      pub target: PathBuf,
      /// Overwrite existing apex.toml if present.
      #[arg(long)]
      pub force: bool,
      /// Output format: "toml" or "json".
      #[arg(long, default_value = "toml")]
      pub format: String,
  }
  ```
- [ ] Create `crates/apex-cli/src/init.rs` implementing `run_init()`:
  1. Instantiate all language runners as `ProbeCollector` implementors
  2. Call `EnvironmentProbe::from_collectors(target, collectors)`
  3. Also run `toolchain::detect_toolchain_versions(target)` for version pins
  4. Generate `apex.toml` with `[environment]` section pre-filled
  5. Write `.apex/environment.json` with cached probe
  6. Print human-readable summary
- [ ] Wire `Init` variant in `run_cli()` dispatcher
- [ ] Add `pub mod init;` to `crates/apex-cli/src/lib.rs`
- [ ] Write test that creates a temp Python project and verifies generated config
- [ ] Run `cargo test -p apex-cli`
- [ ] Commit

### Task 3.2 -- platform crew
**Files:** `crates/apex-cli/src/doctor.rs`

Integrate probe into `apex doctor` output.

- [ ] After running existing doctor checks, also run `EnvironmentProbe::from_collectors()`
- [ ] Display probe results in doctor output (language, version, tools)
- [ ] If `.apex/environment.json` exists and is stale, warn user to re-run `apex init`
- [ ] Write test
- [ ] Run `cargo test -p apex-cli`
- [ ] Commit

### Task 3.3 -- platform crew
**Files:** `crates/apex-cli/src/lib.rs`

Auto-probe on first `apex run` / `apex analyze` if no cached probe exists.

- [ ] In the `run` and `analyze` command handlers, before the main pipeline:
  1. Check if `.apex/environment.json` exists and is fresh
  2. If not, run probe automatically (with `info!` log)
  3. Pass `EnvironmentProbe` to the pipeline for use in runner selection
- [ ] Write integration test
- [ ] Run `cargo test -p apex-cli`
- [ ] Commit

---

## Wave 4 (depends on Wave 3: needs `apex init` + auto-probe working)

### Task 4.1 -- platform crew
**Files:** `crates/apex-cli/src/init.rs`

Config generation: produce complete `apex.toml` from probe results.

- [ ] Map `LanguageProbe` fields to `ApexConfig` sections:
  - Python venv -> `[coverage]` omit_patterns (exclude venv dir)
  - Detected timeouts -> `[instrument.timeouts]` (scale based on project size)
  - Test runner -> auto-set appropriate test command hints
  - Package manager -> auto-set install commands
- [ ] Generate commented TOML with explanations:
  ```toml
  # Auto-generated by `apex init` on 2026-03-23
  # Detected: Python 3.14.3 (uv, pytest, coverage.py 7.13)

  [coverage]
  target = 0.95
  # Omitting .venv detected at /path/to/project/.venv
  omit_patterns = ["__pycache__", ".venv", "node_modules"]
  ```
- [ ] Handle multi-language projects (detect all languages, generate all sections)
- [ ] Write test with multi-language temp project
- [ ] Run `cargo test -p apex-cli`
- [ ] Commit

### Task 4.2 -- foundation crew
**Files:** `crates/apex-core/src/config.rs`

Add `ApexConfig::merge_with_probe()` for runtime config enrichment.

- [ ] Add method that takes an `EnvironmentProbe` and fills in defaults:
  - If probe detects Python venv, ensure `.venv` is in omit_patterns
  - If probe detects slow CI (container), increase timeout defaults
  - If probe detects monorepo, adjust index limits upward
- [ ] Write tests
- [ ] Run `cargo test -p apex-core`
- [ ] Commit

---

## Design Decisions

1. **Probe is synchronous.** All detection uses `std::process::Command` or filesystem checks -- no async needed. This keeps the `ProbeCollector` trait simple.

2. **Cache in `.apex/environment.json`**, not in `apex.toml`. The probe results change frequently (tool versions update, venvs move); config in `apex.toml` should be user-controlled. The cache is machine-local and gitignored.

3. **`apex init` generates `apex.toml`; auto-probe does not.** Running `apex run` will auto-detect and cache to `.apex/environment.json` but will NOT overwrite user's `apex.toml`. Only explicit `apex init` generates/updates the TOML.

4. **`ProbeCollector` reuses existing detection.** Each language runner already has `preflight_check()`, `detect()`, and various helper methods. The `ProbeCollector` implementation wraps these -- no duplication.

5. **CI detection is environment-variable based.** Standard env vars (`GITHUB_ACTIONS`, `CI`, `GITLAB_CI`, etc.) reliably identify the environment. Container detection checks for `/.dockerenv` or cgroup indicators.

## Risks

- **Stale cache.** If a user switches Python versions or installs new tools, the cached probe becomes wrong. Mitigated by the `max_cache_age_secs` config and `is_stale()` check.
- **Slow probe.** Running 12 language probes involves 12+ subprocess calls. Mitigated by only running probes for detected languages (check `detect()` first).
- **Config drift.** Generated `apex.toml` may not track toolchain changes. Mitigated by separating probe cache (`.apex/environment.json`) from user config (`apex.toml`).
