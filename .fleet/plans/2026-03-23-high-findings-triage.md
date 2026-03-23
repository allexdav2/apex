<!-- status: ACTIVE -->

# HIGH Findings Triage — Self-Audit (66 findings)

**Date:** 2026-03-23
**Source:** `cargo run --bin apex -- audit --target . --lang rust --severity-threshold high --output-format json`

## Summary

| Detector | Count | Disposition | Crew |
|---|---|---|---|
| substring-security | 25 | 6 real (taint.rs), 19 FP (test code + non-security `.contains()`) | security-detect |
| typestate | 11 | 11 FP (all test code: intentional bad-pattern examples) | -- (suppress) |
| unsafe-send-sync | 9 | 1 real (shm.rs already has SAFETY), 8 FP (test code) | -- (suppress) |
| info-exposure | 9 | 9 FP (test code: string literals in test assertions) | -- (suppress) |
| multi-ssrf | 7 | 7 FP (test code + static analysis regex constants) | -- (suppress) |
| vecdeque-partial | 4 | 4 FP (test code: intentional bad-pattern examples) | -- (suppress) |
| multi-sql-injection | 1 | 1 FP (test code: intentional bad-pattern example) | -- (suppress) |

**Total: 66 HIGH findings. 6 real. 60 false positives (self-referential test code).**

---

## Detailed Triage

### 1. substring-security (25 HIGH)

#### 6 REAL — taint analysis uses substring matching for security decisions

These are in production code (not tests) and use `.contains()` for matching
function names in security-relevant contexts. While this is intentional (scanning
for patterns in analyzed code), the detector correctly flags that substring
matching can produce false positives in the *analyzed* code's security checks.

However, on deeper inspection:

- **`taint.rs:305`** — `PYTHON_SOURCES.contains(&name.as_str())` — this is
  `<[&str]>::contains()` (slice membership, exact match). **FALSE POSITIVE.**
  The detector cannot distinguish `str::contains()` from `slice::contains()`.

- **`taint.rs:416-417`** — `flow.path.contains(&flow.source)` — this is
  `Vec::contains()` (vector membership). **FALSE POSITIVE.**

- **`taint_store.rs:49,52,55`** — `HashSet::contains(name)` — exact match
  lookup. **FALSE POSITIVE.**

**Revised: 0 real bugs, 25 false positives.**

The substring-security detector conflates `str::contains()` (substring) with
`slice::contains()`, `Vec::contains()`, and `HashSet::contains()` (exact
membership). This is a detector quality bug.

#### 15 in path_normalize.rs — production code, but intentional

Lines 402, 474, 487, 498-499, 563, 568, 591-593, 614-617: These use
`str::contains()` to scan source code for framework markers (`"actix_web"`,
`"vendor/"`, etc.) and normalization calls. This is a static analysis tool
scanning text — not making security decisions on user input. **Expected behavior.**

#### 4 in substring_security.rs — self-referential test code

Lines 138, 161, 214, 230: String literals inside test `files.insert()` calls
that contain `.contains()` patterns. The detector is scanning its own test
fixtures. **Test code self-reference.**

#### 1 in multi_path_traversal.rs — same pattern as path_normalize

Line 255: `source.contains(m)` checking for web framework markers. **Expected.**

### 2. typestate (11 HIGH)

**All 11 are test code** in `crates/apex-cpg/src/typestate.rs` and
`crates/apex-fuzz/src/ensemble.rs`:

- Lines 1005, 1025, 1093, 1148, 1185, 1233, 1238, 1266, 1296, 1442: All inside
  `#[cfg(test)] mod tests` in typestate.rs. These are intentional bad-pattern
  examples that the typestate detector should find (e.g., `f.close(); f.read()`
  to test use-after-close detection).

- Line 300 in ensemble.rs: Two sequential `corpus.lock().unwrap()` calls where
  each guard is dropped before the next. **FALSE POSITIVE** — the detector
  does not track MutexGuard drop points.

**Disposition: 11 FP (test code + lock-guard-drop false positive).**

### 3. unsafe-send-sync (9 HIGH)

- **`shm.rs:27`** — `unsafe impl Send for ShmBitmap {}` — already has a proper
  multi-line `// SAFETY:` comment at lines 21-26. The detector found it but the
  audit still reports it. Checking: the detector allows SAFETY comments within
  3 lines. The comment starts at line 21, the impl is at line 27 — that is 6
  lines away. **REAL: widen detector window or move comment closer.**

- **8 in `unsafe_send_sync.rs`** (lines 116, 131, 144, 157, 169, 181, 193,
  205): All inside `#[cfg(test)] mod tests`. These are intentional bad-pattern
  test fixtures. **Test code self-reference.**

**Disposition: 1 real fix (shm.rs comment proximity), 8 FP.**

### 4. info-exposure (9 HIGH)

- **5 in `security_pattern.rs`** (lines 186, 195, 651, 660, 850): These are
  `SecurityPattern` struct definitions that contain the string `"password"` in
  `user_input_indicators` arrays. The info-exposure detector sees `"password"`
  in source and flags it. **FALSE POSITIVE** — these are detector rule
  definitions, not exposed fields.

- **2 in `api_diff.rs`** (lines 883, 1134): Test code with JSON fixtures like
  `"name": "token"` inside `make_spec()` calls. **Test code self-reference.**

