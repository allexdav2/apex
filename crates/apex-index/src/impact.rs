//! Test Impact Analysis — select tests affected by file changes.
//!
//! Given a [`BranchIndex`] and a set of changed files, determines which tests
//! are affected by looking up branches in those files and finding covering tests.

use crate::types::BranchIndex;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Risk classification for a changed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeRisk {
    /// Changed branches have zero covering tests.
    UntestedChange,
    /// Changed branches covered by only 1 test.
    LowCoverage,
    /// Changed branches well-covered by multiple tests.
    WellTested,
}

/// Per-file impact detail.
#[derive(Debug, Clone)]
pub struct FileImpact {
    pub file: PathBuf,
    pub risk: ChangeRisk,
    pub affected_tests: Vec<String>,
    pub branch_count: usize,
}

/// Full impact analysis result.
#[derive(Debug, Clone)]
pub struct ImpactReport {
    pub file_impacts: Vec<FileImpact>,
    pub affected_tests: Vec<String>,
    pub total_tests: usize,
    pub selected_tests: usize,
    pub untested_files: Vec<PathBuf>,
}

impl ImpactReport {
    /// Speedup factor: total_tests / selected_tests.
    pub fn speedup(&self) -> f64 {
        if self.selected_tests == 0 {
            return 1.0;
        }
        self.total_tests as f64 / self.selected_tests as f64
    }
}

