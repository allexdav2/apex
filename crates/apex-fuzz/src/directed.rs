//! AFLGo-style directed fuzzing with simulated annealing.
//!
//! Two pure functions for computing energy assignments based on distance
//! to a target location and a cooling schedule.

/// Compute energy for a corpus entry based on its distance to the target.
///
/// Uses simulated annealing: at high temperature, energy is uniform (~1.0,
/// exploration). At low temperature, energy favors inputs close to the
/// target (exploitation: 1/distance).
///
/// Returns 1.0 when distance is zero (at target).
pub fn directed_energy(distance: f64, temperature: f64) -> f64 {
    if distance <= 0.0 {
        return 1.0;
    }
    let temperature = temperature.clamp(0.0, 1.0);
    let exploration = 1.0;
    let exploitation = 1.0 / distance;
    temperature * exploration + (1.0 - temperature) * exploitation
}

/// Linear cooling schedule from 1.0 to 0.0.
///
/// Returns 0.0 if `total_iterations` is zero or `current_iteration >= total_iterations`.
pub fn temperature(current_iteration: u64, total_iterations: u64) -> f64 {
    if total_iterations == 0 {
        return 0.0;
    }
    if current_iteration >= total_iterations {
        return 0.0;
    }
    1.0 - (current_iteration as f64 / total_iterations as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_at_target_is_one() {
        assert!((directed_energy(0.0, 0.5) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn energy_far_from_target_low_temp() {
        // distance=10.0, temp=0.0 → pure exploitation = 1/10 = 0.1
        let e = directed_energy(10.0, 0.0);
        assert!((e - 0.1).abs() < 1e-9);
    }

    #[test]
    fn energy_high_temp_is_uniform() {
        // At temp=1.0, energy should be ~1.0 regardless of distance
        let e_near = directed_energy(1.0, 1.0);
        let e_far = directed_energy(100.0, 1.0);
        assert!((e_near - 1.0).abs() < 1e-9);
        assert!((e_far - 1.0).abs() < 1e-9);
    }

    #[test]
    fn temperature_starts_at_one() {
        assert!((temperature(0, 100) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn temperature_ends_at_zero() {
        assert!((temperature(100, 100) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn temperature_midpoint() {
        assert!((temperature(50, 100) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn temperature_zero_total() {
        assert!((temperature(0, 0) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn temperature_past_total_clamped() {
        assert!((temperature(200, 100) - 0.0).abs() < f64::EPSILON);
    }
}
