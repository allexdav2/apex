//! Test case prioritization via rank aggregation of multiple signals.
//! Based on arXiv:2412.00015 — uses Borda count over diverse rankers.

use crate::types::TestTrace;
use std::collections::HashMap;

/// A ranker produces a descending-score ordering of tests.
pub trait TestRanker: Send + Sync {
    fn rank(&self, tests: &[TestTrace]) -> Vec<(String, f64)>;
    fn name(&self) -> &str;
}

/// Ranks tests by number of branches covered (more = higher score).
pub struct CoverageRanker;

impl TestRanker for CoverageRanker {
    fn rank(&self, tests: &[TestTrace]) -> Vec<(String, f64)> {
        let mut ranked: Vec<(String, f64)> = tests
            .iter()
            .map(|t| (t.test_name.clone(), t.branches.len() as f64))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
    }

    fn name(&self) -> &str {
        "coverage"
    }
}

/// Ranks tests by speed (faster = higher score, using 1/duration).
pub struct SpeedRanker;

impl TestRanker for SpeedRanker {
    fn rank(&self, tests: &[TestTrace]) -> Vec<(String, f64)> {
        let mut ranked: Vec<(String, f64)> = tests
            .iter()
            .map(|t| {
                let score = if t.duration_ms == 0 {
                    f64::MAX
                } else {
                    1.0 / t.duration_ms as f64
                };
                (t.test_name.clone(), score)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
    }

    fn name(&self) -> &str {
        "speed"
    }
}

/// Aggregate multiple rankings using Borda count.
///
/// Each ranker's output is treated as a ranked list. Position `i` in a list of
/// `n` items receives a Borda score of `n - i`. Scores are summed across
/// rankings and the final list is sorted descending.
pub fn borda_aggregate(rankings: &[Vec<(String, f64)>]) -> Vec<(String, f64)> {
    if rankings.is_empty() {
        return vec![];
    }

    let mut scores: HashMap<String, f64> = HashMap::new();

    for ranking in rankings {
        let n = ranking.len() as f64;
        for (i, (name, _)) in ranking.iter().enumerate() {
            *scores.entry(name.clone()).or_insert(0.0) += n - i as f64;
        }
    }

    let mut result: Vec<(String, f64)> = scores.into_iter().collect();
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    result
}

/// Combines multiple TestRankers via Borda aggregation.
pub struct TestPrioritizer {
    pub rankers: Vec<Box<dyn TestRanker>>,
}

impl TestPrioritizer {
    pub fn new(rankers: Vec<Box<dyn TestRanker>>) -> Self {
        TestPrioritizer { rankers }
    }

    pub fn prioritize(&self, tests: &[TestTrace]) -> Vec<(String, f64)> {
        let rankings: Vec<Vec<(String, f64)>> =
            self.rankers.iter().map(|r| r.rank(tests)).collect();
        borda_aggregate(&rankings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::ExecutionStatus;

    fn make_trace(name: &str, duration: u64, branch_count: usize) -> TestTrace {
        TestTrace {
            test_name: name.to_string(),
            branches: (0..branch_count as u32)
                .map(|l| apex_core::types::BranchId::new(1, l, 0, 0))
                .collect(),
            duration_ms: duration,
            status: ExecutionStatus::Pass,
        }
    }

    #[test]
    fn borda_aggregate_single_ranking() {
        let rankings = vec![vec![
            ("test_a".to_string(), 3.0),
            ("test_b".to_string(), 2.0),
            ("test_c".to_string(), 1.0),
        ]];
        let result = borda_aggregate(&rankings);
        assert_eq!(result[0].0, "test_a");
        assert_eq!(result[1].0, "test_b");
        assert_eq!(result[2].0, "test_c");
    }

    #[test]
    fn borda_aggregate_two_rankings_winner() {
        // b is ranked first in both rankings => highest total Borda score
        let r1 = vec![
            ("b".to_string(), 3.0),
            ("a".to_string(), 2.0),
            ("c".to_string(), 1.0),
        ];
        let r2 = vec![
            ("b".to_string(), 3.0),
            ("c".to_string(), 2.0),
            ("a".to_string(), 1.0),
        ];
        let result = borda_aggregate(&[r1, r2]);
        // b gets rank 0 in both (n-0=3 points each) => 6 total, higher than a or c
        assert_eq!(result[0].0, "b");
    }

    #[test]
    fn borda_aggregate_empty() {
        let result = borda_aggregate(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn coverage_ranker_sorts_by_branch_count() {
        let traces = vec![
            make_trace("few", 10, 2),
            make_trace("many", 10, 10),
            make_trace("mid", 10, 5),
        ];
        let ranker = CoverageRanker;
        let ranked = ranker.rank(&traces);
        assert_eq!(ranked[0].0, "many");
        assert_eq!(ranked[1].0, "mid");
        assert_eq!(ranked[2].0, "few");
    }

    #[test]
    fn speed_ranker_sorts_by_duration() {
        let traces = vec![
            make_trace("slow", 1000, 5),
            make_trace("fast", 10, 5),
            make_trace("mid", 100, 5),
        ];
        let ranker = SpeedRanker;
        let ranked = ranker.rank(&traces);
        // Faster tests should rank higher (lower duration = higher score)
        assert_eq!(ranked[0].0, "fast");
        assert_eq!(ranked[1].0, "mid");
        assert_eq!(ranked[2].0, "slow");
    }

    #[test]
    fn test_prioritizer_combines_rankers() {
        let traces = vec![
            make_trace("fast_low", 10, 2),
            make_trace("slow_high", 1000, 10),
            make_trace("mid_mid", 100, 5),
        ];
        let prioritizer = TestPrioritizer {
            rankers: vec![Box::new(CoverageRanker), Box::new(SpeedRanker)],
        };
        let result = prioritizer.prioritize(&traces);
        // mid_mid should be a reasonable middle ground
        assert_eq!(result.len(), 3);
        // All test names present
        let names: Vec<&str> = result.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"fast_low"));
        assert!(names.contains(&"slow_high"));
        assert!(names.contains(&"mid_mid"));
    }
}
