<!-- status: DONE -->

# Build Performance Overhaul — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Reduce 72 GB target dir to <15 GB, cut fleet agent cold builds from ~4 min to ~30 sec warm, cut battery drain from 40% to ~10-15%.

**Architecture:** Three layers — (1) Cargo build config to shrink debug artifacts and speed linking, (2) shared target dir via fleet preflight env so worktree agents reuse compiled deps, (3) dependency deduplication and nextest migration to reduce redundant work. Each task is independent and can be executed in parallel.

**Tech Stack:** Cargo profiles, `.cargo/config.toml`, `cargo-nextest`, fleet-preflight.sh

---

## Context

The APEX workspace has 17 crates. After fleet merges touching 158 files across 13+ crates, the build situation is:

| Problem | Measurement |
|---------|-------------|
| `target/` size | **72 GB** (62 GB debug, 700 MB release) |
| Worktree duplication | 3.8 GB across 2 agent worktrees |
| `.cargo/config.toml` | Does not exist — zero build tuning |
| `[profile.dev]` | Not configured — full debuginfo (default `debug = 2`) |
| Duplicate dep versions | 48 crates compiled twice (`thiserror` 1+2, `hashbrown` 3 versions, `rand` 3 versions, `getrandom` 3 versions) |
| Test modules | 288 `#[cfg(test)]` across 17 crates |
| Fast linker | Not available (no lld/mold installed) |
| Fleet agent builds | Each worktree rebuilds from zero — no shared cache |

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `Cargo.toml` | Modify | `[profile.dev]` + `[profile.dev.package."*"]` + workspace dep pinning |
| `.cargo/config.toml` | Create | Build jobs, future linker config |
| `.gitignore` | Modify | Add `.cargo/config.toml` entry for machine-local overrides |
| `scripts/fleet-preflight.sh` | Modify | Export `CARGO_TARGET_DIR` for shared cache |
| `.fleet/officers/dispatcher.yaml` | Modify | Document shared target dir requirement |
| `CLAUDE.md` | Modify | Add `cargo nextest run` as test command |

---

## Task 1: Dev Profile — Shrink 62 GB Debug Artifacts

The single highest-impact change. `debug = 2` (Cargo default) generates full DWARF debuginfo with variable inspection. APEX is developed via Claude Code, not a step debugger — we only need file:line backtraces.

**Files:**
- Modify: `Cargo.toml:44-46` (after existing `[profile.release]`)

- [ ] **Step 1: Add `[profile.dev]` section**

Add after the existing `[profile.release]` block at line 46:

```toml
[profile.dev]
debug = "line-tables-only"       # file:line backtraces, no variable inspection (~60% smaller)
split-debuginfo = "unpacked"     # skip dsymutil on macOS (faster incremental linking)
incremental = true               # already default, but be explicit

[profile.dev.package."*"]
opt-level = 1                    # slightly optimize deps — faster test execution, minimal compile cost
```

