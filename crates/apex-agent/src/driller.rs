//! Driller strategy — symbolic execution to bypass hard-to-fuzz branches.
//!
//! Activated by the orchestrator when the fuzzer stalls. Collects path
//! constraints from recent executions and negates frontier branches to
//! generate coverage-unlocking seeds.

use apex_core::{
    error::Result,
    traits::Strategy,
    types::{ExecutionResult, ExplorationContext, InputSeed, PathConstraint, SeedOrigin},
};
use apex_symbolic::traits::Solver;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// Driller-style symbolic execution strategy.
///
/// When the fuzzer stalls, the orchestrator rotates to this strategy.
/// It collects path constraints from recent executions, negates frontier
/// branches (those targeting uncovered branches), and uses an SMT solver
/// to generate inputs that pass hard-to-fuzz conditions.
pub struct DrillerStrategy {
    solver: Arc<Mutex<dyn Solver>>,
    /// Path constraints collected from traced executions.
    constraints: Mutex<Vec<PathConstraint>>,
    /// Maximum number of constraints to solve per invocation.
    max_constraints: usize,
}

impl DrillerStrategy {
    pub fn new(solver: Arc<Mutex<dyn Solver>>, max_constraints: usize) -> Self {
        Self {
            solver,
            constraints: Mutex::new(Vec::new()),
            max_constraints,
        }
    }

    /// Record path constraints from a traced execution.
    /// Called by the orchestrator after concolic/traced runs.
    pub fn record_constraints(&self, new_constraints: Vec<PathConstraint>) {
        let mut cs = self.constraints.lock().unwrap_or_else(|e| e.into_inner());
        cs.extend(new_constraints);
    }
}

#[async_trait]
impl Strategy for DrillerStrategy {
    fn name(&self) -> &str {
        "driller"
    }

    async fn suggest_inputs(&self, ctx: &ExplorationContext) -> Result<Vec<InputSeed>> {
        let constraints = self
            .constraints
            .lock()
            .map_err(|e| apex_core::error::ApexError::Agent(format!("mutex poisoned: {e}")))?
            .clone();
        if constraints.is_empty() {
            return Ok(Vec::new());
        }

        // Build set of uncovered branch IDs for quick lookup.
        let uncovered: std::collections::HashSet<_> = ctx.uncovered_branches.iter().collect();

        // Find constraints whose branch is still uncovered — these are
        // the frontier branches where negation could unlock new coverage.
        let frontier: Vec<_> = constraints
            .iter()
            .filter(|pc| uncovered.contains(&pc.branch))
            .take(self.max_constraints)
            .collect();

        let mut inputs = Vec::new();
        let solver = self.solver.lock().map_err(|e| {
            apex_core::error::ApexError::Agent(format!("solver mutex poisoned: {e}"))
        })?;

        for pc in &frontier {
            // Build constraint prefix up to this branch, then negate.
            let prefix: Vec<String> = constraints
                .iter()
                .take_while(|c| c.branch != pc.branch)
                .map(|c| c.smtlib2.clone())
                .chain(std::iter::once(pc.smtlib2.clone()))
                .collect();

            if let Ok(Some(seed)) = solver.solve(&prefix, true) {
                inputs.push(InputSeed::new(seed.data.to_vec(), SeedOrigin::Symbolic));
            }
        }

        Ok(inputs)
    }

    async fn observe(&self, _result: &ExecutionResult) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::{BranchId, Language, Target};

    fn make_ctx(uncovered: Vec<BranchId>) -> ExplorationContext {
        ExplorationContext {
            target: Target {
                root: std::path::PathBuf::from("/tmp"),
                language: Language::Rust,
                test_command: vec![],
            },
            uncovered_branches: uncovered,
            iteration: 100,
        }
    }

    /// Stub solver that returns a fixed input when constraints are solvable.
    struct StubSolver {
        result: Option<InputSeed>,
    }

    impl StubSolver {
        fn solvable() -> Self {
            StubSolver {
                result: Some(InputSeed::new(b"solved".to_vec(), SeedOrigin::Symbolic)),
            }
        }

        fn unsolvable() -> Self {
            StubSolver { result: None }
        }
    }

    impl Solver for StubSolver {
        fn solve(&self, _constraints: &[String], _negate_last: bool) -> Result<Option<InputSeed>> {
            Ok(self.result.clone())
        }
        fn set_logic(&mut self, _logic: apex_symbolic::traits::SolverLogic) {}
        fn name(&self) -> &str {
            "stub"
        }
    }

    #[test]
    fn driller_strategy_name() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);
        assert_eq!(driller.name(), "driller");
    }

    #[tokio::test]
    async fn suggest_inputs_with_no_constraints_returns_empty() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);
        let ctx = make_ctx(vec![BranchId::new(1, 10, 0, 0)]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        // No constraints recorded yet — nothing to solve
        assert!(inputs.is_empty());
    }

    #[tokio::test]
    async fn suggest_inputs_solves_recorded_constraints() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);

        // Record some path constraints
        let branch = BranchId::new(1, 10, 0, 0);
        driller.record_constraints(vec![PathConstraint {
            branch: branch.clone(),
            smtlib2: "(assert (> x 0))".into(),
            direction_taken: true,
        }]);

        let ctx = make_ctx(vec![branch]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].origin, SeedOrigin::Symbolic);
        assert_eq!(inputs[0].data, b"solved".as_ref());
    }

    #[tokio::test]
    async fn suggest_inputs_unsolvable_returns_empty() {
        let solver = Arc::new(Mutex::new(StubSolver::unsolvable()));
        let driller = DrillerStrategy::new(solver, 10);

        let branch = BranchId::new(1, 10, 0, 0);
        driller.record_constraints(vec![PathConstraint {
            branch: branch.clone(),
            smtlib2: "(assert false)".into(),
            direction_taken: true,
        }]);

        let ctx = make_ctx(vec![branch]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        assert!(inputs.is_empty());
    }

    #[tokio::test]
    async fn suggest_inputs_respects_max_constraints() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 2); // max 2

        let mut constraints = Vec::new();
        for i in 0..5 {
            constraints.push(PathConstraint {
                branch: BranchId::new(1, i as u32, 0, 0),
                smtlib2: format!("(assert (> x {i}))"),
                direction_taken: true,
            });
        }
        driller.record_constraints(constraints);

        let ctx = make_ctx(vec![
            BranchId::new(1, 0, 0, 0),
            BranchId::new(1, 1, 0, 0),
            BranchId::new(1, 2, 0, 0),
            BranchId::new(1, 3, 0, 0),
            BranchId::new(1, 4, 0, 0),
        ]);
        let inputs = driller.suggest_inputs(&ctx).await.unwrap();
        // Should solve at most max_constraints (2)
        assert!(inputs.len() <= 2);
    }

    #[tokio::test]
    async fn observe_is_noop() {
        let solver = Arc::new(Mutex::new(StubSolver::solvable()));
        let driller = DrillerStrategy::new(solver, 10);
        let result = ExecutionResult {
            seed_id: apex_core::types::SeedId::new(),
            status: apex_core::types::ExecutionStatus::Pass,
            new_branches: vec![],
            trace: None,
            duration_ms: 5,
            stdout: String::new(),
            stderr: String::new(),
        };
        assert!(driller.observe(&result).await.is_ok());
    }
}
