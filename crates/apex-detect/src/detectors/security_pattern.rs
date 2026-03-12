use apex_core::error::Result;
use apex_core::types::Language;
use async_trait::async_trait;
use uuid::Uuid;

use super::util::{in_test_block, is_comment, is_test_file, strip_string_literals};
use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct SecurityPatternDetector;

struct SecurityPattern {
    sink: &'static str,
    description: &'static str,
    category: FindingCategory,
    base_severity: Severity,
    user_input_indicators: &'static [&'static str],
    sanitization_indicators: &'static [&'static str],
}

const RUST_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "Command::new(",
        description: "Command injection — user input flows into shell command",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &[
            "format!", "user", "input", "request", "query", "arg(", "&str",
        ],
        sanitization_indicators: &["escape", "sanitize", "quote", "shell_escape"],
    },
    SecurityPattern {
        sink: "std::process::Command",
        description: "Process command construction — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Medium,
        user_input_indicators: &["format!", "user", "input", "request"],
        sanitization_indicators: &["escape", "sanitize"],
    },
];

const PYTHON_SECURITY_PATTERNS: &[SecurityPattern] = &[
    SecurityPattern {
        sink: "eval(",
        description: "eval() with potential user input — code injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &[
            "request", "input", "param", "query", "form", "argv", "stdin",
        ],
        sanitization_indicators: &["ast.literal_eval", "safe_eval"],
    },
    SecurityPattern {
        sink: "exec(",
        description: "exec() with potential user input — code injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "input", "param", "query", "form"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "pickle.load",
        description: "Pickle deserialization — arbitrary code execution",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &["request", "upload", "file", "open(", "recv", "socket"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "yaml.load(",
        description: "Unsafe YAML loading — arbitrary code execution",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "file", "open(", "read"],
        sanitization_indicators: &["SafeLoader", "safe_load", "CSafeLoader"],
    },
    SecurityPattern {
        sink: "subprocess.call(",
        description: "Subprocess call — potential command injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["shell=True", "format(", "f\"", "request", "input", "%s"],
        sanitization_indicators: &["shlex.quote", "shlex.split", "shell=False"],
    },
    SecurityPattern {
        sink: "os.system(",
        description: "os.system() — command injection risk",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["format(", "f\"", "request", "input", "+", "%"],
        sanitization_indicators: &["shlex.quote"],
    },
    SecurityPattern {
        sink: ".execute(f",
        description: "SQL query with f-string — SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::Critical,
        user_input_indicators: &[],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: ".execute(",
        description: "SQL execute — potential SQL injection",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["%s", "format(", "+", "%", "f\""],
        sanitization_indicators: &["?", "%s,", "parameterize", "placeholder"],
    },
    SecurityPattern {
        sink: "mark_safe(",
        description: "mark_safe() — potential XSS if user input",
        category: FindingCategory::Injection,
        base_severity: Severity::High,
        user_input_indicators: &["request", "user", "input", "form", "query"],
        sanitization_indicators: &["escape", "bleach", "sanitize", "strip_tags"],
    },
    SecurityPattern {
        sink: "hashlib.md5(",
        description: "MD5 hash — weak cryptographic hash",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret", "key", "auth"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "hashlib.sha1(",
        description: "SHA1 hash — weak cryptographic hash",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::Medium,
        user_input_indicators: &["password", "token", "secret", "key", "auth"],
        sanitization_indicators: &[],
    },
    SecurityPattern {
        sink: "verify=False",
        description: "TLS verification disabled — man-in-the-middle risk",
        category: FindingCategory::SecuritySmell,
        base_severity: Severity::High,
        user_input_indicators: &[],
        sanitization_indicators: &[],
    },
];

const CONTEXT_WINDOW: usize = 3;

fn has_indicator(lines: &[&str], line_num: usize, indicators: &[&str]) -> bool {
    if indicators.is_empty() {
        return false;
    }
    let start = line_num.saturating_sub(CONTEXT_WINDOW);
    let end = (line_num + CONTEXT_WINDOW + 1).min(lines.len());
    for line in lines.iter().take(end).skip(start) {
        let line_lower = line.to_lowercase();
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
    // If no user_input_indicators were defined, pattern is inherently dangerous — stay at base
    let sev = if !indicators_defined || has_user_input {
        base
    } else {
        downgrade(base)
    };
    if has_sanitization {
        downgrade(sev)
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

fn patterns_for_language(lang: Language) -> &'static [SecurityPattern] {
    match lang {
        Language::Python => PYTHON_SECURITY_PATTERNS,
        Language::Rust => RUST_SECURITY_PATTERNS,
        _ => &[],
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

                        let has_user_input =
                            has_indicator(&all_lines, line_num, pattern.user_input_indicators);
                        let has_sanitization =
                            has_indicator(&all_lines, line_num, pattern.sanitization_indicators);
                        let indicators_defined = !pattern.user_input_indicators.is_empty();

                        let severity = adjust_severity(
                            pattern.base_severity,
                            has_user_input,
                            has_sanitization,
                            indicators_defined,
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
                            suggestion: "Validate and sanitize input before use".into(),
                            explanation: None,
                            fix: None,
                        });
                        break; // One finding per line max
                    }
                }
            }
        }

        Ok(findings)
    }
}

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

    fn make_ctx(source_files: HashMap<PathBuf, String>, lang: Language) -> AnalysisContext {
        AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: lang,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache: source_files,
            fuzz_corpus: None,
            config: DetectConfig::default(),
        }
    }

    #[tokio::test]
    async fn rust_command_injection_with_format() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "fn run(user: &str) {\n    let cmd = format!(\"echo {}\", user);\n    Command::new(cmd);\n}\n".into(),
        );
        let ctx = make_ctx(files, Language::Rust);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].severity == Severity::High || findings[0].severity == Severity::Critical);
        assert_eq!(findings[0].category, FindingCategory::Injection);
    }

    #[tokio::test]
    async fn python_eval_with_request_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/app.py"),
            "def handle(request):\n    data = request.get('expr')\n    result = eval(data)\n    return result\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn python_eval_without_user_input_downgraded() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/calc.py"),
            "def compute():\n    x = '2 + 2'\n    return eval(x)\n".into(),
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
            PathBuf::from("src/loader.py"),
            "import yaml\ndef load(path):\n    with open(path) as f:\n        return yaml.load(f, Loader=SafeLoader)\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        // base=High, open( in context window -> has_user_input=true -> stays High,
        // SafeLoader on same line -> has_sanitization=true -> downgrade to Medium
        assert_eq!(findings[0].severity, Severity::Medium);
    }

    #[tokio::test]
    async fn python_sql_fstring_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/db.py"),
            "def query(name):\n    cursor.execute(f\"SELECT * FROM users WHERE name='{name}'\")\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Critical);
    }

    #[tokio::test]
    async fn python_pickle_from_socket_is_critical() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/net.py"),
            "import pickle\nimport socket\ndef recv(sock):\n    data = sock.recv(4096)\n    return pickle.load(data)\n".into(),
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
            PathBuf::from("src/http.py"),
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
            PathBuf::from("tests/test_app.py"),
            "def test_eval():\n    eval('2+2')\n".into(),
        );
        let ctx = make_ctx(files, Language::Python);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn no_findings_for_unsupported_language() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.js"),
            "eval('alert(1)');\n".into(),
        );
        let ctx = make_ctx(files, Language::JavaScript);
        let findings = SecurityPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }
}
