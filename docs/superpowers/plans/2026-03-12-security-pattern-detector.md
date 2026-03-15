<!-- status: DONE --># Security Pattern Detector Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a security-pattern detector that finds HIGH and CRITICAL vulnerability patterns (injection, secrets, unsafe deserialization) across Rust, Python, JS, Ruby, and C — filling the severity gap where apex currently maxes out at MEDIUM.

**Architecture:** A new `SecurityPatternDetector` in `apex-detect` that uses two-phase analysis: Phase 1 finds dangerous sinks via language-aware string matching (reusing shared utilities extracted from panic-pattern). Phase 2 checks surrounding context for user-input indicators and sanitization patterns, adjusting severity up/down. A separate `HardcodedSecretDetector` uses regex to find API keys, tokens, and passwords in source code.

**Tech Stack:** Rust, regex crate (already in workspace), apex-detect crate patterns.

---

## File Structure

| File | Responsibility |
|------|---------------|
| **Create:** `crates/apex-detect/src/detectors/util.rs` | Shared utilities: `is_test_file`, `in_test_block`, `strip_string_literals` — extracted from panic-pattern |
| **Create:** `crates/apex-detect/src/detectors/security_pattern.rs` | Sink-based vulnerability detector with context analysis (Injection, RCE, XSS, unsafe deserialization) |
| **Create:** `crates/apex-detect/src/detectors/hardcoded_secret.rs` | Regex-based secret/credential detector (API keys, passwords, tokens, private keys) |
| **Modify:** `crates/apex-detect/src/detectors/panic_pattern.rs` | Replace inline utilities with `use super::util::*` |
| **Modify:** `crates/apex-detect/src/detectors/mod.rs` | Register new detectors |
| **Modify:** `crates/apex-detect/src/config.rs` | Add `"security"` and `"secrets"` to default enabled detectors |
| **Modify:** `crates/apex-detect/src/pipeline.rs` | Wire new detectors in `from_config` |
| **Modify:** `crates/apex-detect/Cargo.toml` | Add `regex` dependency (if not already present) |
| **Modify:** `crates/apex-detect/src/finding.rs` | Add `Copy` to `FindingCategory` derive (needed for `const` pattern arrays) |

---

## Chunk 1: Shared Utilities Extraction

### Task 1: Extract shared utilities from panic_pattern into util.rs

**Files:**
- Create: `crates/apex-detect/src/detectors/util.rs`
- Modify: `crates/apex-detect/src/detectors/panic_pattern.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs`

- [ ] **Step 1: Create `util.rs` with the three shared functions**

```rust
// crates/apex-detect/src/detectors/util.rs

use apex_core::types::Language;

/// Returns true if the file path looks like a test file.
/// Covers: tests/, test/, __tests__/, spec/, benches/,
/// *_test.rs, *_tests.rs, *.test.js, *.spec.ts, conftest.py, testutil.rs, etc.
pub fn is_test_file(path: &std::path::Path) -> bool {
    // Copy exact implementation from panic_pattern.rs (lines 65-97)
    // including file_stem_is_test_helper
}

fn file_stem_is_test_helper(path: &std::path::Path) -> bool {
    // Copy exact implementation from panic_pattern.rs
}

/// Returns true if the target line is inside a `#[cfg(test)]` or `mod tests` block.
/// Tracks brace depth after seeing the test attribute.
pub fn in_test_block(source: &str, target_line: usize) -> bool {
    // Copy exact implementation from panic_pattern.rs
}

/// Strip content inside string literals so patterns inside quotes are ignored.
/// Keeps the quote characters but removes everything between them.
pub fn strip_string_literals(line: &str) -> String {
    // Copy exact implementation from panic_pattern.rs
}

