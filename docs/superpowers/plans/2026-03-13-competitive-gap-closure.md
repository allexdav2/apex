# Close APEX Detection Gaps vs Semgrep/Bearer/OSV-Scanner

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the security detection gaps found by running Semgrep 1.155, Bearer 2.0.1, and OSV-Scanner 2.3.3 against the same targets as APEX and comparing findings.

**Evidence base:** Ran all tools against Flask, Express, and a synthetic test app. Identified 6 categories of missed detections ranked by impact.

**Architecture:** Extend existing detectors with new patterns and a new detector. No new crates. No new heavy deps. Each task independently committable.

---

## Competitive Analysis Summary

### Tools Installed & Compared

| Tool | Version | Focus |
|---|---|---|
| **Semgrep** | 1.155.0 | SAST — pattern-based, 20k+ rules, 35+ languages |
| **Bearer** | 2.0.1 | SAST — dataflow-aware, CWE-mapped, 4 languages |
| **CrossHair** | 0.0.102 | Symbolic execution — Python contract verification |
| **OSV-Scanner** | 2.3.3 | Dependency vulnerability scanning (all ecosystems) |
| **pip-audit** | 2.10.0 | Python dependency audit |
| **APEX** | 0.1.0 | Multi-technique coverage + security analysis |

### Side-by-Side Results (Flask, Express, synthetic app)

**Flask (Python):**
- Semgrep: 15 findings (CSRF, eval/exec, SHA1, SRI integrity, Markup unescape)
- Bearer: 13 findings (CWE-94 code injection, CWE-22 path traversal, CWE-328 weak hash)
- APEX: 42 findings (2 HIGH, 27 MEDIUM, 13 LOW)

**Express (JavaScript):**
- Semgrep: 43 findings (cookie misconfig x30, hardcoded secrets, response-write XSS, template unescape)
- APEX: 37 findings (path-normalize x30, panic-pattern x7)

### APEX Unique Strengths (No Competitor Matches)

1. **Coverage-driven security**: Only tool combining branch coverage with security analysis
2. **Test intelligence suite**: test-optimize, test-prioritize, dead-code, flaky-detect
3. **Multi-technique exploration**: Coverage + fuzzing + concolic in one tool
4. **Coverage ratchet CI gate**: `apex ratchet` prevents coverage regression
5. **7-language instrumentor**: Unified interface to coverage.py, Istanbul, JaCoCo, cargo-llvm-cov, SanCov, wasm-opt

### Awesome Lists Where APEX Could Be Listed

