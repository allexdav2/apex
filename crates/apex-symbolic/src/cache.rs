//! Caching wrapper for any Solver implementation.

use crate::traits::{Solver, SolverLogic};
use apex_core::{error::Result, types::InputSeed};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

pub struct CachingSolver<S: Solver> {
    inner: S,
    cache: Mutex<HashMap<u64, Option<InputSeed>>>,
    hits: Mutex<u64>,
    misses: Mutex<u64>,
}

impl<S: Solver> CachingSolver<S> {
    pub fn new(inner: S) -> Self {
        CachingSolver {
            inner,
            cache: Mutex::new(HashMap::new()),
            hits: Mutex::new(0),
            misses: Mutex::new(0),
        }
    }

    pub fn hit_count(&self) -> u64 {
        *self.hits.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn miss_count(&self) -> u64 {
        *self.misses.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn cache_key(constraints: &[String], negate_last: bool) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        for c in constraints {
            c.hash(&mut hasher);
        }
        negate_last.hash(&mut hasher);
        hasher.finish()
    }
}

impl<S: Solver> Solver for CachingSolver<S> {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        let key = Self::cache_key(constraints, negate_last);
        if let Some(cached) = self
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            *self.hits.lock().unwrap_or_else(|e| e.into_inner()) += 1;
            return Ok(cached.clone());
        }
        *self.misses.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        let result = self.inner.solve(constraints, negate_last)?;
        self.cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key, result.clone());
        Ok(result)
    }

    fn set_logic(&mut self, logic: SolverLogic) {
        self.inner.set_logic(logic);
    }

    fn name(&self) -> &str {
        "caching"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CountingSolver {
        calls: Mutex<u64>,
    }

    impl CountingSolver {
        fn new() -> Self {
            CountingSolver {
                calls: Mutex::new(0),
            }
        }
        fn call_count(&self) -> u64 {
            *self.calls.lock().unwrap()
        }
    }

    impl Solver for CountingSolver {
        fn solve(&self, _constraints: &[String], _negate_last: bool) -> Result<Option<InputSeed>> {
            *self.calls.lock().unwrap() += 1;
            Ok(None)
        }
        fn set_logic(&mut self, _logic: SolverLogic) {}
        fn name(&self) -> &str {
            "counting"
        }
    }

    #[test]
    fn cache_hit_avoids_inner_call() {
        let inner = CountingSolver::new();
        let solver = CachingSolver::new(inner);
        let constraints = vec!["(> x 0)".into()];
        let _ = solver.solve(&constraints, false);
        let _ = solver.solve(&constraints, false);
        assert_eq!(solver.hit_count(), 1);
        assert_eq!(solver.miss_count(), 1);
        assert_eq!(solver.inner.call_count(), 1);
    }

    #[test]
    fn different_constraints_are_separate_keys() {
        let inner = CountingSolver::new();
        let solver = CachingSolver::new(inner);
        let _ = solver.solve(&["(> x 0)".into()], false);
        let _ = solver.solve(&["(< y 5)".into()], false);
        assert_eq!(solver.hit_count(), 0);
        assert_eq!(solver.miss_count(), 2);
    }

    #[test]
    fn negate_last_changes_key() {
        let inner = CountingSolver::new();
        let solver = CachingSolver::new(inner);
        let constraints = vec!["(> x 0)".into()];
        let _ = solver.solve(&constraints, false);
        let _ = solver.solve(&constraints, true);
        assert_eq!(solver.miss_count(), 2);
    }

    #[test]
    fn set_logic_delegates() {
        let inner = CountingSolver::new();
        let mut solver = CachingSolver::new(inner);
        solver.set_logic(SolverLogic::QfLia);
    }

    #[test]
    fn name_returns_caching() {
        let inner = CountingSolver::new();
        let solver = CachingSolver::new(inner);
        assert_eq!(solver.name(), "caching");
    }
}