/// Returns true if the line is a comment for the given language.
pub fn is_comment(trimmed: &str, lang: Language) -> bool {
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
        return true;
    }
    if (lang == Language::Python || lang == Language::Ruby) && trimmed.starts_with('#') {
        return true;
    }
    false
}
```

- [ ] **Step 1b: Add `Copy` to `FindingCategory` derive**

In `crates/apex-detect/src/finding.rs`, line 48, change:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
```
to:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
```

This is required because `SecurityPattern` uses `FindingCategory` in `const` arrays, and `const` aggregates require all fields to be `Copy`. `FindingCategory` is a simple fieldless enum so this is safe. Also update any `.clone()` calls on `FindingCategory` in `security_pattern.rs` to just use copy semantics (remove `.clone()`).

- [ ] **Step 2: Run existing panic-pattern tests to establish baseline**

Run: `cargo test -p apex-detect -- panic_pattern`
Expected: All 64 tests pass

- [ ] **Step 3: Update `mod.rs` to declare `util` module**

Add `pub mod util;` to `crates/apex-detect/src/detectors/mod.rs`.

- [ ] **Step 4: Update `panic_pattern.rs` to use shared utilities**

Replace the inline `is_test_file`, `file_stem_is_test_helper`, `in_test_block`, `strip_string_literals` functions with:

```rust
use super::util::{is_test_file, in_test_block, strip_string_literals, is_comment};
```

Remove the inline functions. Update the `analyze` method to use `is_comment(trimmed, ctx.language)` instead of the inline comment checks.

Move the `is_test_file_checks`, `in_test_block_*`, and `strip_string_literals_works` tests to `util.rs` as unit tests. Keep them also in panic_pattern.rs tests if they test integration behavior.

- [ ] **Step 5: Run tests to verify refactor didn't break anything**

Run: `cargo test -p apex-detect`
Expected: All 64 tests pass (same count, no regressions)

- [ ] **Step 6: Commit**

```bash
git add crates/apex-detect/src/detectors/util.rs crates/apex-detect/src/detectors/mod.rs crates/apex-detect/src/detectors/panic_pattern.rs
git commit -m "refactor: extract shared detector utilities into util.rs"
```

---

## Chunk 2: Security Pattern Detector — Core + Rust/Python

### Task 2: Define SecurityPattern struct and Rust patterns

**Files:**
- Create: `crates/apex-detect/src/detectors/security_pattern.rs`

- [ ] **Step 1: Write failing test for Rust command injection detection**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DetectConfig;
    use crate::context::AnalysisContext;
    use apex_core::types::Language;
    use apex_coverage::CoverageOracle;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_ctx(files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: lang,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache: files,
            fuzz_corpus: None,
            config: DetectConfig::default(),
        }
    }

    #[tokio::test]
    async fn rust_command_injection_with_format() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/run.rs"),
            "fn run(user_input: &str) {\n    Command::new(\"sh\").arg(format!(\"-c {}\", user_input)).spawn();\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].severity == Severity::Critical || findings[0].severity == Severity::High);
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-detect -- security_pattern`
Expected: FAIL (module doesn't exist)

- [ ] **Step 3: Write minimal SecurityPatternDetector**

```rust
// crates/apex-detect/src/detectors/security_pattern.rs

use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{is_test_file, in_test_block, strip_string_literals, is_comment};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct SecurityPatternDetector;

/// A dangerous sink pattern with context-adjustment rules.
struct SecurityPattern {
    /// The string to match in source code.
    sink: &'static str,
    /// Human-readable description.
    description: &'static str,
    /// Finding category (Injection, MemorySafety, etc.).
    category: FindingCategory,
    /// Base severity before context adjustment.
    base_severity: Severity,
    /// If any of these appear on the same line or ±3 lines, severity stays at base.
    /// If none appear, severity is downgraded one level.
    user_input_indicators: &'static [&'static str],
    /// If any of these appear on the same line or ±3 lines, severity is
    /// downgraded one level (sanitization detected).
    sanitization_indicators: &'static [&'static str],
}

// ── Rust patterns ──────────────────────────────────────────────────

const RUST_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "Command::new(",
        description: "Command::new with potential injection — verify args are not user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["format!", "user", "input", "request", "query", "arg(", "&str"],
        sanitization_indicators: &["escape", "sanitize", "quote", "shell_escape"],
    },
    SecurityPattern {
        sink: "std::process::Command",
        description: "process::Command — verify arguments are not user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        user_input_indicators: &["format!", "user", "input", "request"],
        sanitization_indicators: &["escape", "sanitize"],
    },
];

// ── Python patterns ────────────────────────────────────────────────

const PYTHON_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "eval(",
        description: "eval() — arbitrary code execution if input is user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "input", "param", "query", "form", "argv", "stdin"],
        sanitization_indicators: &["ast.literal_eval", "safe_eval"],
    },
    SecurityPattern {
        sink: "exec(",
        description: "exec() — arbitrary code execution",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "input", "param", "query", "form"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "pickle.load",
        description: "pickle deserialization — arbitrary code execution on untrusted data",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "upload", "file", "open(", "recv", "socket"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "yaml.load(",
        description: "yaml.load() without SafeLoader — arbitrary code execution",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "file", "open(", "read"],
        sanitization_indicators: &["SafeLoader", "safe_load", "CSafeLoader"],
    },
    SecurityPattern {
        sink: "subprocess.call(",
        description: "subprocess.call — command injection if shell=True with user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["shell=True", "format(", "f\"", "request", "input", "%s"],
        sanitization_indicators: &["shlex.quote", "shlex.split", "shell=False"],
    },
    SecurityPattern {
        sink: "os.system(",
        description: "os.system() — command injection, prefer subprocess with list args",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["format(", "f\"", "request", "input", "+", "%"],
        sanitization_indicators: &["shlex.quote"],
    },
    SecurityPattern {
        sink: ".execute(f",
        description: "SQL f-string in execute() — SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &[],  // f-string in SQL is always dangerous
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: ".execute(",
        description: "SQL execute() with potential string concatenation — SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["%s", "format(", "+", "%", "f\""],
        sanitization_indicators: &["?", "%s,", "parameterize", "placeholder"],
    },
    SecurityPattern {
        sink: "mark_safe(",
        description: "mark_safe() — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "user", "input", "form", "query"],
        sanitization_indicators: &["escape", "bleach", "sanitize", "strip_tags"],
    },
    SecurityPattern {
        sink: "hashlib.md5(",
        description: "MD5 hash — cryptographically broken, do not use for security",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret", "key", "auth"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "hashlib.sha1(",
        description: "SHA1 hash — cryptographically weak, do not use for security",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret", "key", "auth"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "verify=False",
        description: "SSL verification disabled — MITM vulnerability",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::High,
        user_input_indicators: &[],
        sanitization_indicators: &[],
    },
];

// ── Context analysis ───────────────────────────────────────────────

/// Look at ±CONTEXT_WINDOW lines around the match for indicators.
const CONTEXT_WINDOW: usize = 3;

fn has_indicator(lines: &[&str], line_num: usize, indicators: &[&str]) -> bool {
    if indicators.is_empty() {
        return false;
    }
    let start = line_num.saturating_sub(CONTEXT_WINDOW);
    let end = (line_num + CONTEXT_WINDOW + 1).min(lines.len());
    for i in start..end {
        let line_lower = lines[i].to_lowercase();
        for indicator in indicators {
            if line_lower.contains(&indicator.to_lowercase()) {
                return true;
            }
        }
    }
    false
}

fn adjust_severity(
    base: Severity,
    has_user_input: bool,
    has_sanitization: bool,
    indicators_defined: bool,
) -> Severity {
    // If no user_input_indicators were defined, the pattern is inherently
    // dangerous (e.g. gets(), verify=False) — stay at base severity.
    let sev = if !indicators_defined || has_user_input {
        base // always dangerous or confirmed user input
    } else {
        downgrade(base) // indicators defined but none found → one level down
    };
    if has_sanitization {
        downgrade(sev) // sanitization present → one more level down
    } else {
        sev
    }
}

fn downgrade(s: Severity) -> Severity {
    match s {
        Severity::Critical => Severity::High,
        Severity::High => Severity::Medium,
        Severity::Medium => Severity::Low,
        Severity::Low => Severity::Low,
        Severity::Info => Severity::Info,
    }
}

// ── Detector implementation ────────────────────────────────────────

fn patterns_for_language(lang: Language) -> &'static [SecurityPattern] {
    match lang {
        Language::Python => PYTHON_SECURITY_PATTERNS,
        Language::Rust => RUST_SECURITY_PATTERNS,
        _ => &[], // JS, Ruby, C added in later tasks
    }
}

#[async_trait]
impl Detector for SecurityPatternDetector {
    fn name(&self) -> &str {
        "security-pattern"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();
        let patterns = patterns_for_language(ctx.language);

        if patterns.is_empty() {
            return Ok(findings);
        }

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) {
                continue;
            }

            let all_lines: Vec<&str> = source.lines().collect();
            for (line_num, line) in all_lines.iter().enumerate() {
                let trimmed = line.trim();

                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                if trimmed == "#[test]" || trimmed == "#[tokio::test]" {
                    continue;
                }

                if in_test_block(source, line_num) {
                    continue;
                }

                let stripped = strip_string_literals(trimmed);

                for pattern in patterns {
                    if stripped.contains(pattern.sink) {
                        let line_1based = (line_num + 1) as u32;

                        let has_user_input = has_indicator(
                            &all_lines, line_num, pattern.user_input_indicators,
                        );
                        let has_sanitization = has_indicator(
                            &all_lines, line_num, pattern.sanitization_indicators,
                        );
                        let severity = adjust_severity(
                            pattern.base_severity,
                            has_user_input,
                            has_sanitization,
                            !pattern.user_input_indicators.is_empty(),
                        );

                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity,
                            category: pattern.category,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!("{} at line {line_1based}", pattern.description),
                            description: format!(
                                "Pattern `{}` found in {}:{}",
                                pattern.sink,
                                path.display(),
                                line_1based
                            ),
                            evidence: vec![],
                            covered: false,
                            suggestion: format!("{}", pattern.description),
                            explanation: None,
                            fix: None,
                        });
                        break; // one finding per line
                    }
                }
            }
        }

        Ok(findings)
    }
}
```

- [ ] **Step 4: Register in `mod.rs`**

Add to `crates/apex-detect/src/detectors/mod.rs`:
```rust
pub mod security_pattern;
pub use security_pattern::SecurityPatternDetector;
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p apex-detect -- security_pattern`
Expected: PASS

- [ ] **Step 6: Add more Rust + Python tests**

```rust
    #[tokio::test]
    async fn python_eval_with_request_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app/views.py"),
            "def handler(request):\n    result = eval(request.GET['expr'])\n    return result\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }

    #[tokio::test]
    async fn python_eval_without_user_input_downgraded() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("lib/math.py"),
            "def compute(expr):\n    return eval(expr)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High); // downgraded from Critical
    }

    #[tokio::test]
    async fn python_yaml_safe_loader_downgraded() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("lib/config.py"),
            "import yaml\ndef load(path):\n    with open(path) as f:\n        return yaml.load(f, Loader=SafeLoader)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        // SafeLoader detected → downgraded from High
        assert_eq!(findings[0].severity, Severity::Low);
    }

    #[tokio::test]
    async fn python_sql_fstring_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app/db.py"),
            "def get_user(name):\n    cursor.execute(f\"SELECT * FROM users WHERE name='{name}'\")\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        // Two matches: .execute(f and .execute( — deduplicated by the `break` per line
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical); // .execute(f matched first, empty indicators = always dangerous
    }

    #[tokio::test]
    async fn python_pickle_from_socket_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("server/handler.py"),
            "import pickle\ndef handle(socket):\n    data = socket.recv(4096)\n    obj = pickle.loads(data)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn python_verify_false_is_high() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("lib/client.py"),
            "import requests\ndef fetch(url):\n    return requests.get(url, verify=False)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/test_handler.py"),
            "def test_eval():\n    eval('1+1')\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn no_findings_for_unsupported_language() {
        let mut files = HashMap::new();
        files.insert(PathBuf::from("src/App.java"), "eval(x);".into());
        let ctx = make_ctx(files, Language::Java);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }
```

- [ ] **Step 7: Run all tests**

Run: `cargo test -p apex-detect`
Expected: All pass (64 existing + ~9 new = ~73)

- [ ] **Step 8: Commit**

```bash
git add crates/apex-detect/src/detectors/security_pattern.rs crates/apex-detect/src/detectors/mod.rs
git commit -m "feat: security-pattern detector with Rust + Python patterns"
```

---

### Task 3: Add JavaScript, Ruby, and C security patterns

**Files:**
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs`

- [ ] **Step 1: Write failing test for JS eval detection**

```rust
    #[tokio::test]
    async fn js_eval_with_user_input_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/handler.js"),
            "function handle(req) {\n    const result = eval(req.body.code);\n    return result;\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }
```

- [ ] **Step 2: Add JS security patterns**

```rust
const JS_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "eval(",
        description: "eval() — arbitrary code execution if input is user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["req.", "request", "params", "query", "body", "input", "argv"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "Function(",
        description: "new Function() — dynamic code generation, equivalent to eval",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["req.", "request", "params", "query", "body", "input"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "child_process.exec(",
        description: "child_process.exec — command injection via shell",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["req.", "request", "params", "query", "body", "input", "${", "`"],
        sanitization_indicators: &["escape", "sanitize", "execFile"],
    },
    SecurityPattern {
        sink: "innerHTML",
        description: "innerHTML assignment — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["req.", "request", "user", "input", "query", "param", "response"],
        sanitization_indicators: &["sanitize", "escape", "DOMPurify", "encode", "textContent"],
    },
    SecurityPattern {
        sink: "dangerouslySetInnerHTML",
        description: "dangerouslySetInnerHTML — XSS, React's escape hatch for raw HTML",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["user", "input", "props", "state", "data", "response"],
        sanitization_indicators: &["sanitize", "DOMPurify", "bleach"],
    },
    SecurityPattern {
        sink: "document.write(",
        description: "document.write() — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["user", "input", "location", "search", "hash", "referrer"],
        sanitization_indicators: &["escape", "encode", "sanitize"],
    },
    SecurityPattern {
        sink: "vm.runIn",
        description: "vm.runInContext/vm.runInNewContext — sandbox escape risk",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["req.", "request", "user", "input"],
        sanitization_indicators: &[],
    },
];
```

- [ ] **Step 3: Add Ruby security patterns**

```rust
const RUBY_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "eval(",
        description: "eval() — arbitrary code execution",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["params", "request", "input", "gets", "ARGV"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "instance_eval",
        description: "instance_eval — arbitrary code execution in object context",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["params", "request", "input", "gets"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "class_eval",
        description: "class_eval — arbitrary code execution in class context",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["params", "request", "input"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "send(",
        description: "send() — arbitrary method invocation if argument is user-controlled",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["params", "request", "input", "gets"],
        sanitization_indicators: &["whitelist", "allow_list", "permitted", "include?"],
    },
    SecurityPattern {
        sink: "constantize",
        description: "constantize — arbitrary class instantiation from user string",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["params", "request", "input"],
        sanitization_indicators: &["whitelist", "allow_list", "permitted", "include?"],
    },
    SecurityPattern {
        sink: ".html_safe",
        description: ".html_safe — XSS if content includes user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["params", "user", "input", "request", "@"],
        sanitization_indicators: &["sanitize", "strip_tags", "escape", "h("],
    },
    SecurityPattern {
        sink: "Marshal.load",
        description: "Marshal.load — arbitrary code execution on untrusted data",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "file", "socket", "params", "upload"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "YAML.load(",
        description: "YAML.load — arbitrary code execution without safe_load",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "file", "params"],
        sanitization_indicators: &["safe_load", "safe_load_file", "permitted_classes"],
    },
    SecurityPattern {
        sink: ".where(",
        description: "ActiveRecord .where() with potential string interpolation — SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["#{", "params", "request", "input", "+"],
        sanitization_indicators: &["sanitize_sql", "?", "placeholder", "where("],
    },
];
```

- [ ] **Step 4: Add C security patterns**

```rust
const C_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "gets(",
        description: "gets() — unbounded read, guaranteed buffer overflow",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::Critical,
        user_input_indicators: &[],  // always dangerous
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "strcpy(",
        description: "strcpy() — no bounds checking, use strncpy or strlcpy",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "read(", "getenv"],
        sanitization_indicators: &["strlen", "sizeof", "strlcpy", "strncpy"],
    },
    SecurityPattern {
        sink: "sprintf(",
        description: "sprintf() — no bounds checking, use snprintf",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv", "%s"],
        sanitization_indicators: &["snprintf"],
    },
    SecurityPattern {
        sink: "strcat(",
        description: "strcat() — no bounds checking, use strncat or strlcat",
        category: FindingCategory::MemorySafety,
        base_severity: Severity::High,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv"],
        sanitization_indicators: &["strncat", "strlcat", "strlen"],
    },
    SecurityPattern {
        sink: "system(",
        description: "system() — command injection if argument contains user input",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["argv", "stdin", "fgets", "recv", "getenv", "sprintf"],
        sanitization_indicators: &["escape", "sanitize"],
    },
];
```

- [ ] **Step 5: Update `patterns_for_language` to include all languages**

```rust
fn patterns_for_language(lang: Language) -> &'static [SecurityPattern] {
    match lang {
        Language::Python => PYTHON_SECURITY_PATTERNS,
        Language::Rust => RUST_SECURITY_PATTERNS,
        Language::JavaScript => JS_SECURITY_PATTERNS,
        Language::Ruby => RUBY_SECURITY_PATTERNS,
        Language::C => C_SECURITY_PATTERNS,
        _ => &[],
    }
}
```

- [ ] **Step 6: Add tests for JS, Ruby, C**

```rust
    #[tokio::test]
    async fn js_innerhtml_with_user_data_is_high() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/render.js"),
            "function render(userData) {\n    el.innerHTML = userData;\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn ruby_eval_with_params_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app/controllers/calc_controller.rb"),
            "def calculate\n  result = eval(params[:expr])\n  render json: result\nend\n".into(),
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn c_gets_is_always_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.c"),
            "int main() {\n    char buf[64];\n    gets(buf);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::C);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn c_strcpy_without_user_input_is_medium() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/util.c"),
            "void copy(char *dst, const char *src) {\n    strcpy(dst, src);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::C);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Medium); // downgraded, no user input
    }

    #[tokio::test]
    async fn ruby_marshal_from_request_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("app/services/deserialize.rb"),
            "def load_data(request)\n  Marshal.load(request.body)\nend\n".into(),
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }
```

- [ ] **Step 7: Run all tests**

Run: `cargo test -p apex-detect`
Expected: All pass

- [ ] **Step 8: Commit**

```bash
git add crates/apex-detect/src/detectors/security_pattern.rs
git commit -m "feat: add JS, Ruby, C security patterns with context analysis"
```

---

## Chunk 3: Hardcoded Secrets Detector

### Task 4: Implement HardcodedSecretDetector

**Files:**
- Create: `crates/apex-detect/src/detectors/hardcoded_secret.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs`
- Modify: `crates/apex-detect/Cargo.toml` (add regex if needed)

- [ ] **Step 1: Check if regex is already available**

Run: `grep regex crates/apex-detect/Cargo.toml`
If not present, add `regex = "1"` to `[dependencies]`.

- [ ] **Step 2: Write failing test for AWS key detection**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    // ... (same test helpers as security_pattern tests)

    #[tokio::test]
    async fn detects_aws_access_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/config.py"),
            "AWS_ACCESS_KEY_ID = \"AKIAIOSFODNN7EXAMPLE\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
        assert_eq!(findings[0].category, FindingCategory::SecuritySmell);
    }
}
```

