//! Source code segment extraction for LLM-guided test synthesis.
//!
//! Extracts the code context around an uncovered line, and cleans up
//! test-runner error output before passing it to the LLM.

/// A segment of source code extracted around a target line.
#[derive(Debug, Clone)]
pub struct CodeSegment {
    /// The extracted source lines, joined by newlines.
    pub code: String,
    /// 1-based line number of the first line in `code`.
    pub start_line: u32,
    /// 1-based line number of the last line in `code`.
    pub end_line: u32,
    /// Which lines (1-based) are tagged as uncovered within this segment.
    pub tagged_lines: Vec<u32>,
}

/// Extract the source code segment around `target_line` (1-based).
///
/// Returns at most `context_lines` lines before and after `target_line`.
/// The target line itself is always included.
pub fn extract_segment(source: &str, target_line: u32, context_lines: u32) -> CodeSegment {
    let lines: Vec<&str> = source.lines().collect();
    let total = lines.len() as u32;

    // Guard against empty input or out-of-range target line.
    if lines.is_empty() || target_line == 0 || target_line > total {
        return CodeSegment {
            code: String::new(),
            start_line: 0,
            end_line: 0,
            tagged_lines: vec![],
        };
    }

    // Clamp to valid range (1-based → 0-based internally).
    let target_0 = target_line.saturating_sub(1);

    let start_0 = target_0.saturating_sub(context_lines);
    let end_0 = (target_0 + context_lines).min(total.saturating_sub(1));

    let start_line = start_0 + 1;
    let end_line = end_0 + 1;

    let code = lines[start_0 as usize..=end_0 as usize].join("\n");

    // Tag the target line if it falls within the extracted range.
    let tagged_lines = if target_line >= start_line && target_line <= end_line {
        vec![target_line]
    } else {
        vec![]
    };

    CodeSegment {
        code,
        start_line,
        end_line,
        tagged_lines,
    }
}

/// Clean pytest / test-runner error output for LLM consumption.
///
/// Strips separator lines that start with `===` or `---`, leaving only
/// the meaningful error text (tracebacks, assertion messages, etc.).
pub fn clean_error_output(raw: &str) -> String {
    raw.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("===") && !trimmed.starts_with("---")
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_extraction_finds_target_line() {
        let src = "def foo():\n    x = 1\n    if x > 0:\n        return x\n    return 0\n";
        let seg = extract_segment(src, 4, 10);
        assert!(seg.code.contains("return x"));
        assert!(seg.tagged_lines.contains(&4));
    }

    #[test]
    fn segment_with_context_window() {
        let src = (1..=20).map(|i| format!("line{i}\n")).collect::<String>();
        let seg = extract_segment(&src, 10, 3);
        assert!(seg.start_line >= 7);
        assert!(seg.end_line <= 13);
    }

    #[test]
    fn clean_error_removes_pytest_decoration() {
        let raw =
            "===== FAILURES =====\nAssertionError: expected 1\n===== short test summary =====";
        let cleaned = clean_error_output(raw);
        assert!(!cleaned.contains("====="));
        assert!(cleaned.contains("AssertionError"));
    }

    #[test]
    fn segment_target_line_always_included() {
        let src = "a\nb\nc\nd\ne\n";
        let seg = extract_segment(&src, 3, 1);
        assert!(seg.code.contains('c'));
        assert!(seg.tagged_lines.contains(&3));
    }

    #[test]
    fn segment_near_start_does_not_underflow() {
        let src = "line1\nline2\nline3\nline4\nline5\n";
        let seg = extract_segment(&src, 1, 5);
        assert_eq!(seg.start_line, 1);
        assert!(seg.tagged_lines.contains(&1));
    }

    #[test]
    fn segment_near_end_does_not_overflow() {
        let src = "line1\nline2\nline3\nline4\nline5\n";
        let seg = extract_segment(&src, 5, 5);
        assert_eq!(seg.end_line, 5);
        assert!(seg.tagged_lines.contains(&5));
    }

    #[test]
    fn clean_error_preserves_assertion_lines() {
        let raw = "--- test teardown ---\nAssertionError: 1 != 2\n--- short ---";
        let cleaned = clean_error_output(raw);
        assert!(cleaned.contains("AssertionError"));
        assert!(!cleaned.contains("---"));
    }

    #[test]
    fn clean_error_empty_input() {
        assert_eq!(clean_error_output(""), "");
    }

    #[test]
    fn segment_zero_context() {
        let src = "a\nb\nc\n";
        let seg = extract_segment(&src, 2, 0);
        assert_eq!(seg.start_line, 2);
        assert_eq!(seg.end_line, 2);
        assert_eq!(seg.code, "b");
    }

    #[test]
    fn extract_segment_empty_source() {
        let seg = extract_segment("", 1, 5);
        assert!(seg.code.is_empty());
        assert_eq!(seg.start_line, 0);
        assert_eq!(seg.end_line, 0);
    }

    #[test]
    fn extract_segment_out_of_range() {
        let seg = extract_segment("line1\nline2\n", 100, 5);
        assert!(seg.code.is_empty());
        assert_eq!(seg.start_line, 0);
        assert_eq!(seg.end_line, 0);
    }

    /// The `else` branch returning `vec![]` in `tagged_lines` (line 52) is
    /// structurally unreachable given the guard and clamping logic: the target
    /// line is always within [start_line, end_line] for any valid input.
    /// This test documents and verifies that invariant: even with a large
    /// context window the target line is always tagged.
    #[test]
    fn tagged_lines_always_includes_target_line() {
        let src = "a\nb\nc\nd\ne\n";
        for target in 1u32..=5 {
            for ctx in 0u32..=10 {
                let seg = extract_segment(src, target, ctx);
                assert!(
                    seg.tagged_lines.contains(&target),
                    "target_line={target} ctx={ctx} should always be tagged"
                );
            }
        }
    }
}
