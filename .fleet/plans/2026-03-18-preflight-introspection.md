<!-- status: DONE -->

# Project Introspection and Preflight Checks for Language Runners

## Problem

`apex run` fails on real-world projects because language runners blindly invoke tools without checking:
1. What build system the project uses
2. Whether required tools are installed and on PATH
3. Whether the environment needs setup (venv, JAVA_HOME, etc.)

## File Map

| Crew | Files |
|------|-------|
| lang-python | crates/apex-lang/src/python.rs |
| lang-js | crates/apex-lang/src/javascript.rs, crates/apex-lang/src/js_env.rs |
| lang-jvm (java) | crates/apex-lang/src/java.rs |
| lang-jvm (kotlin) | crates/apex-lang/src/kotlin.rs |
| lang-go | crates/apex-lang/src/go.rs |
| lang-swift | crates/apex-lang/src/swift.rs |
| lang-dotnet | crates/apex-lang/src/csharp.rs |
| lang-ruby | crates/apex-lang/src/ruby.rs |
| lang-c-cpp (c) | crates/apex-lang/src/c.rs |
| lang-c-cpp (cpp) | crates/apex-lang/src/cpp.rs |
| lang-rust | crates/apex-lang/src/rust_lang.rs |

## Wave 1 (all 9 in parallel -- each touches only their own lang files)

### Task 1.1 -- Python crew
**Files:** crates/apex-lang/src/python.rs
- [x] Add `ProjectInfo` struct with detected config (test framework, package manager, venv path, PEP 668 status)
- [x] Add `preflight_check(target) -> Result<ProjectInfo>` method
- [x] Detect pytest vs unittest vs nose from imports/configs
- [x] Handle PEP 668 externally-managed Python (already partially done via `is_externally_managed`)
- [x] Handle stdlib source dirs (no setup.py/pyproject.toml) -- use system python with `--rootdir`
- [x] Create venv OUTSIDE target dir if target is a source tree
- [x] Add tests with mock project layouts
- [x] Run `cargo test -p apex-lang` for python tests
- [x] Commit

### Task 1.2 -- JavaScript crew
**Files:** crates/apex-lang/src/javascript.rs, crates/apex-lang/src/js_env.rs
- [x] Add `ProjectInfo` struct with detected config
- [x] Add `preflight_check(target) -> Result<ProjectInfo>` method
- [x] Check if `node_modules/` exists (skip npm install if so)
- [x] Sniff Node.js version for V8 coverage format compatibility
- [x] Detect monorepo (lerna.json, nx.json, pnpm-workspace.yaml)
- [x] Add tests
- [x] Commit

### Task 1.3 -- Java crew
**Files:** crates/apex-lang/src/java.rs
- [x] Add `ProjectInfo` with build tool, JaCoCo config, multi-module detection
- [x] Add `preflight_check(target) -> Result<ProjectInfo>`
- [x] For Gradle multi-module: find which subprojects have tests
- [x] Check if JaCoCo plugin is already configured
- [x] Verify XML report path matches what the project produces
- [x] Check for gradlew vs gradle vs mvn on PATH
- [x] Add tests
- [x] Commit

### Task 1.4 -- Kotlin crew
**Files:** crates/apex-lang/src/kotlin.rs
- [x] Add `ProjectInfo` with build tool, Kover/JaCoCo detection, multiplatform flag
- [x] Add `preflight_check(target) -> Result<ProjectInfo>`
- [x] Handle Kotlin multiplatform projects (detect from build.gradle.kts)
- [x] Check for Kover vs JaCoCo (already partially done via `detect_kover_plugin`)
- [x] Verify gradlew exists and is executable
- [x] Add tests
- [x] Commit

### Task 1.5 -- Go crew
**Files:** crates/apex-lang/src/go.rs
- [x] Add `ProjectInfo` with module path, Go version, monorepo flag
- [x] Add `preflight_check(target) -> Result<ProjectInfo>`
- [x] Parse `go.mod` for module path
- [x] Detect monorepo layout (multiple go.mod files, or top-level go.mod with many packages)
- [x] Verify `go` binary on PATH and check version
- [x] Add tests
- [x] Commit

### Task 1.6 -- Swift crew
**Files:** crates/apex-lang/src/swift.rs
- [x] Add `ProjectInfo` with Xcode vs CommandLineTools, codecov path
- [x] Add `preflight_check(target) -> Result<ProjectInfo>`
- [x] Check Package.swift for swift-tools-version
- [x] Verify `swift test --enable-code-coverage` is supported
- [x] Use `swift test --show-codecov-path` to get actual coverage JSON path
- [x] Detect Xcode vs CommandLineTools toolchain
- [x] Add tests
- [x] Commit

### Task 1.7 -- C# crew
**Files:** crates/apex-lang/src/csharp.rs
- [x] Add `ProjectInfo` with solution/project structure, coverlet status
- [x] Add `preflight_check(target) -> Result<ProjectInfo>`
- [x] Check for `*.sln` vs `*.csproj` (already partially done in `detect`)
- [x] Verify `dotnet` on PATH (already has `dotnet_path()` helper)
- [x] Check if coverlet is in test project dependencies
- [x] Add tests
- [x] Commit

### Task 1.8 -- Ruby crew
**Files:** crates/apex-lang/src/ruby.rs
- [x] Add `ProjectInfo` with test framework, Ruby version, bundler version
- [x] Add `preflight_check(target) -> Result<ProjectInfo>`
- [x] Detect RSpec vs Minitest (already done via `detect_test_runner`)
- [x] Check Ruby version >= 3.0 (already done via `resolve_ruby`)
- [x] Detect Bundler version mismatch from Gemfile.lock (already done via `detect_required_bundler`)
- [x] Check for missing system headers (mysql-client etc.) by inspecting Gemfile
- [x] Add tests
- [x] Commit

### Task 1.9a -- C crew
**Files:** crates/apex-lang/src/c.rs
- [x] Add `ProjectInfo` with build system, compiler, coverage backend
- [x] Add `preflight_check(target) -> Result<ProjectInfo>`
- [x] Detect Makefile vs CMakeLists.txt vs Meson vs xmake (already done)
- [x] Check if `clang` supports `-fprofile-instr-generate`
- [x] Check if `gcov` is available as fallback
- [x] Detect kernel/subsystem projects (no standalone build)
- [x] Add tests
- [x] Commit

### Task 1.9b -- C++ crew
**Files:** crates/apex-lang/src/cpp.rs
- [x] Same as C but for C++ runner
- [x] Add GoogleTest detection (already done via `has_googletest`)
- [x] Add tests
- [x] Commit

### Task 1.10 -- Rust crew
**Files:** crates/apex-lang/src/rust_lang.rs
- [x] Add `ProjectInfo` with workspace detection, llvm-cov status, nextest availability
- [x] Add `preflight_check(target) -> Result<ProjectInfo>`
- [x] Detect workspace vs single-crate
- [x] Check `cargo-llvm-cov` installed
- [x] Check `cargo-nextest` installed
- [x] Add tests
- [x] Commit
