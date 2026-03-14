//! Continuous [0,1] branch distance heuristics (EvoMaster / Korel approach).

use apex_core::types::BranchId;

/// Continuous [0.0, 1.0] heuristic for a branch condition.
/// 1.0 = covered, 0.0 = maximally far from flipping.
#[derive(Debug, Clone)]
pub struct BranchHeuristic {
    pub branch_id: BranchId,
    pub score: f64,
    pub operand_a: Option<i64>,
    pub operand_b: Option<i64>,
}

/// Normalize a non-negative distance into [0, 1) via `x / (x + 1)`.
fn normalize(x: f64) -> f64 {
    x / (x + 1.0)
}

/// Branch distance for `a == b`.
/// Returns 1.0 when equal, approaching 0.0 as `|a - b|` grows.
pub fn branch_distance_eq(a: i64, b: i64) -> f64 {
    1.0 - normalize((a - b).unsigned_abs() as f64)
}

/// Branch distance for `a < b`.
/// Returns 1.0 when satisfied, otherwise decreasing toward 0.0.
pub fn branch_distance_lt(a: i64, b: i64) -> f64 {
    if a < b {
        1.0
    } else {
        1.0 - normalize((a - b + 1) as f64)
    }
}

/// Branch distance for `a > b`.
/// Returns 1.0 when satisfied, otherwise decreasing toward 0.0.
pub fn branch_distance_gt(a: i64, b: i64) -> f64 {
    if a > b {
        1.0
    } else {
        1.0 - normalize((b - a + 1) as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_equality_exact_match() {
        assert_eq!(branch_distance_eq(42, 42), 1.0);
    }

    #[test]
    fn distance_equality_close() {
        let d = branch_distance_eq(40, 42);
        // distance=2, normalize(2)=2/3, so score=1/3 ~0.333
        assert!(d > 0.3, "expected > 0.3, got {d}");
        assert!(d < 1.0, "expected < 1.0, got {d}");
        // Closer values should yield higher scores
        let d_closer = branch_distance_eq(41, 42);
        assert!(d_closer > d, "closer values should score higher");
    }

    #[test]
    fn distance_equality_far() {
        let d = branch_distance_eq(0, 1_000_000);
        assert!(d < 0.01, "expected < 0.01, got {d}");
    }

    #[test]
    fn distance_less_than_satisfied() {
        assert_eq!(branch_distance_lt(5, 10), 1.0);
    }

    #[test]
    fn distance_less_than_boundary() {
        let d = branch_distance_lt(10, 10);
        assert!(d < 1.0, "expected < 1.0, got {d}");
        assert!(d > 0.0, "expected > 0.0, got {d}");
    }

    #[test]
    fn distance_less_than_far() {
        let d = branch_distance_lt(1_000_000, 0);
        assert!(d < 0.01, "expected < 0.01, got {d}");
    }

    #[test]
    fn distance_greater_than_satisfied() {
        assert_eq!(branch_distance_gt(10, 5), 1.0);
    }

    #[test]
    fn distance_greater_than_boundary() {
        let d = branch_distance_gt(10, 10);
        assert!(d < 1.0, "expected < 1.0, got {d}");
        assert!(d > 0.0, "expected > 0.0, got {d}");
    }

    #[test]
    fn distance_greater_than_far() {
        let d = branch_distance_gt(0, 1_000_000);
        assert!(d < 0.01, "expected < 0.01, got {d}");
    }

    #[test]
    fn normalize_zero_is_zero() {
        assert_eq!(normalize(0.0), 0.0);
    }

    #[test]
    fn normalize_monotonic() {
        let a = normalize(1.0);
        let b = normalize(10.0);
        let c = normalize(100.0);
        assert!(a < b);
        assert!(b < c);
        assert!(c < 1.0);
    }

    #[test]
    fn distance_eq_symmetric() {
        assert_eq!(branch_distance_eq(10, 20), branch_distance_eq(20, 10));
    }

    #[test]
    fn distance_eq_negative_values() {
        assert_eq!(branch_distance_eq(-5, -5), 1.0);
        let d = branch_distance_eq(-100, 100);
        assert!(d < 0.01);
    }
}
