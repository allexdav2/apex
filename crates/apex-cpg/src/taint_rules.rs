//! Configurable taint rules for CPG taint analysis.
//!
//! Provides built-in rule sets for Python and JavaScript, plus a merge
//! mechanism for user-defined custom rules.

use serde::{Deserialize, Serialize};

/// A set of taint analysis rules: sources, sinks, and sanitizers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintRuleSet {
    pub sources: Vec<String>,
    pub sinks: Vec<String>,
    pub sanitizers: Vec<String>,
}

impl TaintRuleSet {
    /// Create an empty rule set.
    pub fn empty() -> Self {
        TaintRuleSet {
            sources: Vec::new(),
            sinks: Vec::new(),
            sanitizers: Vec::new(),
        }
    }

    /// Default Python taint rules.
    pub fn python_defaults() -> Self {
        TaintRuleSet {
            sources: vec![
                "request.args".into(),
                "request.form".into(),
                "request.data".into(),
                "request.json".into(),
                "request.get_json".into(),
                "sys.argv".into(),
                "input".into(),
                "os.environ".into(),
            ],
            sinks: vec![
                "execute".into(),
                "executemany".into(),
                "os.system".into(),
                "os.popen".into(),
                "subprocess.call".into(),
                "subprocess.run".into(),
                "eval".into(),
                "exec".into(),
                "open".into(),
                "render_template_string".into(),
            ],
            sanitizers: vec![
                "escape".into(),
                "quote".into(),
                "sanitize".into(),
                "clean".into(),
                "parameterize".into(),
                "bleach.clean".into(),
                "markupsafe.escape".into(),
            ],
        }
    }

    /// Default JavaScript taint rules.
    pub fn javascript_defaults() -> Self {
        TaintRuleSet {
            sources: vec![
                "req.body".into(),
                "req.params".into(),
                "req.query".into(),
                "req.headers".into(),
                "document.location".into(),
                "window.location".into(),
                "process.argv".into(),
                "process.env".into(),
            ],
            sinks: vec![
                "eval".into(),
                "exec".into(),
                "execSync".into(),
                "innerHTML".into(),
                "document.write".into(),
                "child_process.exec".into(),
                "db.query".into(),
                "pool.query".into(),
                "fs.readFile".into(),
                "fs.writeFile".into(),
            ],
            sanitizers: vec![
                "escape".into(),
                "sanitize".into(),
                "encodeURIComponent".into(),
                "DOMPurify.sanitize".into(),
                "validator.escape".into(),
            ],
        }
    }

    /// Merge another rule set into this one (additive, no duplicates).
    pub fn merge(&mut self, other: &TaintRuleSet) {
        for src in &other.sources {
            if !self.sources.contains(src) {
                self.sources.push(src.clone());
            }
        }
        for sink in &other.sinks {
            if !self.sinks.contains(sink) {
                self.sinks.push(sink.clone());
            }
        }
        for san in &other.sanitizers {
            if !self.sanitizers.contains(san) {
                self.sanitizers.push(san.clone());
            }
        }
    }

    /// Check if a function name matches any source pattern.
    pub fn is_source(&self, name: &str) -> bool {
        self.sources.iter().any(|s| name.contains(s.as_str()))
    }

    /// Check if a function name matches any sink pattern.
    pub fn is_sink(&self, name: &str) -> bool {
        self.sinks.iter().any(|s| name.contains(s.as_str()))
    }

    /// Check if a function name matches any sanitizer pattern.
    pub fn is_sanitizer(&self, name: &str) -> bool {
        self.sanitizers.iter().any(|s| name.contains(s.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_python_rules_have_sources() {
        let rules = TaintRuleSet::python_defaults();
        assert!(!rules.sources.is_empty());
        assert!(rules.sources.iter().any(|s| s.contains("request")));
    }

    #[test]
    fn default_python_rules_have_sinks() {
        let rules = TaintRuleSet::python_defaults();
        assert!(!rules.sinks.is_empty());
        assert!(rules.sinks.iter().any(|s| s.contains("execute")));
    }

    #[test]
    fn default_python_rules_have_sanitizers() {
        let rules = TaintRuleSet::python_defaults();
        assert!(!rules.sanitizers.is_empty());
    }

    #[test]
    fn javascript_rules_have_sources() {
        let rules = TaintRuleSet::javascript_defaults();
        assert!(!rules.sources.is_empty());
        assert!(rules.sources.iter().any(|s| s.contains("req")));
    }

    #[test]
    fn javascript_rules_have_sinks() {
        let rules = TaintRuleSet::javascript_defaults();
        assert!(!rules.sinks.is_empty());
    }

    #[test]
    fn custom_rules_merge() {
        let mut rules = TaintRuleSet::python_defaults();
        let custom = TaintRuleSet {
            sources: vec!["custom_source".into()],
            sinks: vec!["custom_sink".into()],
            sanitizers: vec![],
        };
        rules.merge(&custom);
        assert!(rules.sources.contains(&"custom_source".to_string()));
        assert!(rules.sinks.contains(&"custom_sink".to_string()));
    }

    #[test]
    fn is_source_checks_membership() {
        let rules = TaintRuleSet {
            sources: vec!["request.args".into()],
            sinks: vec![],
            sanitizers: vec![],
        };
        assert!(rules.is_source("request.args"));
        assert!(!rules.is_source("safe_func"));
    }

    #[test]
    fn is_sink_checks_membership() {
        let rules = TaintRuleSet {
            sources: vec![],
            sinks: vec!["execute".into()],
            sanitizers: vec![],
        };
        assert!(rules.is_sink("execute"));
        assert!(!rules.is_sink("safe_func"));
    }

    #[test]
    fn is_sanitizer_checks_membership() {
        let rules = TaintRuleSet {
            sources: vec![],
            sinks: vec![],
            sanitizers: vec!["escape".into()],
        };
        assert!(rules.is_sanitizer("escape"));
        assert!(!rules.is_sanitizer("noop"));
    }

    #[test]
    fn empty_rules() {
        let rules = TaintRuleSet::empty();
        assert!(rules.sources.is_empty());
        assert!(rules.sinks.is_empty());
        assert!(rules.sanitizers.is_empty());
    }
}
