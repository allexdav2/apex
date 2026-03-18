<!-- status: ACTIVE -->

# mise Universal Version Manager + Kover Kotlin Coverage

**Date:** 2026-03-18
**Goal:** Two P1 modernizations: (A) detect mise in `apex doctor` for unified version management, (B) add Kover support as preferred Kotlin coverage tool over JaCoCo.

## Analysis

### Current State

**doctor.rs** (apex-cli): 413 lines. Groups checks by language: core, python, javascript, java, c, wasm, firecracker. Each group calls `check_required`/`check_optional` with binary name + version args. No awareness of version managers (mise, asdf, nvm, pyenv, etc.).

**Kotlin coverage pipeline:**
- `apex-lang/src/kotlin.rs` -- KotlinRunner: detect, install_deps, run_tests via Gradle/Maven
- `apex-instrument/src/java.rs` -- JaCoCo instrumentor: runs `gradlew jacocoTestReport` or Maven JaCoCo plugin, parses XML
- `apex-index/src/kotlin.rs` -- Kotlin indexer: calls `parse_jacoco_xml` from java module, builds BranchIndex
- No Kover awareness anywhere in the codebase (grep confirms zero matches)

### File Map

| Crew     | Files                                                  |
|----------|--------------------------------------------------------|
| platform | `crates/apex-cli/src/doctor.rs`                        |
| runtime  | `crates/apex-lang/src/kotlin.rs`                       |
| runtime  | `crates/apex-instrument/src/java.rs` (shared JaCoCo)   |
| runtime  | `crates/apex-index/src/kotlin.rs`                      |

### Crew Assignments

- **platform crew** -- Part A (mise in doctor)
- **runtime crew** -- Part B (Kover for Kotlin)

No foundation changes needed -- core types (BranchId, InstrumentedTarget) already support what both features need.

---

## Wave 1 (no dependencies -- both parts are independent)

### Task 1.1 -- platform crew: mise detection in doctor

**Files:** `crates/apex-cli/src/doctor.rs`

**Context:** mise is a Rust-based universal version manager (`mise.jdx.dev`). When installed, `mise --version` returns version string. `mise ls --json` returns JSON listing managed tool+version pairs. When mise manages a language runtime, APEX can skip individual binary checks for that language since mise guarantees availability via shims or `mise exec`.

- [ ] Add `checks_version_managers(runner)` function that checks for `mise` (optional) and `asdf` (optional)
- [ ] For mise specifically: if found, run `mise ls --current --json` to discover which tools/languages mise manages in the current directory context
- [ ] Add a new `"Version Managers"` print group that appears before all language groups
- [ ] When mise manages a language (e.g., `python`, `node`, `java`), annotate the corresponding language group checks: still run them but note "managed by mise" in the Ok status
- [ ] Write test: `MockRunner` that returns mise version + `mise ls` JSON, verify checks produce Ok status with mise annotation
- [ ] Write test: mise not found, verify Warn status and no change to language checks
- [ ] Run `cargo nextest run -p apex-cli -- doctor`, confirm all pass
- [ ] Commit

### Task 1.2 -- runtime crew: Kover detection and report parsing

**Files:** `crates/apex-instrument/src/java.rs` (add Kover parser), `crates/apex-lang/src/kotlin.rs` (detect Kover plugin)

**Context:** Kover is JetBrains' Kotlin coverage tool. Applied as Gradle plugin: `id("org.jetbrains.kotlinx.kover")` in `build.gradle.kts`. Reports generated via `./gradlew koverXmlReport`. XML output at `build/reports/kover/report.xml`. Kover XML format uses same JaCoCo XML schema (counter elements with BRANCH type), so existing `parse_jacoco_xml` should work -- but verify and document.

- [ ] In `kotlin.rs` (apex-lang): add `pub fn detect_kover_plugin(target: &Path) -> bool` that reads `build.gradle.kts` and checks for `kotlinx.kover` plugin declaration
- [ ] Write test for `detect_kover_plugin`: fixture `build.gradle.kts` with plugin present/absent
- [ ] In `java.rs` (apex-instrument): add `run_kover(target, runner) -> Result<PathBuf>` that runs `./gradlew koverXmlReport --quiet` and returns path to `build/reports/kover/report.xml`
- [ ] Write test for `run_kover` with mock runner: success case returns correct path, failure case returns error
- [ ] Run `cargo nextest run -p apex-instrument -p apex-lang -- kotlin`, confirm all pass
- [ ] Commit

---

## Wave 2 (depends on Wave 1)

### Task 2.1 -- runtime crew: Wire Kover into Kotlin indexer

**Files:** `crates/apex-index/src/kotlin.rs`

**Context:** The Kotlin indexer currently hardcodes JaCoCo. After Wave 1 provides `detect_kover_plugin` and `run_kover`, the indexer should prefer Kover when the plugin is present.

- [ ] In `build_kotlin_index`: call `detect_kover_plugin(target_root)` first
- [ ] If Kover detected: run `koverXmlReport` instead of `jacocoTestReport`, parse the resulting XML with `parse_jacoco_xml` (Kover uses JaCoCo-compatible XML format)
- [ ] If Kover not detected: keep existing JaCoCo path (no regression)
- [ ] Add log line: `info!("using Kover for Kotlin coverage")` or `info!("using JaCoCo for Kotlin coverage")`
- [ ] Write test: mock filesystem with `build.gradle.kts` containing Kover plugin, verify `detect_kover_plugin` returns true and indexer attempts Kover path
- [ ] Write test: mock filesystem without Kover plugin, verify JaCoCo path is used
- [ ] Run `cargo nextest run -p apex-index -- kotlin`, confirm all pass
- [ ] Commit

### Task 2.2 -- platform crew: Integration smoke test

**Files:** `crates/apex-cli/src/doctor.rs` (or integration test file)

- [ ] Write test: full `run_doctor_with_runner` with mock that simulates mise managing python + node, verify output contains "managed by mise" annotations
- [ ] Verify no regressions: run full `cargo nextest run -p apex-cli`, confirm all pass
- [ ] Commit

---

## Verification

After all waves complete:
```bash
cargo check --workspace
cargo nextest run --workspace
cargo clippy --workspace -- -D warnings
```

## Notes

- Kover XML uses the same schema as JaCoCo XML (both report `<counter type="BRANCH">` elements). This means `parse_jacoco_xml` should work without modification. If there are Kover-specific differences (e.g., inline function coverage, coroutine branch IDs), they will surface as test failures in Wave 2.
- mise detection is read-only and informational -- it does not change how APEX runs tools, only what `apex doctor` reports. A future enhancement could use `mise exec <tool> -- <command>` for version-pinned execution.
- asdf is included as a secondary check since mise is asdf-compatible and many users are mid-migration.
