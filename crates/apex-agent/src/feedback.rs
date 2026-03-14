//! Feedback aggregator — merges strategy outputs into a unified signal.

use apex_core::types::BranchId;
use std::collections::HashSet;

/// Feedback from a single strategy execution round.
#[derive(Debug, Clone)]
pub struct StrategyFeedback {
    pub new_branches: Vec<BranchId>,
    pub best_heuristic: f64,
    pub errors: u32,
}

/// Aggregated summary across all strategies.
#[derive(Debug, Clone)]
pub struct AggregatedSummary {
    pub total_new_branches: usize,
    pub best_heuristic: f64,
    pub total_errors: u32,
    pub strategies: Vec<String>,
}

/// Collects and merges feedback from multiple strategies.
pub struct FeedbackAggregator {
    entries: Vec<(String, StrategyFeedback)>,
}

impl FeedbackAggregator {
    pub fn new() -> Self {
        FeedbackAggregator {
            entries: Vec::new(),
        }
    }

    /// Record feedback from a named strategy.
    pub fn record(&mut self, strategy: &str, feedback: StrategyFeedback) {
        self.entries.push((strategy.to_string(), feedback));
    }

    /// Summarize all recorded feedback with deduplication.
    pub fn summarize(&self) -> AggregatedSummary {
        let mut all_branches: HashSet<BranchId> = HashSet::new();
        let mut best_heuristic: f64 = 0.0;
        let mut total_errors: u32 = 0;
        let mut strategies: Vec<String> = Vec::new();

        for (name, fb) in &self.entries {
            for b in &fb.new_branches {
                all_branches.insert(b.clone());
            }
            if fb.best_heuristic > best_heuristic {
                best_heuristic = fb.best_heuristic;
            }
            total_errors += fb.errors;
            if !strategies.contains(name) {
                strategies.push(name.clone());
            }
        }

        AggregatedSummary {
            total_new_branches: all_branches.len(),
            best_heuristic,
            total_errors,
            strategies,
        }
    }

    /// Clear all recorded feedback.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl Default for FeedbackAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::BranchId;

    #[test]
    fn aggregate_empty() {
        let agg = FeedbackAggregator::new();
        let summary = agg.summarize();
        assert_eq!(summary.total_new_branches, 0);
        assert!(summary.strategies.is_empty());
    }

    #[test]
    fn aggregate_single_strategy() {
        let mut agg = FeedbackAggregator::new();
        agg.record(
            "fuzz",
            StrategyFeedback {
                new_branches: vec![BranchId::new(1, 1, 0, 0)],
                best_heuristic: 0.8,
                errors: 0,
            },
        );
        let summary = agg.summarize();
        assert_eq!(summary.total_new_branches, 1);
        assert_eq!(summary.strategies.len(), 1);
    }

    #[test]
    fn aggregate_multiple_strategies() {
        let mut agg = FeedbackAggregator::new();
        agg.record(
            "fuzz",
            StrategyFeedback {
                new_branches: vec![BranchId::new(1, 1, 0, 0), BranchId::new(1, 2, 0, 0)],
                best_heuristic: 0.6,
                errors: 1,
            },
        );
        agg.record(
            "solver",
            StrategyFeedback {
                new_branches: vec![BranchId::new(1, 3, 0, 0)],
                best_heuristic: 0.95,
                errors: 0,
            },
        );
        let summary = agg.summarize();
        assert_eq!(summary.total_new_branches, 3);
        assert_eq!(summary.best_heuristic, 0.95);
        assert_eq!(summary.total_errors, 1);
    }

    #[test]
    fn aggregate_deduplicates_branches() {
        let mut agg = FeedbackAggregator::new();
        let branch = BranchId::new(1, 1, 0, 0);
        agg.record(
            "fuzz",
            StrategyFeedback {
                new_branches: vec![branch.clone()],
                best_heuristic: 0.5,
                errors: 0,
            },
        );
        agg.record(
            "solver",
            StrategyFeedback {
                new_branches: vec![branch],
                best_heuristic: 0.5,
                errors: 0,
            },
        );
        let summary = agg.summarize();
        // Same branch from both strategies — deduped to 1.
        assert_eq!(summary.total_new_branches, 1);
    }

    #[test]
    fn clear_resets_aggregator() {
        let mut agg = FeedbackAggregator::new();
        agg.record(
            "fuzz",
            StrategyFeedback {
                new_branches: vec![BranchId::new(1, 1, 0, 0)],
                best_heuristic: 0.5,
                errors: 0,
            },
        );
        agg.clear();
        let summary = agg.summarize();
        assert_eq!(summary.total_new_branches, 0);
    }
}
