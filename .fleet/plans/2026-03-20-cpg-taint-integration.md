<!-- status: ACTIVE -->

# CPG Taint Integration -- Replace Substring Matching with Reachability

**Goal:** Wire the existing CPG taint analysis into the security detector pipeline so
detectors use data-flow reachability instead of substring/regex heuristics.
When a CPG is available, only flag findings where untrusted input actually flows
to a dangerous sink. Fall back to current pattern matching when no CPG exists.

## Current State

| Component | Status | Location |
|-----------|--------|----------|
| CPG structure | Done | `crates/apex-cpg/src/lib.rs` -- nodes, edges, merge |
| Python CPG builder | Done | `crates/apex-cpg/src/builder.rs` -- line-based parser |
| Reaching definitions | Done | `crates/apex-cpg/src/reaching_def.rs` -- iterative MOP |
| Taint analysis | Done | `crates/apex-cpg/src/taint.rs` -- backward BFS with sanitizers |
| TaintSpecStore | Done | `crates/apex-cpg/src/taint_store.rs` -- runtime-extensible specs |
| CPG built in CLI `run()` | Done | `crates/apex-cli/src/lib.rs:966` -- Python only, no `add_reaching_def_edges` |
| CPG in AnalysisContext | Done | `crates/apex-detect/src/context.rs:25` -- `cpg: Option<Arc<Cpg>>` |
| Detectors using CPG | **Not done** | All detectors use substring matching only |
| JS/TS CPG builder | **Not done** | Only Python builder exists |
| Go/Rust CPG builder | **Not done** | Only Python builder exists |

### Critical Gap

The CPG built in `run()` (line 966) does NOT call `add_reaching_def_edges()`.
Without ReachingDef edges, backward taint traversal returns nothing.
Only the standalone `data-flow` subcommand (line 4089) calls it.

## File Map

| Crew | Files |
|------|-------|
| foundation | `crates/apex-cpg/src/builder.rs` (new builders), `crates/apex-cpg/src/lib.rs` (builder trait) |
| security-detect | `crates/apex-detect/src/detectors/security_pattern.rs`, `multi_command_injection.rs`, `multi_sql_injection.rs`, `multi_ssrf.rs` |
| platform | `crates/apex-cli/src/lib.rs` (add reaching_def call, multi-lang CPG dispatch) |

---

## Wave 1 -- Foundation: CPG pipeline fix + builder trait (no dependencies)

### Task 1.1 -- platform crew
**Files:** `crates/apex-cli/src/lib.rs`
**Summary:** Add `add_reaching_def_edges` call after CPG construction in `run()`

This is the single most impactful change. The CPG already exists for Python but
has no data-flow edges, making taint analysis impossible for detectors.

- [ ] In `run()` at line ~973 (after `combined_cpg.merge(file_cpg)`), add `apex_cpg::reaching_def::add_reaching_def_edges(&mut combined_cpg)` before wrapping in `Arc`
- [ ] Do the same for all other CPG construction sites (lines 1361, 2168, 2760, 4198)
- [ ] Write test: build CPG for a Python project, verify ReachingDef edges exist
- [ ] Run `cargo nextest run -p apex-cli` -- confirm pass
- [ ] Commit

### Task 1.2 -- foundation crew
**Files:** `crates/apex-cpg/src/lib.rs`, new file `crates/apex-cpg/src/builder_trait.rs`
**Summary:** Extract a `CpgBuilder` trait so language-specific builders share an interface

- [ ] Define trait:
  ```rust
  pub trait CpgBuilder {
      fn language(&self) -> &str;
      fn build(&self, source: &str, filename: &str) -> Cpg;
  }
  ```
- [ ] Implement `CpgBuilder` for existing `PythonCpgBuilder` (keep `build_python_cpg` as convenience fn)
- [ ] Write test: `PythonCpgBuilder` satisfies the trait contract
- [ ] Run `cargo nextest run -p apex-cpg` -- confirm pass
- [ ] Commit

### Task 1.3 -- security-detect crew
**Files:** `crates/apex-detect/src/detectors/security_pattern.rs`
**Summary:** Add a `taint_check` helper that queries the CPG for source-to-sink reachability

This is the core integration point. When `ctx.cpg` is `Some`, check if any taint
flow exists from a source to the detected sink call. If no flow exists, suppress
the finding (or downgrade severity).