- [ ] **Step 3: Implement HardcodedSecretDetector**

```rust
// crates/apex-detect/src/detectors/hardcoded_secret.rs

use apex_core::error::Result;
use async_trait::async_trait;
use regex::Regex;
use std::sync::LazyLock;
use uuid::Uuid;

use super::util::{is_test_file, is_comment};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct HardcodedSecretDetector;

struct SecretPattern {
    name: &'static str,
    regex: &'static str,
    severity: Severity,
    description: &'static str,
}

const SECRET_PATTERNS: &[SecretPattern] = &[
    SecretPattern {
        name: "AWS Access Key",
        regex: r"AKIA[0-9A-Z]{16}",
        severity: Severity::Critical,
        description: "AWS access key ID — rotate immediately if committed",
    },
    SecretPattern {
        name: "Private Key",
        regex: r"-----BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY-----",
        severity: Severity::Critical,
        description: "Private key in source code — must not be committed",
    },
    SecretPattern {
        name: "GitHub Token",
        regex: r"gh[pousr]_[A-Za-z0-9_]{36,}",
        severity: Severity::Critical,
        description: "GitHub personal access token — rotate immediately",
    },
    SecretPattern {
        name: "Generic API Key Assignment",
        regex: r#"(?i)(api[_-]?key|apikey)\s*[:=]\s*["'][A-Za-z0-9+/=]{20,}["']"#,
        severity: Severity::High,
        description: "Hardcoded API key — use environment variables instead",
    },
    SecretPattern {
        name: "Password Assignment",
        regex: r#"(?i)(password|passwd|pwd)\s*[:=]\s*["'][^"']{8,}["']"#,
        severity: Severity::High,
        description: "Hardcoded password — use environment variables or secrets manager",
    },
    SecretPattern {
        name: "Generic Secret/Token",
        regex: r#"(?i)(secret|token|auth_token|access_token)\s*[:=]\s*["'][A-Za-z0-9+/=_-]{16,}["']"#,
        severity: Severity::High,
        description: "Hardcoded secret/token — use environment variables instead",
    },
    SecretPattern {
        name: "Stripe Key",
        regex: r"sk_(live|test)_[A-Za-z0-9]{20,}",
        severity: Severity::Critical,
        description: "Stripe secret key — rotate immediately if committed",
    },
    SecretPattern {
        name: "Slack Token",
        regex: r"xox[baprs]-[A-Za-z0-9-]{10,}",
        severity: Severity::High,
        description: "Slack token — rotate and use environment variables",
    },
];

/// Values that indicate a placeholder, not a real secret.
const FALSE_POSITIVE_VALUES: &[&str] = &[
    "changeme", "CHANGEME", "your-", "YOUR_", "xxx", "XXX",
    "placeholder", "PLACEHOLDER", "example", "EXAMPLE",
    "replace_me", "REPLACE_ME", "TODO", "FIXME", "test",
    "dummy", "fake", "sample", "demo",
];

/// File patterns that typically contain example/template secrets.
fn is_example_file(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy();
    s.contains(".example") || s.contains(".sample") || s.contains(".template")
        || s.ends_with(".md") || s.ends_with(".txt") || s.ends_with(".rst")
}

static COMPILED_PATTERNS: LazyLock<Vec<(&'static SecretPattern, Regex)>> = LazyLock::new(|| {
    SECRET_PATTERNS
        .iter()
        .map(|p| (p, Regex::new(p.regex).expect("invalid secret regex")))
        .collect()
});

#[async_trait]
impl Detector for HardcodedSecretDetector {
    fn name(&self) -> &str {
        "hardcoded-secret"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            if is_test_file(path) || is_example_file(path) {
                continue;
            }

            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();
                // Skip comments (reuse shared utility)
                if is_comment(trimmed, ctx.language) {
                    continue;
                }

                // Skip lines with placeholder values
                let line_lower = trimmed.to_lowercase();
                if FALSE_POSITIVE_VALUES.iter().any(|fp| line_lower.contains(&fp.to_lowercase())) {
                    continue;
                }

                // Skip environment variable references
                if line.contains("env(") || line.contains("ENV[")
                    || line.contains("os.environ") || line.contains("process.env")
                    || line.contains("std::env") || line.contains("getenv(")
                {
                    continue;
                }

                for (pattern, re) in COMPILED_PATTERNS.iter() {
                    if re.is_match(trimmed) {
                        let line_1based = (line_num + 1) as u32;
                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity: pattern.severity,
                            category: FindingCategory::SecuritySmell,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!("{} at line {line_1based}", pattern.name),
                            description: pattern.description.into(),
                            evidence: vec![],
                            covered: false,
                            suggestion: pattern.description.into(),
                            explanation: None,
                            fix: None,
                        });
                        break; // one finding per line
                    }
                }
            }
        }

        Ok(findings)
    }
}
```

