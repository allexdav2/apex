//! Differential coverage utilities.
//!
//! Parses `git diff --unified=0` output to extract changed line ranges,
//! enabling `--diff <REF>` filtering in `apex run` and `apex audit`.

use crate::command::{CommandRunner, CommandSpec};
use crate::error::{ApexError, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Map from file path (relative to repo root) to the set of line numbers
/// changed in that file relative to `base_ref`.
pub type ChangedLineMap = HashMap<PathBuf, HashSet<u32>>;

/// Get changed lines from `git diff --unified=0 <base_ref> -- .`.
///
/// Parses hunk headers of the form `@@ -a,b +c,d @@` and collects
/// the `+` (new-file) line numbers. The result is keyed by repo-relative
/// file path.
pub async fn changed_lines(
    runner: &dyn CommandRunner,
    repo: &Path,
    base_ref: &str,
) -> Result<ChangedLineMap> {
    let spec = CommandSpec::new("git", repo)
        .args(["diff", "--unified=0", base_ref, "--", "."])
        .timeout(30_000);

    let output = runner
        .run_command(&spec)
        .await
        .map_err(|e| ApexError::Detect(format!("git diff: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_diff_output(&stdout))
}

/// Parse the unified diff text and return a map of file -> changed line numbers.
///
/// Handles:
/// - `+++ b/<path>` headers to identify the current file.
/// - `@@ -old,old_count +new,new_count @@` hunk headers.
/// - `@@ -old +new @@` (single-line form, count is implicitly 1).
pub fn parse_diff_output(diff: &str) -> ChangedLineMap {
    let mut result: ChangedLineMap = HashMap::new();
    let mut current_file: Option<PathBuf> = None;

    for line in diff.lines() {
        // New file header: `+++ b/path/to/file`
        if let Some(rest) = line.strip_prefix("+++ ") {
            // Strip `b/` prefix that git uses for new-side paths
            let path_str = rest.strip_prefix("b/").unwrap_or(rest);
            // `/dev/null` means a deletion — no new lines
            if path_str == "/dev/null" {
                current_file = None;
            } else {
                current_file = Some(PathBuf::from(path_str));
            }
            continue;
        }

        // Hunk header: `@@ -a[,b] +c[,d] @@ ...`
        if line.starts_with("@@") {
            if let Some(file) = &current_file {
                if let Some(lines) = parse_hunk_header(line) {
                    let entry = result.entry(file.clone()).or_default();
                    entry.extend(lines);
                }
            }
        }
    }

    result
}

/// Parse a hunk header line and return the set of new-side line numbers.
///
/// Hunk header format: `@@ -<old_start>[,<old_count>] +<new_start>[,<new_count>] @@`
///
/// If `new_count` is 0 (pure deletion), returns an empty set.
fn parse_hunk_header(line: &str) -> Option<HashSet<u32>> {
    // Find the `+` part between `@@` markers
    let inner = line.strip_prefix("@@ ")?;
    // Skip over the `-old` part
    let after_old = inner.split_once(' ')?.1;
    // after_old starts with `+new_start[,new_count]`
    let new_part = after_old.trim_start_matches('+');
    // May end with ` @@` — take only up to the next space
    let new_part = new_part.split_once(' ').map_or(new_part, |(n, _)| n);

    let (start, count) = if let Some((s, c)) = new_part.split_once(',') {
        let start: u32 = s.parse().ok()?;
        let count: u32 = c.parse().ok()?;
        (start, count)
    } else {
        let start: u32 = new_part.parse().ok()?;
        (start, 1)
    };

    // count == 0 means pure deletion — no new lines
    if count == 0 {
        return Some(HashSet::new());
    }

    Some((start..start + count).collect())
}

/// Filter a set of findings to only those whose `line` field falls within
/// the changed lines for their file.
///
/// `findings_file` returns the file path and optional line for each finding.
/// Returns a `Vec` of filtered indices (into the original slice).
pub fn filter_findings_by_changed_lines<F, T>(
    findings: &[T],
    changed: &ChangedLineMap,
    file_line: F,
) -> Vec<usize>
where
    F: Fn(&T) -> (PathBuf, Option<u32>),
{
    findings
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let (file, line) = file_line(f);
            let line = line?;
            // Normalize to relative path in case findings use absolute paths
            let key = changed.keys().find(|k| file.ends_with(*k) || *k == &file)?;
            if changed[key].contains(&line) {
                Some(i)
            } else {
                None
            }
        })
        .collect()
}

/// Summary of coverage limited to changed lines.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiffCoverageReport {
    /// The git ref used as the comparison base.
    pub base_ref: String,
    /// Number of files with changed lines.
    pub changed_files: usize,
    /// Total number of changed lines across all files.
    pub changed_lines: usize,
    /// How many of those changed lines are covered.
    pub covered_lines: usize,
    /// Coverage percentage of changed lines (0.0–100.0).
    pub coverage_pct: f64,
}

impl DiffCoverageReport {
    pub fn new(
        base_ref: impl Into<String>,
        changed_lines_total: usize,
        covered: usize,
        changed_files: usize,
    ) -> Self {
        let coverage_pct = if changed_lines_total == 0 {
            100.0
        } else {
            (covered as f64 / changed_lines_total as f64) * 100.0
        };
        DiffCoverageReport {
            base_ref: base_ref.into(),
            changed_files,
            changed_lines: changed_lines_total,
            covered_lines: covered,
            coverage_pct,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandOutput;
    use crate::fixture_runner::FixtureRunner;

    const SAMPLE_DIFF: &str = r#"diff --git a/src/lib.rs b/src/lib.rs
index abc..def 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -10,3 +10,5 @@ fn old() {}
+fn new_a() {}
+fn new_b() {}
 fn unchanged() {}
+fn new_c() {}
+fn new_d() {}
@@ -50 +52,1 @@ fn other() {}
+fn added_single() {}
diff --git a/src/main.rs b/src/main.rs
index 111..222 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,0 +2,3 @@ fn main() {
+    let x = 1;
+    let y = 2;
+    let z = x + y;
"#;

    #[test]
    fn parse_diff_extracts_changed_lines() {
        let changed = parse_diff_output(SAMPLE_DIFF);

        let lib_lines = &changed[&PathBuf::from("src/lib.rs")];
        // First hunk: +10,5 → lines 10,11,12,13,14
        assert!(lib_lines.contains(&10));
        assert!(lib_lines.contains(&11));
        assert!(lib_lines.contains(&14));
        // Second hunk: +52,1 → line 52
        assert!(lib_lines.contains(&52));

        let main_lines = &changed[&PathBuf::from("src/main.rs")];
        // Hunk: +2,3 → lines 2,3,4
        assert!(main_lines.contains(&2));
        assert!(main_lines.contains(&3));
        assert!(main_lines.contains(&4));
        assert!(!main_lines.contains(&1));
    }

    #[test]
    fn parse_diff_empty_input() {
        let changed = parse_diff_output("");
        assert!(changed.is_empty());
    }

    #[test]
    fn parse_hunk_header_standard() {
        let lines = parse_hunk_header("@@ -10,3 +20,4 @@ fn foo() {}").unwrap();
        assert_eq!(lines, (20u32..24).collect());
    }

    #[test]
    fn parse_hunk_header_single_line_no_comma() {
        // `@@ -10 +20 @@` — no comma means count of 1
        let lines = parse_hunk_header("@@ -10 +20 @@ fn bar() {}").unwrap();
        assert_eq!(lines.len(), 1);
        assert!(lines.contains(&20));
    }

    #[test]
    fn parse_hunk_header_pure_deletion() {
        // `@@ -5,3 +5,0 @@` — count 0, no new lines
        let lines = parse_hunk_header("@@ -5,3 +5,0 @@").unwrap();
        assert!(lines.is_empty());
    }

    #[test]
    fn filter_findings_returns_matching_indices() {
        let changed = {
            let mut m = ChangedLineMap::new();
            let mut s = HashSet::new();
            s.insert(10u32);
            s.insert(20u32);
            m.insert(PathBuf::from("src/lib.rs"), s);
            m
        };

        // Findings: (file, line)
        let findings = vec![
            (PathBuf::from("src/lib.rs"), Some(10u32)),   // matches
            (PathBuf::from("src/lib.rs"), Some(15u32)),   // no match
            (PathBuf::from("src/lib.rs"), Some(20u32)),   // matches
            (PathBuf::from("src/other.rs"), Some(10u32)), // different file
            (PathBuf::from("src/lib.rs"), None),          // no line
        ];

        let indices = filter_findings_by_changed_lines(&findings, &changed, |f| f.clone());
        assert_eq!(indices, vec![0, 2]);
    }

    #[test]
    fn diff_coverage_report_pct() {
        let report = DiffCoverageReport::new("main", 45, 38, 3);
        let expected_pct = 38.0 / 45.0 * 100.0;
        assert!((report.coverage_pct - expected_pct).abs() < 0.01);
        assert_eq!(report.changed_lines, 45);
        assert_eq!(report.covered_lines, 38);
        assert_eq!(report.changed_files, 3);
    }

    #[test]
    fn diff_coverage_report_zero_lines() {
        let report = DiffCoverageReport::new("HEAD~1", 0, 0, 0);
        assert!((report.coverage_pct - 100.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn changed_lines_calls_git_diff() {
        let diff_output = b"diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,0 +5,2 @@\n+fn a() {}\n+fn b() {}\n".to_vec();
        let runner = FixtureRunner::new().on("git", CommandOutput::success(diff_output));
        let changed = changed_lines(&runner, Path::new("/repo"), "main")
            .await
            .unwrap();
        let lib_lines = &changed[&PathBuf::from("src/lib.rs")];
        assert!(lib_lines.contains(&5));
        assert!(lib_lines.contains(&6));
    }
}
