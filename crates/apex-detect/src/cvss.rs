//! CVSS v3.1 base scoring for security findings.
//!
//! Provides CWE-to-CVSS mapping and the standard base score formula.

use crate::finding::Finding;

/// Attack Vector metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackVector {
    Network,
    Adjacent,
    Local,
    Physical,
}

/// Attack Complexity metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackComplexity {
    Low,
    High,
}

/// Privileges Required metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivilegesRequired {
    None,
    Low,
    High,
}

/// User Interaction metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserInteraction {
    None,
    Required,
}

/// Scope metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Unchanged,
    Changed,
}

/// Impact metric (used for Confidentiality, Integrity, and Availability).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impact {
    None,
    Low,
    High,
}

/// CVSS v3.1 base metric group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CvssBase {
    pub attack_vector: AttackVector,
    pub attack_complexity: AttackComplexity,
    pub privileges_required: PrivilegesRequired,
    pub user_interaction: UserInteraction,
    pub scope: Scope,
    pub confidentiality: Impact,
    pub integrity: Impact,
    pub availability: Impact,
}

impl AttackVector {
    fn weight(self) -> f64 {
        match self {
            AttackVector::Network => 0.85,
            AttackVector::Adjacent => 0.62,
            AttackVector::Local => 0.55,
            AttackVector::Physical => 0.20,
        }
    }

    fn abbrev(self) -> &'static str {
        match self {
            AttackVector::Network => "N",
            AttackVector::Adjacent => "A",
            AttackVector::Local => "L",
            AttackVector::Physical => "P",
        }
    }
}

impl AttackComplexity {
    fn weight(self) -> f64 {
        match self {
            AttackComplexity::Low => 0.77,
            AttackComplexity::High => 0.44,
        }
    }

    fn abbrev(self) -> &'static str {
        match self {
            AttackComplexity::Low => "L",
            AttackComplexity::High => "H",
        }
    }
}

impl PrivilegesRequired {
    fn weight(self, scope: Scope) -> f64 {
        match (self, scope) {
            (PrivilegesRequired::None, _) => 0.85,
            (PrivilegesRequired::Low, Scope::Unchanged) => 0.62,
            (PrivilegesRequired::Low, Scope::Changed) => 0.68,
            (PrivilegesRequired::High, Scope::Unchanged) => 0.27,
            (PrivilegesRequired::High, Scope::Changed) => 0.50,
        }
    }

    fn abbrev(self) -> &'static str {
        match self {
            PrivilegesRequired::None => "N",
            PrivilegesRequired::Low => "L",
            PrivilegesRequired::High => "H",
        }
    }
}

impl UserInteraction {
    fn weight(self) -> f64 {
        match self {
            UserInteraction::None => 0.85,
            UserInteraction::Required => 0.62,
        }
    }

    fn abbrev(self) -> &'static str {
        match self {
            UserInteraction::None => "N",
            UserInteraction::Required => "R",
        }
    }
}

impl Scope {
    fn abbrev(self) -> &'static str {
        match self {
            Scope::Unchanged => "U",
            Scope::Changed => "C",
        }
    }
}

impl Impact {
    fn weight(self) -> f64 {
        match self {
            Impact::None => 0.0,
            Impact::Low => 0.22,
            Impact::High => 0.56,
        }
    }

    fn abbrev(self) -> &'static str {
        match self {
            Impact::None => "N",
            Impact::Low => "L",
            Impact::High => "H",
        }
    }
}

/// Return default CVSS base metrics for a given CWE ID.
///
/// Maps common CWEs to their typical base metric profiles. Unknown CWEs
/// receive a medium-severity default (~5.3).
pub fn cwe_default_cvss(cwe_id: u32) -> CvssBase {
    match cwe_id {
        // CWE-78: OS Command Injection → 9.8
        78 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        },
        // CWE-79: Cross-site Scripting (XSS) → 6.1
        79 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::Required,
            scope: Scope::Changed,
            confidentiality: Impact::Low,
            integrity: Impact::Low,
            availability: Impact::None,
        },
        // CWE-89: SQL Injection → 9.8
        89 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        },
        // CWE-94: Code Injection → 9.8
        94 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        },
        // CWE-22: Path Traversal → 7.5
        22 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::None,
            availability: Impact::None,
        },
        // CWE-502: Deserialization of Untrusted Data → 9.8
        502 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        },
        // CWE-798: Hardcoded Credentials → 9.8
        798 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::High,
        },
        // CWE-918: Server-Side Request Forgery (SSRF) → 8.6
        918 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Changed,
            confidentiality: Impact::High,
            integrity: Impact::Low,
            availability: Impact::None,
        },
        // CWE-328: Weak Hash → 7.5
        328 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::None,
            availability: Impact::None,
        },
        // CWE-295: Improper Certificate Validation → 7.4
        295 => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::High,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::High,
            integrity: Impact::High,
            availability: Impact::None,
        },
        // Unknown CWE → medium default (~5.3)
        _ => CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::Low,
            integrity: Impact::None,
            availability: Impact::None,
        },
    }
}

