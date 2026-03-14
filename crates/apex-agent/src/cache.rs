use std::collections::HashMap;

/// Cached satisfiability result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SatResult {
    Sat,
    Unsat,
    Unknown,
}

/// Cache satisfiability results with negation inference.
/// If ¬C is cached as UNSAT, infer C is SAT without querying solver.
pub struct SolverCache {
    cache: HashMap<String, SatResult>,
}

impl SolverCache {
    pub fn new() -> Self {
        SolverCache {
            cache: HashMap::new(),
        }
    }

    /// Look up a constraint in the cache.
    /// Uses negation inference: if not(C) is UNSAT, C is SAT.
    pub fn check(&self, constraint: &str) -> Option<SatResult> {
        if let Some(r) = self.cache.get(constraint) {
            return Some(*r);
        }
        // Negation inference: if (not C) is UNSAT, then C is SAT
        let negated = format!("(not {constraint})");
        if self.cache.get(&negated) == Some(&SatResult::Unsat) {
            return Some(SatResult::Sat);
        }
        // Reverse: if C stored as UNSAT and this constraint IS a negation of C
        if let Some(inner) = constraint
            .strip_prefix("(not ")
            .and_then(|s| s.strip_suffix(')'))
        {
            if self.cache.get(inner) == Some(&SatResult::Unsat) {
                return Some(SatResult::Sat);
            }
        }
        None
    }

    /// Insert a constraint result into the cache.
    pub fn insert(&mut self, constraint: String, result: SatResult) {
        self.cache.insert(constraint, result);
    }

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Clear the cache.
    pub fn clear(&mut self) {
        self.cache.clear();
    }
}

impl Default for SolverCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_basic_insert_and_lookup() {
        let mut cache = SolverCache::new();
        cache.insert("(> x 5)".into(), SatResult::Sat);
        assert_eq!(cache.check("(> x 5)"), Some(SatResult::Sat));
    }

    #[test]
    fn cache_miss_returns_none() {
        let cache = SolverCache::new();
        assert_eq!(cache.check("(> x 5)"), None);
    }

    #[test]
    fn cache_negation_inference() {
        let mut cache = SolverCache::new();
        cache.insert("(not (> x 5))".into(), SatResult::Unsat);
        assert_eq!(cache.check("(> x 5)"), Some(SatResult::Sat));
    }

    #[test]
    fn cache_reverse_negation_inference() {
        let mut cache = SolverCache::new();
        cache.insert("(> x 5)".into(), SatResult::Unsat);
        assert_eq!(cache.check("(not (> x 5))"), Some(SatResult::Sat));
    }

    #[test]
    fn cache_no_false_inference() {
        let mut cache = SolverCache::new();
        cache.insert("(not (> x 5))".into(), SatResult::Sat);
        // Sat negation doesn't let us infer anything about the positive
        assert_eq!(cache.check("(> x 5)"), None);
    }

    #[test]
    fn cache_len_and_clear() {
        let mut cache = SolverCache::new();
        assert!(cache.is_empty());
        cache.insert("a".into(), SatResult::Sat);
        cache.insert("b".into(), SatResult::Unsat);
        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn cache_overwrite_existing() {
        let mut cache = SolverCache::new();
        cache.insert("(> x 5)".into(), SatResult::Unknown);
        cache.insert("(> x 5)".into(), SatResult::Sat);
        assert_eq!(cache.check("(> x 5)"), Some(SatResult::Sat));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_unsat_no_negation_inference_for_positive() {
        // Storing C=UNSAT does not mean (not C) is inferred as SAT via the
        // forward negation path (only via the reverse strip_prefix path).
        let mut cache = SolverCache::new();
        cache.insert("(> x 5)".into(), SatResult::Unsat);
        // The forward check for "(> x 5)" should return Unsat directly.
        assert_eq!(cache.check("(> x 5)"), Some(SatResult::Unsat));
    }

    #[test]
    fn cache_default_is_empty() {
        let cache = SolverCache::default();
        assert!(cache.is_empty());
    }

    /// Exercises the false branch of the inner `if` at line 40:
    /// the constraint has `(not …)` form, so `strip_prefix`/`strip_suffix`
    /// succeeds and we enter the outer `if let`, but the inner constraint
    /// is NOT cached as `Unsat` (it's absent), so the branch falls through
    /// to `None` (line 44).
    #[test]
    fn negation_inference_inner_not_unsat_returns_none() {
        let cache = SolverCache::new();
        // "(not (> x 5))" strips to "(> x 5)" which is not in cache at all.
        assert_eq!(cache.check("(not (> x 5))"), None);
    }

    /// Same false branch but the inner constraint IS present and cached as `Sat`
    /// (not `Unsat`), so negation inference must not fire.
    #[test]
    fn negation_inference_inner_sat_returns_none() {
        let mut cache = SolverCache::new();
        cache.insert("(> x 5)".into(), SatResult::Sat);
        // "(not (> x 5))" is not in cache; "(not (not (> x 5)))" is not either;
        // inner is Sat (≠ Unsat), so no inference → None.
        assert_eq!(cache.check("(not (> x 5))"), None);
    }

    /// Same false branch with inner cached as `Unknown`.
    #[test]
    fn negation_inference_inner_unknown_returns_none() {
        let mut cache = SolverCache::new();
        cache.insert("(> x 5)".into(), SatResult::Unknown);
        assert_eq!(cache.check("(not (> x 5))"), None);
    }
}
