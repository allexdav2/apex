# APEX Suppressed Warnings Registry

Centralized catalog of all false-positive suppression mechanisms across APEX detectors.

---

## Global Suppressions (All Detectors)

### Test File Exclusion
**File:** `crates/apex-detect/src/detectors/util.rs` — `is_test_file()`

| Pattern Type | Values |
|---|---|
| Test directories | `tests/`, `test/`, `__tests__/`, `spec/` |
| Benchmark dirs | `benches/` |
| File suffixes | `_test.rs`, `_test.py`, `_test.go` |
| File prefixes | `test_` (filename only) |
| Helper stems | `testutil`, `testutils`, `test_util`, `test_utils`, `test_helpers`, `test_helper`, `testing`, `conftest` |
| NOT matched | `test_data/`, `test_fixtures/` (data dirs, not test code) |

### Comment Exclusion
**File:** `crates/apex-detect/src/detectors/util.rs` — `is_comment()`

| Language | Patterns |
|---|---|
| All | `//`, `/*`, `* `, `*` |
| Python, Ruby | `#` prefix |

### Test Block Exclusion (Rust)
**File:** `crates/apex-detect/src/detectors/util.rs` — `in_test_block()`

Skips code inside `#[cfg(test)]` and `mod tests { }` blocks (brace-depth tracking).

---

## Per-Detector Suppressions

### hardcoded_secret / secret_scan

**Placeholder Values** (shared between both detectors):
`changeme`, `CHANGEME`, `your-`, `YOUR_`, `xxx`, `XXX`, `placeholder`, `PLACEHOLDER`, `example`, `EXAMPLE`, `replace_me`, `REPLACE_ME`, `TODO`, `FIXME`, `test`, `dummy`, `fake`, `sample`, `demo`

**Environment Variable Markers** (`util.rs`):
`env(`, `ENV[`, `os.environ`, `process.env`, `std::env`, `getenv(`

**Example/Doc File Patterns:**
`.example`, `.sample`, `.template`, `.md`, `.txt`, `.rst`

**secret_scan additional skip patterns:**
`instrument`, `generated`, `source_map`, `fixture`, `/detectors/*.rs`

**secret_scan additional filters:**
- Const string declarations: `const FOO: &str = "..."` (Rust only)
- Entropy threshold: 4.5 bits/char minimum
- String literal minimum length: 8 characters

### bandit (Python only)

| Rule | Suppressor Regex |
|---|---|
| B506 (`yaml.load`) | `(?i)SafeLoader\|CSafeLoader\|safe_load` |
| All others | None |

### security_pattern (All Languages)

**Context window:** ±3 lines around target line.

**Severity adjustment:**
- User input indicators defined but NOT found → downgrade (Critical→High→Medium→Low)
- Sanitization indicators found → downgrade again

**Sanitization indicators by language/sink:**

| Sink | Language | Sanitizers |
|---|---|---|
| `Command::new()` | Rust | `escape`, `sanitize`, `quote`, `shell_escape` |
| `eval()` | Python | `ast.literal_eval`, `safe_eval` |
| `yaml.load()` | Python | `SafeLoader`, `safe_load`, `CSafeLoader` |
| `subprocess.*()` | Python | `shlex.quote`, `shlex.split`, `shell=False` |
| `.execute()` (SQL) | Python | `?`, `%s,`, `parameterize`, `placeholder` |
| `child_process.exec()` | JS | `escape`, `sanitize`, `execFile` |
| `innerHTML` | JS | `sanitize`, `escape`, `DOMPurify`, `encode`, `textContent` |
| `send()` | Ruby | `whitelist`, `allow_list`, `permitted`, `include?` |
| `.where()` | Ruby | `sanitize_sql`, `?`, `placeholder`, `where(` |
| `strcpy()` | C | `strlen`, `sizeof`, `strlcpy`, `strncpy` |
| `sprintf()` | C | `snprintf` |

**Code pattern indicators** (filtered before threat-model evaluation):
`shell=true`, `shell=false`, `format!`, `format(`, `f"`, `%s`, `%s,`, `%`, `+`, `open(`, `read`, `parameterize`, `placeholder`, `?`

---

## Threat Model Suppressions

**File:** `crates/apex-detect/src/threat_model.rs`

Source trust per threat model type:

