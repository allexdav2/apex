//! Coverage delta tracker — diffs two branch lists to find newly covered branches.

use apex_core::types::BranchId;
use std::collections::HashSet;

/// Compute the set of branches in `after` that are not in `before`.
pub fn coverage_delta(before: &[BranchId], after: &[BranchId]) -> Vec<BranchId> {
    let before_set: HashSet<_> = before.iter().collect();
    after
        .iter()
        .filter(|b| !before_set.contains(b))
        .cloned()
        .collect()
}

/// Format a human-readable summary of a coverage delta.
pub fn format_delta_summary(delta: &[BranchId]) -> String {
    if delta.is_empty() {
        "No new branches covered.".to_string()
    } else {
        format!(
            "{} new branch{} covered.",
            delta.len(),
            if delta.len() == 1 { "" } else { "es" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::BranchId;

    #[test]
    fn delta_empty_when_same() {
        let before = vec![BranchId::new(1, 1, 0, 0), BranchId::new(1, 2, 0, 0)];
        let after = vec![BranchId::new(1, 1, 0, 0), BranchId::new(1, 2, 0, 0)];
        let delta = coverage_delta(&before, &after);
        assert!(delta.is_empty());
    }

    #[test]
    fn delta_reports_new_branches() {
        let before = vec![BranchId::new(1, 1, 0, 0)];
        let after = vec![
            BranchId::new(1, 1, 0, 0),
            BranchId::new(1, 2, 0, 0),
            BranchId::new(1, 3, 0, 0),
        ];
        let delta = coverage_delta(&before, &after);
        assert_eq!(delta.len(), 2);
    }

    #[test]
    fn delta_ignores_removed_branches() {
        let before = vec![BranchId::new(1, 1, 0, 0), BranchId::new(1, 2, 0, 0)];
        let after = vec![BranchId::new(1, 1, 0, 0)];
        let delta = coverage_delta(&before, &after);
        assert!(delta.is_empty());
    }

    #[test]
    fn delta_from_empty_baseline() {
        let before: Vec<BranchId> = vec![];
        let after = vec![BranchId::new(1, 5, 0, 0)];
        let delta = coverage_delta(&before, &after);
        assert_eq!(delta.len(), 1);
    }

    #[test]
    fn delta_summary_formats_correctly() {
        let delta = vec![BranchId::new(1, 10, 0, 0), BranchId::new(2, 20, 0, 0)];
        let summary = format_delta_summary(&delta);
        assert!(summary.contains("2 new branch"));
    }
}
