<!-- status: PARKED -->

# Config Wiring Plan: Hardcoded Constants -> apex.toml

Generated 2026-03-18 from full codebase audit.

## Summary

The reference config (`apex.reference.toml`) documents 80+ parameters. Of these,
~30 are already wired to `ApexConfig` and read from `apex.toml`. The remaining
~50 are hardcoded constants that users cannot override without recompilation.

This plan prioritizes wiring by user impact.

---

## Priority 1: High-Impact (users will want to tune these)

### 1.1 Language-specific instrumentation timeouts

**Problem:** All language toolchain timeouts (JVM build, C compile, Swift test,
etc.) are hardcoded per-file constants ranging from 60s to 600s. Users with slow
CI or large projects cannot override them.

**Files:**
- `crates/apex-instrument/src/c_coverage.rs` -- COMPILE_TIMEOUT_MS (300s), TEST_RUN_TIMEOUT_MS (120s), GCOV_TIMEOUT_MS (60s)
- `crates/apex-instrument/src/csharp.rs` -- INSTRUMENT_TIMEOUT_MS (600s)
- `crates/apex-instrument/src/swift.rs` -- COV_TEST_TIMEOUT_MS (600s), CODECOV_PATH_TIMEOUT_MS (60s)
- `crates/apex-instrument/src/java.rs` -- JVM_BUILD_TIMEOUT_MS (600s)
- `crates/apex-lang/src/csharp.rs` -- RESTORE_TIMEOUT_MS (300s), TEST_TIMEOUT_MS (600s)
- `crates/apex-lang/src/swift.rs` -- RESOLVE_TIMEOUT_MS (300s), TEST_TIMEOUT_MS (600s)
- `crates/apex-lang/src/java.rs` -- JVM_BUILD_TIMEOUT_MS (600s)
- `crates/apex-lang/src/kotlin.rs` -- JVM_BUILD_TIMEOUT_MS (600s)

**Action:** Add `[instrument.timeouts]` section to `InstrumentConfig`. Thread
the config through `InstrumentDriver` and `LangRunner` constructors. Each
language reads its timeout from config with the current constant as fallback.

### 1.2 CLI source file limits

**Problem:** `MAX_SOURCE_FILES` (10,000) and `MAX_SOURCE_FILE_BYTES` (1MB) in
`apex-cli/src/lib.rs` prevent analysis of large repos but cannot be overridden.

**Files:**
- `crates/apex-cli/src/lib.rs` lines 1750-1754

**Action:** Add `[index]` section to `ApexConfig` with `max_source_files` and
`max_source_file_bytes`. Read in CLI walker.

### 1.3 Secret scan entropy threshold

**Problem:** `DEFAULT_ENTROPY_THRESHOLD` (5.0) in secret_scan.rs controls
false-positive rate. Users with high-entropy codebases (crypto, encodings) need
to raise this.

**File:** `crates/apex-detect/src/detectors/secret_scan.rs` line 349

**Action:** Add `entropy_threshold` to detect config. Pass through detector
constructor.

### 1.4 Detector pipeline concurrency

**Problem:** `MAX_SUBPROCESS_CONCURRENCY` (4) in pipeline.rs caps parallel
subprocess detectors. CI machines with many cores are underutilized.

**File:** `crates/apex-detect/src/pipeline.rs` line 173

**Action:** Add `max_subprocess_concurrency` to `DetectConfig`. Default 4.

---

## Priority 2: Medium-Impact (power users / performance tuning)

### 2.1 PSO scheduler parameters (W, C1, C2, PROB_MIN)

**File:** `crates/apex-fuzz/src/scheduler.rs` lines 157-163

**Action:** Add `[fuzz.pso]` section. Pass through PsoMOptScheduler::new().

### 2.2 FOX controller rates

**File:** `crates/apex-fuzz/src/control.rs` lines 13-16, 29

**Action:** Add `[fuzz.fox]` section with `mutation_rate`, `exploration_rate`, `alpha`.

### 2.3 Semantic feedback weights

**File:** `crates/apex-fuzz/src/semantic_feedback.rs` lines 20-22

**Action:** Add `[fuzz.semantic]` section with `branch_weight`, `semantic_weight`.

### 2.4 Thompson beta cap

**File:** `crates/apex-fuzz/src/thompson.rs` line 34

**Action:** Add `beta_cap` to FuzzConfig or a sub-section.

### 2.5 CmpLog ring buffer size

**File:** `crates/apex-fuzz/src/cmplog.rs` line 191

**Action:** Add `cmplog_ring_max` to FuzzConfig. Requires changing const to
constructor parameter.

### 2.6 Coverage monitor window size

**File:** `crates/apex-agent/src/orchestrator.rs` line 63

**Action:** Add `monitor_window_size` to AgentConfig. Pass to CoverageMonitor::new().

### 2.7 Reachability max depth

**File:** `crates/apex-reach/src/engine.rs` line 40

**Action:** Add `[reach]` section to ApexConfig with `max_depth = 20`.

### 2.8 CPG query max rows

**File:** `crates/apex-cpg/src/query/executor.rs` line 137

**Action:** Add `[cpg]` section with `max_query_rows = 100_000`.

---

## Priority 3: Low-Impact (rarely need tuning)

### 3.1 Synth chunk sizes

**Files:** All `crates/apex-synth/src/*.rs` -- `chunk_size = 20` (or 10 for C/C++)

**Action:** Add `[synth]` section with `chunk_size`. Low priority since the
value is already reasonable.

### 3.2 SeedMind prompt limits

**File:** `crates/apex-fuzz/src/seedmind.rs` line 21 -- `.take(20)`

**Action:** Add `max_branches_in_prompt` to a fuzz or synth config.

### 3.3 Analyzer skip dirs

**File:** `crates/apex-detect/src/analyzer_registry.rs` lines 83-94

**Action:** Merge with `coverage.omit_patterns` or add separate `detect.skip_dirs`.

### 3.4 Security pattern context window

**File:** `crates/apex-detect/src/detectors/security_pattern.rs` line 652

**Action:** Add `context_window` to detect config. Default 3.

### 3.5 HGFuzzer unknown distance energy

**File:** `crates/apex-fuzz/src/hgfuzzer.rs` line 34 -- `0.1`

**Action:** Add to fuzz.directed config.

---

## Unreasonable Defaults

| Parameter | Current | Issue | Suggested |
|-----------|---------|-------|-----------|
| `agent.deadline_secs` | None (30min internal) | Documented in code comment. Good default now. | OK |
| `fuzz.stall_iterations` | 50 | Reasonable for small targets, too low for complex ones | Consider 200 |
| `agent.stall_threshold` | 10 | Very aggressive -- may stop exploration too early | Consider 20 |
| `instrument.timeouts.*` | 600s (10min) | OK for most; C compile at 300s may be tight for Linux kernel | OK |
| `coverage.target` | 1.0 (100%) | Unreachable for most real projects -- causes infinite loops until deadline | Consider 0.95 |
| `detect.sanitizer.replay_top_percent` | 1 | Only replays top 1% of inputs. Very conservative. | Consider 5 |

### Note on `coverage.target = 1.0`

The default target of 100% coverage is effectively unreachable for non-trivial
projects. When no deadline is set, the internal 30-minute cap prevents infinite
runs, but the intent is misleading. A default of 0.95 would be more practical
and would let the orchestrator declare success earlier.

---

## Implementation Order

1. Priority 1 items (4 tasks) -- direct user pain
2. `coverage.target` default change to 0.95 -- prevents misleading behavior
3. Priority 2 items (8 tasks) -- power user features
4. Priority 3 items (5 tasks) -- nice to have