- [ ] **Step 4: Register in `mod.rs`**

Add:
```rust
pub mod hardcoded_secret;
pub use hardcoded_secret::HardcodedSecretDetector;
```

- [ ] **Step 5: Add comprehensive tests**

```rust
    #[tokio::test]
    async fn detects_private_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("certs/server.py"),
            "KEY = \"\"\"-----BEGIN RSA PRIVATE KEY-----\nMIIBogIBAAJ...\n-----END RSA PRIVATE KEY-----\"\"\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn detects_password_assignment() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("config/settings.py"),
            "DATABASE_PASSWORD = \"s3cretP@ssw0rd!\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[tokio::test]
    async fn skips_placeholder_values() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("config/settings.py"),
            "PASSWORD = \"changeme\"\nAPI_KEY = \"your-api-key-here\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_env_var_references() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("config/settings.py"),
            "PASSWORD = os.environ.get('DB_PASSWORD')\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_example_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("config/settings.example.py"),
            "API_KEY = \"AKIAIOSFODNN7EXAMPLE\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn skips_test_files() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("tests/test_auth.py"),
            "API_KEY = \"AKIAIOSFODNN7EXAMPLE\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn detects_stripe_key() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/billing.rb"),
            "Stripe.api_key = \"sk_live_EXAMPLE_REDACTED_KEY\"\n".into(),
        );
        let ctx = make_ctx(files, Language::Ruby);
        let findings = HardcodedSecretDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }
```

