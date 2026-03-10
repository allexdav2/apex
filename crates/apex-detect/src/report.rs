use serde::{Deserialize, Serialize};

use crate::finding::{Finding, Severity};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub findings: Vec<Finding>,
    pub detector_status: Vec<(String, bool)>,
}

impl AnalysisReport {
    pub fn security_summary(&self) -> SecuritySummary {
        let mut critical = 0;
        let mut high = 0;
        let mut medium = 0;
        let mut low = 0;

        for f in &self.findings {
            match f.severity {
                Severity::Critical => critical += 1,
                Severity::High => high += 1,
                Severity::Medium => medium += 1,
                Severity::Low => low += 1,
                Severity::Info => {}
            }
        }

        let detectors_run: Vec<String> = self
            .detector_status
            .iter()
            .map(|(name, _)| name.clone())
            .collect();

        let top_risk = self
            .findings
            .iter()
            .filter(|f| f.severity.rank() <= Severity::High.rank())
            .min_by_key(|f| (f.severity.rank(), f.covered as u8))
            .map(|f| format!("{} — {}", f.file.display(), f.title));

        SecuritySummary {
            critical,
            high,
            medium,
            low,
            detectors_run,
            top_risk,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySummary {
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub detectors_run: Vec<String>,
    pub top_risk: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Finding, FindingCategory, Severity};
    use std::path::PathBuf;

    fn make_finding(severity: Severity) -> Finding {
        Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity,
            category: FindingCategory::PanicPath,
            file: PathBuf::from("test.rs"),
            line: Some(1),
            title: "t".into(),
            description: "d".into(),
            evidence: vec![],
            covered: false,
            suggestion: "s".into(),
            explanation: None,
            fix: None,
        }
    }

    #[test]
    fn security_summary_counts_severities() {
        let report = AnalysisReport {
            findings: vec![
                make_finding(Severity::Critical),
                make_finding(Severity::High),
                make_finding(Severity::High),
                make_finding(Severity::Medium),
                make_finding(Severity::Low),
                make_finding(Severity::Info),
            ],
            detector_status: vec![("test".into(), true)],
        };
        let summary = report.security_summary();
        assert_eq!(summary.critical, 1);
        assert_eq!(summary.high, 2);
        assert_eq!(summary.medium, 1);
        assert_eq!(summary.low, 1);
    }

    #[test]
    fn empty_report_gives_zero_summary() {
        let report = AnalysisReport {
            findings: vec![],
            detector_status: vec![],
        };
        let summary = report.security_summary();
        assert_eq!(summary.critical, 0);
        assert_eq!(summary.high, 0);
        assert!(summary.top_risk.is_none());
    }

    #[test]
    fn security_summary_serializes() {
        let summary = SecuritySummary {
            critical: 1,
            high: 2,
            medium: 3,
            low: 4,
            detectors_run: vec!["panic".into()],
            top_risk: Some("src/main.rs — uncovered panic".into()),
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"critical\":1"));
        assert!(json.contains("\"top_risk\""));
    }
}
