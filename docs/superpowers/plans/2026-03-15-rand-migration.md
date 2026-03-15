<!-- status: FUTURE -->

# rand 0.8 → 0.9 Migration Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate rand/getrandom/rand_core version duplication (3 versions each) by upgrading to rand 0.9 + rand_distr 0.5.

**Architecture:** High-churn migration touching 46 call sites across apex-fuzz and apex-agent. Requires simultaneous rand_distr upgrade due to rand_core version incompatibility. Deterministic seeded tests will break (different RNG sequences in 0.9).

**Tech Stack:** rand 0.9, rand_distr 0.5, rand_core 0.9

---

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|------------|
| `thread_rng()` → `rng()` rename | 46 call sites | Mechanical find-replace |
| `rand_distr` 0.4 incompatible with rand 0.9 | `rand_core` 0.6 vs 0.9 conflict | Must upgrade to `rand_distr = "0.5"` simultaneously |
| `SeedableRng::seed_from_u64` sequences changed | ~10 deterministic tests produce different outputs | Update expected values or use `rand::rngs::mock::StepRng` |
| `Distribution` trait changes in rand_distr 0.5 | API breakage in sampling code | Check all `use rand_distr::*` sites |

## Scope

**Crates affected:**
- `apex-fuzz` — `rand = "0.8"`, `rand_distr = "0.4"` (heaviest user)
- `apex-agent` — uses `rand::thread_rng()` in strategy selection
- `apex-concolic` — uses `rand` for seed mutation
- `apex-symbolic` — uses `rand` in solver diversity

**Call sites to change:**
```
rand::thread_rng()  →  rand::rng()           # 46 sites
use rand::rngs::StdRng  →  unchanged         # still available
StdRng::seed_from_u64(n)  →  unchanged API   # but different sequences
```

## Tasks

### Task 1: Upgrade rand + rand_distr in Cargo.toml files
### Task 2: Mechanical rename thread_rng → rng (46 sites)
### Task 3: Fix rand_distr API changes
### Task 4: Fix deterministic seeded tests
### Task 5: Verify deduplication (expect getrandom 3→1, rand_core 3→1)

## Dedup Impact

Eliminates ~8 duplicate crate compilations:
- `rand` 0.8 + 0.9 → 0.9 only
- `rand_core` 0.6 + 0.9 → 0.9 only
- `rand_chacha` 0.3 + 0.9 → 0.9 only
- `getrandom` 0.2 + 0.3 + 0.4 → 0.4 only (proptest may still pull 0.2)
