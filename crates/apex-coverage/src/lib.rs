//! Coverage tracking for APEX — bitmap-based edge coverage oracle with delta computation.

pub mod oracle;

pub use oracle::{CoverageOracle, DeltaCoverage};

mod heuristic;
pub use heuristic::{branch_distance_eq, branch_distance_gt, branch_distance_lt, BranchHeuristic};
