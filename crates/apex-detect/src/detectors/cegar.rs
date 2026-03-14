//! CEGAR-based specification refinement.
//! Based on the SmCon paper — iteratively refines specifications
//! using counterexample-guided abstraction refinement.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// A specification refined through CEGAR iterations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CegarSpec {
    pub function_name: String,
    pub allowed: HashSet<String>,
    pub violations: HashSet<String>,
    pub iteration: u32,
    /// Track iteration numbers when violations were last added.
    last_violation_iteration: u32,
}

impl CegarSpec {
    pub fn new(function_name: &str) -> Self {
        CegarSpec {
            function_name: function_name.to_string(),
            allowed: HashSet::new(),
            violations: HashSet::new(),
            iteration: 0,
            last_violation_iteration: 0,
        }
    }

    /// Refine the spec with a counterexample.
    ///
    /// If `is_genuine` is true, the counterexample is a real violation.
    /// If false, it was spurious and should be added to the allowed set.
    pub fn refine(&mut self, counterexample: &str, is_genuine: bool) {
        self.iteration += 1;
        if is_genuine {
            self.violations.insert(counterexample.to_string());
            self.allowed.remove(counterexample);
            self.last_violation_iteration = self.iteration;
        } else {
            self.allowed.insert(counterexample.to_string());
        }
    }

    /// Check if the spec has converged (no new violations for `patience` iterations).
    pub fn is_converged(&self, patience: u32) -> bool {
        if self.iteration == 0 {
            return true; // no refinements attempted
        }
        self.iteration - self.last_violation_iteration >= patience
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_refinement_creation() {
        let spec = CegarSpec::new("validate_input");
        assert_eq!(spec.function_name, "validate_input");
        assert_eq!(spec.iteration, 0);
    }

    #[test]
    fn refine_adds_counterexample() {
        let mut spec = CegarSpec::new("f");
        spec.allowed.insert("normal_call".to_string());

        let counterexample = "dangerous_call".to_string();
        let is_genuine = true; // confirmed as a real violation
        spec.refine(&counterexample, is_genuine);

        assert_eq!(spec.iteration, 1);
        assert!(spec.violations.contains(&counterexample));
        assert!(!spec.allowed.contains(&counterexample));
    }

    #[test]
    fn refine_spurious_counterexample_adds_to_allowed() {
        let mut spec = CegarSpec::new("f");

        let counterexample = "actually_safe_call".to_string();
        let is_genuine = false; // spurious — add to allowed
        spec.refine(&counterexample, is_genuine);

        assert_eq!(spec.iteration, 1);
        assert!(spec.allowed.contains(&counterexample));
        assert!(!spec.violations.contains(&counterexample));
    }

    #[test]
    fn is_converged_after_no_new_violations() {
        let mut spec = CegarSpec::new("f");
        spec.allowed.insert("a".to_string());
        // No refinements => converged
        assert!(spec.is_converged(3));
    }

    #[test]
    fn not_converged_with_recent_refinements() {
        let mut spec = CegarSpec::new("f");
        spec.refine(&"bad".to_string(), true);
        assert!(!spec.is_converged(3));
    }
}