- [ ] Add helper function:
  ```rust
  fn taint_confirms_flow(cpg: &apex_cpg::Cpg, sink_name: &str, line: u32) -> bool
  ```
  that finds the Call node matching `sink_name` at `line`, then runs backward
  BFS via `apex_cpg::taint::reachable_by` to check if any source reaches it.
- [ ] Write test with a manually-constructed CPG: taint flow present -> returns true
- [ ] Write test with a manually-constructed CPG: no taint flow -> returns false
- [ ] Run `cargo nextest run -p apex-detect` -- confirm pass
- [ ] Commit

---

## Wave 2 -- Integration: wire taint checks into detectors (depends on Wave 1)

### Task 2.1 -- security-detect crew
**Files:** `crates/apex-detect/src/detectors/security_pattern.rs`
**Summary:** In `SecurityPatternDetector::analyze`, use CPG taint when available

- [ ] After sink pattern match (line ~1095), check `if let Some(cpg) = &ctx.cpg`
- [ ] If CPG available: call `taint_confirms_flow(cpg, pattern.sink, line_1based)`
- [ ] If taint confirms NO flow: skip finding (continue) or downgrade to Info
- [ ] If CPG not available: fall back to existing substring logic (no regression)
- [ ] Write test: Python source with safe `Command::new("cargo")` -- no finding with CPG
- [ ] Write test: Python source with tainted `subprocess.run(user_input)` -- finding with CPG
- [ ] Write test: non-Python source -- falls back to substring matching
- [ ] Run `cargo nextest run -p apex-detect` -- confirm pass
- [ ] Commit

### Task 2.2 -- security-detect crew
**Files:** `crates/apex-detect/src/detectors/multi_command_injection.rs`
**Summary:** Add CPG taint check to MultiCommandInjectionDetector

- [ ] Same pattern as 2.1: check `ctx.cpg`, call taint_confirms_flow, fall back if absent
- [ ] Write test: tainted input to `os.system` -- finding preserved
- [ ] Write test: hardcoded string to `subprocess.call` -- finding suppressed with CPG
- [ ] Run `cargo nextest run -p apex-detect` -- confirm pass
- [ ] Commit

### Task 2.3 -- security-detect crew
**Files:** `crates/apex-detect/src/detectors/multi_sql_injection.rs`
**Summary:** Add CPG taint check to MultiSqlInjectionDetector

- [ ] Same pattern: check `ctx.cpg`, call taint_confirms_flow
- [ ] Write test: `cursor.execute(f"SELECT {user_id}")` -- finding preserved
- [ ] Write test: `cursor.execute(CONSTANT_SQL)` -- finding suppressed with CPG
- [ ] Run `cargo nextest run -p apex-detect` -- confirm pass
- [ ] Commit

### Task 2.4 -- security-detect crew
**Files:** `crates/apex-detect/src/detectors/multi_ssrf.rs`
**Summary:** Add CPG taint check to MultiSsrfDetector

- [ ] Same pattern: check `ctx.cpg`, call taint_confirms_flow
- [ ] Write test: `requests.get(user_url)` -- finding preserved
- [ ] Write test: `requests.get("https://api.internal.com")` -- finding suppressed
- [ ] Run `cargo nextest run -p apex-detect` -- confirm pass
- [ ] Commit

---

## Wave 3 -- Multi-language CPG builders (depends on Wave 1.2 trait)

### Task 3.1 -- foundation crew
**Files:** new file `crates/apex-cpg/src/builder_js.rs`
**Summary:** JavaScript/TypeScript CPG builder using line-based parsing (same approach as Python)

- [ ] Parse: `function name(params) {`, `const name = (params) => {`, arrow fns
- [ ] Parse: `name(args)` calls, `const x = expr` assignments
- [ ] Parse: `if/while/for/try` control structures
- [ ] Generate CFG edges between sequential statements in function bodies
- [ ] Add `build_js_cpg(source, filename) -> Cpg` public function
- [ ] Implement `CpgBuilder` trait
- [ ] Write tests: method detection, call detection, CFG edges, assignments
- [ ] Run `cargo nextest run -p apex-cpg` -- confirm pass
- [ ] Commit