- [ ] **Step 6: Run all tests**

Run: `cargo test -p apex-detect`
Expected: All pass

- [ ] **Step 7: Commit**

```bash
git add crates/apex-detect/src/detectors/hardcoded_secret.rs crates/apex-detect/src/detectors/mod.rs crates/apex-detect/Cargo.toml
git commit -m "feat: hardcoded-secret detector with regex patterns and false-positive filtering"
```

---

## Chunk 4: Pipeline Integration + Validation

### Task 5: Wire detectors into pipeline and config

**Files:**
- Modify: `crates/apex-detect/src/config.rs`
- Modify: `crates/apex-detect/src/pipeline.rs`

- [ ] **Step 1: Add `"security"` and `"secrets"` to default enabled detectors**

In `crates/apex-detect/src/config.rs`, update `default_enabled()`:

```rust
fn default_enabled() -> Vec<String> {
    vec!["unsafe".into(), "deps".into(), "panic".into(), "static".into(), "security".into(), "secrets".into()]
}
```

- [ ] **Step 2: Wire into `DetectorPipeline::from_config`**

In `crates/apex-detect/src/pipeline.rs`, add after the static-analysis block:

```rust
        if cfg.enabled.contains(&"security".to_string()) {
            detectors.push(Box::new(SecurityPatternDetector));
        }
        if cfg.enabled.contains(&"secrets".to_string()) {
            detectors.push(Box::new(HardcodedSecretDetector));
        }
```

