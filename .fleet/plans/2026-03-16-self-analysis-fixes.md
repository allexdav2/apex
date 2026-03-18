<!-- status: DONE -->

# Fix Self-Analysis Findings

Date: 2026-03-16
Goal: Fix the top 6 issues found by APEX's self-analysis, prioritized by severity.

## Analysis Summary

After reading all flagged source locations, here is the real situation:

### Finding 1: Command Injection — RE-ASSESSED as LOWER risk

- **mcp.rs:110** (`run_apex_command`): Calls `Command::new(&exe)` where `exe = std::env::current_exe()` — this is the APEX binary itself, NOT user input. The args come from MCP tool parameters (target paths, lang strings). These are passed as array elements to `.args()`, NOT through a shell. **No shell injection possible.** However, the `params.target` path is not validated/canonicalized — a malicious MCP client could pass `../../etc/passwd` as a target. Risk: path traversal, not command injection.
- **lib.rs:2268** (`run_diff`): Calls `Command::new("git")` with `args.base` from CLI `--base` flag. Passed as array arg, NOT through shell. Git itself validates ref names. However, `args.base` could contain `--` prefixed strings that git interprets as flags. Risk: argument injection, not command injection.

**Real fixes needed:**
1. Validate `args.base` is a plausible git ref (no `--` prefix, alphanumeric + `/._-`)
2. Canonicalize `params.target` in MCP handlers before passing to subprocess
3. Use `--` separator before positional args in git commands

### Finding 2: Unsafe Send/Sync — ALREADY FIXED

- **shm.rs:27**: Lines 21-26 already contain a detailed `// SAFETY:` comment. The detector flagged this because of line numbering — the comment IS there. This is a **false positive from the self-analysis**.
- Test fixtures in `unsafe_send_sync.rs`: These are intentionally unsafe for testing. No action needed.

**No fix needed.** Remove from plan.

### Finding 3: Secret-scan false positives — REAL, needs detector improvement

The `secret_scan.rs` detector already skips:
- `is_skip_file()` — files with "test" in path, `.example`, `.sample`, `.md`, `.txt`
- `is_comment()` — comment lines
- `in_test_block()` — code inside test blocks
- `contains_placeholder()` — known placeholder values
- `references_env_var()` — env var references

The 30 false positives likely come from:
- Instrumentation template strings (high-entropy hex in `apex-instrument`)
- Detector test data that isn't inside `#[cfg(test)]` blocks but in const fixtures
- Files not matching the "test" path heuristic

**Fix:** Add `#[cfg(test)]`-aware block detection for Rust, and add instrumentation template file patterns to `is_skip_file()`.

### Finding 4: Path normalization — 3 real locations in apex-cli

- `lib.rs:1682` — `std::fs::write(&path, ...)` for audit output
- `lib.rs:2616` — `std::fs::write(&path, ...)` for docs output
- `lib.rs:3687` — `std::fs::write(&out_path, ...)` for compliance report

All three are `--output` flag values from CLI. The risk is writing to unintended locations. Fix: canonicalize parent directory, refuse to write outside project root or check the path is not a symlink to a sensitive location. Pragmatically: these are CLI tools run by the user on their own machine — the real risk is minimal. But defense-in-depth is good practice.

**Fix:** Add a `validate_output_path()` helper that canonicalizes and warns on suspicious paths.

### Finding 5: Dependency-audit detector failure — REAL

`dep_audit.rs` calls `ctx.runner.run_command()` with `cargo audit`. If `cargo-audit` isn't installed, the command runner returns an error that propagates as `ApexError::Detect`. The detector should catch this and return an info-level finding instead of failing.

**Fix:** Wrap the `run_command` call, detect "command not found" / exit code patterns, return a graceful info finding.

### Finding 6: Panic patterns — LOW priority, deferred

212 `unwrap()`/`expect()` calls across library code. Most are in:
- Test helpers (expected)
- LazyLock regex compilation (expected — compile-time known patterns)
- `serde_json` field access with fallback chains

This is a code quality sweep, not a security fix. Defer to a separate PR.

---

## File Map

| Crew | Files | Finding |
|------|-------|---------|
| platform | `crates/apex-cli/src/lib.rs` | #1 (git arg injection), #4 (path validation) |
| mcp-integration | `crates/apex-cli/src/mcp.rs` | #1 (MCP path validation) |
| security-detect | `crates/apex-detect/src/detectors/secret_scan.rs` | #3 (false positive suppression) |
| security-detect | `crates/apex-detect/src/detectors/dep_audit.rs` | #5 (graceful fallback) |

## Wave 1 (no dependencies)

All four tasks are independent — different files, different crews.

### Task 1.1 — platform crew
**Finding:** #1 + #4 — Git ref validation + output path validation in apex-cli
**Files:** `crates/apex-cli/src/lib.rs`
**Complexity:** Medium (3 locations for path validation, 1 for git ref validation)

