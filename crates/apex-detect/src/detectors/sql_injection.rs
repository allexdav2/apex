//! SQL injection detector — identifies unsanitized user input in SQL queries.
//!
//! Scans for string formatting/concatenation patterns used to build SQL
//! queries. A full CPG-based version would trace taint flows; this initial
//! implementation uses pattern matching on common injection vectors.

use crate::finding::{Finding, FindingCategory, Severity};
use regex::Regex;
use std::path::PathBuf;
use uuid::Uuid;

/// SQL execution function patterns.
const SQL_EXEC_PATTERNS: &[&str] = &[
    "execute(",
    "executemany(",
    "raw(",
    "cursor.execute(",
    "db.execute(",
    "conn.execute(",
    "session.execute(",
];

/// Scan source code for SQL injection vulnerabilities.
pub fn scan_sql_injection(source: &str, file_path: &str) -> Vec<Finding> {
    let mut findings = Vec::new();

    // Pattern 1: f-string with SQL keywords
    let fstring_sql =
        Regex::new(r#"f["'](?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s.*\{[^}]+\}.*["']"#).unwrap();

    // Pattern 2: % formatting with SQL keywords
    let percent_sql =
        Regex::new(r#"["'](?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s.*%[sd].*["']\s*%"#).unwrap();

    // Pattern 3: String concatenation with SQL keywords
    let concat_sql =
        Regex::new(r#"["'](?i)(SELECT|INSERT|UPDATE|DELETE|DROP)\s.*["']\s*\+"#).unwrap();

    for (line_num, line) in source.lines().enumerate() {
        let line_1based = (line_num + 1) as u32;
        let trimmed = line.trim();

        // Skip parameterized queries (safe pattern).
        if SQL_EXEC_PATTERNS.iter().any(|p| trimmed.contains(p))
            && (trimmed.contains("%s\", (")
                || trimmed.contains("%s\", [")
                || trimmed.contains("?, (")
                || trimmed.contains("?, ["))
        {
            continue;
        }

        let is_vuln = fstring_sql.is_match(trimmed)
            || percent_sql.is_match(trimmed)
            || concat_sql.is_match(trimmed);

        if is_vuln {
            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "sql_injection".into(),
                severity: Severity::High,
                category: FindingCategory::Injection,
                file: PathBuf::from(file_path),
                line: Some(line_1based),
                title: "Potential SQL injection via string interpolation".into(),
                description: format!(
                    "SQL query constructed with string formatting at line {line_1based}. \
                     Use parameterized queries instead."
                ),
                evidence: vec![],
                covered: false,
                suggestion: "Use parameterized queries (e.g., cursor.execute(\"SELECT ... WHERE x = %s\", (val,)))".into(),
                explanation: None,
                fix: None,
                cwe_ids: vec![89],
            });
        }
    }

    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_string_format_injection() {
        let source = r#"
def get_user(request):
    name = request.args.get('name')
    query = "SELECT * FROM users WHERE name = '%s'" % name
    cursor.execute(query)
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(!findings.is_empty());
        assert!(findings[0].title.contains("SQL"));
    }

    #[test]
    fn detect_fstring_injection() {
        let source = r#"
def get_user(name):
    query = f"SELECT * FROM users WHERE name = '{name}'"
    db.execute(query)
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn safe_parameterized_query_not_flagged() {
        let source = r#"
def get_user(name):
    cursor.execute("SELECT * FROM users WHERE name = %s", (name,))
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn safe_no_user_input() {
        let source = r#"
def get_count():
    cursor.execute("SELECT COUNT(*) FROM users")
"#;
        let findings = scan_sql_injection(source, "app.py");
        assert!(findings.is_empty());
    }

    #[test]
    fn detect_concatenation_injection() {
        let source = r#"
def search(query_str):
    sql = "SELECT * FROM items WHERE name = '" + query_str + "'"
    conn.execute(sql)
"#;
        let findings = scan_sql_injection(source, "search.py");
        assert!(!findings.is_empty());
    }

    #[test]
    fn finding_has_correct_category() {
        let source = "query = f\"SELECT * FROM t WHERE x = '{user_input}'\"\ndb.execute(query)";
        let findings = scan_sql_injection(source, "x.py");
        if !findings.is_empty() {
            assert_eq!(findings[0].category, FindingCategory::Injection);
        }
    }
}