/// Analyze test impact for a set of changed files.
pub fn analyze(index: &BranchIndex, changed_files: &[PathBuf]) -> ImpactReport {
    // Build forward map: file_path → file_id
    let path_to_id: HashMap<&Path, u64> = index
        .file_paths
        .iter()
        .map(|(id, path)| (path.as_path(), *id))
        .collect();

    // Collect all unique test names from the index
    let total_tests: HashSet<&str> = index.traces.iter().map(|t| t.test_name.as_str()).collect();

    // For each changed file, find branches and their covering tests
    let mut all_affected_tests: HashSet<String> = HashSet::new();
    let mut file_impacts = Vec::new();
    let mut untested_files = Vec::new();

    for changed_file in changed_files {
        let file_id = match path_to_id.get(changed_file.as_path()) {
            Some(id) => *id,
            None => {
                // File not in index — untested
                untested_files.push(changed_file.clone());
                file_impacts.push(FileImpact {
                    file: changed_file.clone(),
                    risk: ChangeRisk::UntestedChange,
                    affected_tests: vec![],
                    branch_count: 0,
                });
                continue;
            }
        };

        // Find all profiles for branches in this file
        let mut file_tests: HashSet<String> = HashSet::new();
        let mut branch_count = 0;

        for profile in index.profiles.values() {
            if profile.branch.file_id == file_id {
                branch_count += 1;
                for test_name in &profile.test_names {
                    file_tests.insert(test_name.clone());
                }
            }
        }

        let risk = if file_tests.is_empty() {
            untested_files.push(changed_file.clone());
            ChangeRisk::UntestedChange
        } else if file_tests.len() == 1 {
            ChangeRisk::LowCoverage
        } else {
            ChangeRisk::WellTested
        };

        all_affected_tests.extend(file_tests.iter().cloned());

        file_impacts.push(FileImpact {
            file: changed_file.clone(),
            risk,
            affected_tests: file_tests.into_iter().collect(),
            branch_count,
        });
    }

    let mut affected_tests: Vec<String> = all_affected_tests.into_iter().collect();
    affected_tests.sort();

    let selected_tests = affected_tests.len();

    ImpactReport {
        file_impacts,
        affected_tests,
        total_tests: total_tests.len(),
        selected_tests,
        untested_files,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BranchProfile, TestTrace};
    use apex_core::types::{BranchId, ExecutionStatus, Language};
    use std::collections::HashMap;

    fn make_branch(file_id: u64, line: u32, direction: u8) -> BranchId {
        BranchId::new(file_id, line, 0, direction)
    }

    fn make_index(traces: Vec<TestTrace>, file_paths: HashMap<u64, PathBuf>) -> BranchIndex {
        let profiles = BranchIndex::build_profiles(&traces);
        let covered_branches = profiles.len();
        BranchIndex {
            traces,
            profiles,
            file_paths,
            total_branches: covered_branches + 5,
            covered_branches,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::from("/project"),
            source_hash: String::new(),
        }
    }

    #[test]
    fn analyze_finds_affected_tests() {
        let file_id = 42;
        let traces = vec![
            TestTrace {
                test_name: "test_auth".into(),
                branches: vec![make_branch(file_id, 10, 0), make_branch(file_id, 20, 1)],
                duration_ms: 100,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_login".into(),
                branches: vec![make_branch(file_id, 10, 0), make_branch(99, 5, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_unrelated".into(),
                branches: vec![make_branch(99, 5, 0)],
                duration_ms: 30,
                status: ExecutionStatus::Pass,
            },
        ];
        let file_paths = HashMap::from([
            (file_id, PathBuf::from("src/auth.py")),
            (99, PathBuf::from("src/utils.py")),
        ]);

        let index = make_index(traces, file_paths);
        let report = analyze(&index, &[PathBuf::from("src/auth.py")]);

        assert_eq!(report.selected_tests, 2);
        assert!(report.affected_tests.contains(&"test_auth".to_string()));
        assert!(report.affected_tests.contains(&"test_login".to_string()));
        assert!(!report
            .affected_tests
            .contains(&"test_unrelated".to_string()));
        assert_eq!(report.total_tests, 3);
        assert!(report.untested_files.is_empty());
    }

    #[test]
    fn analyze_untested_file() {
        let traces = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![make_branch(1, 10, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let file_paths = HashMap::from([(1, PathBuf::from("src/covered.py"))]);

        let index = make_index(traces, file_paths);
        let report = analyze(&index, &[PathBuf::from("src/new_file.py")]);

        assert_eq!(report.selected_tests, 0);
        assert_eq!(report.untested_files.len(), 1);
        assert_eq!(report.file_impacts[0].risk, ChangeRisk::UntestedChange);
    }

    #[test]
    fn analyze_low_coverage() {
        let file_id = 10;
        let traces = vec![TestTrace {
            test_name: "test_only".into(),
            branches: vec![make_branch(file_id, 5, 0)],
            duration_ms: 20,
            status: ExecutionStatus::Pass,
        }];
        let file_paths = HashMap::from([(file_id, PathBuf::from("src/risky.py"))]);

        let index = make_index(traces, file_paths);
        let report = analyze(&index, &[PathBuf::from("src/risky.py")]);

        assert_eq!(report.file_impacts[0].risk, ChangeRisk::LowCoverage);
        assert_eq!(report.selected_tests, 1);
    }

    #[test]
    fn analyze_well_tested() {
        let file_id = 10;
        let traces = vec![
            TestTrace {
                test_name: "test_a".into(),
                branches: vec![make_branch(file_id, 5, 0)],
                duration_ms: 20,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_b".into(),
                branches: vec![make_branch(file_id, 5, 0)],
                duration_ms: 20,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_c".into(),
                branches: vec![make_branch(file_id, 10, 1)],
                duration_ms: 20,
                status: ExecutionStatus::Pass,
            },
        ];
        let file_paths = HashMap::from([(file_id, PathBuf::from("src/solid.py"))]);

        let index = make_index(traces, file_paths);
        let report = analyze(&index, &[PathBuf::from("src/solid.py")]);

        assert_eq!(report.file_impacts[0].risk, ChangeRisk::WellTested);
        assert_eq!(report.selected_tests, 3);
    }

    #[test]
    fn analyze_empty_changed_files() {
        let traces = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![make_branch(1, 10, 0)],
            duration_ms: 10,
            status: ExecutionStatus::Pass,
        }];
        let index = make_index(traces, HashMap::from([(1, PathBuf::from("src/a.py"))]));
        let report = analyze(&index, &[]);

        assert_eq!(report.selected_tests, 0);
        assert!(report.affected_tests.is_empty());
        assert!(report.file_impacts.is_empty());
    }

    #[test]
    fn analyze_empty_index() {
        let index = make_index(vec![], HashMap::new());
        let report = analyze(&index, &[PathBuf::from("src/new.py")]);

        assert_eq!(report.total_tests, 0);
        assert_eq!(report.selected_tests, 0);
        assert_eq!(report.untested_files.len(), 1);
    }

    #[test]
    fn analyze_multiple_changed_files() {
        let traces = vec![
            TestTrace {
                test_name: "test_a".into(),
                branches: vec![make_branch(1, 10, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_b".into(),
                branches: vec![make_branch(2, 20, 0)],
                duration_ms: 10,
                status: ExecutionStatus::Pass,
            },
        ];
        let file_paths = HashMap::from([
            (1, PathBuf::from("src/a.py")),
            (2, PathBuf::from("src/b.py")),
        ]);

        let index = make_index(traces, file_paths);
        let report = analyze(
            &index,
            &[PathBuf::from("src/a.py"), PathBuf::from("src/b.py")],
        );

        assert_eq!(report.selected_tests, 2);
        assert_eq!(report.file_impacts.len(), 2);
    }

    #[test]
    fn speedup_calculation() {
        let report = ImpactReport {
            file_impacts: vec![],
            affected_tests: vec!["test_a".into(), "test_b".into()],
            total_tests: 100,
            selected_tests: 2,
            untested_files: vec![],
        };
        assert!((report.speedup() - 50.0).abs() < 0.01);
    }

    #[test]
    fn speedup_zero_selected() {
        let report = ImpactReport {
            file_impacts: vec![],
            affected_tests: vec![],
            total_tests: 100,
            selected_tests: 0,
            untested_files: vec![],
        };
        assert!((report.speedup() - 1.0).abs() < 0.01);
    }

    #[test]
    fn affected_tests_deduplicated() {
        // Same test covers branches in two changed files — should appear once
        let file_id_a = 1;
        let file_id_b = 2;
        let traces = vec![TestTrace {
            test_name: "test_shared".into(),
            branches: vec![make_branch(file_id_a, 10, 0), make_branch(file_id_b, 20, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let file_paths = HashMap::from([
            (file_id_a, PathBuf::from("src/a.py")),
            (file_id_b, PathBuf::from("src/b.py")),
        ]);

        let index = make_index(traces, file_paths);
        let report = analyze(
            &index,
            &[PathBuf::from("src/a.py"), PathBuf::from("src/b.py")],
        );

        assert_eq!(report.selected_tests, 1);
        assert_eq!(report.affected_tests, vec!["test_shared"]);
    }
}