- [ ] **Step 3: Update config test**

Update the `default_config_has_tier1_detectors` test to check for 6 detectors:

```rust
    #[test]
    fn default_config_has_tier1_detectors() {
        let cfg = DetectConfig::default();
        assert!(cfg.enabled.contains(&"panic".to_string()));
        assert!(cfg.enabled.contains(&"deps".to_string()));
        assert!(cfg.enabled.contains(&"unsafe".to_string()));
        assert!(cfg.enabled.contains(&"static".to_string()));
        assert!(cfg.enabled.contains(&"security".to_string()));
        assert!(cfg.enabled.contains(&"secrets".to_string()));
    }
```

And `empty_toml_gives_defaults`:
```rust
    assert_eq!(cfg.enabled.len(), 6);
```

Also update the pipeline test `from_config_enables_all_by_default` in `crates/apex-detect/src/pipeline.rs` (around line 268) to expect 6 detectors:
```rust
    assert_eq!(pipeline.detectors.len(), 6);
```

- [ ] **Step 4: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All pass, no regressions

- [ ] **Step 5: Commit**

```bash
git add crates/apex-detect/src/config.rs crates/apex-detect/src/pipeline.rs
git commit -m "feat: wire security-pattern and hardcoded-secret detectors into pipeline"
```

---

