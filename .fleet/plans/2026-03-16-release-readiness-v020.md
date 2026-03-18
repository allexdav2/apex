<!-- status: DONE -->

# Release Readiness Assessment: APEX v0.2.0

**Date:** 2026-03-16
**Captain:** apex-captain
**Recommendation:** NO-GO (3 blockers, 6 warnings)

---

## Checklist

| # | Criterion | Status | Detail |
|---|-----------|--------|--------|
| 1 | Tests pass | PASS | 4,887 tests, 0 failures, 2 ignored (doc-tests behind features) |
| 2 | Clippy clean | PASS | 0 warnings with `-D warnings` across all 17 crates |
| 3 | Format clean | FAIL | 1 file has formatting diff: `crates/apex-reach/src/extractors/python.rs:300` |
| 4 | Changelog | PASS | Comprehensive `[Unreleased]` section covers all v0.2.0 work |
| 5 | Version bump | FAIL | Still `0.1.0` everywhere -- needs `./scripts/bump-version.sh 0.2.0` |
| 6 | CI config | PASS | ci.yml has 9 jobs: check, test, clippy, fmt, changelog, npm, python, homebrew, nix, install-script |
| 7 | Distribution URLs | PASS | All distribution files (Cargo.toml, npm, pip, Homebrew, Nix, install.sh) reference `sahajamoth`. Zero `allexdav2` references in code/config (only in `.claude/settings.local.json` audit log, harmless) |
| 8 | Remaining plans | WARN | 2 ACTIVE plans, 1 PARKED, 3 FUTURE for v0.2.0 scope |
| 9 | Known bugs | FAIL | 109 `bug_` test functions across 14 files document known issues; 8 unfixed bugs in self-analysis plan |
| 10 | Dependencies | WARN | 1 vulnerability (shlex RUSTSEC-2024-0006 via libafl), 1 unsoundness warning (lru RUSTSEC-2026-0002 via ratatui/libafl), 5 allowed warnings |

---

## Blockers (must fix before release)

### B1. Version is still 0.1.0
All 6 locations (Cargo.toml, npm/package.json, python/pyproject.toml, python/__init__.py, HomebrewFormula/apex.rb, flake.nix) show `0.1.0`. The `[Unreleased]` section in CHANGELOG.md needs stamping to `[0.2.0]`.

**Fix:** `./scripts/bump-version.sh 0.2.0` (updates 5 of 6 locations; flake.nix requires manual update).

### B2. Formatting violation
`crates/apex-reach/src/extractors/python.rs:300` has a multi-line `if` condition that rustfmt wants reformatted. CI `fmt` job will fail.

**Fix:** `cargo fmt` (single file, trivial).

### B3. 8 unfixed bugs from self-analysis (3 undocumented)
The self-analysis plan (`docs/superpowers/plans/2026-03-15-apex-self-analysis-fixes.md`, status: FUTURE) documents:
- **3 undocumented bugs** (no `bug_` tests): BudgetAllocator div-by-zero (#5, #6), SSA intersect infinite loop (#7)
- **5 documented bugs** with `bug_` tests: MOptScheduler panic, yield>1.0, Thompson empty select, Thompson slow recovery, RPC proto truncation

The 3 undocumented bugs are the most critical -- they can cause panics or infinite loops in production with no guard.

**Fix:** Implement Tasks 1-4 of the self-analysis plan (estimated 2-3 hours of crew work).

---

## Warnings (non-blocking but noteworthy)

### W1. Dependency vulnerability: shlex (RUSTSEC-2024-0006)
Transitive via libafl 0.13.2. Only compiled with `libafl-backend` feature flag (not default). Low risk for most users.

### W2. Dependency unsoundness: lru (RUSTSEC-2026-0002)
Transitive via ratatui -> libafl. Same feature-gated path. Stacked Borrows violation in `IterMut` -- unlikely to trigger in APEX's usage.

### W3. 20+ tasks remain on v0.2.0 roadmap
Per `plans/STATUS.md`:
- `language-support` (P0, 6 tasks) -- C/C++ index, Go/C#/Swift/C++ detector parity, Ruby index
- `worktree-drift-prevention` (P1, 1 task) -- T4 live protocol test
- `mcp-server` (P1, ~4 tasks) -- FUTURE, not started
- `detect-findings-remediation` (P2, 2 tasks) -- PARKED
- `rand-migration` (P2, ~5 tasks) -- FUTURE, not started

**Recommendation:** Ship v0.2.0 with current scope. Move `mcp-server`, `rand-migration`, and remaining `language-support` tasks to v0.3.0.

### W4. Homebrew sha256 checksums are placeholders
`HomebrewFormula/apex.rb` has `# sha256 "UPDATE_AFTER_FIRST_RELEASE"` for all 4 targets. These MUST be updated after CI builds release binaries, before `brew install` will work.

### W5. flake.nix version not covered by bump script
`scripts/bump-version.sh` updates 5 locations but does NOT update `version = "0.1.0"` in `flake.nix`. This will cause version mismatch after bump.

### W6. 109 `bug_` test functions document known edge cases
These are regression tests for bugs that were found and fixed, plus a few that document known-but-accepted behavior. The naming convention makes them easy to audit, but the sheer count (109 across 14 files) suggests reviewing which are truly "fixed" vs "documented but unfixed."

---

## File Map

| Area | Key Files |
|------|-----------|
| Version | `Cargo.toml:25`, `npm/package.json:3`, `python/pyproject.toml:7`, `HomebrewFormula/apex.rb:5`, `flake.nix:23` |
| Formatting | `crates/apex-reach/src/extractors/python.rs:300` |
| Unfixed bugs | `crates/apex-fuzz/src/scheduler.rs`, `crates/apex-fuzz/src/thompson.rs`, `crates/apex-agent/src/budget.rs`, `crates/apex-cpg/src/ssa.rs`, `crates/apex-rpc/src/coordinator.rs` |
| Bump script | `scripts/bump-version.sh` |
| CI | `.github/workflows/ci.yml`, `.github/workflows/release.yml` |
| Plans | `plans/STATUS.md`, `docs/superpowers/plans/2026-03-15-apex-self-analysis-fixes.md` |

---

## Recommended Release Sequence

1. Fix formatting: `cargo fmt` (1 min)
2. Fix 3 undocumented bugs (Tasks 2-3 of self-analysis plan) -- guards for div-by-zero and infinite loop (30 min)
3. Decide v0.2.0 scope cut: move mcp-server, rand-migration, remaining language-support to v0.3.0
4. Update `plans/STATUS.md` to reflect scope decision
5. Run `./scripts/bump-version.sh 0.2.0`
6. Manually update `flake.nix` version to `0.2.0`
7. Update CHANGELOG `[Unreleased]` -> `[0.2.0] -- 2026-03-16`
8. Create PR, verify CI green
9. Merge, tag `v0.2.0`, push tag
10. After CI builds release: update Homebrew sha256 values
11. `npm publish` / `twine upload`
