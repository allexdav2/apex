<!-- status: ACTIVE -->

# Bun-First JavaScript Toolchain

**Goal:** When bun is available, use `bun test --coverage` as the primary JS coverage
path. Graceful fallback: bun -> node+npm (nyc/c8/vitest). Follows the same
detect -> prefer -> fallback pattern as the uv-first Python plan.

## Analysis Summary

### Current JS Touchpoints

| File | What it does | Bun status |
|------|-------------|------------|
| `crates/apex-lang/src/js_env.rs` | `JsEnvironment::detect` — detects runtime, pkg manager, test runner | Already detects bun via `bun.lockb`/`bunfig.toml`. Returns `JsRuntime::Bun`, `PkgManager::Bun`, `JsTestRunner::BunTest`. |
| `crates/apex-lang/src/javascript.rs` | `JavaScriptRunner` — installs deps, runs tests | Already calls `bun install` and `bun test` when bun detected. Works. |
| `crates/apex-instrument/src/javascript.rs` | `JavaScriptInstrumentor` — runs coverage tool | `select_coverage_tool` returns `CoverageTool::Bun` with `CoverageOutput::Stdout`, but `instrument()` returns `Err("V8 coverage from stdout not yet implemented")`. **Broken.** |
| `crates/apex-sandbox/src/javascript.rs` | `JavaScriptTestSandbox` — runs probe tests | Hard-codes `node node_modules/.bin/jest`. Ignores bun entirely. |
| `crates/apex-cli/src/doctor.rs` | `checks_javascript` | Only checks `node`, `npm`, `npx`. No bun check. |
| `tests/fixtures/js_project/` | Jest-only fixture | CommonJS + Jest. No bun fixture. |

### What Already Works

- `JsEnvironment::detect` correctly identifies bun projects via `bun.lockb` or `bunfig.toml`
- `JavaScriptRunner` already uses `bun install` and `bun test` for bun projects
- `select_coverage_tool` already has a `JsRuntime::Bun` arm returning `CoverageTool::Bun`
- `js_env::test_command` returns `("bun", ["test"])` for `JsTestRunner::BunTest`

### What Needs to Change

1. **Instrumentor:** `select_coverage_tool` for Bun must produce a file-based coverage output, not stdout. Bun supports `bun test --coverage --coverageReporter=lcov --coverageDir=<dir>`. Alternatively, Bun can output V8-format JSON via `NODE_V8_COVERAGE=<dir> bun test` env var, which writes `.json` files to that directory that our existing `v8_coverage::parse_v8_coverage` can parse.

   **Best approach:** Use `NODE_V8_COVERAGE=<dir>` env var with `bun test`. This writes V8 coverage JSON files to the directory, which we already know how to parse. No new parser needed.

2. **Sandbox:** `JavaScriptTestSandbox` should detect bun and use `bun test` instead of `node node_modules/.bin/jest`.

3. **Doctor:** Add bun as optional (preferred) check. Show message when bun is found.

4. **Fixture:** Add `tests/fixtures/tiny-js/` with a minimal bun-compatible project (also works with node+jest).

5. **Integration test:** End-to-end test that instruments the fixture.

### Bun Coverage Mechanics

