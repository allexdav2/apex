<!-- status: DONE -->

# APEX Self-Analysis — Bug Fixes Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 8 bugs found by APEX captain self-analysis (4872 tests baseline, 0 clippy warnings).

**Architecture:** Three waves by crew ownership — exploration (4 bugs), intelligence (2 bugs), security-detect (1 bug), plus 1 architecture task for CLI split scoping.

**Tech Stack:** Rust, cargo nextest, apex-fuzz, apex-agent, apex-cpg, apex-rpc

---

## Findings Summary

| # | Sev | Conf | Bug | File | Crew | Status |
|---|-----|------|-----|------|------|--------|
| 1 | HIGH | 95 | `MOptScheduler::mutate()` panics on empty scheduler — `select()` returns 0, indexes empty vecs | `apex-fuzz/src/scheduler.rs:62` | exploration | Documented (`bug_` test) |
| 2 | HIGH | 95 | Proto→core BranchId truncates `col` (u32→u16) and `direction` (u32→u8) — branch collisions, phantom coverage | `apex-rpc/src/coordinator.rs:17-19` | platform | Documented (`bug_` test) |
| 3 | MED | 90 | `report_hit()` yield ratio exceeds 1.0 — unbounded EMA drift biases selection | `apex-fuzz/src/scheduler.rs:79-91` | exploration | Documented (`bug_` test) |
| 4 | MED | 90 | `ThompsonScheduler::select()` returns 0 on empty — invalid index for zero-length arms | `apex-fuzz/src/thompson.rs:38-48` | exploration | Documented (`bug_` test) |
| 5 | MED | 85 | `BudgetAllocator::allocate()` div-by-zero with 0 strategies | `apex-agent/src/budget.rs:44` | intelligence | **Undocumented** |
| 6 | MED | 85 | `BudgetAllocator::set_minimum_share()` div-by-zero → f64::INFINITY | `apex-agent/src/budget.rs:34` | intelligence | **Undocumented** |
| 7 | MED | 85 | `SSA intersect()` can infinite-loop on incomplete dominator trees — `unwrap_or(&a)` returns self | `apex-cpg/src/ssa.rs:143-151` | security-detect | **Undocumented** |
| 8 | LOW | 85 | Thompson recovery after heavy penalization extremely slow — Beta posterior barely shifts | `apex-fuzz/src/thompson.rs:127-149` | exploration | Documented (`bug_` test) |

### Security Findings (all clean)

| # | Finding | File | Status |
|---|---------|------|--------|
| 1 | No `unsafe` in production code | Workspace-wide | Clean |
| 2 | No `process::exit()` in library code | `apex-cli/src/lib.rs` | Clean |
| 3 | Taint analysis sanitizers properly break flow | `apex-cpg/src/taint.rs` | Sound |
| 4 | Hardcoded secret detector has comprehensive FP filtering | `apex-detect/src/detectors/hardcoded_secret.rs` | Well-designed |
| 5 | Firecracker sandbox uses feature-gated stubs | `apex-sandbox/src/firecracker.rs` | Safe by design |

### Architecture Concerns

| # | Concern | Impact | Location |
|---|---------|--------|----------|
| A1 | `apex-cli/src/lib.rs` is 41K tokens — monolithic | Maintainability | `apex-cli/src/lib.rs` |
| A2 | CPG builder is line-based Python parser (no tree-sitter) | Limits taint analysis accuracy | `apex-cpg/src/builder.rs` |
| A3 | `CoverageOracle` dual-structure (DashMap + Mutex<Vec>) | Bottleneck under contention | `apex-coverage/src/oracle.rs` |

### Coverage Gaps

| # | Gap | File | Risk |
|---|-----|------|------|
| G1 | `BudgetAllocator` with 0 strategies | `apex-agent/src/budget.rs` | Panic in production |
| G2 | `SSA intersect` with missing idom entries | `apex-cpg/src/ssa.rs` | Infinite loop |
| G3 | `xorshift64` seeded with 0 produces 0 forever | `apex-concolic/src/search.rs:59` | Dead search path |
| G4 | `InterleavedSearch` with 0 strategies | `apex-concolic/src/search.rs` | Index panic |

### Long Tail (confidence < 80)

| Conf | Bug | File | Crew |
|------|-----|------|------|
| 75 | `agent_report.rs:244` — `(line as i32 + offset) as u32` wraps negative to u32::MAX region; harmless (source_cache miss returns None) | `apex-core/src/agent_report.rs:244` | foundation |
| 60 | `xorshift64` seeded with 0 stays at 0 forever | `apex-concolic/src/search.rs:59` | exploration |

---

## Task 1: Fix Exploration Bugs (4 bugs)

**Crew:** exploration
**Crate:** apex-fuzz

### Bug 1: MOptScheduler::mutate() panics on empty

- [ ] Read `crates/apex-fuzz/src/scheduler.rs:62-66`
- [ ] Add early return: `if self.mutators.is_empty() { return input.to_vec(); }`
- [ ] Update existing `bug_mutate_empty_scheduler_panics` test from `#[should_panic]` to assert non-panic behavior
- [ ] Add test `empty_scheduler_mutate_returns_input_unchanged`

