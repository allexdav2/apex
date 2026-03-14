//! Test code extractor — parses markdown fence blocks from LLM responses.

/// A code block extracted from a markdown-fenced LLM response.
#[derive(Debug, Clone)]
pub struct CodeBlock {
    pub language: Option<String>,
    pub code: String,
}

/// Extract all fenced code blocks from an LLM response.
pub fn extract_code_blocks(response: &str) -> Vec<CodeBlock> {
    let mut blocks = Vec::new();
    let mut lines = response.lines().peekable();

    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            let lang_tag = trimmed.strip_prefix("```").unwrap().trim();
            let language = if lang_tag.is_empty() {
                None
            } else {
                Some(lang_tag.to_string())
            };

            let mut code_lines = Vec::new();
            let mut closed = false;
            for inner in lines.by_ref() {
                if inner.trim().starts_with("```") {
                    closed = true;
                    break;
                }
                code_lines.push(inner);
            }
            if closed {
                blocks.push(CodeBlock {
                    language,
                    code: code_lines.join("\n"),
                });
            }
        }
    }

    blocks
}

/// Select the best test code block from an LLM response.
///
/// Prefers blocks tagged with the target language. Falls back to the first
/// block containing "test" or "def test" or "#[test]".
pub fn best_test_block(response: &str, target_language: &str) -> Option<String> {
    let blocks = extract_code_blocks(response);
    // Prefer matching language tag.
    if let Some(b) = blocks.iter().find(|b| {
        b.language
            .as_deref()
            .is_some_and(|l| l.eq_ignore_ascii_case(target_language))
    }) {
        return Some(b.code.clone());
    }
    // Fall back to first block containing test indicators.
    if let Some(b) = blocks
        .iter()
        .find(|b| b.code.contains("test") || b.code.contains("Test"))
    {
        return Some(b.code.clone());
    }
    // Last resort: first block.
    blocks.into_iter().next().map(|b| b.code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_single_python_block() {
        let response = "Here's the test:\n```python\ndef test_foo():\n    assert True\n```\n";
        let blocks = extract_code_blocks(response);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].code.contains("def test_foo"));
        assert_eq!(blocks[0].language.as_deref(), Some("python"));
    }

    #[test]
    fn extract_multiple_blocks() {
        let response = "```python\nblock1\n```\ntext\n```rust\nblock2\n```\n";
        let blocks = extract_code_blocks(response);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].language.as_deref(), Some("python"));
        assert_eq!(blocks[1].language.as_deref(), Some("rust"));
    }

    #[test]
    fn extract_no_language_tag() {
        let response = "```\ncode here\n```\n";
        let blocks = extract_code_blocks(response);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].language.is_none());
    }

    #[test]
    fn extract_no_blocks() {
        let blocks = extract_code_blocks("just plain text");
        assert!(blocks.is_empty());
    }

    #[test]
    fn extract_unclosed_fence_ignored() {
        let response = "```python\ncode without closing fence";
        let blocks = extract_code_blocks(response);
        assert!(blocks.is_empty());
    }

    #[test]
    fn best_test_block_prefers_python() {
        let response = "```python\ndef test_x(): pass\n```\n```\nother\n```\n";
        let best = best_test_block(response, "python");
        assert!(best.is_some());
        assert!(best.unwrap().contains("test_x"));
    }
}
