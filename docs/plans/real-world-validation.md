<!-- status: ACTIVE -->
# Real-World Validation: Run APEX on 10 Popular Repos + Suppressed Warnings Registry

## Context

APEX has 100+ detectors across 12 languages but has only been tested on synthetic/unit-test inputs. False positive suppression is scattered across individual detectors with no central catalog. We need to validate against real-world codebases to find crashes, false positives, performance issues, and missing patterns — and build a centralized suppressed warnings registry from the results.

---

## Phase 1: Suppressed Warnings Registry

Create `docs/suppressed-warnings-registry.md` cataloging all existing suppression mechanisms:

- [x] **T1.1** Audit all detectors for false-positive filtering logic
  - `hardcoded_secret.rs` — `FALSE_POSITIVE_VALUES` (18 entries), `ENV_VAR_MARKERS`, `is_example_file`
  - `secret_scan.rs` — duplicate `FALSE_POSITIVE_VALUES` (missing "test")
  - `detectors/util.rs` — `is_test_file`, `is_comment`, `in_test_block`
  - `bandit.rs` — per-rule `suppressor` regexes
  - `security_pattern.rs` — per-language `sanitization_indicators`
  - `threat_model.rs` — `SOURCE_TRUST_TABLE`, `should_suppress`
- [x] **T1.2** Write `docs/suppressed-warnings-registry.md` with sections: global suppressions, per-detector suppressions, threat-model suppressions
- [x] **T1.3** Deduplicate `FALSE_POSITIVE_VALUES` — extract shared const to `detectors/util.rs`, import from both `hardcoded_secret.rs` and `secret_scan.rs`

**Files:**
- `crates/apex-detect/src/detectors/util.rs`
- `crates/apex-detect/src/detectors/hardcoded_secret.rs`
- `crates/apex-detect/src/detectors/secret_scan.rs`
- `crates/apex-detect/src/detectors/security_pattern.rs`
- `crates/apex-detect/src/threat_model.rs`

---

## Phase 2: Clone 10 Repos (shallow)

All clones go to `/tmp/apex-validation/repos/`. Shallow (`--depth 1`) to save disk.

| # | Repo | Lang(s) | Target subdir | Why this repo |
|---|------|---------|---------------|---------------|
| 1 | `torvalds/linux` | **C** | `kernel/` | REQUIRED. Largest C project, stress-tests perf |
| 2 | `python/cpython` | **C + Python** | `Objects/` (C), `Lib/` (Py) | Two languages in one repo |
| 3 | `microsoft/TypeScript` | **TypeScript** | `src/` | Reference TS compiler |
| 4 | `BurntSushi/ripgrep` | **Rust** | full repo | Small, clean Rust project |
| 5 | `spring-projects/spring-boot` | **Java** | `spring-boot-project/spring-boot/src/` | Top Java framework |
| 6 | `kubernetes/kubernetes` | **Go** | `pkg/` | Top Go project |
| 7 | `dotnet/runtime` | **C#** | `src/libraries/System.Text.Json/` | Core .NET library |
| 8 | `vapor/vapor` | **Swift** | full repo | Popular Swift server framework |
| 9 | `rails/rails` | **Ruby** | `activerecord/lib/` | Top Ruby framework |
| 10 | `JetBrains/ktor` | **Kotlin** | `ktor-server/ktor-server-core/` | Popular Kotlin server |

**Note:** C++ covered via Linux kernel headers. WebAssembly has no top repo — skip for now.

---

## Phase 3: Run APEX on Each Repo

- [x] **T3.0** Build APEX: `cargo build --release --bin apex`
- [ ] **T3.1** Create `scripts/validate-real-world.sh` automation script

For each repo, run `apex run` (or equivalent CLI command) capturing:
- Wall-clock time (`/usr/bin/time -l`)
- Peak memory
- Finding count by severity and detector
- Any panics/crashes/errors
- Exit code

Run matrix (11 runs for 10 repos — cpython runs twice):

| Run | Target | `--lang` | Time cap |
|-----|--------|----------|----------|
| linux-kernel | `repos/linux/kernel` | `c` | 10min |
| cpython-c | `repos/cpython/Objects` | `c` | 5min |
| cpython-py | `repos/cpython/Lib` | `python` | 5min |
| typescript | `repos/typescript/src` | `js` | 5min |
| ripgrep | `repos/ripgrep` | `rust` | 3min |
| spring-boot | `repos/spring-boot/.../src` | `java` | 5min |
| kubernetes | `repos/kubernetes/pkg` | `go` | 10min |
| dotnet | `repos/dotnet-runtime/.../System.Text.Json` | `c-sharp` | 5min |
| vapor | `repos/vapor` | `swift` | 3min |
| rails | `repos/rails/activerecord/lib` | `ruby` | 5min |
| ktor | `repos/ktor/.../ktor-server-core` | `kotlin` | 3min |

Results go to `/tmp/apex-validation/results/`.

---

## Phase 4: Triage Findings

For each repo's output:
- [x] **T4.1** Group findings by detector
- [x] **T4.2** Sample up to 10 findings per detector, read source at flagged location
- [x] **T4.3** Classify each as: **True Positive** / **False Positive (pattern)** / **False Positive (context)** / **Crash** / **Performance issue** / **Missing pattern**
- [x] **T4.4** Write triage report per repo

**Expected high-FP areas:**
- `hardcoded_secret` on Linux kernel hex constants and `#define` values
- Command injection on kernel `exec*()` / `system()` calls
- Bandit rules on cpython stdlib (`eval`, `exec`, `pickle` are legitimate there)
- SSRF on Kubernetes internal HTTP clients

---

## Phase 5: Fix Issues + Update Registry

- [x] **T5.1** Update `docs/suppressed-warnings-registry.md` with all newly discovered FP patterns
- [ ] **T5.2** Fix any crashes found
- [ ] **T5.3** Add new suppression patterns to detectors where FPs are systematic
- [ ] **T5.4** Performance hardening if large repos hit issues:
  - `build_source_cache` (`crates/apex-cli/src/lib.rs:1691`) — may need file-size cap or streaming for Linux kernel
  - `walkdir` (`crates/apex-cli/src/lib.rs:1724`) — should respect `.gitignore` patterns
- [ ] **T5.5** Update `plans/STATUS.md` with this plan entry

---

## Verification

1. All 11 runs complete without panics (exit code 0)
2. Suppressed warnings registry exists and covers all detectors
3. `FALSE_POSITIVE_VALUES` deduplicated (single source in `util.rs`)
4. FP rate per detector documented with before/after counts
5. `cargo nextest run --workspace` passes after all changes
6. `cargo clippy --workspace -- -D warnings` clean

---

## Dependency Graph

```
Phase 1 (registry) ─────┐
Phase 2 (clone repos) ──┼── parallel
                         │
Phase 3 (run APEX) ──────┤ depends on Phase 2
Phase 4 (triage) ────────┤ depends on Phase 3
Phase 5 (fixes) ─────────┘ depends on Phase 1 + Phase 4
```
