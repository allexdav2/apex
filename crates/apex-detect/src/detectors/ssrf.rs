//! SSRF detector — identifies server-side request forgery patterns (CWE-918).

use crate::finding::{Finding, FindingCategory, Severity};
use std::path::PathBuf;
use uuid::Uuid;

/// HTTP request functions that could be SSRF vectors.
const HTTP_FUNCS: &[&str] = &[
    "requests.get(",
    "requests.post(",
    "requests.put(",
    "requests.delete(",
    "urllib.request.urlopen(",
    "http.get(",
    "fetch(",
    "HttpClient",
    "urlopen(",
];

/// Indicators that the URL comes from user input.
const USER_INPUT_INDICATORS: &[&str] = &[
    "request.", "params[", "args[", "input(", "sys.argv", "os.environ",
    "req.", "query[", "body[", "GET[", "POST[",
];

/// Sanitization indicators that suggest the URL is validated.
const SANITIZATION_INDICATORS: &[&str] = &[
    "urlparse", "validators.url", "allowlist", "whitelist",
    "ALLOWED_HOSTS", "validate_url",
];

/// Scan source code for SSRF vulnerabilities.
pub fn scan_ssrf(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Check if file has any sanitization imports/usage globally.
    let has_sanitization = SANITIZATION_INDICATORS
        .iter()
        .any(|s| source.contains(s));

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        // Skip comments.
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        // Check if line has an HTTP request function.
        let has_http_func = HTTP_FUNCS.iter().any(|f| trimmed.contains(f));
        if !has_http_func {
            continue;
        }

        // Check if line references user input.
        let has_user_input = USER_INPUT_INDICATORS.iter().any(|i| trimmed.contains(i));

        // Also detect f-string URL construction or concatenation with variables.
        let has_dynamic_url = trimmed.contains("f\"") || trimmed.contains("f'")
            || (trimmed.contains('+') && trimmed.contains("http"));

        if !has_user_input && !has_dynamic_url {
            continue;
        }

        // Skip if sanitization is present.
        if has_sanitization {
            continue;
        }

        findings.push(Finding {
            id: Uuid::new_v4(),
            detector: "ssrf".into(),
            severity: Severity::High,
            category: FindingCategory::SecuritySmell,
            file: PathBuf::from(file_path),
            line: Some(line_1based),
            title: "Potential server-side request forgery (SSRF)".into(),
            description: format!(
                "HTTP request at line {line_1based} uses user-controlled URL. \
                 An attacker could access internal services."
            ),
            evidence: vec![],
            covered: false,
            suggestion: "Validate URLs against an allowlist. Use urlparse to check \
                         scheme and host before making requests."
                .into(),
            explanation: None,
            fix: None,
            cwe_ids: vec![918],
        });
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_requests_get_with_user_input() {
        let source = "url = request.args['url']\nrequests.get(request.args['url'])\n";
        let findings = scan_ssrf(source, "api.py");
        assert!(!findings.is_empty());
        assert!(findings[0].cwe_ids.contains(&918));
    }

    #[test]
    fn detect_urllib_with_variable() {
        let source = "target = request.form['url']\nurllib.request.urlopen(request.form['u'])\n";
        let findings = scan_ssrf(source, "fetch.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn skip_hardcoded_urls() {
        let source = "requests.get(\"https://api.example.com/data\")\n";
        let findings = scan_ssrf(source, "client.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_fstring_url_construction() {
        let source = "resp = requests.get(f\"http://internal/{path}\")\n";
        let findings = scan_ssrf(source, "proxy.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn skip_with_urlparse_validation() {
        let source = "from urllib.parse import urlparse\nparsed = urlparse(url)\nrequests.get(request.args['url'])\n";
        let findings = scan_ssrf(source, "safe.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_fetch_with_variable() {
        let source = "const resp = fetch(req.body.url)\n";
        let findings = scan_ssrf(source, "handler.js");
        assert!(!findings.is_empty());
    }

    #[test]
    fn no_false_positive_on_internal_url_constants() {
        let source = "API_URL = \"https://internal.corp/api\"\nrequests.get(API_URL)\n";
        let findings = scan_ssrf(source, "config.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_in_multiple_files() {
        let s1 = "requests.post(request.args['u'])\n";
        let s2 = "http.get(params['target'])\n";
        let f1 = scan_ssrf(s1, "a.py");
        let f2 = scan_ssrf(s2, "b.js");
        assert!(!f1.is_empty());
        assert!(!f2.is_empty());
    }
}
