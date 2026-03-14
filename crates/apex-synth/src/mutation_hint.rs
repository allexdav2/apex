//! Mutation-hint prompt enrichment for LLM test synthesis.
//!
//! Injects near-miss comparison data from the fuzzer into the LLM prompt
//! so the model knows exactly what threshold to cross.

/// A single mutation hint from fuzzer near-miss data.
#[derive(Debug, Clone)]
pub struct MutationHint {
    pub variable: String,
    pub operator: String,
    pub threshold: i64,
    pub closest_value: i64,
}

impl MutationHint {
    /// Format this hint as a human-readable string for LLM consumption.
    pub fn format(&self) -> String {
        format!(
            "Branch `{var} {op} {thresh}` was tested with `{var}={closest}` \
             (distance: {dist}). Write a test where `{var} {op} {thresh}`.",
            var = self.variable,
            op = self.operator,
            thresh = self.threshold,
            closest = self.closest_value,
            dist = (self.threshold - self.closest_value).abs(),
        )
    }
}

/// Format a block of mutation hints for inclusion in an LLM prompt.
/// Returns empty string if there are no hints.
pub fn format_hints_block(hints: &[MutationHint]) -> String {
    if hints.is_empty() {
        return String::new();
    }
    let mut out = String::from("Mutation hints from fuzzer:\n");
    for hint in hints {
        out.push_str(&format!("- {}\n", hint.format()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_formats_comparison() {
        let hint = MutationHint {
            variable: "x".into(),
            operator: ">".into(),
            threshold: 42,
            closest_value: 40,
        };
        let text = hint.format();
        assert!(text.contains("x"));
        assert!(text.contains("42"));
        assert!(text.contains("40"));
    }

    #[test]
    fn format_hints_block_multiple() {
        let hints = vec![
            MutationHint {
                variable: "x".into(),
                operator: ">".into(),
                threshold: 42,
                closest_value: 40,
            },
            MutationHint {
                variable: "y".into(),
                operator: "==".into(),
                threshold: 0,
                closest_value: 1,
            },
        ];
        let block = format_hints_block(&hints);
        assert!(block.contains("x"));
        assert!(block.contains("y"));
        assert!(block.contains("Mutation hints"));
    }

    #[test]
    fn format_hints_block_empty() {
        let block = format_hints_block(&[]);
        assert!(block.is_empty());
    }
}