Bun supports two coverage mechanisms:
- `bun test --coverage` — prints a text summary table to stdout (not machine-parseable)
- `NODE_V8_COVERAGE=<dir> bun test` — writes V8-format coverage JSON files to `<dir>/` (same format as Node's built-in V8 coverage)

The `NODE_V8_COVERAGE` approach is ideal because:
- It produces files (not stdout), avoiding the stdout parsing TODO
- The output format matches what `v8_coverage::parse_v8_coverage` already handles
- It works with any test runner bun executes, not just `bun test --coverage`

## File Map

| Crew | Files |
|------|-------|
| runtime | `crates/apex-lang/src/js_env.rs`, `crates/apex-lang/src/javascript.rs`, `crates/apex-sandbox/src/javascript.rs` |
| exploration | `crates/apex-instrument/src/javascript.rs` |
| platform | `crates/apex-cli/src/doctor.rs`, `tests/fixtures/tiny-js/` |

## Wave 1 — Runtime: bun-aware coverage tool selection (no dependencies)

### Task 1.1 — runtime crew
**Files:** `crates/apex-lang/src/js_env.rs`

Add a helper to detect whether bun is available on PATH (for projects that do not have `bun.lockb` but where we might still prefer bun). This mirrors `resolve_uv()` from the Python plan.

- [ ] Add `pub fn bun_available() -> bool` using `which::which("bun").is_ok()` or `std::process::Command::new("bun").arg("--version").status().is_ok()`
- [ ] Write unit test for the helper
- [ ] Run `cargo nextest run -p apex-lang`
- [ ] Commit

### Task 1.2 — exploration crew
**Files:** `crates/apex-instrument/src/javascript.rs`

Fix the Bun coverage path in `select_coverage_tool` and `instrument()`:

- [ ] Change `select_coverage_tool` Bun arm: instead of `CoverageOutput::Stdout`, use `CoverageOutput::FilePath` pointing to `<target>/.apex_coverage_js/`. Set `command` to `["bun", "test"]` (coverage is triggered by env var, not CLI flag).
- [ ] In `instrument()` Stage 3, when `config.tool == CoverageTool::Bun`: set env var `NODE_V8_COVERAGE=<report_dir>` on the `CommandSpec` before running. This causes bun to write V8 coverage JSON files to the directory.
- [ ] In Stage 4, V8 format + FilePath: after the command runs, scan the report_dir for `*.json` files (bun writes one per script). Parse and merge them via `v8_coverage::parse_v8_coverage`.
- [ ] Write unit tests with mock CommandRunner: verify bun path sets NODE_V8_COVERAGE env var and reads correct output path
- [ ] Write test: verify fallback to nyc/c8 when runtime is Node
- [ ] Run `cargo nextest run -p apex-instrument`
- [ ] Commit

Note: `CommandSpec` already has `.env(key, value)` builder method in `crates/apex-core/src/command.rs`. No foundation work needed.

## Wave 2 — Runtime + Platform: sandbox and doctor (depends on Wave 1)

### Task 2.1 — runtime crew
**Files:** `crates/apex-sandbox/src/javascript.rs`

Make `JavaScriptTestSandbox` bun-aware:

- [ ] Add `runtime: JsRuntime` field to `JavaScriptTestSandbox`, set during construction
- [ ] When `runtime == JsRuntime::Bun`: use `bun test <test_file>` instead of `node node_modules/.bin/jest <test_file>`
- [ ] When bun: set `NODE_V8_COVERAGE=<coverage_dir>` env var for coverage collection
- [ ] When bun: parse V8 coverage JSON files from coverage_dir (same as Istanbul parse but using `v8_coverage` module)
- [ ] Fallback: keep existing jest/Istanbul path for Node runtime
- [ ] Write tests for bun sandbox path with mock process
- [ ] Run `cargo nextest run -p apex-sandbox`
- [ ] Commit

### Task 2.2 — platform crew
**Files:** `crates/apex-cli/src/doctor.rs`

Update `checks_javascript` to show bun status:

- [ ] Add `bun` as an optional check: `check_optional(runner, "bun", "Bun runtime (preferred)", "bun", &["--version"])`
- [ ] Keep `node` and `npm` as required (they remain the fallback)
- [ ] Keep `npx` as optional
- [ ] Update description of `npx` from "for nyc" to "for nyc/c8 (node fallback)"
- [ ] Write test: mock bun found -> verify it appears as Ok in checks
- [ ] Write test: mock bun missing -> verify it appears as Warn (not Fail)
- [ ] Run `cargo nextest run -p apex-cli`
- [ ] Commit

## Wave 3 — Platform: fixture project and integration test (depends on Wave 1+2)

### Task 3.1 — platform crew
**Files:** `tests/fixtures/tiny-js/`

Create a minimal JS fixture that works with both bun and node+jest:

- [ ] Create `tests/fixtures/tiny-js/package.json`:
  ```json
  {
    "name": "tiny-js",
    "version": "0.1.0",
    "scripts": { "test": "bun test || npx jest" },
    "devDependencies": { "jest": "^29.0.0" }
  }
  ```
- [ ] Create `tests/fixtures/tiny-js/index.js`: simple module with 2-3 branches (if/else)
- [ ] Create `tests/fixtures/tiny-js/index.test.js`: bun-compatible test file (bun's test runner uses `import { expect, test } from "bun:test"` but also supports jest globals when jest is installed)
  - Better approach: use a plain Jest-style test that works under both `bun test` and `npx jest`
- [ ] Create `tests/fixtures/tiny-js/__tests__/index.test.js` for jest compatibility
- [ ] Verify locally: `cd tests/fixtures/tiny-js && bun test` works (if bun installed)
- [ ] Verify locally: `cd tests/fixtures/tiny-js && npx jest` works (node fallback)
- [ ] Commit

### Task 3.2 — platform crew
**Files:** integration test (location TBD based on test harness pattern)

Add integration test for JS baseline:

- [ ] Add `#[ignore]` integration test `test_run_js_baseline_succeeds` that:
  1. Copies `tests/fixtures/tiny-js/` to a temp dir
  2. Runs `JavaScriptRunner::install_deps`
  3. Runs `JavaScriptInstrumentor::instrument` (which runs coverage)
  4. Asserts branches were found (branch_ids.len() > 0)
  5. Asserts some branches were executed (executed_branch_ids.len() > 0)
- [ ] Skip gracefully if neither bun nor node is available
- [ ] Test with bun (if available) and with node fallback
- [ ] Run test locally to verify
- [ ] Commit

## Risk Assessment

- **Low risk:** All changes are additive. Bun detection is runtime-only, no new compile deps.
- **No breaking changes:** Node+npm fallback is unchanged. Bun is only used when detected.
- **ENV var approach is robust:** `NODE_V8_COVERAGE` is a well-documented Node/Bun standard. Bun explicitly supports it.
- **Parser reuse:** V8 coverage JSON format is already parsed by `v8_coverage::parse_v8_coverage`. No new parser needed.
- **No prerequisites:** `CommandSpec::env()` already exists in apex-core. No foundation changes needed.