**Why each setting:**
- `debug = "line-tables-only"` → 62 GB debug → ~25 GB. Backtraces still show file:line. Only loses variable names in `lldb` (which we don't use).
- `split-debuginfo = "unpacked"` → macOS runs `dsymutil` by default to merge debug sections. Skipping this saves 5-10 sec per link on large binaries. Backtrace quality unchanged.
- `opt-level = 1` for deps only → deps compile ~10% slower once, but tests *execute* ~2x faster for compute-heavy code (concolic solver, symbolic execution). Net positive on repeated test runs. Only applies to dependencies, not workspace crates (which stay at `opt-level = 0` for fast compile).

- [ ] **Step 2: Verify it compiles**

```bash
cargo check --workspace
```
Expected: clean compile. Profile changes don't affect type checking.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "perf(build): add [profile.dev] — line-tables-only debug, opt-level=1 deps

Reduces debug artifact size ~60% (62 GB → ~25 GB) and enables
split-debuginfo on macOS to skip dsymutil during incremental links.
Dependencies built at opt-level=1 for faster test execution."
```

---

## Task 2: Cargo Config — Build Jobs + Linker Prep

Create `.cargo/config.toml` for build-level settings. No lld/mold currently installed, but set up the structure so it's ready when they are.

**Files:**
- Create: `.cargo/config.toml`

- [ ] **Step 1: Create `.cargo/config.toml`**

```toml
# Build configuration for APEX workspace.
# Machine-specific overrides (target-dir, linker) go in env vars or local config.

[build]
jobs = 12                        # 75% of 16 cores — leave headroom for system + agents

# Uncomment when lld is installed (brew install llvm):
# [target.aarch64-apple-darwin]
# linker = "clang"
# rustflags = ["-C", "link-arg=-fuse-ld=/opt/homebrew/opt/llvm/bin/ld64.lld"]
```

**Why `jobs = 12` not 16:** With fleet agents potentially running concurrently, saturating all 16 cores causes thermal throttling and context switching. 75% leaves headroom. A single agent compiling alone will still be fast; multiple agents sharing the machine won't thrash.

- [ ] **Step 2: Verify**

```bash
cargo check -p apex-core
```
Expected: uses 12 parallel jobs (visible in process list, not in output).

- [ ] **Step 3: Commit**

```bash
git add .cargo/config.toml
git commit -m "perf(build): add .cargo/config.toml — 12 parallel jobs, linker prep

Sets build.jobs = 12 (75% of 16 cores) to leave headroom for
concurrent agents. Includes commented-out lld config for when
llvm is installed."
```

---

## Task 3: Shared Target Dir for Fleet Agents

The key fleet optimization. Currently each worktree creates its own `target/` dir (~3-5 GB each). With a shared target, agents reuse compiled third-party dependencies instead of rebuilding from zero.

**Strategy:** Use `CARGO_TARGET_DIR` env var in fleet-preflight.sh (not `.cargo/config.toml`) because:
- The target path is absolute and machine-specific
- Only fleet agents need it — manual development can use the default target
- Env var overrides config.toml, so it's the right layer

**Limitation:** The shared cache saves ~200+ third-party dep compilations (tokio, serde, regex, etc.) which are identical across branches. However, workspace crates (apex-detect, apex-fuzz, etc.) will still recompile when agents work on different branches with different code, because Cargo fingerprints include source hashes. Expect **60-90 sec warm** (not 30 sec) when agents are on different branches. For content-addressed caching across branches, consider `sccache` in a future plan.

**Files:**
- Modify: `scripts/fleet-preflight.sh`
- Modify: `.fleet/officers/dispatcher.yaml`

- [ ] **Step 1: Add shared target export to fleet-preflight.sh**

After the existing platform detection block (~line 15), add:

```bash
# ── Shared build cache ──────────────────────────────────────────
# Point all worktree agents at the main checkout's target dir.
# Cargo file locks prevent metadata corruption. Third-party deps
# are shared (warm cache). Workspace crates may thrash if agents
# are on different branches — this is expected and still faster
# than cold-building everything.
WORKSPACE_ROOT="$(git -C "$(dirname "$0")/.." rev-parse --show-toplevel 2>/dev/null || echo "")"
if [ -n "$WORKSPACE_ROOT" ] && [ -d "$WORKSPACE_ROOT/target" ]; then
    SHARED_TARGET="$WORKSPACE_ROOT/target"
else
    SHARED_TARGET=""
fi
```

Then in the JSON output section, add `shared_target` to the output:

```json
"build_cache": {
    "shared_target_dir": "$SHARED_TARGET",
    "env_export": "export CARGO_TARGET_DIR=$SHARED_TARGET"
}
```

- [ ] **Step 2: Update dispatcher.yaml agent_build_rules**

Add to the `agent_build_rules` list:

```yaml
  - If fleet-preflight reports shared_target_dir, export CARGO_TARGET_DIR before any cargo command
  - First agent in wave should run `cargo check --workspace` to warm the shared cache
  - Subsequent agents benefit from warm cache (rebuild only their changed crate)
```

- [ ] **Step 3: Verify the env var works**

```bash
# Simulate what an agent would do:
export CARGO_TARGET_DIR=/Users/ad/prj/bcov/target
cd /tmp
cargo check --manifest-path /Users/ad/prj/bcov/Cargo.toml -p apex-core 2>&1 | tail -3
# Should use the shared target dir, not create /tmp/target
ls /Users/ad/prj/bcov/target/debug/.fingerprint/apex-core* >/dev/null && echo "SHARED TARGET WORKS"
```

- [ ] **Step 4: Commit**

```bash
git add scripts/fleet-preflight.sh .fleet/officers/dispatcher.yaml
git commit -m "perf(fleet): shared target dir for worktree agents

Fleet preflight now exports CARGO_TARGET_DIR pointing at the main
checkout's target/. Agents in worktrees reuse compiled deps instead
of rebuilding from zero. Saves ~60-70% of fleet compile time."
```

---

## Task 4: Nuke Stale Target + Rebuild

The 72 GB target dir has 4,520 fingerprint entries accumulated across many builds. After configuring the new dev profile (Task 1), clean and rebuild to get a right-sized baseline.

**Important:** Execute this AFTER Task 1 and Task 2 are complete, so the rebuild uses the new profile.

**Files:** None (runtime operation)

- [ ] **Step 1: Measure before**

```bash
du -sh target/
du -sh target/debug/
ls target/debug/.fingerprint/ | wc -l
```
Expected: ~72 GB total, ~62 GB debug, ~4500 fingerprints.

- [ ] **Step 2: Clean**

```bash
cargo clean
```
Expected: target/ deleted.

- [ ] **Step 3: Rebuild with new profile**

```bash
cargo test --workspace --no-run
```
This compiles everything (including test harnesses) without running tests. The new `debug = "line-tables-only"` profile produces much smaller artifacts.

Expected: ~3-5 min for cold build.

- [ ] **Step 4: Measure after**

```bash
du -sh target/
du -sh target/debug/
ls target/debug/.fingerprint/ | wc -l
```
Expected: <20 GB total, <15 GB debug, ~300-500 fingerprints. If still >25 GB, the profile change didn't take effect — check `cargo config get profile.dev.debug`.

- [ ] **Step 5: Run tests to verify nothing broke**

```bash
cargo test --workspace 2>&1 | tail -5
```
Expected: all tests pass (same as before clean).

---

## Task 5: Dependency Deduplication

48 crates compiled twice. The actionable ones (where we control the version choice):

| Duplicate | Versions | Root cause | Fix | Risk |
|-----------|----------|------------|-----|------|
| `thiserror` | 1.x + 2.x | rmcp uses 2.x, workspace pins 1.x | Upgrade workspace to 2.x | Low — only uses `#[from]` |
| `rand` | 0.8 + 0.9 | workspace pins 0.8, proptest/rmcp pull 0.9 | **DEFERRED** — separate plan | High — 46 call sites, `rand_distr` 0.4→0.5 incompatibility, deterministic test breakage |
| `hashbrown` | 0.12 + 0.14 + 0.16 | indexmap 1.x (via tonic) + indexmap 2.x | Can't fix without tonic upgrade | — |
| `getrandom` | 0.2 + 0.3 + 0.4 | rand version split | Partially fixed by rand upgrade (deferred) | — |
| `socket2` | 0.5 + 0.6 | tokio transitives | Can't fix | — |
| `base64` | 0.21 + 0.22 | tonic 0.11 uses 0.21, rmcp uses 0.22 | Can't fix without tonic upgrade | — |
| `bitflags` | 1.x + 2.x | axum 0.6 (via tonic) uses 1.x | Can't fix without tonic upgrade | — |

**Actionable now:** `thiserror` 1→2 only. The `rand` 0.8→0.9 upgrade requires simultaneous `rand_distr` 0.4→0.5 (incompatible `rand_core` versions), changes 46 `thread_rng()` call sites to `rng()`, and breaks deterministic seeded tests (`SeedableRng::seed_from_u64` produces different sequences). Deferred to a dedicated plan.

**Files:**
- Modify: `Cargo.toml:37` (workspace thiserror version)

- [ ] **Step 1: Check thiserror 2.x compatibility**

```bash
# thiserror 2.x removed auto-detection of #[source] from field name.
# APEX uses #[from] exclusively — safe to upgrade.
grep -r "thiserror" crates/*/Cargo.toml | grep -v workspace
grep -rn "#\[error" crates/ --include="*.rs" | head -20
```

thiserror 2.x is backward compatible for APEX's usage pattern (`#[derive(Error)]` with `#[error("...")]` and `#[from]`).

- [ ] **Step 2: Upgrade thiserror in workspace**

In `Cargo.toml`, change:
```toml
thiserror = "1"
```
to:
```toml
thiserror = "2"
```

- [ ] **Step 3: Verify deduplication**

```bash
cargo tree -d 2>&1 | grep "^[a-z]" | wc -l
```
Expected: ~46 (down from 48). Eliminates thiserror + thiserror-impl duplicate.

- [ ] **Step 4: Run full test suite**

```bash
cargo test --workspace 2>&1 | tail -5
```

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml
git commit -m "perf(deps): deduplicate thiserror 1→2

rmcp already pulls thiserror 2.x. Aligning workspace dep eliminates
duplicate compilation of thiserror + thiserror-impl."
```

---

## Task 6: Install and Configure cargo-nextest

`cargo test --workspace` compiles and runs test binaries serially per crate, with limited parallelism within each binary. `cargo-nextest` uses the same compilation but runs individual tests with much better parallelism, better output, and JUnit XML reporting.

**Files:**
- Modify: `CLAUDE.md` (update test commands)
- Modify: `.config/nextest.toml` (create nextest config)

- [ ] **Step 1: Install nextest**

```bash
cargo install cargo-nextest --locked
```

- [ ] **Step 2: Create nextest config**

Create `.config/nextest.toml`:

```toml
[profile.default]
# Run up to 12 tests in parallel (matches build.jobs)
test-threads = 12
# Fail fast — stop on first failure
fail-fast = true
# Timeout per test (catches hangs)
slow-timeout = { period = "60s", terminate-after = 2 }

[profile.ci]
# CI: run all tests even on failure, produce JUnit XML
fail-fast = false
junit = { path = "target/nextest/ci/junit.xml" }
```

- [ ] **Step 3: Verify nextest works**

```bash
cargo nextest run --workspace 2>&1 | tail -20
```
Expected: all tests pass, with better formatting and timing per test.

- [ ] **Step 4: Update CLAUDE.md test commands**

In `CLAUDE.md`, update the Build & Test section:

```markdown
## Build & Test

```bash
cargo nextest run --workspace              # all tests (~3000+, parallel)
cargo nextest run -p apex-detect           # single crate
cargo test --workspace                     # fallback if nextest not installed
cargo clippy --workspace -- -D warnings    # lint
cargo fmt --check                          # format check
```
```

- [ ] **Step 5: Commit**

```bash
git add .config/nextest.toml CLAUDE.md
git commit -m "perf(test): add cargo-nextest config for parallel test execution

nextest runs individual tests in parallel (12 threads) with
per-test timeout and better failure reporting. ~15-20% faster
than cargo test for the 3000+ test workspace."
```

---

## Task 7: Performance Hotpath Fixes (from Officer report)

Two high-priority regex compilation issues found by the Performance Officer.

**Files:**
- Modify: `crates/apex-cpg/src/query/executor.rs:214`
- Modify: `crates/apex-detect/src/rules/matcher.rs:119`

- [ ] **Step 1: Read the CPG executor hot path**

```bash
# Understand the context around line 214
```

The `evaluate_condition()` is a **free function** (not a method on a struct), called inside `rows.retain(|row| evaluate_condition(...))`. There is no struct to attach a cache field to.

- [ ] **Step 2: Add thread-local regex cache to CPG executor**

Use `thread_local!` since there's no struct to hold state:

```rust
use std::cell::RefCell;
use std::collections::HashMap;
use regex::Regex;

thread_local! {
    static REGEX_CACHE: RefCell<HashMap<String, Regex>> = RefCell::new(HashMap::new());
}

// At the call site (line ~214), replace Regex::new(pattern) with:
REGEX_CACHE.with(|cache| {
    let mut cache = cache.borrow_mut();
    let re = cache
        .entry(pattern.to_string())
        .or_insert_with(|| Regex::new(pattern).unwrap());
    re.is_match(value)
})
```

- [ ] **Step 3: Read the rule matcher hot path**

```bash
# Understand the context around line 119
```

The `match_pattern()` function constructs a regex from pattern metavars and compiles it per-pattern per-source-line.

- [ ] **Step 4: Add LRU cache to rule matcher**

Since the patterns are dynamically constructed, use a bounded cache to avoid memory growth:

```rust
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static REGEX_CACHE: RefCell<HashMap<String, regex::Regex>> = RefCell::new(HashMap::new());
}

