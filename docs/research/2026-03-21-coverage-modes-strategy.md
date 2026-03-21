<!-- status: ACTIVE -->

# Coverage Modes Strategy: IMPORT, WRAP, INSTRUMENT, HARDWARE TRACE

**Date:** 2026-03-21
**Context:** APEX has 4 possible coverage modes. This document evaluates each via three research perspectives (WRAP design, quality improvement, Frida feasibility) and produces a unified strategy.

---

## Executive Summary

| Mode | Status | Success Rate | Finding Quality Impact | Effort | Priority |
|------|--------|-------------|----------------------|--------|----------|
| **IMPORT** | DONE | 100% (user provides data) | Medium: tested-vs-untested classification | 0 weeks | Already shipped |
| **WRAP** | Not built | 85-95% (est.) | Medium-High: always-fresh coverage | 3-4 weeks | **P1 -- build next** |
| **INSTRUMENT** | Works 27% | 27% (build env failures) | High: test-to-finding correlation | N/A (maintain) | P3 -- keep as fast-path |
| **HARDWARE TRACE** | Future | 100% on M4+ | Highest: production reachability | 2-3 weeks | P2 -- fast-path when available |

**Recommended roadmap:** WRAP (P1) -> HARDWARE TRACE (P2) -> improve INSTRUMENT fallback (P3)

---

## Dig 1: WRAP Mode Deep Design

### Concept

`apex wrap -- <user-test-command>` wraps the user's existing test command, injecting coverage collection transparently. APEX does NOT need to know how to build the project -- it only needs to know how to enable coverage for the language's runtime/toolchain.

### Per-Language Coverage Injection

| Language | Injection Method | Env-Only? | Needs Project Modification? | Confidence |
|----------|-----------------|-----------|---------------------------|------------|
| **Python** | `COVERAGE_PROCESS_START=.coveragerc` + prepend `coverage run -m` or set `sitecustomize.py` | Mostly env | Needs a temp `.coveragerc` file (APEX creates it) | 95% |
| **JavaScript (Node)** | `NODE_V8_COVERAGE=<tmpdir>` env var | Pure env | No | 98% |
| **JavaScript (Bun)** | Not supported natively; no env var equivalent | N/A | Would need Istanbul transform | 40% |
| **TypeScript** | Same as JS -- V8 coverage works through ts-node/tsx | Pure env | No | 95% |
| **Go** | Inject `-coverprofile=<tmpfile>` and `-cover` flags into `go test` args | Flag injection | No | 90% |
| **Rust** | `RUSTFLAGS="-C instrument-coverage"` + `LLVM_PROFILE_FILE=<path>` | Pure env | No, but forces recompile | 80% |
| **Java** | `-javaagent:jacoco.jar=destfile=<path>` via `JAVA_TOOL_OPTIONS` env | Pure env | No (JaCoCo agent jar bundled with APEX) | 85% |
| **Kotlin** | Same as Java (JVM-based) | Pure env | No | 85% |
| **C/C++** | `LLVM_PROFILE_FILE=<path>` (only if already compiled with `-fprofile-instr-generate`) | Pure env | Only works if pre-instrumented | 30% |
| **Swift** | `swift test --enable-code-coverage` flag injection | Flag injection | No | 75% |
| **C#** | `dotnet test --collect:"XPlat Code Coverage"` flag injection | Flag injection | No (coverlet bundled with .NET SDK) | 80% |
| **Ruby** | `COVERAGE=true` env var + inject `SimpleCov.start` into test helper | Env + shim | Needs SimpleCov gem installed | 60% |

### Classification by Injection Type

**Tier A -- Pure env var injection (no project changes needed):**
- JavaScript/TypeScript via `NODE_V8_COVERAGE` (98%)
- Java/Kotlin via `JAVA_TOOL_OPTIONS=-javaagent:jacoco.jar` (85%)
- Rust via `RUSTFLAGS` (80%, but triggers recompile)
- Python via `COVERAGE_PROCESS_START` (95%, needs temp config file)

