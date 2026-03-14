//! Pattern expression parser and matcher.
//!
//! Supports simple pattern expressions with metavariables (`$VAR`)
//! for matching source code patterns.

use std::collections::HashMap;

/// A pattern expression for matching source code.
#[derive(Debug, Clone, PartialEq)]
pub enum PatternExpr {
    /// Literal string match.
    Literal(String),
    /// Metavariable that captures any identifier (`$VAR`, `$FUNC`, etc.).
    Metavar(String),
    /// Sequence of pattern elements (e.g., `$FUNC($ARG)`).
    Sequence(Vec<PatternExpr>),
    /// Match any of the alternatives.
    Any(Vec<PatternExpr>),
}

/// Result of a successful pattern match — captured metavar bindings.
pub type MatchBindings = HashMap<String, String>;

/// Parse a pattern string into a [`PatternExpr`].
///
/// Syntax:
/// - `$IDENTIFIER` — Metavar
/// - `literal text` — Literal
/// - `{a | b | c}` — Any (alternatives)
/// - Adjacent elements form a Sequence
pub fn parse_pattern(pattern: &str) -> PatternExpr {
    // Handle alternatives: {a | b | c}
    if let Some(inner) = pattern.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        let alternatives: Vec<PatternExpr> = inner.split('|').map(|s| parse_pattern(s.trim())).collect();
        return PatternExpr::Any(alternatives);
    }

    let parts = tokenize(pattern);
    if parts.len() == 1 {
        parts.into_iter().next().unwrap()
    } else {
        PatternExpr::Sequence(parts)
    }
}

fn tokenize(pattern: &str) -> Vec<PatternExpr> {
    let mut parts = Vec::new();
    let mut literal_buf = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1].is_ascii_uppercase() {
            // Flush any accumulated literal
            if !literal_buf.is_empty() {
                parts.push(PatternExpr::Literal(literal_buf.clone()));
                literal_buf.clear();
            }
            // Collect the metavar name
            let mut name = String::new();
            i += 1; // skip '$'
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                name.push(chars[i]);
                i += 1;
            }
            parts.push(PatternExpr::Metavar(name));
        } else {
            literal_buf.push(chars[i]);
            i += 1;
        }
    }

    if !literal_buf.is_empty() {
        parts.push(PatternExpr::Literal(literal_buf));
    }

    parts
}

/// Match a pattern against a source line, returning bindings if matched.
pub fn match_pattern(pattern: &PatternExpr, source: &str) -> Option<MatchBindings> {
    match pattern {
        PatternExpr::Literal(lit) => {
            if source.contains(lit.as_str()) {
                Some(HashMap::new())
            } else {
                None
            }
        }
        PatternExpr::Metavar(name) => {
            // Metavar alone matches any non-empty identifier-like token
            let token = source.split_whitespace().next()?;
            let mut bindings = HashMap::new();
            bindings.insert(name.clone(), token.to_string());
            Some(bindings)
        }
        PatternExpr::Sequence(parts) => {
            // Build a regex from the sequence: metavars become named capture groups,
            // literals are regex-escaped.
            let mut regex_str = String::new();
            let mut metavar_names = Vec::new();
            for part in parts {
                match part {
                    PatternExpr::Literal(lit) => {
                        regex_str.push_str(&regex::escape(lit));
                    }
                    PatternExpr::Metavar(name) => {
                        // Use a named capture group for identifiers
                        regex_str.push_str(&format!(r"(?P<{name}>\w+)"));
                        metavar_names.push(name.clone());
                    }
                    _ => {
                        // Nested Any/Sequence not supported in sequence regex
                        return None;
                    }
                }
            }
            let re = regex::Regex::new(&regex_str).ok()?;
            let caps = re.captures(source)?;
            let mut bindings = HashMap::new();
            for name in &metavar_names {
                if let Some(m) = caps.name(name) {
                    bindings.insert(name.clone(), m.as_str().to_string());
                }
            }
            Some(bindings)
        }
        PatternExpr::Any(alternatives) => {
            for alt in alternatives {
                if let Some(bindings) = match_pattern(alt, source) {
                    return Some(bindings);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_match() {
        let pattern = parse_pattern("eval(");
        assert!(match_pattern(&pattern, "x = eval(user_input)").is_some());
    }

    #[test]
    fn literal_no_match() {
        let pattern = parse_pattern("eval(");
        assert!(match_pattern(&pattern, "print(x)").is_none());
    }

    #[test]
    fn metavar_captures_token() {
        let pattern = parse_pattern("$FUNC");
        let bindings = match_pattern(&pattern, "eval user_input").unwrap();
        assert_eq!(bindings.get("FUNC").unwrap(), "eval");
    }

    #[test]
    fn sequence_matches() {
        let pattern = parse_pattern("$FUNC($ARG)");
        assert!(match_pattern(&pattern, "result = eval(user_input)").is_some());
    }

    #[test]
    fn sequence_captures_bindings() {
        let pattern = parse_pattern("$FUNC($ARG)");
        let bindings = match_pattern(&pattern, "result = eval(user_input)").unwrap();
        assert_eq!(bindings.get("FUNC").unwrap(), "eval");
        assert_eq!(bindings.get("ARG").unwrap(), "user_input");
    }

    #[test]
    fn sequence_no_match() {
        let pattern = parse_pattern("$FUNC($ARG)");
        assert!(match_pattern(&pattern, "just a plain string").is_none());
    }

    #[test]
    fn any_matches_first() {
        let pattern = parse_pattern("{eval | exec}");
        assert!(match_pattern(&pattern, "x = eval()").is_some());
    }

    #[test]
    fn any_matches_second() {
        let pattern = parse_pattern("{eval | exec}");
        assert!(match_pattern(&pattern, "x = exec()").is_some());
    }

    #[test]
    fn any_no_match() {
        let pattern = parse_pattern("{eval | exec}");
        assert!(match_pattern(&pattern, "x = print()").is_none());
    }

    #[test]
    fn parse_literal() {
        let expr = parse_pattern("hello");
        assert_eq!(expr, PatternExpr::Literal("hello".to_string()));
    }

    #[test]
    fn parse_metavar() {
        let expr = parse_pattern("$VAR");
        assert_eq!(expr, PatternExpr::Metavar("VAR".to_string()));
    }

    #[test]
    fn parse_sequence_with_metavar() {
        let expr = parse_pattern("$FUNC($ARG)");
        assert!(matches!(expr, PatternExpr::Sequence(_)));
        if let PatternExpr::Sequence(parts) = expr {
            assert_eq!(parts.len(), 4); // $FUNC, "(", $ARG, ")"
            assert_eq!(parts[0], PatternExpr::Metavar("FUNC".to_string()));
            assert_eq!(parts[1], PatternExpr::Literal("(".to_string()));
            assert_eq!(parts[2], PatternExpr::Metavar("ARG".to_string()));
            assert_eq!(parts[3], PatternExpr::Literal(")".to_string()));
        }
    }
}