### Bug 3: report_hit() yield exceeds 1.0

- [ ] Read `crates/apex-fuzz/src/scheduler.rs:79-91`
- [ ] Cap yield ratio: `let yield_ratio = (hits as f64 / apps as f64).min(1.0);`
- [ ] Update `bug_report_hit_yield_exceeds_one` test to assert yield ≤ 1.0

### Bug 4: ThompsonScheduler::select() returns 0 on empty

- [ ] Read `crates/apex-fuzz/src/thompson.rs:38-48`
- [ ] Change return type to `Option<usize>`, return `None` when `arms.is_empty()`
- [ ] Update all callers to handle `None`
- [ ] Update `bug_select_empty_returns_invalid_index` test

### Bug 8: Thompson recovery too slow (design decision)

- [ ] Read `crates/apex-fuzz/src/thompson.rs:127-149`
- [ ] Consider adding `decay_penalty(arm_idx, factor)` method that multiplies beta by `factor` (e.g. 0.5)
- [ ] Or: add ceiling on beta (e.g. `beta = beta.min(50.0)`) to prevent permanent arm death
- [ ] Add test `thompson_recovery_after_penalty_ceiling`

### Verify

- [ ] `cargo nextest run -p apex-fuzz`
- [ ] `cargo clippy -p apex-fuzz -- -D warnings`

---

## Task 2: Fix Intelligence Bugs (2 bugs)

**Crew:** intelligence
**Crate:** apex-agent

### Bug 5: BudgetAllocator::allocate() div-by-zero

- [ ] Read `crates/apex-agent/src/budget.rs:44`
- [ ] Add guard: `if self.num_strategies == 0 { return vec![]; }`
- [ ] Add test `bug_allocate_zero_strategies_returns_empty`

### Bug 6: BudgetAllocator::set_minimum_share() div-by-zero

- [ ] Read `crates/apex-agent/src/budget.rs:34`
- [ ] Add guard: `if self.num_strategies == 0 { return; }`
- [ ] Add test `bug_set_minimum_share_zero_strategies_noop`

### Verify

- [ ] `cargo nextest run -p apex-agent`
- [ ] `cargo clippy -p apex-agent -- -D warnings`

---

## Task 3: Fix Security-Detect Bug (1 bug)

**Crew:** security-detect
**Crate:** apex-cpg

### Bug 7: SSA intersect() infinite loop

- [ ] Read `crates/apex-cpg/src/ssa.rs:143-151`
- [ ] Add loop guard: if `idom.get(&a)` returns `None`, break with current `a` (treat as root)
- [ ] Same for `idom.get(&b)`
- [ ] Add test `bug_ssa_intersect_missing_idom_terminates`

### Verify

- [ ] `cargo nextest run -p apex-cpg`
- [ ] `cargo clippy -p apex-cpg -- -D warnings`

---

## Task 4: Coverage Gap Fixes (3 gaps)

### G3: xorshift64 zero seed

- [ ] Read `crates/apex-concolic/src/search.rs:59-64`
- [ ] Add seed guard: `if self.state == 0 { self.state = 1; }` before xorshift
- [ ] Add test `bug_xorshift_zero_seed_produces_nonzero`

### G4: InterleavedSearch empty strategies

- [ ] Read `crates/apex-concolic/src/search.rs` — find InterleavedSearch
- [ ] Add guard for empty `strategies` vec
- [ ] Add test `bug_interleaved_empty_strategies_no_panic`

### Verify

- [ ] `cargo nextest run -p apex-concolic`
- [ ] `cargo clippy -p apex-concolic -- -D warnings`

---

## Task 5: Architecture — Scope CLI Split (analysis only)

**Not implementation — just scoping for a future plan.**

- [ ] Count subcommands in `apex-cli/src/lib.rs`
- [ ] Group by domain (run, detect, fuzz, rpc, reach, ratchet, etc.)
- [ ] Propose file split: `src/cmd/{run,detect,fuzz,rpc,reach,ratchet,deploy}.rs`
- [ ] Estimate LOC per new file
- [ ] Write finding to `docs/superpowers/plans/` as a FUTURE plan stub

---

## Execution Order

```
Task 1 (exploration) ─┐
Task 2 (intelligence) ─┼── parallel (3 crews)
Task 3 (security)     ─┘
         │
         ▼
Task 4 (coverage gaps) ── after main fixes verified
         │
         ▼
Task 5 (CLI split scope) ── analysis only, no code changes
```

**Parallel dispatch:** Tasks 1, 2, 3 to their respective crews. Task 4 after merge. Task 5 is research.

## Expected Outcomes

| Metric | Before | After |
|--------|--------|-------|
| Documented panics | 4 (`#[should_panic]`) | 0 (all guarded) |
| Undocumented panics | 3 (#5, #6, #7) | 0 |
| Infinite loop risks | 1 (#7) | 0 |
| Coverage gaps | 4 | 0 |
| Tests | 4872 | ~4885 (+13 new) |