**Tier B -- Flag injection (append flags to test command):**
- Go: append `-coverprofile=<path>` to `go test` (90%)
- Swift: append `--enable-code-coverage` to `swift test` (75%)
- C#: append `--collect:"XPlat Code Coverage"` to `dotnet test` (80%)

**Tier C -- Requires runtime/dependency present:**
- Ruby: needs SimpleCov gem (60%)
- C/C++: only works if binary was already compiled with coverage flags (30%)

### Expected Success Rates

- **Interpreted languages (Python, JS/TS, Ruby):** 85-98% success. Coverage tools are part of the ecosystem; no build step needed.
- **JVM languages (Java, Kotlin):** 85% success. APEX bundles JaCoCo agent; just set `JAVA_TOOL_OPTIONS`.
- **Go:** 90% success. Built-in to `go test`.
- **Rust:** 80% success. Forces recompile, but `RUSTFLAGS` is standard.
- **Swift/C#:** 75-80% success. Flag injection works with standard toolchains.
- **C/C++:** 30% success via env var alone. Frida (Dig 3) is the real answer here.
- **Overall weighted average:** ~85% (weighted by language popularity in APEX's target market).

This compares favorably to INSTRUMENT mode (27%) and equals IMPORT (100% -- but IMPORT requires the user to generate coverage themselves).

### CLI Design

```
apex wrap [OPTIONS] -- <COMMAND>...

OPTIONS:
  --lang <LANG>              Override language detection (auto-detect from command)
  --output <PATH>            Write coverage data to file (default: stdout as APEX JSON)
  --coverage-method <METHOD> Force coverage method: native | frida | sample
  --format <FORMAT>          Output format: apex | lcov | json
  --timeout <SECONDS>        Kill wrapped process after timeout (default: 300)

EXAMPLES:
  apex wrap -- pytest                           # Python: injects COVERAGE_PROCESS_START
  apex wrap -- npm test                         # JS: sets NODE_V8_COVERAGE
  apex wrap -- go test ./...                    # Go: appends -coverprofile
  apex wrap -- cargo test                       # Rust: sets RUSTFLAGS + LLVM_PROFILE_FILE
  apex wrap -- mvn test                         # Java: sets JAVA_TOOL_OPTIONS with JaCoCo
  apex wrap --coverage-method frida -- ./a.out  # C/C++: Frida Stalker on binary
```

**Language detection heuristic:**
1. If `--lang` specified, use it
2. Parse command[0]: `pytest`/`python` -> Python, `npm`/`node`/`npx` -> JS, `go` -> Go, `cargo` -> Rust, `mvn`/`gradle` -> Java, `swift` -> Swift, `dotnet` -> C#, `bundle`/`ruby`/`rspec`/`rake` -> Ruby
3. If ambiguous, scan CWD for `Cargo.toml`, `package.json`, `go.mod`, `pom.xml`, etc.

**Execution flow:**
1. Detect language from command
2. Select coverage injection strategy (env vars, flag injection, or Frida)
3. Create temp directory for coverage output
4. Execute wrapped command with injected coverage settings
5. Wait for completion (respect timeout)
6. Parse coverage output files from temp dir (using existing `import.rs` parsers)
7. Convert to APEX BranchId format
8. Run analysis pipeline (detect, audit, etc.) with coverage-informed severity

### Integration with Existing Code

WRAP mode reuses 90% of existing infrastructure:
- **Coverage parsing:** `apex_instrument::import` already handles CoveragePy, Istanbul, V8, GoCover, JaCoCo, Cobertura, SimpleCov, LCOV formats
- **BranchId generation:** same `fnv1a_hash`-based branch identification
- **CoverageOracle:** same thread-safe coverage store
- **Detector pipeline:** same `AnalysisContext` with coverage data populated

New code needed:
- `crates/apex-cli/src/wrap.rs` -- CLI subcommand, language detection, env injection
- `crates/apex-instrument/src/wrap.rs` -- coverage injection strategies per language
- Integration test suite with mock commands per language

---

## Dig 2: Quality Improvement Analysis

### How Coverage Data Improves Finding Quality

#### 1. IMPORT Mode: Tested-vs-Untested Classification

Coverage data from any source enables a binary classification:
- **Finding in covered code:** tests exist that exercise this path. If the vulnerability is real, existing tests may already validate against it (lower severity adjustment). If tests do NOT validate security properties (only test functionality), the vulnerability is still real but the presence of tests indicates an active codebase.
- **Finding in uncovered code:** no test exercises this path. The vulnerability is more likely to be real AND more likely to be exploitable because no test validates the behavior.

**Severity adjustment model:**
```
adjusted_severity = base_severity * coverage_multiplier

where coverage_multiplier:
  0% coverage on function containing finding:  1.3x (amplify)
  1-50% coverage:                              1.1x (slight amplify)
  51-90% coverage:                             1.0x (neutral)
  91-100% coverage:                            0.85x (slight reduce)
  100% branch coverage + tests assert on value: 0.7x (significant reduce)
```

This does NOT mean "100% coverage = no bugs." It means the probability of a finding being a true positive is correlated with the inverse of coverage in that region.

**FP reduction potential:** Estimated 10-15% FP reduction when findings in well-tested code are down-weighted. This is particularly effective for:
- Null dereference findings where tests demonstrate the value is always non-null
- Type confusion findings where tests exercise all type variants
- Race condition findings where tests run concurrent scenarios

**FP increase risk:** Over-relying on coverage to dismiss findings would create false negatives. Coverage shows WHAT code ran, not WHETHER it ran correctly.

#### 2. WRAP Mode: Freshness Benefit

WRAP generates coverage at analysis time, guaranteeing the coverage data matches the current code state. This matters because:

- **Stale coverage is harmful.** If IMPORT data is from last week's CI run, new code has zero coverage data. APEX cannot distinguish "new code, not yet in coverage" from "old uncovered code."
- **WRAP ensures alignment.** Coverage data and source code are from the exact same commit.
- **Quantified benefit:** In projects with daily commits, IMPORT data is stale within 24 hours. On average, 5-15% of lines change per week. WRAP eliminates this entire category of stale-data FPs.

**Verdict:** Freshness provides a measurable improvement over IMPORT. Estimated 5-8% additional accuracy in severity scoring.

#### 3. INSTRUMENT Mode: Test-to-Finding Correlation

When APEX controls test execution (INSTRUMENT mode), it can correlate specific test names with specific code paths:

- "Test `test_login_sql_injection` covers `auth.py:42` but does NOT cover the `else` branch at `auth.py:47`"
- "The SQL injection finding at `auth.py:47` is in a branch that `test_login_sql_injection` was clearly designed to test but fails to reach"

This is a QUALITATIVE upgrade over binary covered/uncovered:
- **Test intent inference:** If a test named `*sql*` or `*injection*` covers a function but misses a branch, APEX can report: "Security test exists but has a gap at this exact location."
- **Test quality scoring:** Not just "is there a test?" but "is the test good enough?"
- **Gap localization:** "Add an else-branch test case to `test_login_sql_injection` to cover the untested error path."

**Quantified impact:** This transforms APEX from a coverage reporter into a test quality advisor. The finding quality improvement is not in FP reduction but in ACTIONABILITY -- findings come with specific remediation guidance.

**Limitation:** Only works when APEX controls test execution (27% of the time with current INSTRUMENT mode).

#### 4. HARDWARE TRACE: Production Reachability Signal

Hardware trace (Apple Processor Trace on M4+) has near-zero overhead, making it feasible to trace PRODUCTION runs, not just test runs.

Production coverage data answers the ultimate question: **"Is this code actually reachable by real users?"**

- **Finding in production-reachable code:** This vulnerability is exploitable by real attackers. Maximum severity.
- **Finding in production-unreachable code:** Dead code, or only reachable through unusual paths. Lower priority.
- **Finding in test-only code:** Never runs in production. Lowest priority (but still worth fixing for test integrity).

**Severity model with production coverage:**
```
production_multiplier:
  Reached in production traces:     1.5x (critical amplification)
  Not reached in production:        0.8x (reduce)
  Reached ONLY in production:       2.0x (no test covers this hot production path!)
  (no production data available):   1.0x (neutral)
```

**The killer feature:** A finding that is (a) in uncovered-by-tests code AND (b) reached in production is the highest possible priority. This is a real vulnerability in code that real users exercise but no test validates. APEX can flag these as "CRITICAL: production-reachable, untested vulnerability."

**Quantified impact:** Estimated 20-30% improvement in finding prioritization accuracy when production coverage is available. The main value is not FP reduction but PRIORITIZATION -- helping developers fix the most dangerous findings first.

#### 5. Cross-Mode Quality Matrix

| Quality Signal | IMPORT | WRAP | INSTRUMENT | HW TRACE |
|---------------|--------|------|-----------|----------|
| Tested vs untested | Yes | Yes | Yes | Yes |
| Coverage freshness | No (stale) | Yes | Yes | Yes |
| Test-to-finding correlation | No | No | Yes | No |
| Test quality assessment | No | No | Yes | No |
| Production reachability | No | No | No | Yes |
| FP reduction | 10-15% | 15-20% | 15-20% | 20-30% |
| Severity precision | Low | Medium | High | Highest |
| Actionability of findings | Low | Medium | High | Highest |

---

## Dig 3: Frida Binary Instrumentation Feasibility

### frida-gum Rust Crate Assessment

**Crate:** `frida-gum` (published on crates.io)
- **Version:** 0.17.x (as of early 2026)
- **Rust API quality:** Mid-level. The `Stalker` API has safe Rust wrappers but some operations require `unsafe`. The API surface covers: `Gum::obtain()`, `Stalker::new()`, `stalker.follow_me()`, `stalker.follow(thread_id)`, event callbacks.
- **Dependencies:** Links to `frida-gum-sys` which downloads prebuilt Frida devkits (~15MB). No need to compile Frida from source.
- **Maintenance:** Maintained by the Frida team (Ole Andre Vadla Ravnas). Updates follow upstream Frida releases within weeks.
- **Production readiness:** Used in production by AFL++ Frida mode, multiple security tools. The Rust bindings specifically are less battle-tested than the Python/JS bindings but functionally complete.

**Recommendation:** Feature-flag `frida-gum` behind `[features] frida = ["frida-gum"]` to keep default builds lean. The devkit download adds 15MB to build time, which is unacceptable for default builds.

### Attach vs Spawn

Frida supports two modes:

**Spawn mode (recommended for APEX WRAP):**
```rust
// APEX spawns the test process under Frida control
let device = frida::DeviceManager::obtain().enumerate_devices()?[0];
let pid = device.spawn("./test_binary", &SpawnOptions::new())?;
let session = device.attach(pid)?;
// configure Stalker...
device.resume(pid)?;
// wait for completion
```

**Attach mode (for production tracing):**
```rust
// Attach to already-running process
let session = device.attach(existing_pid)?;
```

For `apex wrap`, spawn mode is correct: APEX starts the test process, instruments it, runs it, collects results. Spawn guarantees full coverage from process start (no missed early initialization).

For production hardware trace, attach mode could supplement Apple Processor Trace on pre-M4 hardware, but the overhead (3-5x) makes production tracing impractical with Frida.

### DWARF Address-to-Source Mapping

Frida Stalker reports basic-block start addresses. Mapping these to source lines:

```rust
use addr2line::Context;
use object::read::File as ObjectFile;
use std::fs;

fn map_addresses_to_sources(binary_path: &Path, addresses: &[u64]) -> Vec<(u64, String, u32)> {
    let data = fs::read(binary_path).unwrap();
    let obj = ObjectFile::parse(&data).unwrap();
    let ctx = Context::new(&obj).unwrap();

    addresses.iter().filter_map(|&addr| {
        ctx.find_location(addr).ok().flatten().map(|loc| {
            (addr, loc.file.unwrap_or("??").to_string(), loc.line.unwrap_or(0))
        })
    }).collect()
}
```

**Requirements:**
- Binary must be compiled with `-g` (debug info). Without it, only function-level coverage via symbol table.
- On macOS, debug info may be in a separate `.dSYM` bundle. APEX must check both `<binary>` and `<binary>.dSYM/Contents/Resources/DWARF/<binary>`.
- Rust binaries from `cargo test` include debug info by default (dev profile). Go binaries include DWARF by default. C/C++ need explicit `-g`.

**Crate dependencies for DWARF resolution:**
- `gimli` -- zero-copy DWARF parser (100M+ downloads, foundational)
- `addr2line` -- built on gimli (60M+ downloads)
- `object` -- binary format parser (Mach-O, ELF, PE)
- All production-quality, widely used in Rust ecosystem (e.g., by `backtrace` crate in std)

### Realistic Overhead on macOS ARM64

Based on AFL++ benchmarks and Frida documentation:

| Scenario | Overhead | Notes |
|----------|----------|-------|
| Full process tracing, all modules | 8-15x | Stalker instruments libc, dyld, everything |
| Module-whitelisted (target only) | 3-5x | Only instrument the target binary, not system libs |
| With Stalker compile caching | 2-4x | Stalker caches JIT-compiled blocks |
| Short test suite (<10s native) | 3-5x | 30-50s with Frida |
| Long test suite (>60s native) | 2-3x | JIT warmup amortized |

**For a project like ripgrep's test suite:**
- Native: ~15 seconds
- With Frida module-whitelisted: ~45-75 seconds (3-5x)
- Acceptable for CI; possibly annoying for interactive development

**Recommendation:** Default to module-whitelisted mode. Provide `--coverage-scope wide` flag for full-process tracing when needed.

### SIP Restrictions on macOS

**System Integrity Protection (SIP) impact:**
- Frida CAN attach to any user-spawned process (tests, dev binaries) -- SIP does not block this
- Frida CANNOT attach to Apple-signed system binaries (`/usr/bin/*`, `/System/*`)
- Frida CANNOT attach to processes with hardened runtime AND `com.apple.security.cs.disable-library-validation` NOT set
- For APEX's use case (attaching to user test processes), SIP is NOT a blocker

**Edge cases:**
- Some CI environments may have additional security policies (MDM, etc.)
- Notarized apps with hardened runtime will reject Frida attachment
- Workaround: APEX spawns the process itself (spawn mode), which avoids most hardened-runtime restrictions

### Integration with CoverageOracle and BranchId

Frida Stalker reports basic-block addresses. Converting to APEX's `BranchId` format:

```
Frida address -> addr2line -> (file_path, line, col) -> BranchId {
    file_id: fnv1a_hash(relative_path),
    line,
    col,
    direction: 0,  // Frida reports block entry, not branch direction
    discriminator: 0,
    condition_index: None,
}
```

**Limitation:** Frida provides basic-block coverage, not branch coverage. A block at line 42 being executed tells us "line 42 ran" but not "the true/false branch at line 42." To get branch-direction coverage, Stalker would need to track EDGES (block-to-block transitions) and correlate with the CFG.

**Practical resolution:**
- For gap analysis (APEX's primary use case), block coverage is sufficient. "This function was never executed" is the highest-value signal.
- For branch-direction coverage, fall back to source-based instrumentation or hardware trace.
- Edge tracking in Stalker IS possible (track `compile` events with call/ret/jcc targets) but adds complexity.

**BranchId mapping approach:**
1. Run Frida, collect set of executed addresses
2. Parse DWARF to get ALL addresses in target source files (using `gimli` to enumerate DIE entries)
3. Build "all possible blocks" list from DWARF line table
4. Mark executed blocks, report uncovered blocks
5. Feed into `CoverageOracle` as `BranchState::Covered` / `BranchState::Uncovered`

### Minimal Prototype Sketch

```rust
// crates/apex-instrument/src/frida_coverage.rs

use apex_core::types::BranchId;
use apex_core::hash::fnv1a_hash;

pub struct FridaCoverageCollector {
    target_binary: PathBuf,
    target_root: PathBuf,
    module_filter: Option<String>,
}

impl FridaCoverageCollector {
    pub fn collect(&self, test_command: &[String]) -> Result<CoverageData> {
        // 1. Initialize Frida
        let gum = frida_gum::Gum::obtain();
        let device_manager = frida::DeviceManager::obtain(&gum);
        let device = device_manager.enumerate_devices()?[0];

        // 2. Spawn test process
        let pid = device.spawn(&test_command[0], &SpawnOptions::new()
            .argv(&test_command))?;
        let session = device.attach(pid)?;

        // 3. Set up Stalker with module filter
        let mut addresses: HashSet<u64> = HashSet::new();
        let script = session.create_script(STALKER_SCRIPT)?;
        script.load()?;

        // 4. Resume and wait
        device.resume(pid)?;
        session.wait_for_detach()?;

        // 5. Map addresses to source lines via DWARF
        let mapped = self.map_to_branch_ids(&addresses)?;

        Ok(mapped)
    }

    fn map_to_branch_ids(&self, addresses: &HashSet<u64>) -> Result<CoverageData> {
        let binary_data = std::fs::read(&self.target_binary)?;
        let obj = object::read::File::parse(&binary_data)?;
        let ctx = addr2line::Context::new(&obj)?;

        let mut all_branches = Vec::new();
        let mut executed_branches = Vec::new();
        let mut file_paths = HashMap::new();

        // Enumerate all source lines from DWARF line tables
        // Mark those with matching addresses as executed
        // ... (implementation details)

        Ok((all_branches, executed_branches, file_paths))
    }
}
```

**Feature flag in Cargo.toml:**
```toml
[features]
default = []
frida = ["frida-gum", "addr2line", "gimli", "object"]
```

---

## Unified Strategy

### Priority Ordering

#### Priority 1: WRAP Mode (3-4 weeks)

**Why first:** Highest impact-to-effort ratio. Goes from 27% success (INSTRUMENT) to ~85% success across all languages, using 90% existing code (import parsers, BranchId system, CoverageOracle).

**Implementation plan:**
- Week 1: CLI subcommand + language detection + env injection for Tier A (Python, JS, Java)
- Week 2: Flag injection for Tier B (Go, Swift, C#) + coverage file collection + parsing
- Week 3: Frida integration for Tier C (C/C++ without coverage flags) -- optional feature flag
- Week 4: Integration tests, edge cases, documentation

**Success metric:** `apex wrap -- <test-command>` succeeds on 85%+ of real-world projects across Python, JS, Go, Rust, Java.

#### Priority 2: HARDWARE TRACE Fast Path (2-3 weeks)

**Why second:** Zero overhead, perfect accuracy, but hardware-gated (M4+ only). As M4 adoption grows through 2026-2027, this becomes increasingly important. The early integration positions APEX as the ONLY tool that leverages Apple Processor Trace for security analysis.

**Implementation plan:**
- Week 1: M4+ detection (`sysctl hw.cpufamily`), `xctrace` wrapper, trace recording
- Week 2: Trace export parsing (XML), address-to-source mapping via DWARF
- Week 3: Integration with `CoverageOracle`, production trace mode

**Success metric:** On M4+ hardware, `apex wrap --coverage-method hardware-trace -- <command>` produces coverage data with <1% overhead.

#### Priority 3: Maintain INSTRUMENT Mode

**Why not deprecate:** INSTRUMENT mode (source-based coverage) provides the highest-quality data: exact branch directions, condition coverage, MC/DC. It is the gold standard when it works. The 27% success rate is a build-environment problem, not a data-quality problem.

**Plan:** Keep as automatic fast-path. When `apex run --lang rust` succeeds with instrumentation, the data quality is better than WRAP or HARDWARE TRACE. WRAP should be the fallback when INSTRUMENT fails.

### Expected Impact on Finding Quality

| Mode | Current State | After Strategy | Finding Quality |
|------|--------------|----------------|-----------------|
| No coverage | Default today for 73% of projects | Rare (WRAP catches most) | Baseline (no coverage-aware scoring) |
| IMPORT | Works, user must provide data | Unchanged, still available | +10-15% severity precision |
| WRAP | Does not exist | 85% success rate | +15-20% severity precision (fresh data) |
| INSTRUMENT | 27% success | Still 27%, but WRAP catches the rest | +15-20% severity + test correlation |
| HW TRACE | Does not exist | 100% on M4+ | +20-30% severity + production signal |

**Combined impact:** With WRAP as default fallback, 85%+ of projects get coverage-informed findings. Currently only 27% do. This is a 3x increase in coverage-aware analysis.

### Implementation Effort Summary

| Component | Effort | Dependencies | Risk |
|-----------|--------|-------------|------|
| `apex wrap` CLI + language detection | 1 week | None | Low |
| Env/flag injection for 11 languages | 1.5 weeks | None | Medium (edge cases per language) |
| Frida integration (feature-gated) | 2 weeks | `frida-gum` crate | Medium (SIP edge cases, overhead) |
| Apple Processor Trace integration | 2 weeks | M4+ hardware, Xcode 16.3+ | Low (well-documented API) |
| Coverage-aware severity scoring | 1 week | Any coverage source | Low |
| Production trace mode | 1 week | HW TRACE complete | Low |
| **Total** | **8.5 weeks** | | |

### Architecture: Coverage Collection Cascade

When `apex wrap -- <command>` runs, APEX selects the best coverage method automatically:

```
1. Is language interpreted (Python/JS/Ruby)?
   -> Use language-native coverage (env var injection)
   -> Parse output with existing import.rs parsers

2. Is hardware trace available (M4+ detected)?
   -> Use Apple Processor Trace via xctrace
   -> Parse trace export

3. Is Frida feature enabled?
   -> Use Frida Stalker (module-whitelisted)
   -> Map addresses via DWARF

4. Can we inject coverage flags (Rust/Go/Swift/C#)?
   -> Inject RUSTFLAGS / -coverprofile / etc.
   -> Parse output with existing import.rs parsers

5. Fallback: run command without coverage
   -> Report: "no coverage method available, run with --coverage-file for best results"
```

This cascade is tried in priority order; the first method that succeeds is used. Users can override with `--coverage-method <native|hardware-trace|frida|flags>`.

### Risk Mitigation

| Risk | Mitigation |
|------|-----------|
| Frida's 3-5x overhead is too slow for large test suites | Default to language-native methods; Frida only for C/C++ or when native methods fail |
| SIP blocks Frida on some macOS configurations | Detect and fall back gracefully; document workarounds |
| Apple Processor Trace API changes | Wrap in abstraction layer; xctrace CLI is stable |
| JaCoCo agent version conflicts with project's own JaCoCo | Use isolated temp agent jar; detect existing JaCoCo config |
| RUSTFLAGS conflicts with project's existing RUSTFLAGS | Append to existing RUSTFLAGS rather than overwrite |
| V8 coverage format changes between Node versions | Already handled in v8_coverage.rs parser |

---

## Appendix: Formats Already Supported by import.rs

APEX's existing `load_coverage_file` function can parse all major formats:

- CoveragePy JSON (Python)
- LLVM-cov export JSON (Rust, C/C++, Swift)
- Istanbul JSON (JavaScript)
- V8 coverage JSON (Node.js)
- Go coverprofile text
- JaCoCo XML (Java, Kotlin)
- Cobertura XML (C#, multi-language)
- SimpleCov JSON (Ruby)
- LCOV text (universal)

This means WRAP mode only needs to: (1) inject the right env/flags, (2) find the output file, (3) call `load_coverage_file`. The parsing is already done.