### Task 6: Validate on real-world repos

**Files:** None (validation only)

- [ ] **Step 1: Run apex self-audit and verify HIGH/CRITICAL findings appear**

```bash
cargo run --bin apex -- run --target /Users/ad/prj/bcov --lang rust
```

Expected: Should still work, may show new findings if any security patterns match.

- [ ] **Step 2: Run on Python repos (flask, requests, fastapi)**

```bash
for repo in flask requests fastapi; do
  cargo run --bin apex -- run --target /tmp/claude/apex-targets/$repo --lang python --output /tmp/claude/apex-targets/${repo}-security.txt 2>/dev/null
  head -5 /tmp/claude/apex-targets/${repo}-security.txt
done
```

Expected: Should now see HIGH/CRITICAL findings from security-pattern and hardcoded-secret detectors.

- [ ] **Step 3: Run on Ruby repos (rails, discourse, sinatra)**

```bash
for repo in rails discourse sinatra; do
  cargo run --bin apex -- run --target /tmp/claude/apex-targets/$repo --lang ruby --output /tmp/claude/apex-targets/${repo}-security.txt 2>/dev/null
  head -5 /tmp/claude/apex-targets/${repo}-security.txt
done
```

Expected: Should see eval, instance_eval, Marshal.load, .html_safe findings.

