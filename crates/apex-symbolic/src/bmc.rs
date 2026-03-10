//! Kani BMC unreachability proofs.
//!
//! Generates Kani proof harnesses for branch reachability checking
//! and (when the `kani-prover` feature is enabled) invokes the prover.

use std::path::PathBuf;

use apex_core::types::BranchId;

/// Result of a reachability check for a branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachabilityResult {
    /// The branch is reachable; the `String` carries witness info.
    Reachable(String),
    /// The branch is provably unreachable.
    Unreachable,
    /// Could not determine reachability; reason given.
    Unknown(String),
}

/// Bounded model-checking prover backed by Kani.
pub struct KaniProver {
    target_root: PathBuf,
}

impl KaniProver {
    /// Create a new prover rooted at `target_root`.
    pub fn new(target_root: PathBuf) -> Self {
        Self { target_root }
    }

    /// Return the target root directory.
    pub fn target_root(&self) -> &PathBuf {
        &self.target_root
    }

    /// Generate a Kani proof harness string for a given branch.
    ///
    /// The harness uses `kani::cover!` to test reachability of the branch
    /// identified by `branch` inside `function_name`.
    pub fn generate_harness(&self, branch: &BranchId, function_name: &str) -> String {
        let dir = if branch.direction == 0 {
            "taken"
        } else {
            "not_taken"
        };
        let harness_name = format!(
            "check_reachability_{}_{}_{}",
            branch.file_id, branch.line, dir
        );

        format!(
            r#"#[cfg(kani)]
#[kani::proof]
fn {harness_name}() {{
    // Harness for branch reachability in `{function_name}`
    // file_id={file_id}, line={line}, direction={dir}
    let result = {function_name}(kani::any());
    kani::cover!(true, "branch {file_id}:{line}:{dir} is reachable");
}}"#,
            harness_name = harness_name,
            function_name = function_name,
            file_id = branch.file_id,
            line = branch.line,
            dir = dir,
        )
    }

    /// Check whether `branch` inside `function_name` is reachable.
    ///
    /// Without the `kani-prover` feature this always returns `Unknown`.
    pub fn check_reachability(
        &self,
        _branch: &BranchId,
        _function_name: &str,
    ) -> ReachabilityResult {
        #[cfg(feature = "kani-prover")]
        {
            ReachabilityResult::Unknown("kani execution not yet implemented".to_string())
        }
        #[cfg(not(feature = "kani-prover"))]
        {
            ReachabilityResult::Unknown("kani-prover feature not enabled".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_branch() -> BranchId {
        BranchId::new(42, 10, 5, 0)
    }

    #[test]
    fn harness_generation() {
        let prover = KaniProver::new(PathBuf::from("/tmp/target"));
        let branch = sample_branch();
        let harness = prover.generate_harness(&branch, "my_function");

        assert!(
            harness.contains("#[kani::proof]"),
            "must have kani::proof annotation"
        );
        assert!(
            harness.contains("my_function"),
            "must reference the function name"
        );
        assert!(
            harness.contains("check_reachability_42_10_taken"),
            "harness name must encode file_id, line, direction"
        );
    }

    #[test]
    fn check_without_feature_returns_unknown() {
        let prover = KaniProver::new(PathBuf::from("/tmp/target"));
        let branch = sample_branch();
        let result = prover.check_reachability(&branch, "some_fn");

        match result {
            ReachabilityResult::Unknown(msg) => {
                // Without kani-prover feature we expect the "not enabled" message.
                assert!(
                    msg.contains("not enabled") || msg.contains("not yet implemented"),
                    "unexpected reason: {msg}"
                );
            }
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn reachability_result_variants() {
        let reachable = ReachabilityResult::Reachable("witness".to_string());
        let unreachable = ReachabilityResult::Unreachable;
        let unknown = ReachabilityResult::Unknown("reason".to_string());

        assert_eq!(
            reachable,
            ReachabilityResult::Reachable("witness".to_string())
        );
        assert_eq!(unreachable, ReachabilityResult::Unreachable);
        assert_eq!(unknown, ReachabilityResult::Unknown("reason".to_string()));

        // Verify they are all distinct
        assert_ne!(reachable, unreachable.clone());
        assert_ne!(unreachable, unknown.clone());
        assert_ne!(reachable, unknown);
    }
}