| Source | CLI Tool | Web Service | Library | CI Pipeline |
|---|---|---|---|---|
| `argv`, `arg(` | Trusted | Untrusted | Untrusted | Trusted |
| `request`, `query`, `form`, `param` | N/A | **Untrusted** | Untrusted | N/A |
| `input` | Trusted | **Untrusted** | Untrusted | N/A |
| `stdin` | Trusted | **Untrusted** | Untrusted | Trusted |
| `environ`, `getenv` | Trusted | Trusted | Untrusted | Trusted |
| `recv`, `socket` | **Untrusted** | **Untrusted** | **Untrusted** | **Untrusted** |
| `file` | Trusted | **Untrusted** | Untrusted | Trusted |
| `user`, `upload` | N/A | **Untrusted** | Untrusted | N/A |
| `format!`, `&str` | Trusted | Untrusted | Trusted | Trusted |

**Logic:** If ALL matched indicators are Trusted/N/A → suppress finding. If ANY is Untrusted → report.

**User overrides:** `trusted_sources` and `untrusted_sources` in config take precedence over built-in table.

---

## Known Duplication Issues

| Issue | Files | Status |
|---|---|---|
| `FALSE_POSITIVE_VALUES` duplicated | `hardcoded_secret.rs`, `secret_scan.rs` | **FIXED** — extracted to `util.rs` |

---

## Real-World Validation Findings (2026-03-16)

**11 runs across 10 repos, 12,770 total findings, 0 crashes.**

### Summary

| Repo | Language | Findings | Time | Notes |
|---|---|---|---|---|
| linux-kernel | C | 2,377 | 4m 8s | CWE-134 FPs dominate (1,470) |
| cpython-c | C | 2,172 | 1m 29s | Legitimate `gets()` criticals (4) |
| cpython-py | Python | 2,393 | 1m 28s | Pickle/eval in stdlib = context FP |
| typescript | JS/TS | 3,656 | 21m 54s | path-normalize FPs (1,638) |
| ripgrep | Rust | 585 | 13s | Clean, mostly static-analysis (264) |
| spring-boot | Java | 29 | ~1s | Low noise |
| kubernetes | Go | 408 | 10m 8s | 1 TP: hardcoded test private key |
| dotnet | C# | 75 | 7s | Low noise |
| vapor | Swift | 113 | 2s | 2 TP: hardcoded private keys |
| rails | Ruby | 950 | 4s | secret-scan entropy FPs (177) |
| ktor | Kotlin | 12 | <1s | Cleanest result |

### FP Classification

| Category | Count | % of Total | Action |
|---|---|---|---|
| `panic-pattern` (low sev) | 3,768 | 29% | Code quality, not security — consider separate report |
| `mixed-bool-ops` | 2,021 | 16% | Code quality, not security |
| `path-normalize` (TS compiler) | 1,638 | 13% | FP: compiler legitimately manipulates paths |
| CWE-134 format string (C) | 1,584 | 12% | FP: `printk`/`printf` with literal format strings |
| `secret-scan` entropy-only | 838 | 7% | FP: high-entropy code strings, not secrets |
| `process-exit-in-lib` | 74 | 1% | FP in binaries, valid in libraries only |
| **Likely FP total** | **9,923** | **77%** | |
| **Potentially actionable** | **2,847** | **23%** | |

### True Positives Found

| Repo | Detector | Finding | Verdict |
|---|---|---|---|
| kubernetes | hardcoded-secret | Private key in `testserver.go:60` | TP — test key, but correctly flagged |
| vapor | hardcoded-secret | Private key in `configure.swift:49` | TP — development key in source |
| cpython-c | security-pattern | `gets()` in `obmalloc.c`, `fileobject.c` | TP — banned function CWE-242 |
| cpython-py | security-pattern | pickle.load in `multiprocessing/spawn.py` | Context FP — stdlib implementation, not user code |
| cpython-py | security-pattern | f-string SQL in `sqlite3/dump.py` | Context FP — internal tool, not user-facing |

### Top FP Patterns to Fix

1. **CWE-134 format string in C/C++**: Detector matches `printf`-family calls but doesn't verify format arg is a variable (not literal). Fix: skip if first arg after function name is a string literal.
2. **`secret-scan` entropy threshold too low**: 4.5 bits/char catches normal code identifiers like `cpumask_pr_args`, `MHcCAQEEIEZm`. Fix: raise threshold to 5.0 or add language-aware token filtering.
3. **`path-normalize` in compilers/build tools**: Tools that manipulate file paths by design trigger path traversal warnings. Fix: add context suppression for compiler/toolchain repos.
4. **`mixed-bool-ops` volume**: Not security-relevant. Consider making opt-in or separating from security audit.
5. **`panic-pattern` volume**: Code quality finding, not security. Same recommendation.
