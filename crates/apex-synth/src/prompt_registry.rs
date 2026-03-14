//! Prompt template registry for language-aware test synthesis.
//!
//! Maps `(language, gap_kind)` keys to Tera template strings. Supports
//! variable substitution for file paths, line numbers, and code segments.

use std::collections::HashMap;

/// Registry mapping `(language, gap_kind)` to prompt template strings.
#[derive(Debug, Clone)]
pub struct PromptRegistry {
    templates: HashMap<(String, String), String>,
}

impl PromptRegistry {
    pub fn new() -> Self {
        PromptRegistry {
            templates: HashMap::new(),
        }
    }

    /// Create a registry pre-loaded with default templates.
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register(
            "python",
            "branch",
            "File: {{ file }}\nUncovered: {{ lines }}\n\n\
             Source:\n```python\n{{ segment }}\n```\n\n\
             Write a pytest test that exercises {{ lines }} in {{ file }}.",
        );
        reg.register(
            "rust",
            "branch",
            "File: {{ file }}\nUncovered: {{ lines }}\n\n\
             Source:\n```rust\n{{ segment }}\n```\n\n\
             Write a #[test] function that exercises {{ lines }} in {{ file }}.",
        );
        reg.register(
            "javascript",
            "branch",
            "File: {{ file }}\nUncovered: {{ lines }}\n\n\
             Source:\n```javascript\n{{ segment }}\n```\n\n\
             Write a Jest test that exercises {{ lines }} in {{ file }}.",
        );
        reg
    }

    /// Register a template for a `(language, gap_kind)` pair.
    pub fn register(&mut self, language: &str, gap_kind: &str, template: &str) {
        self.templates
            .insert((language.to_string(), gap_kind.to_string()), template.to_string());
    }

    /// Look up a template by `(language, gap_kind)`.
    pub fn lookup(&self, language: &str, gap_kind: &str) -> Option<&str> {
        self.templates
            .get(&(language.to_string(), gap_kind.to_string()))
            .map(|s| s.as_str())
    }

    /// Render a template with variable substitution.
    pub fn render(
        &self,
        language: &str,
        gap_kind: &str,
        vars: &HashMap<String, String>,
    ) -> Result<String, String> {
        let template = self
            .lookup(language, gap_kind)
            .ok_or_else(|| format!("no template for ({language}, {gap_kind})"))?;
        let mut result = template.to_string();
        for (key, value) in vars {
            result = result.replace(&format!("{{{{ {key} }}}}"), value);
        }
        Ok(result)
    }
}

impl Default for PromptRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup_template() {
        let mut registry = PromptRegistry::new();
        registry.register("python", "branch", "Write a test for {{ file }}");
        let tmpl = registry.lookup("python", "branch");
        assert!(tmpl.is_some());
        assert!(tmpl.unwrap().contains("{{ file }}"));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let registry = PromptRegistry::new();
        assert!(registry.lookup("rust", "branch").is_none());
    }

    #[test]
    fn render_template_substitutes_variables() {
        let mut registry = PromptRegistry::new();
        registry.register("python", "branch", "Test for {{ file }} line {{ line }}");
        let mut vars = std::collections::HashMap::new();
        vars.insert("file".into(), "app.py".into());
        vars.insert("line".into(), "42".into());
        let rendered = registry.render("python", "branch", &vars).unwrap();
        assert!(rendered.contains("app.py"));
        assert!(rendered.contains("42"));
    }

    #[test]
    fn render_missing_template_returns_error() {
        let registry = PromptRegistry::new();
        let result = registry.render("go", "branch", &std::collections::HashMap::new());
        assert!(result.is_err());
    }

    #[test]
    fn default_registry_has_python_branch() {
        let registry = PromptRegistry::with_defaults();
        assert!(registry.lookup("python", "branch").is_some());
    }
}
