//! Strategy rotation policy for the orchestrator.
//!
//! Codifies escalation thresholds: after N stalled iterations, rotate to the
//! next strategy in the configured order.

/// Policy governing when and how to rotate between exploration strategies.
#[derive(Debug, Clone)]
pub struct RotationPolicy {
    strategies: Vec<String>,
    current_index: usize,
    stall_threshold: u64,
}

impl RotationPolicy {
    /// Create a new rotation policy with the given strategy order.
    pub fn new(strategies: Vec<String>) -> Self {
        RotationPolicy {
            strategies,
            current_index: 0,
            stall_threshold: 5,
        }
    }

    /// Get the name of the currently active strategy.
    pub fn current(&self) -> &str {
        &self.strategies[self.current_index]
    }

    /// Advance to the next strategy in round-robin order.
    pub fn rotate(&mut self) {
        self.current_index = (self.current_index + 1) % self.strategies.len();
    }

    /// Check whether rotation is warranted given the current stall count.
    pub fn should_rotate(&self, stall_iterations: u64) -> bool {
        stall_iterations >= self.stall_threshold
    }

    /// Set the stall threshold (number of iterations without progress before rotating).
    pub fn set_stall_threshold(&mut self, threshold: u64) {
        self.stall_threshold = threshold;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_policy_starts_with_fuzz() {
        let policy = RotationPolicy::new(vec!["fuzz".into(), "solver".into(), "llm".into()]);
        assert_eq!(policy.current(), "fuzz");
    }

    #[test]
    fn rotate_advances_to_next() {
        let mut policy = RotationPolicy::new(vec!["fuzz".into(), "solver".into()]);
        policy.rotate();
        assert_eq!(policy.current(), "solver");
    }

    #[test]
    fn rotate_wraps_around() {
        let mut policy = RotationPolicy::new(vec!["a".into(), "b".into()]);
        policy.rotate();
        policy.rotate();
        assert_eq!(policy.current(), "a");
    }

    #[test]
    fn should_rotate_after_stall() {
        let policy = RotationPolicy::new(vec!["fuzz".into(), "solver".into()]);
        assert!(!policy.should_rotate(0));
        assert!(!policy.should_rotate(4));
        assert!(policy.should_rotate(10));
    }

    #[test]
    fn custom_stall_threshold() {
        let mut policy = RotationPolicy::new(vec!["a".into(), "b".into()]);
        policy.set_stall_threshold(3);
        assert!(!policy.should_rotate(2));
        assert!(policy.should_rotate(3));
    }

    #[test]
    fn single_strategy_wraps() {
        let mut policy = RotationPolicy::new(vec!["only".into()]);
        policy.rotate();
        assert_eq!(policy.current(), "only");
    }
}
