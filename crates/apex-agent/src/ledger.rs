//! Thread-safe, dedup-aware bug accumulator.
//!
//! The `BugLedger` collects [`BugReport`]s discovered during an exploration run,
//! deduplicating by `(class, location)` so that the same crash from different
//! inputs is only reported once.

use apex_core::types::{BugClass, BugReport, BugSummary, ExecutionResult};
use std::collections::HashSet;
use std::sync::Mutex;

/// Accumulates bugs found during exploration, deduplicating by class + location.
pub struct BugLedger {
    reports: Mutex<Vec<BugReport>>,
    seen: Mutex<HashSet<String>>,
}

impl BugLedger {
    pub fn new() -> Self {
        BugLedger {
            reports: Mutex::new(Vec::new()),
            seen: Mutex::new(HashSet::new()),
        }
    }

    /// Record a bug from an execution result, if the result represents a bug
    /// and hasn't been seen before. Returns `true` if a new bug was recorded.
    pub fn record_from_result(&self, result: &ExecutionResult, iteration: u64) -> bool {
        let class = match BugClass::from_status(result.status) {
            Some(c) => c,
            None => return false,
        };

        let mut report = BugReport::new(class, result.seed_id, result.stderr.clone());
        report.triggering_branches = result.new_branches.clone();
        report.discovered_at_iteration = iteration;

        // Try to extract location from stderr (first file:line pattern).
        report.location = extract_location(&result.stderr);

        self.record(report)
    }

    /// Record a pre-built bug report. Returns `true` if it was new (not a duplicate).
    pub fn record(&self, report: BugReport) -> bool {
        let key = report.dedup_key();
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        if seen.contains(&key) {
            return false;
        }
        seen.insert(key);
        drop(seen);

        self.reports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(report);
        true
    }