| Awesome List | Why APEX Fits |
|---|---|
| [analysis-tools-dev/static-analysis](https://github.com/analysis-tools-dev/static-analysis) | Multi-language SAST with security pattern detection |
| [cpuu/awesome-fuzzing](https://github.com/cpuu/awesome-fuzzing) | Coverage-guided fuzzing engine (LibAFL integration) |
| [ksluckow/awesome-symbolic-execution](https://github.com/ksluckow/awesome-symbolic-execution) | Concolic engine for Python |
| [sottlmarek/DevSecOps](https://github.com/sottlmarek/DevSecOps) | Full SDLC security: SAST + dep audit + coverage gate |
| [TheJambo/awesome-testing](https://github.com/TheJambo/awesome-testing) | Test intelligence (prioritize, dead-code, ratchet) |
| [atinfo/awesome-test-automation](https://github.com/atinfo/awesome-test-automation) | Coverage-guided test optimization |
| [devsecops/awesome-devsecops](https://github.com/devsecops/awesome-devsecops) | CI gate (ratchet), attack surface, dep audit |

### Interesting Projects Discovered

| Project | What It Does | Why Interesting for APEX |
|---|---|---|
| [Joern](https://github.com/joernio/joern) | Code Property Graph for 8 languages | Could replace regex-based SAST with CPG queries |
| [CoverUp](https://arxiv.org/html/2403.16218v3) | LLM-guided test generation for coverage gaps | Same goal as APEX's synth engine |
| [EvoMaster](https://github.com/EMResearch/EvoMaster) | Evolutionary API fuzzing | Similar evolutionary approach |
| [SymCC](https://github.com/eurecom-s3/symcc) | Compiler-embedded symbolic execution | Could extend concolic to C/C++ |
| [Owi](https://github.com/OCamlPro/owi) | Parallel symbolic execution on WASM | Could add real WASM symbolic support |
| [OSV-Scanner](https://github.com/google/osv-scanner) | Universal dep scanner | Could replace 3 separate audit wrappers |

---

## Findings Gap Summary (from real-world runs)

| Gap | Who Catches It | APEX Miss Reason | Impact |
|---|---|---|---|
| `subprocess.run(shell=True)` | Semgrep, Bearer | Only `subprocess.call(` pattern exists | P0 — command injection is top-10 OWASP |
| Cookie session misconfig | Semgrep (30 findings on Express) | Zero framework-aware rules | P1 — 30 findings missed per Express scan |
| CWE-ID mapping | Bearer (every finding) | No `cwe` field on Finding | P2 — compliance/reporting blocker |
| Expression-level path traversal | Bearer (4 findings on test app) | Path detector is function-scoped only | P3 — misses inline `open(path)` |
| `res.write()`/`res.send()` with user data | Semgrep (7 findings on Express) | No output context patterns | P4 — XSS via response |
| `__import__()` code injection | Bearer | Missing pattern | P5 — trivial to add |

---

## Task 1: Expand Python Command Injection Patterns

**Why:** `subprocess.run(` is the modern Python API (replaced `call()` in 3.5). APEX only detects `subprocess.call(`. Both Semgrep and Bearer caught `subprocess.run(shell=True)` — APEX missed it completely.

**File:** `crates/apex-detect/src/detectors/security_pattern.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[tokio::test]
async fn detects_subprocess_run_shell_true() {
    let src = "def execute(cmd):\n    subprocess.run(cmd, shell=True)\n";
    let ctx = make_ctx("app.py", src, Language::Python);
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty(), "should detect subprocess.run with shell=True");
    assert_eq!(findings[0].category, FindingCategory::Injection);
}

#[tokio::test]
async fn detects_subprocess_popen() {
    let src = "def run(cmd):\n    p = subprocess.Popen(cmd, shell=True)\n";
    let ctx = make_ctx("app.py", src, Language::Python);
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty(), "should detect subprocess.Popen");
}

#[tokio::test]
async fn detects_os_popen() {
    let src = "def run(cmd):\n    os.popen(cmd)\n";
    let ctx = make_ctx("app.py", src, Language::Python);
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty(), "should detect os.popen");
}

#[tokio::test]
async fn detects_dunder_import() {
    let src = "def load(name):\n    mod = __import__(name)\n";
    let ctx = make_ctx("app.py", src, Language::Python);
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty(), "should detect __import__");
}
```

- [ ] **Step 2: Add 4 new Python patterns**

Add to `PYTHON_SECURITY_PATTERNS`:
```rust
SecurityPattern {
    sink: "subprocess.run(",
    description: "subprocess.run() — command injection if shell=True or unsanitized input",
    category: FindingCategory::Injection,
    base_severity: Severity::High,
    user_input_indicators: &["shell=True", "format(", "f\"", "request", "input", "%s"],
    sanitization_indicators: &["shlex.quote", "shlex.split", "shell=False"],
},
SecurityPattern {
    sink: "subprocess.Popen(",
    description: "subprocess.Popen() — command injection if shell=True or unsanitized input",
    category: FindingCategory::Injection,
    base_severity: Severity::High,
    user_input_indicators: &["shell=True", "format(", "f\"", "request", "input", "%s"],
    sanitization_indicators: &["shlex.quote", "shlex.split", "shell=False"],
},
SecurityPattern {
    sink: "os.popen(",
    description: "os.popen() — command injection, always uses shell",
    category: FindingCategory::Injection,
    base_severity: Severity::High,
    user_input_indicators: &["format(", "f\"", "request", "input", "%s", "+"],
    sanitization_indicators: &["shlex.quote"],
},
SecurityPattern {
    sink: "__import__(",
    description: "__import__() — arbitrary module loading if input is user-controlled",
    category: FindingCategory::Injection,
    base_severity: Severity::High,
    user_input_indicators: &["request", "input", "param", "query", "form", "argv"],
    sanitization_indicators: &["allowlist", "whitelist", "ALLOWED"],
},
```

- [ ] **Step 3: Run tests, verify pass**

Run: `cargo test -p apex-detect -- security_pattern`

- [ ] **Step 4: Commit**

---

## Task 2: Add JS Framework Security Patterns

**Why:** Semgrep found 30 cookie/session misconfig findings on Express that APEX missed entirely. Also missing: `res.write()` XSS, `require()` injection, `child_process.spawn()`.

**File:** `crates/apex-detect/src/detectors/security_pattern.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[tokio::test]
async fn detects_res_write_xss() {
    let src = "function handle(req, res) {\n  res.write(req.query.data);\n}\n";
    let ctx = make_ctx("app.js", src, Language::JavaScript);
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty());
}

#[tokio::test]
async fn detects_child_process_spawn() {
    let src = "const { spawn } = require('child_process');\nfunction run(cmd) {\n  spawn(cmd);\n}\n";
    let ctx = make_ctx("app.js", src, Language::JavaScript);
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty());
}

#[tokio::test]
async fn detects_require_with_variable() {
    let src = "function load(name) {\n  const mod = require(name);\n}\n";
    let ctx = make_ctx("app.js", src, Language::JavaScript);
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty());
}
```

- [ ] **Step 2: Add new JS patterns**

Add to `JS_SECURITY_PATTERNS`:
```rust
// Response XSS patterns
SecurityPattern {
    sink: "res.write(",
    description: "res.write() — XSS if response includes unsanitized user input",
    category: FindingCategory::Injection,
    base_severity: Severity::High,
    user_input_indicators: &["req.", "request", "params", "query", "body", "input"],
    sanitization_indicators: &["escape", "encode", "sanitize", "textContent"],
},
SecurityPattern {
    sink: "res.send(",
    description: "res.send() — XSS if response includes unsanitized user input",
    category: FindingCategory::Injection,
    base_severity: Severity::High,
    user_input_indicators: &["req.", "request", "params", "query", "body", "input"],
    sanitization_indicators: &["escape", "encode", "sanitize", "json"],
},
// Command injection
SecurityPattern {
    sink: "child_process.spawn(",
    description: "child_process.spawn — command injection if input is user-controlled",
    category: FindingCategory::Injection,
    base_severity: Severity::High,
    user_input_indicators: &["req.", "request", "params", "query", "body", "input"],
    sanitization_indicators: &["escape", "sanitize"],
},
SecurityPattern {
    sink: "child_process.execSync(",
    description: "child_process.execSync — synchronous command injection via shell",
    category: FindingCategory::Injection,
    base_severity: Severity::Critical,
    user_input_indicators: &["req.", "request", "params", "query", "body", "input", "${", "`"],
    sanitization_indicators: &["escape", "sanitize"],
},
// Dynamic require
SecurityPattern {
    sink: "require(",
    description: "Dynamic require() — arbitrary module loading if path is user-controlled",
    category: FindingCategory::Injection,
    base_severity: Severity::Medium,
    user_input_indicators: &["req.", "request", "params", "query", "body", "input", "argv"],
    sanitization_indicators: &["allowlist", "whitelist", "ALLOWED", "path.join"],
},
```

- [ ] **Step 3: Run tests, verify pass**

- [ ] **Step 4: Commit**

---

## Task 3: Cookie/Session Security Detector

**Why:** Semgrep found 30 cookie-session misconfig findings on Express (no-httponly, no-secure, no-domain, no-expires, hardcoded secret). This is an entirely new detector category — not just pattern matching but configuration analysis.

**Files:**
- Create: `crates/apex-detect/src/detectors/session_security.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs`
- Modify: `crates/apex-detect/src/pipeline.rs`
- Modify: `crates/apex-detect/src/config.rs`
- Modify: `crates/apex-detect/src/finding.rs` — add `InsecureConfig` category

- [ ] **Step 1: Add `InsecureConfig` to FindingCategory**

- [ ] **Step 2: Write failing tests**

```rust
#[tokio::test]
async fn detects_express_session_no_secure() {
    let src = r#"
const session = require('express-session');
app.use(session({
    secret: 'keyboard cat',
    cookie: { maxAge: 60000 }
}));
"#;
    let ctx = make_ctx("app.js", src, Language::JavaScript);
    let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
    // Should detect: hardcoded secret + missing secure + missing httpOnly
    assert!(findings.len() >= 2);
}

#[tokio::test]
async fn detects_flask_hardcoded_secret_key() {
    let src = "app = Flask(__name__)\napp.secret_key = 'super-secret'\n";
    let ctx = make_ctx("app.py", src, Language::Python);
    let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty());
}

#[tokio::test]
async fn no_finding_when_env_var_used() {
    let src = "app.secret_key = os.environ.get('SECRET_KEY')\n";
    let ctx = make_ctx("app.py", src, Language::Python);
    let findings = SessionSecurityDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}
```

- [ ] **Step 3: Implement SessionSecurityDetector**

Detection logic per language:

**Python:**
- `app.secret_key = 'literal'` → hardcoded secret [High]
- `SECRET_KEY = 'literal'` → hardcoded secret [High]
- Ignore if `os.environ`, `os.getenv`, `config[`, `settings.`

**JavaScript:**
- `session({ secret: 'literal' })` → hardcoded secret [High]
- Session config missing `secure: true` → insecure cookie [Medium]
- Session config missing `httpOnly: true` → XSS-accessible cookie [Medium]
- Session config missing `sameSite` → CSRF risk [Low]
- Cookie config without `domain` → overly broad scope [Low]

**Detection approach:** Multi-line scan. When `session(` or `cookie-session` found, collect lines until closing `)`. Parse config object for missing fields.

- [ ] **Step 4: Register in mod.rs, pipeline.rs, config.rs**

- [ ] **Step 5: Run tests, verify pass**

- [ ] **Step 6: Commit**

---

## Task 4: Add CWE IDs to Findings

**Why:** Bearer maps every finding to CWE IDs. This is table-stakes for compliance reporting (SOC2, HIPAA, PCI-DSS). APEX has zero CWE mapping.

**Files:**
- Modify: `crates/apex-detect/src/finding.rs` — add `cwe_ids: Vec<u32>` to Finding
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs` — add `cwe` field to SecurityPattern
- Modify: `crates/apex-detect/src/detectors/path_normalize.rs` — add CWE-22
- Modify: `crates/apex-detect/src/detectors/hardcoded_secret.rs` — add CWE-798
- Modify: `crates/apex-detect/src/detectors/dep_audit.rs` — preserve CVE's CWE if available
- Modify: CLI output to show CWE IDs

- [ ] **Step 1: Add `cwe_ids` field to Finding struct**

```rust
pub struct Finding {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cwe_ids: Vec<u32>,
}
```

- [ ] **Step 2: Add `cwe` to SecurityPattern struct**

```rust
struct SecurityPattern {
    // ... existing fields ...
    cwe: &'static [u32],
}
```

CWE mappings:
| Pattern | CWE |
|---|---|
| eval/exec/pickle/yaml/__import__ | CWE-94 (Code Injection) |
| subprocess/os.system/os.popen/child_process | CWE-78 (OS Command Injection) |
| .execute() SQL | CWE-89 (SQL Injection) |
| innerHTML/document.write/res.write XSS | CWE-79 (XSS) |
| mark_safe/dangerouslySetInnerHTML | CWE-79 |
| MD5/SHA1 | CWE-328 (Weak Hash) |
| verify=False | CWE-295 (Improper Certificate Validation) |
| Command::new (Rust) | CWE-78 |
| gets/strcpy/sprintf/strcat (C) | CWE-120 (Buffer Overflow) |
| system() (C) | CWE-78 |
| Marshal.load/YAML.load (Ruby) | CWE-502 (Deserialization) |
| vm.runIn (JS) | CWE-94 |
| require() dynamic | CWE-94 |
| session config | CWE-614 (Secure Cookie), CWE-1004 (HttpOnly) |
| hardcoded secret | CWE-798 (Hard-coded Credentials) |
| path traversal | CWE-22 (Path Traversal) |

- [ ] **Step 3: Propagate CWE to all detectors**

Update every detector's Finding construction to include `cwe_ids`.

- [ ] **Step 4: Update CLI output format**

In audit output, show `[CWE-78]` before description when `cwe_ids` is non-empty.

- [ ] **Step 5: Write tests**

```rust
#[test]
fn all_security_patterns_have_cwe() {
    for lang in [Language::Python, Language::JavaScript, Language::Rust, Language::Ruby, Language::C] {
        for p in patterns_for_language(lang) {
            assert!(!p.cwe.is_empty(), "pattern '{}' missing CWE", p.sink);
        }
    }
}
```

- [ ] **Step 6: Run tests, verify pass**

- [ ] **Step 7: Commit**

---

## Task 5: Expression-Level Path Traversal Detection

**Why:** Bearer found 4 path traversal findings in the test app at expression level (`open(path)`, `os.path.join("/static", path)`) that APEX missed because the path-normalize detector only scans function signatures.

**File:** `crates/apex-detect/src/detectors/path_normalize.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[tokio::test]
async fn detects_inline_open_with_path_variable() {
    let src = "path = request.args.get('file')\ndata = open(path).read()\n";
    let ctx = make_ctx_with_source("app.py", src, Language::Python);
    let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty(), "should detect open(path) without normalization");
}

#[tokio::test]
async fn detects_os_path_join_without_normalization() {
    let src = "full = os.path.join('/static', user_path)\nopen(full)\n";
    let ctx = make_ctx_with_source("app.py", src, Language::Python);
    let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty());
}

#[tokio::test]
async fn detects_fs_readfile_with_path_variable() {
    let src = "const data = fs.readFileSync(req.params.path);\n";
    let ctx = make_ctx_with_source("app.js", src, Language::JavaScript);
    let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty());
}

#[tokio::test]
async fn no_finding_when_normpath_before_open() {
    let src = "safe = os.path.normpath(path)\ndata = open(safe).read()\n";
    let ctx = make_ctx_with_source("app.py", src, Language::Python);
    let findings = PathNormalizationDetector.analyze(&ctx).await.unwrap();
    assert!(findings.is_empty());
}
```

- [ ] **Step 2: Add file-operation sink scanning**

Add a second pass to `analyze()` that scans for file operation sinks with path arguments:

**Python sinks:** `open(`, `os.remove(`, `os.unlink(`, `shutil.copy(`, `pathlib.Path(`
**JS sinks:** `fs.readFile(`, `fs.readFileSync(`, `fs.writeFile(`, `fs.unlink(`
**Rust sinks:** `fs::read(`, `fs::write(`, `fs::remove_file(`, `File::open(`

For each sink found, check if the argument contains a user-input indicator AND no normalization call exists within a 5-line window above the sink.

- [ ] **Step 3: Run tests, verify pass**

- [ ] **Step 4: Commit**

---

## Task 6: Java Security Patterns (New Language)

**Why:** APEX has zero security patterns for Java (`patterns_for_language(Java) => &[]`). Java is one of the 7 supported languages but has no security detection at all. Both Semgrep and CodeQL have extensive Java rules.

**File:** `crates/apex-detect/src/detectors/security_pattern.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[tokio::test]
async fn detects_java_runtime_exec() {
    let src = "Runtime.getRuntime().exec(cmd);";
    let ctx = make_ctx("App.java", src, Language::Java);
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty());
}

#[tokio::test]
async fn detects_java_sql_injection() {
    let src = "stmt.executeQuery(\"SELECT * FROM users WHERE id=\" + userId);";
    let ctx = make_ctx("Dao.java", src, Language::Java);
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty());
}

#[tokio::test]
async fn detects_java_deserialization() {
    let src = "ObjectInputStream ois = new ObjectInputStream(socket.getInputStream());\nObject obj = ois.readObject();";
    let ctx = make_ctx("Server.java", src, Language::Java);
    let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
    assert!(!findings.is_empty());
}
```

- [ ] **Step 2: Add JAVA_SECURITY_PATTERNS**

```rust
const JAVA_SECURITY_PATTERNS: &[SecurityPattern] = &[
    // Command injection
    SecurityPattern {
        sink: "Runtime.getRuntime().exec(",
        description: "Runtime.exec() — OS command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        cwe: &[78],
        user_input_indicators: &["request", "getParameter", "input", "args"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "ProcessBuilder(",
        description: "ProcessBuilder — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        cwe: &[78],
        user_input_indicators: &["request", "getParameter", "input", "args"],
        sanitization_indicators: &[],
    },
    // SQL injection
    SecurityPattern {
        sink: "executeQuery(",
        description: "SQL query execution — potential injection if string concatenated",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        cwe: &[89],
        user_input_indicators: &["+", "format", "request", "getParameter", "concat"],
        sanitization_indicators: &["PreparedStatement", "parameterized", "?"],
    },
    SecurityPattern {
        sink: "executeUpdate(",
        description: "SQL update execution — potential injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        cwe: &[89],
        user_input_indicators: &["+", "format", "request", "getParameter", "concat"],
        sanitization_indicators: &["PreparedStatement", "parameterized", "?"],
    },
    // Deserialization
    SecurityPattern {
        sink: "readObject(",
        description: "Java deserialization — arbitrary code execution on untrusted data",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        cwe: &[502],
        user_input_indicators: &["socket", "request", "upload", "input", "InputStream"],
        sanitization_indicators: &["ObjectInputFilter", "ValidatingObjectInputStream"],
    },
    // XSS
    SecurityPattern {
        sink: "getWriter().print(",
        description: "Direct response output — XSS if includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        cwe: &[79],
        user_input_indicators: &["request", "getParameter", "getHeader", "getCookie"],
        sanitization_indicators: &["encode", "escape", "sanitize", "ESAPI"],
    },
    // SSRF
    SecurityPattern {
        sink: "new URL(",
        description: "URL construction — SSRF if input is user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        cwe: &[918],
        user_input_indicators: &["request", "getParameter", "input", "param"],
        sanitization_indicators: &["allowlist", "whitelist", "ALLOWED"],
    },
    // Weak crypto
    SecurityPattern {
        sink: "MessageDigest.getInstance(\"MD5\"",
        description: "MD5 hash — weak cryptographic hash",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        cwe: &[328],
        user_input_indicators: &["password", "token", "secret"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "MessageDigest.getInstance(\"SHA-1\"",
        description: "SHA-1 hash — weak cryptographic hash",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        cwe: &[328],
        user_input_indicators: &["password", "token", "secret"],
        sanitization_indicators: &[],
    },
];
```

- [ ] **Step 3: Update patterns_for_language() to include Java**

- [ ] **Step 4: Run tests, verify pass**

- [ ] **Step 5: Commit**

---

## Verification

After all tasks:
```bash
cargo test --workspace                         # all tests pass
cargo clippy --workspace -- -D warnings        # no warnings
# Re-run comparison
cargo run --bin apex -- audit --target /tmp/claude/apex-comparison --lang python
# Compare against semgrep
semgrep scan --config auto /tmp/claude/apex-comparison
```

Expected outcome: APEX should now detect `subprocess.run(shell=True)`, session misconfig, and produce CWE-mapped findings — closing the gap from 6 to 42 findings on Flask (current) to comparable detection coverage with Semgrep/Bearer on their shared pattern categories.

## Key Files Reference

| File | Role |
|---|---|
| `crates/apex-detect/src/detectors/security_pattern.rs` | Security pattern detector — Tasks 1, 2, 6 |
| `crates/apex-detect/src/detectors/session_security.rs` | NEW: Session/cookie security — Task 3 |
| `crates/apex-detect/src/detectors/path_normalize.rs` | Path traversal detector — Task 5 |
| `crates/apex-detect/src/detectors/mod.rs` | Detector registry — Task 3 |
| `crates/apex-detect/src/finding.rs` | Finding types — Tasks 3, 4 |
| `crates/apex-detect/src/pipeline.rs` | Detector composition — Task 3 |
| `crates/apex-detect/src/config.rs` | Default enabled detectors — Task 3 |
| `crates/apex-cli/src/lib.rs` | CLI output format — Task 4 |