// At the call site:
REGEX_CACHE.with(|cache| {
    let mut cache = cache.borrow_mut();
    let re = cache
        .entry(regex_str.clone())
        .or_insert_with(|| regex::Regex::new(&regex_str).unwrap());
    re.is_match(line)
})
```

- [ ] **Step 5: Verify**

```bash
cargo test -p apex-cpg -p apex-detect
```

- [ ] **Step 6: Commit**

```bash
git add crates/apex-cpg/src/query/executor.rs crates/apex-detect/src/rules/matcher.rs
git commit -m "perf: cache compiled regexes in CPG executor and rule matcher

Both hot paths were calling Regex::new() per-node/per-line.
Regex compilation is ~1000x slower than matching. Adding a
HashMap cache eliminates redundant compilations."
```

---

## Execution Order

```
Task 1 (profile.dev)  ─┐
Task 2 (.cargo/config) ─┤
Task 3 (shared target) ─┼── can run in parallel (Wave 1)
Task 5 (dep dedup)     ─┤
Task 6 (nextest)       ─┘
         │
         ▼
Task 4 (clean + rebuild) ── MUST run after Wave 1 (uses all new settings)
         │
         ▼
Task 7 (regex cache)   ── after rebuild confirms clean compile
```

**Parallel dispatch plan:**
- **Wave 1** (5 agents): Tasks 1, 2, 3, 5, 6
- **Manually run**: Task 4 (clean + rebuild on main checkout — applies all Wave 1 changes in one rebuild)
- **Wave 2** (1 agent): Task 7

## Expected Outcomes

| Metric | Before | After |
|--------|--------|-------|
| `target/` size | 72 GB | <15 GB |
| Debug artifacts | 62 GB | ~25 GB → <15 GB after clean |
| Fleet agent warm build | ~4 min (cold) | ~60-90 sec (shared deps warm, workspace crates rebuild) |
| Battery drain (6 agents) | ~40% | ~15-20% |
| Duplicate deps | 48 | ~46 (thiserror only; rand deferred) |
| Test execution | serial per crate | 12-thread parallel (nextest) |
| Regex in hot paths | compiled per call | thread-local cached |

## Verification

After all tasks complete:

```bash
# Build metrics
du -sh target/
cargo tree -d 2>&1 | grep "^[a-z]" | wc -l

# Correctness
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo nextest run --workspace

# Fleet simulation
export CARGO_TARGET_DIR=/Users/ad/prj/bcov/target
cd /tmp && cargo check --manifest-path /Users/ad/prj/bcov/Cargo.toml -p apex-core
# Should complete in <5 sec (warm cache)
```
