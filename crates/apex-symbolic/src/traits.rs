//! Solver trait abstraction for SMT backends.

use apex_core::{error::Result, types::InputSeed};

/// Which SMT logic to set on the solver. Guides solver heuristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolverLogic {
    /// Quantifier-free linear integer arithmetic (Python targets).
    QfLia,
    /// Quantifier-free arrays + bitvectors (C/Rust compiled targets).
    QfAbv,
    /// Quantifier-free strings (JavaScript/web targets).
    QfS,
    /// Let the solver auto-detect (default).
    Auto,
}

/// Abstraction over SMT solver backends (Z3, Bitwuzla, CVC5, etc.).
pub trait Solver: Send + Sync {
    /// Solve a constraint set. If `negate_last` is true, negate the final constraint
    /// to find an input that takes the opposite branch.
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>>;

    /// Solve multiple constraint sets in one batch. Default implementation
    /// calls `solve()` for each set. Backends can override for efficiency.
    fn solve_batch(
        &self,
        sets: &[Vec<String>],
        negate_last: bool,
    ) -> Vec<Result<Option<InputSeed>>> {
        sets.iter().map(|cs| self.solve(cs, negate_last)).collect()
    }

    /// Set the SMT logic for this solver instance.
    fn set_logic(&mut self, logic: SolverLogic);

    /// Human-readable name for logging.
    fn name(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solver_logic_debug() {
        assert_eq!(format!("{:?}", SolverLogic::QfLia), "QfLia");
        assert_eq!(format!("{:?}", SolverLogic::QfAbv), "QfAbv");
        assert_eq!(format!("{:?}", SolverLogic::QfS), "QfS");
        assert_eq!(format!("{:?}", SolverLogic::Auto), "Auto");
    }

    #[test]
    fn solver_logic_eq() {
        assert_eq!(SolverLogic::QfLia, SolverLogic::QfLia);
        assert_ne!(SolverLogic::QfLia, SolverLogic::QfAbv);
    }
}