### Task 3.2 -- foundation crew
**Files:** new file `crates/apex-cpg/src/builder_go.rs`
**Summary:** Go CPG builder using line-based parsing

- [ ] Parse: `func name(params) {` and `func (recv) name(params) {`
- [ ] Parse: `name(args)` calls, `x := expr` and `x = expr` assignments
- [ ] Parse: `if/for/switch` control structures
- [ ] Generate CFG edges between sequential statements
- [ ] Add `build_go_cpg(source, filename) -> Cpg` public function
- [ ] Implement `CpgBuilder` trait
- [ ] Write tests: method detection, receiver methods, call detection, assignments
- [ ] Run `cargo nextest run -p apex-cpg` -- confirm pass
- [ ] Commit

### Task 3.3 -- foundation crew
**Files:** `crates/apex-cpg/src/taint_store.rs`, new file `crates/apex-cpg/src/taint_specs.rs`
**Summary:** Add JS and Go taint source/sink/sanitizer defaults

- [ ] Add `TaintSpecStore::javascript_defaults()` with JS sources (req.body, req.query, process.argv), sinks (child_process.exec, eval), sanitizers (escape)
- [ ] Add `TaintSpecStore::go_defaults()` with Go sources (r.FormValue, os.Args), sinks (exec.Command, sql.Query), sanitizers (filepath.Clean)
- [ ] Write tests for each language's defaults
- [ ] Run `cargo nextest run -p apex-cpg` -- confirm pass
- [ ] Commit

---

## Wave 4 -- Platform wiring for multi-language CPG (depends on Wave 3)

### Task 4.1 -- platform crew
**Files:** `crates/apex-cli/src/lib.rs`
**Summary:** Dispatch to language-specific CPG builder in `run()`

- [ ] Replace Python-only CPG construction with language dispatch:
  ```rust
  let cpg = match lang {
      Language::Python => build_python_cpg(...),
      Language::JavaScript | Language::TypeScript => build_js_cpg(...),
      Language::Go => build_go_cpg(...),
      _ => None,
  };
  ```
- [ ] Always call `add_reaching_def_edges` after building (from Task 1.1)
- [ ] Apply same dispatch to all CPG construction sites
- [ ] Write test: JS project gets CPG built
- [ ] Write test: Go project gets CPG built
- [ ] Run `cargo nextest run -p apex-cli` -- confirm pass
- [ ] Commit

### Task 4.2 -- security-detect crew
**Files:** `crates/apex-detect/src/detectors/security_pattern.rs`
**Summary:** End-to-end integration test with multi-language CPG

- [ ] Write integration test: Python project with tainted and safe patterns -- verify FP reduction
- [ ] Write integration test: JS project with `child_process.exec(userInput)` -- finding
- [ ] Write integration test: JS project with `child_process.exec("ls")` -- no finding (with CPG)
- [ ] Run `cargo nextest run -p apex-detect` -- confirm pass
- [ ] Commit

---

## Dependency Graph

```
Wave 1: [1.1 platform] [1.2 foundation] [1.3 security-detect]  -- all parallel
         |                |                |
Wave 2:  |                |               [2.1] -> [2.2] -> [2.3] -> [2.4]
         |                |                  (sequential within security-detect)
Wave 3:  |               [3.1 foundation] [3.2 foundation] [3.3 foundation]
         |                |                |                |
Wave 4: [4.1 platform depends on 3.1, 3.2] [4.2 security-detect depends on 4.1]
```

## Success Criteria

1. Python projects: security findings only raised when taint flow confirmed (FP reduction)
2. Non-Python projects: no regression (substring fallback preserved)
3. JS/Go projects: CPG built and taint analysis available (new capability)
4. All existing tests pass (`cargo nextest run --workspace`)
5. No new clippy warnings (`cargo clippy --workspace -- -D warnings`)

## Risk Assessment

| Risk | Mitigation |
|------|------------|
| CPG builder misses AST patterns -> false negatives | Fallback to substring matching when CPG coverage < threshold |
| ReachingDef computation too slow for large codebases | Already iterative MOP; can add timeout or file-count limit |
| Taint specs incomplete for JS/Go | Start with conservative defaults, expand via TaintSpecStore |
| Breaking existing detector tests | All changes gated behind `if let Some(cpg)` -- no CPG = no change |
