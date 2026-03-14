//! YAML rule loader for external security rule definitions.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Severity level for a rule finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSeverity {
    Error,
    Warning,
    Info,
}

/// A security rule definition loaded from YAML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleDefinition {
    /// Unique rule identifier (e.g., "APEX-SQL-001").
    pub id: String,
    /// Human-readable message describing the issue.
    pub message: String,
    /// Severity level.
    pub severity: RuleSeverity,
    /// Pattern expressions to match (any match triggers the rule).
    pub patterns: Vec<String>,
    /// CWE identifier (e.g., 89 for SQL injection).
    #[serde(default)]
    pub cwe: Option<u32>,
    /// Languages this rule applies to (empty = all).
    #[serde(default)]
    pub languages: Vec<String>,
    /// Fix suggestion text.
    #[serde(default)]
    pub fix: Option<String>,
}

/// A collection of rules from a YAML file.
#[derive(Debug, Deserialize)]
struct RuleFile {
    rules: Vec<RuleDefinition>,
}

/// Load rules from a YAML string.
pub fn load_rules_from_yaml(yaml: &str) -> Result<Vec<RuleDefinition>, String> {
    let rule_file: RuleFile =
        serde_yaml::from_str(yaml).map_err(|e| format!("failed to parse YAML rules: {e}"))?;
    Ok(rule_file.rules)
}

/// Load rules from a YAML file path.
pub fn load_rules_from_file(path: &Path) -> Result<Vec<RuleDefinition>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read rule file {}: {e}", path.display()))?;
    load_rules_from_yaml(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_valid_yaml() {
        let yaml = r#"
rules:
  - id: "APEX-SQL-001"
    message: "SQL injection via string concatenation"
    severity: error
    patterns:
      - "execute($QUERY)"
      - "raw_sql($INPUT)"
    cwe: 89
    languages: ["python", "ruby"]
    fix: "Use parameterized queries instead"
"#;
        let rules = load_rules_from_yaml(yaml).unwrap();
        assert_eq!(rules.len(), 1);
        let rule = &rules[0];
        assert_eq!(rule.id, "APEX-SQL-001");
        assert_eq!(rule.message, "SQL injection via string concatenation");
        assert_eq!(rule.severity, RuleSeverity::Error);
        assert_eq!(rule.patterns.len(), 2);
        assert_eq!(rule.cwe, Some(89));
        assert_eq!(rule.languages, vec!["python", "ruby"]);
        assert_eq!(rule.fix.as_deref(), Some("Use parameterized queries instead"));
    }

    #[test]
    fn load_minimal_yaml() {
        let yaml = r#"
rules:
  - id: "RULE-001"
    message: "Bad pattern"
    severity: warning
    patterns:
      - "eval("
"#;
        let rules = load_rules_from_yaml(yaml).unwrap();
        assert_eq!(rules.len(), 1);
        let rule = &rules[0];
        assert_eq!(rule.id, "RULE-001");
        assert_eq!(rule.cwe, None);
        assert!(rule.languages.is_empty());
        assert_eq!(rule.fix, None);
    }

    #[test]
    fn load_multiple_rules() {
        let yaml = r#"
rules:
  - id: "R1"
    message: "Rule 1"
    severity: error
    patterns: ["p1"]
  - id: "R2"
    message: "Rule 2"
    severity: warning
    patterns: ["p2"]
  - id: "R3"
    message: "Rule 3"
    severity: info
    patterns: ["p3"]
"#;
        let rules = load_rules_from_yaml(yaml).unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].id, "R1");
        assert_eq!(rules[1].id, "R2");
        assert_eq!(rules[2].id, "R3");
    }

    #[test]
    fn load_with_cwe() {
        let yaml = r#"
rules:
  - id: "XSS-001"
    message: "XSS"
    severity: error
    patterns: ["innerHTML"]
    cwe: 79
"#;
        let rules = load_rules_from_yaml(yaml).unwrap();
        assert_eq!(rules[0].cwe, Some(79));
    }

    #[test]
    fn load_with_languages() {
        let yaml = r#"
rules:
  - id: "LANG-001"
    message: "Language-specific"
    severity: info
    patterns: ["unsafe"]
    languages: ["rust", "go", "c"]
"#;
        let rules = load_rules_from_yaml(yaml).unwrap();
        assert_eq!(rules[0].languages, vec!["rust", "go", "c"]);
    }

    #[test]
    fn load_invalid_yaml() {
        let yaml = "not: valid: yaml: [";
        let result = load_rules_from_yaml(yaml);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("failed to parse YAML rules"));
    }

    #[test]
    fn load_empty_rules() {
        let yaml = "rules: []";
        let rules = load_rules_from_yaml(yaml).unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn severity_variants() {
        let yaml = r#"
rules:
  - id: "E1"
    message: "err"
    severity: error
    patterns: ["a"]
  - id: "W1"
    message: "warn"
    severity: warning
    patterns: ["b"]
  - id: "I1"
    message: "info"
    severity: info
    patterns: ["c"]
"#;
        let rules = load_rules_from_yaml(yaml).unwrap();
        assert_eq!(rules[0].severity, RuleSeverity::Error);
        assert_eq!(rules[1].severity, RuleSeverity::Warning);
        assert_eq!(rules[2].severity, RuleSeverity::Info);
    }
}