/// CVSS v3.1 roundup: smallest number >= x that is a multiple of 0.1.
fn roundup(x: f64) -> f32 {
    let int_x = (x * 100_000.0) as u64;
    if int_x % 10_000 == 0 {
        (int_x as f64 / 100_000.0) as f32
    } else {
        ((int_x / 10_000 + 1) as f64 * 10_000.0 / 100_000.0) as f32
    }
}

/// Calculate the CVSS v3.1 base score from a set of base metrics.
pub fn calculate_cvss_score(base: &CvssBase) -> f32 {
    let isc_raw = 1.0
        - (1.0 - base.confidentiality.weight())
            * (1.0 - base.integrity.weight())
            * (1.0 - base.availability.weight());

    let impact = match base.scope {
        Scope::Unchanged => 6.42 * isc_raw,
        Scope::Changed => {
            7.52 * (isc_raw - 0.029) - 3.25 * (isc_raw - 0.02).powf(15.0)
        }
    };

    if impact <= 0.0 {
        return 0.0;
    }

    let exploitability = 8.22
        * base.attack_vector.weight()
        * base.attack_complexity.weight()
        * base.privileges_required.weight(base.scope)
        * base.user_interaction.weight();

    let raw = match base.scope {
        Scope::Unchanged => {
            let s = impact + exploitability;
            if s > 10.0 { 10.0 } else { s }
        }
        Scope::Changed => {
            let s = 1.08 * (impact + exploitability);
            if s > 10.0 { 10.0 } else { s }
        }
    };

    roundup(raw)
}

/// Produce the CVSS v3.1 vector string for the given base metrics.
pub fn cvss_vector_string(base: &CvssBase) -> String {
    format!(
        "CVSS:3.1/AV:{}/AC:{}/PR:{}/UI:{}/S:{}/C:{}/I:{}/A:{}",
        base.attack_vector.abbrev(),
        base.attack_complexity.abbrev(),
        base.privileges_required.abbrev(),
        base.user_interaction.abbrev(),
        base.scope.abbrev(),
        base.confidentiality.abbrev(),
        base.integrity.abbrev(),
        base.availability.abbrev(),
    )
}

/// Score a [`Finding`] by its first CWE ID.
///
/// Returns `(Some(score), Some(vector))` if the finding has at least one CWE ID,
/// or `(None, None)` otherwise.
pub fn score_finding(finding: &Finding) -> (Option<f32>, Option<String>) {
    match finding.cwe_ids.first() {
        Some(&cwe_id) => {
            let base = cwe_default_cvss(cwe_id);
            let score = calculate_cvss_score(&base);
            let vector = cvss_vector_string(&base);
            (Some(score), Some(vector))
        }
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cwe_78_scores_critical() {
        let base = cwe_default_cvss(78);
        let score = calculate_cvss_score(&base);
        assert!(
            score >= 9.0,
            "CWE-78 score {score} should be >= 9.0 (critical)"
        );
    }

    #[test]
    fn cwe_79_scores_medium() {
        let base = cwe_default_cvss(79);
        let score = calculate_cvss_score(&base);
        assert!(
            (5.0..=7.0).contains(&score),
            "CWE-79 score {score} should be between 5.0 and 7.0 (medium)"
        );
    }

    #[test]
    fn cwe_89_scores_critical() {
        let base = cwe_default_cvss(89);
        let score = calculate_cvss_score(&base);
        assert!(
            score >= 9.0,
            "CWE-89 score {score} should be >= 9.0 (critical)"
        );
    }

    #[test]
    fn unknown_cwe_scores_medium() {
        let base = cwe_default_cvss(99999);
        let score = calculate_cvss_score(&base);
        let diff = (score - 5.3_f32).abs();
        assert!(
            diff < 0.2,
            "Unknown CWE score {score} should be ~5.3"
        );
    }

    #[test]
    fn cvss_vector_format() {
        let base = cwe_default_cvss(78);
        let vec = cvss_vector_string(&base);
        assert!(
            vec.starts_with("CVSS:3.1/"),
            "Vector string should start with CVSS:3.1/, got: {vec}"
        );
        assert_eq!(vec, "CVSS:3.1/AV:N/AC:L/PR:N/UI:N/S:U/C:H/I:H/A:H");
    }

    #[test]
    fn score_zero_impact() {
        let base = CvssBase {
            attack_vector: AttackVector::Network,
            attack_complexity: AttackComplexity::Low,
            privileges_required: PrivilegesRequired::None,
            user_interaction: UserInteraction::None,
            scope: Scope::Unchanged,
            confidentiality: Impact::None,
            integrity: Impact::None,
            availability: Impact::None,
        };
        let score = calculate_cvss_score(&base);
        assert!(
            score == 0.0,
            "All-None impact should yield score 0.0, got {score}"
        );
    }

    #[test]
    fn roundup_to_nearest_tenth() {
        // 4.0 should stay 4.0
        assert_eq!(roundup(4.0), 4.0);
        // 4.02 should round up to 4.1
        assert_eq!(roundup(4.02), 4.1);
        // 4.1 should stay 4.1
        assert_eq!(roundup(4.1), 4.1);
        // 4.91 should round up to 5.0
        assert_eq!(roundup(4.91), 5.0);
    }
}