Steps:
- [ ] Add `fn validate_git_ref(s: &str) -> Result<()>` that rejects strings starting with `-`, containing `..`, or with shell metacharacters
- [ ] Add `fn validate_output_path(p: &Path) -> Result<PathBuf>` that canonicalizes the parent dir and returns the resolved path
- [ ] Write failing tests: git ref with `--exec`, path with `../../../etc/passwd`, valid ref, valid path
- [ ] Run tests, confirm failures
- [ ] Implement both validators
- [ ] Apply `validate_git_ref` to `args.base` in `run_diff` (line 2267) — add `--` before ref arg
- [ ] Apply `validate_output_path` at lines 1682, 2616, 3687
- [ ] Run tests, confirm pass
- [ ] Run `cargo clippy -p apex-cli -- -D warnings`
- [ ] Commit

### Task 1.2 — mcp-integration crew
**Finding:** #1 — MCP target path validation
**Files:** `crates/apex-cli/src/mcp.rs`
**Complexity:** Low (add canonicalization to MCP param handling)

Steps:
- [ ] Add path canonicalization for `params.target` in each MCP tool handler
- [ ] Reject targets that resolve outside a reasonable scope (or at minimum, must exist)
- [ ] Write test for `run_apex_command` with a nonexistent target path
- [ ] Run tests, confirm behavior
- [ ] Commit

### Task 1.3 — security-detect crew
**Finding:** #3 — Secret-scan false positive suppression
**Files:** `crates/apex-detect/src/detectors/secret_scan.rs`
**Complexity:** Medium (improve `is_skip_file` + `in_test_block` heuristics)

Steps:
- [ ] Add instrumentation template patterns to `is_skip_file()`: files containing `instrument`, `template`, `fixture`, `generated`
- [ ] Improve `in_test_block()` for Rust: detect `#[cfg(test)]` module scope, `const` test data outside test modules
- [ ] Add `is_detector_test_data()` check — const strings in detector source files that contain known patterns
- [ ] Write test with a mock source file containing high-entropy instrumentation hex — should NOT trigger
- [ ] Write test with a real secret pattern outside test context — should still trigger
- [ ] Run tests, confirm pass
- [ ] Run `cargo clippy -p apex-detect -- -D warnings`
- [ ] Commit

### Task 1.4 — security-detect crew
**Finding:** #5 — Dependency-audit graceful fallback
**Files:** `crates/apex-detect/src/detectors/dep_audit.rs`
**Complexity:** Low (wrap error handling around run_command)

Steps:
- [ ] In `audit_cargo()`, `audit_pip()`, `audit_npm()`: catch command-not-found errors
- [ ] On command-not-found, return a single Info-severity finding: "{tool} not installed — skipping dependency audit"
- [ ] Write test: FixtureRunner that returns error for "cargo" command
- [ ] Verify detector returns info finding instead of propagating error
- [ ] Run tests, confirm pass
- [ ] Commit

## Wave 2 (depends on Wave 1)

### Task 2.1 — platform crew
**Finding:** Cross-cutting — CHANGELOG + integration verification
**Files:** `CHANGELOG.md`
**Complexity:** Low

Steps:
- [ ] Update CHANGELOG.md under `[Unreleased]` with all fixes
- [ ] Run full `cargo nextest run --workspace` to verify no regressions
- [ ] Run `cargo clippy --workspace -- -D warnings`
- [ ] Commit

---

## Deferred (separate PR)

### Finding 6: Panic patterns (LOW)
- 212 `unwrap()`/`expect()` calls in library code
- Requires crate-by-crate audit to distinguish legitimate (LazyLock, test) from risky (user input paths)
- Estimated: 2-3 hours of work across 10+ files
- Track in TODO.md

### Finding 2: Unsafe Send/Sync (FALSE POSITIVE)
- shm.rs already has the SAFETY comment at lines 21-26
- Self-analysis line numbering was off — no code change needed

---

## Estimated Effort

| Task | Crew | Complexity | Est. Time |
|------|------|-----------|-----------|
| 1.1 | platform | Medium | 30-45 min |
| 1.2 | mcp-integration | Low | 15-20 min |
| 1.3 | security-detect | Medium | 30-45 min |
| 1.4 | security-detect | Low | 15-20 min |
| 2.1 | platform | Low | 10-15 min |
| **Total** | | | **~2 hours** |

## Decision Gates

1. **Before Wave 1**: Confirm the re-assessment of Finding 1 (command injection downgraded to arg/path injection) and Finding 2 (false positive, no fix needed). Proceed?
2. **After Wave 1**: Review all crew changes before CHANGELOG + integration test wave.
3. **After Wave 2**: Decide whether to open a separate PR for Finding 6 (panic patterns).
