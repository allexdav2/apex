//! Exploration history log — structured record of agent decisions.

use std::collections::HashMap;

/// A single entry in the exploration log.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub iteration: u64,
    pub strategy: String,
    pub branches_found: u64,
    pub action_taken: String,
}

/// Append-only log of exploration decisions for post-hoc analysis.
#[derive(Debug, Clone)]
pub struct ExplorationLog {
    entries: Vec<LogEntry>,
}

impl ExplorationLog {
    pub fn new() -> Self {
        ExplorationLog {
            entries: Vec::new(),
        }
    }

    /// Append a log entry.
    pub fn record(&mut self, entry: LogEntry) {
        self.entries.push(entry);
    }

    /// Number of entries recorded.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the log is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get all entries in chronological order.
    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    /// Total branches found across all entries.
    pub fn total_branches_found(&self) -> u64 {
        self.entries.iter().map(|e| e.branches_found).sum()
    }

    /// Count of iterations per strategy.
    pub fn strategy_summary(&self) -> HashMap<String, u64> {
        let mut counts = HashMap::new();
        for entry in &self.entries {
            *counts.entry(entry.strategy.clone()).or_insert(0) += 1;
        }
        counts
    }
}

impl Default for ExplorationLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_records_entries() {
        let mut log = ExplorationLog::new();
        log.record(LogEntry {
            iteration: 1,
            strategy: "fuzz".into(),
            branches_found: 3,
            action_taken: "normal".into(),
        });
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn log_entries_ordered() {
        let mut log = ExplorationLog::new();
        log.record(LogEntry {
            iteration: 1,
            strategy: "fuzz".into(),
            branches_found: 1,
            action_taken: "normal".into(),
        });
        log.record(LogEntry {
            iteration: 2,
            strategy: "solver".into(),
            branches_found: 5,
            action_taken: "rotate".into(),
        });
        let entries = log.entries();
        assert_eq!(entries[0].iteration, 1);
        assert_eq!(entries[1].iteration, 2);
    }

    #[test]
    fn total_branches_found() {
        let mut log = ExplorationLog::new();
        log.record(LogEntry {
            iteration: 1,
            strategy: "fuzz".into(),
            branches_found: 3,
            action_taken: "normal".into(),
        });
        log.record(LogEntry {
            iteration: 2,
            strategy: "solver".into(),
            branches_found: 7,
            action_taken: "normal".into(),
        });
        assert_eq!(log.total_branches_found(), 10);
    }

    #[test]
    fn strategy_summary() {
        let mut log = ExplorationLog::new();
        for i in 0..5 {
            log.record(LogEntry {
                iteration: i,
                strategy: if i < 3 { "fuzz" } else { "solver" }.into(),
                branches_found: 1,
                action_taken: "normal".into(),
            });
        }
        let summary = log.strategy_summary();
        assert_eq!(summary["fuzz"], 3);
        assert_eq!(summary["solver"], 2);
    }

    #[test]
    fn empty_log() {
        let log = ExplorationLog::new();
        assert_eq!(log.len(), 0);
        assert_eq!(log.total_branches_found(), 0);
        assert!(log.strategy_summary().is_empty());
    }
}
