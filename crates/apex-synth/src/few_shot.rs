//! Few-shot prompt strategy: include example tests in LLM prompts.

/// A single few-shot example pairing a source gap with its successful test.
#[derive(Debug, Clone)]
pub struct FewShotExample {
    pub gap_kind: String,
    pub source_snippet: String,
    pub test_code: String,
}

/// A bounded bank of few-shot examples, evicting oldest when full.
#[derive(Debug, Clone)]
pub struct FewShotBank {
    examples: Vec<FewShotExample>,
    capacity: usize,
}

impl FewShotBank {
    pub fn new(capacity: usize) -> Self {
        FewShotBank {
            examples: Vec::new(),
            capacity,
        }
    }

    pub fn add_example(&mut self, example: FewShotExample) {
        if self.examples.len() >= self.capacity {
            self.examples.remove(0);
        }
        self.examples.push(example);
    }

    /// Retrieve up to `limit` examples matching `gap_kind`.
    pub fn retrieve(&self, gap_kind: &str, limit: usize) -> Vec<&FewShotExample> {
        self.examples
            .iter()
            .filter(|e| e.gap_kind == gap_kind)
            .take(limit)
            .collect()
    }
}

/// Format a block of few-shot examples for inclusion in an LLM prompt.
pub fn format_few_shot_block(examples: &[FewShotExample]) -> String {
    let mut out = String::new();
    for (i, ex) in examples.iter().enumerate() {
        out.push_str(&format!(
            "Example {}:\nSource:\n```\n{}\n```\nTest:\n```\n{}\n```\n\n",
            i + 1,
            ex.source_snippet,
            ex.test_code,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bank_stores_and_retrieves_examples() {
        let mut bank = FewShotBank::new(5);
        bank.add_example(FewShotExample {
            gap_kind: "branch".into(),
            source_snippet: "if x > 0:".into(),
            test_code: "def test_positive(): assert f(1) == 1".into(),
        });
        let examples = bank.retrieve("branch", 3);
        assert_eq!(examples.len(), 1);
        assert!(examples[0].test_code.contains("test_positive"));
    }

    #[test]
    fn bank_respects_capacity_limit() {
        let mut bank = FewShotBank::new(2);
        for i in 0..5 {
            bank.add_example(FewShotExample {
                gap_kind: "branch".into(),
                source_snippet: format!("line {i}"),
                test_code: format!("test_{i}"),
            });
        }
        // Oldest examples evicted; at most 2 remain.
        let examples = bank.retrieve("branch", 10);
        assert!(examples.len() <= 2);
    }

    #[test]
    fn format_few_shot_block_generates_markdown() {
        let examples = vec![FewShotExample {
            gap_kind: "branch".into(),
            source_snippet: "if x > 0:".into(),
            test_code: "def test_pos(): assert f(1)".into(),
        }];
        let block = format_few_shot_block(&examples);
        assert!(block.contains("Example"));
        assert!(block.contains("def test_pos"));
        assert!(block.contains("if x > 0:"));
    }
}