- [ ] **Step 4: Run on JS repos (express, webpack, svelte)**

```bash
for repo in express webpack svelte; do
  cargo run --bin apex -- run --target /tmp/claude/apex-targets/$repo --lang js --output /tmp/claude/apex-targets/${repo}-security.txt 2>/dev/null
  head -5 /tmp/claude/apex-targets/${repo}-security.txt
done
```

Expected: Should see eval, innerHTML, child_process.exec findings.

- [ ] **Step 5: Spot-check for false positives**

Review the first 20 findings from each report. If false-positive rate is >30%, adjust patterns or add exclusions. Common issues:
- `eval()` in build tools (legitimate) — consider adding build-tool file exclusions
- `innerHTML` in framework internals (sanitized) — check if sanitization detection works
- Password regex matching config keys — check placeholder filtering

- [ ] **Step 6: Commit any adjustments**

```bash
git add -A
git commit -m "fix: tune security patterns based on real-world validation"
```

---

## Dependency Graph

```
Task 1 (util extraction) ──┬── Task 2 (security patterns: Rust+Python)
                            │         │
                            │   Task 3 (JS/Ruby/C patterns)
                            │
                            ├── Task 4 (hardcoded secrets)
                            │
                            └── Task 5 (pipeline wiring) ── Task 6 (validation)
```

Tasks 2-4 depend on Task 1. Tasks 2 and 4 are independent of each other.
Task 3 depends on Task 2. Task 5 depends on Tasks 2-4.
Task 6 depends on Task 5.

## Verification

After each task:
```bash
cargo test -p apex-detect
```

After Task 5 (full integration):
```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

After Task 6:
```bash
# Should see HIGH/CRITICAL on at least 3 of the 30 repos
cargo run --bin apex -- run --target /tmp/claude/apex-targets/flask --lang python 2>/dev/null | head -3
# Expected: CRITICAL > 0 or HIGH > 0
```