- **2 in `info_exposure.rs`** (lines 388, 423): Test code with intentional
  bad-pattern fixtures (`traceback.format_exc()`, `'password'` in serializer).
  **Test code self-reference.**

**Disposition: 9 FP.**

### 5. multi-ssrf (7 HIGH)

- **`agent_report.rs:645`** — Test code: `"let resp = reqwest::get(url).await?"`
  inside a test fixture string. **Test code self-reference.**

- **`service_map.rs:41`** — Regex pattern `reqwest::Client|hyper::Client` in a
  `LazyLock<Vec<Regex>>` for detecting HTTP clients in analyzed code. Not an
  actual HTTP call. **FALSE POSITIVE.**

- **5 in `missing_async_timeout.rs`** (lines 25-26, 204, 315, 328): Lines 25-26
  are `const` string patterns for detection. Lines 204, 315, 328 are test code
  with intentional bad-pattern fixtures. **FP + test code self-reference.**

**Disposition: 7 FP.**

### 6. vecdeque-partial (4 HIGH)

All 4 in `vecdeque_partial.rs` (lines 104, 119, 155, 167): Test code with
intentional bad-pattern fixtures like `"let data = ring.as_slices().0;\n"`.
**Test code self-reference.**

**Disposition: 4 FP.**

### 7. multi-sql-injection (1 HIGH)

Line 590 in `multi_sql_injection.rs`: Test code with fixture string
`"sqlx::query(&format!(\"SELECT * FROM users WHERE id={}\", id))"`.
**Test code self-reference.**

**Disposition: 1 FP.**

---

## Action Plan

### Wave 1 — All parallel (no dependencies)

#### Task 1.1 — security-detect crew: Fix substring-security detector (HIGH priority)

The detector cannot distinguish `str::contains()` from `slice::contains()`,
`Vec::contains()`, or `HashSet::contains()`. All of these resolve to a method
named `contains` but only `str::contains()` does substring matching.

**Fix:** In the substring-security detector, add heuristics to skip:
1. `<slice>.contains(&item)` — the `&` before the argument suggests typed membership
2. Lines where the receiver is clearly a collection (HashSet, Vec, BTreeSet, slice)
3. Alternatively: require the `.contains()` call to have a string literal or
   `.as_str()` argument without `&` prefix

**Files:** `crates/apex-detect/src/detectors/substring_security.rs`

#### Task 1.2 — runtime crew: Fix shm.rs SAFETY comment proximity

The SAFETY comment at lines 21-26 is 6 lines above the `unsafe impl Send` at
line 27. The detector window is 3 lines. Either:
- (a) Move the SAFETY comment to line 25-26 (immediately above), or
- (b) Widen the detector window (risks false negatives)

Preferred: (a) — consolidate the multi-line comment to be directly above.

**Files:** `crates/apex-sandbox/src/shm.rs`

#### Task 1.3 — security-detect crew: Suppress test-code self-reference findings

48 of 66 HIGH findings are test code scanning itself. The detectors already have
`is_test_file()` checks for files in `tests/` directories, but they do not
recognize `#[cfg(test)] mod tests` blocks inside the same source file.

**Options (pick one):**
- (a) Add inline `#[cfg(test)]` block detection to skip test modules within
  production files. This is complex and fragile for regex-based detectors.
- (b) Accept that self-scan of test fixtures is inherent to self-audit. Add a
  `.apex/suppressions.toml` or `--exclude-self-test` flag. Suppressions file
  with detector+file+line entries.
- (c) Mark these findings as `noisy: true` when the finding's file path matches
  a file that IS a detector (i.e., in `crates/apex-detect/src/detectors/`).

**Recommended: (c)** — When the audit target is the APEX project itself, findings
in detector test code are expected and should be auto-marked noisy. This can be
implemented as a post-filter in the audit command.

**Files:** `crates/apex-cli/src/lib.rs` (audit command) or
`crates/apex-detect/src/lib.rs` (finding post-processor)

#### Task 1.4 — security-detect crew: Reduce path_normalize FP rate

15 findings in path_normalize.rs come from the detector's own use of
`.contains()` for scanning source code. These are not security decisions on user
input. The substring-security detector should recognize that scanning patterns
in a static analysis tool's source is not the same as making auth/access
decisions based on substring matching.

**Fix:** Same as Task 1.1 — improving substring-security's precision will
eliminate these. No separate action needed.

**Merged into Task 1.1.**

---

## Revised Finding Counts After Triage

| Category | Count | Disposition |
|---|---|---|
| Real bug (fix code) | 1 | shm.rs comment proximity |
| Detector quality bug (fix detector) | 25 | substring-security conflates str/collection contains |
| Test code self-reference (suppress) | 39 | detectors scanning own test fixtures |
| Rule definition FP (suppress) | 1 | info-exposure flagging SecurityPattern defs with "password" |

## Crew Assignments

| Task | Crew | Priority | Files |
|---|---|---|---|
| 1.1 | security-detect | HIGH | `crates/apex-detect/src/detectors/substring_security.rs` |
| 1.2 | runtime | LOW | `crates/apex-sandbox/src/shm.rs` |
| 1.3 | security-detect | MEDIUM | `crates/apex-detect/src/lib.rs` or `crates/apex-cli/src/lib.rs` |