    /// Number of unique bugs recorded.
    pub fn count(&self) -> usize {
        self.reports.lock().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Build a summary of all recorded bugs.
    pub fn summary(&self) -> BugSummary {
        let reports = self
            .reports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        BugSummary::new(reports)
    }

    /// Get all reports (cloned).
    pub fn reports(&self) -> Vec<BugReport> {
        self.reports
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl Default for BugLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract first `file:line` pattern from stderr text.
fn extract_location(stderr: &str) -> Option<String> {
    // Common patterns:
    //   File "foo.py", line 42
    //   src/main.rs:42:5
    //   at foo.js:10:3
    for line in stderr.lines() {
        let trimmed = line.trim();
        // Python-style: File "path", line N
        if let Some(rest) = trimmed.strip_prefix("File \"") {
            if let Some(end_quote) = rest.find('"') {
                let path = &rest[..end_quote];
                if let Some(line_part) = rest.get(end_quote + 1..) {
                    if let Some(num_start) = line_part.find("line ") {
                        let num_str = &line_part[num_start + 5..];
                        let num: String =
                            num_str.chars().take_while(|c| c.is_ascii_digit()).collect();
                        if !num.is_empty() {
                            return Some(format!("{path}:{num}"));
                        }
                    }
                }
            }
        }
        // Rust/JS-style: path:line or path:line:col
        // Scan whitespace-delimited tokens for "path.ext:line" patterns.
        for token in trimmed.split_whitespace() {
            // Strip leading/trailing parens: "(foo.rs:10)" → "foo.rs:10"
            let token = token.trim_matches(|c| c == '(' || c == ')' || c == ',');
            if let Some(colon_pos) = token.find(':') {
                let before = &token[..colon_pos];
                let after = &token[colon_pos + 1..];
                if (before.contains('.') || before.contains('/')) && before.len() > 1 {
                    let line_num: String =
                        after.chars().take_while(|c| c.is_ascii_digit()).collect();
                    if !line_num.is_empty() {
                        return Some(format!("{before}:{line_num}"));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{ExecutionStatus, SeedId};

    fn make_result(status: ExecutionStatus, stderr: &str) -> ExecutionResult {
        ExecutionResult {
            seed_id: SeedId::new(),
            status,
            new_branches: vec![],
            trace: None,
            duration_ms: 100,
            stdout: String::new(),
            stderr: stderr.into(),
        }
    }

    #[test]
    fn record_crash() {
        let ledger = BugLedger::new();
        let result = make_result(ExecutionStatus::Crash, "segfault at src/main.rs:42");
        assert!(ledger.record_from_result(&result, 0));
        assert_eq!(ledger.count(), 1);
    }

    #[test]
    fn skip_pass() {
        let ledger = BugLedger::new();
        let result = make_result(ExecutionStatus::Pass, "");
        assert!(!ledger.record_from_result(&result, 0));
        assert_eq!(ledger.count(), 0);
    }

    #[test]
    fn dedup_same_location() {
        let ledger = BugLedger::new();
        let r1 = make_result(ExecutionStatus::Crash, "segfault at src/main.rs:42");
        let r2 = make_result(ExecutionStatus::Crash, "segfault at src/main.rs:42");
        assert!(ledger.record_from_result(&r1, 0));
        assert!(!ledger.record_from_result(&r2, 1));
        assert_eq!(ledger.count(), 1);
    }

    #[test]
    fn different_locations_recorded() {
        let ledger = BugLedger::new();
        let r1 = make_result(ExecutionStatus::Crash, "src/a.rs:10");
        let r2 = make_result(ExecutionStatus::Crash, "src/b.rs:20");
        assert!(ledger.record_from_result(&r1, 0));
        assert!(ledger.record_from_result(&r2, 1));
        assert_eq!(ledger.count(), 2);
    }

    #[test]
    fn different_classes_not_deduped() {
        let ledger = BugLedger::new();
        let r1 = make_result(ExecutionStatus::Crash, "src/main.rs:42");
        let r2 = make_result(ExecutionStatus::Timeout, "src/main.rs:42");
        assert!(ledger.record_from_result(&r1, 0));
        assert!(ledger.record_from_result(&r2, 1));
        assert_eq!(ledger.count(), 2);
    }

    #[test]
    fn summary_aggregation() {
        let ledger = BugLedger::new();
        ledger.record_from_result(&make_result(ExecutionStatus::Crash, "src/a.rs:1"), 0);
        ledger.record_from_result(&make_result(ExecutionStatus::Crash, "src/b.rs:2"), 1);
        ledger.record_from_result(&make_result(ExecutionStatus::Timeout, "src/c.rs:3"), 2);

        let summary = ledger.summary();
        assert_eq!(summary.total, 3);
        assert_eq!(summary.by_class["crash"], 2);
        assert_eq!(summary.by_class["timeout"], 1);
    }

    #[test]
    fn record_fail_as_assertion_failure() {
        let ledger = BugLedger::new();
        let result = make_result(ExecutionStatus::Fail, "assert failed");
        assert!(ledger.record_from_result(&result, 5));
        let reports = ledger.reports();
        assert_eq!(reports[0].class, BugClass::AssertionFailure);
        assert_eq!(reports[0].discovered_at_iteration, 5);
    }

    #[test]
    fn record_oom() {
        let ledger = BugLedger::new();
        let result = make_result(ExecutionStatus::OomKill, "killed");
        assert!(ledger.record_from_result(&result, 0));
        assert_eq!(ledger.reports()[0].class, BugClass::OomKill);
    }

    #[test]
    fn extract_location_python() {
        let loc = extract_location("Traceback:\n  File \"foo.py\", line 42, in test\n    x()");
        assert_eq!(loc.as_deref(), Some("foo.py:42"));
    }

    #[test]
    fn extract_location_rust() {
        let loc = extract_location("thread panicked at src/main.rs:42:5");
        assert_eq!(loc.as_deref(), Some("src/main.rs:42"));
    }

    #[test]
    fn extract_location_js() {
        let loc = extract_location("    at Object.<anonymous> (test.js:10:3)");
        assert_eq!(loc.as_deref(), Some("test.js:10"));

        let loc2 = extract_location("test.js:10:3");
        assert_eq!(loc2.as_deref(), Some("test.js:10"));
    }

    #[test]
    fn extract_location_none() {
        assert_eq!(extract_location("no location info here"), None);
        assert_eq!(extract_location(""), None);
    }

    #[test]
    fn default_impl() {
        let ledger = BugLedger::default();
        assert_eq!(ledger.count(), 0);
    }

    #[test]
    fn manual_record() {
        let ledger = BugLedger::new();
        let report = BugReport::new(BugClass::Crash, SeedId::new(), "boom".into());
        assert!(ledger.record(report.clone()));
        // Duplicate with same dedup key
        assert!(!ledger.record(report));
    }
}
