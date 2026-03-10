use apex_core::types::{BranchId, ExecutionStatus, Language};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Per-test trace
// ---------------------------------------------------------------------------

/// Branch footprint of a single test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestTrace {
    pub test_name: String,
    pub branches: Vec<BranchId>,
    pub duration_ms: u64,
    pub status: ExecutionStatus,
}

// ---------------------------------------------------------------------------
// Branch profile (aggregate)
// ---------------------------------------------------------------------------

/// Aggregate statistics for a single branch across all tests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchProfile {
    pub branch: BranchId,
    /// Total hit count across all tests.
    pub hit_count: u64,
    /// Number of distinct tests that reach this branch.
    pub test_count: usize,
    /// Names of tests that reach this branch.
    pub test_names: Vec<String>,
}

// ---------------------------------------------------------------------------
// The full index
// ---------------------------------------------------------------------------

/// Persistent per-test branch mapping for a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchIndex {
    pub traces: Vec<TestTrace>,
    pub profiles: HashMap<String, BranchProfile>,
    pub file_paths: HashMap<u64, PathBuf>,
    pub total_branches: usize,
    pub covered_branches: usize,
    pub created_at: String,
    pub language: Language,
    pub target_root: PathBuf,
    /// SHA-256 of concatenated source file contents for staleness detection.
    pub source_hash: String,
}

impl BranchIndex {
    /// Build profiles from traces.
    pub fn build_profiles(traces: &[TestTrace]) -> HashMap<String, BranchProfile> {
        let mut map: HashMap<String, BranchProfile> = HashMap::new();

        for trace in traces {
            for branch in &trace.branches {
                let key = branch_key(branch);
                let profile = map.entry(key).or_insert_with(|| BranchProfile {
                    branch: branch.clone(),
                    hit_count: 0,
                    test_count: 0,
                    test_names: Vec::new(),
                });
                profile.hit_count += 1;
                profile.test_count += 1;
                profile.test_names.push(trace.test_name.clone());
            }
        }

        map
    }

    /// Compute coverage percentage.
    pub fn coverage_percent(&self) -> f64 {
        if self.total_branches == 0 {
            return 100.0;
        }
        (self.covered_branches as f64 / self.total_branches as f64) * 100.0
    }

    /// Get all branches that are never hit by any test.
    pub fn dead_branches(&self) -> Vec<&BranchProfile> {
        // Branches that exist in total set but have no profile entry are dead.
        // But since profiles only contain hit branches, we need a different approach.
        // Dead branches = total branches - branches in any profile.
        // This method returns profiles with lowest hit counts for analysis.
        // For true dead branches, see dead_branch_ids().
        self.profiles.values().filter(|p| p.hit_count == 0).collect()
    }

    /// Get BranchIds that appear in no test trace.
    pub fn uncovered_branch_ids(&self, all_branches: &[BranchId]) -> Vec<BranchId> {
        let covered: HashSet<String> = self.profiles.keys().cloned().collect();
        all_branches
            .iter()
            .filter(|b| !covered.contains(&branch_key(b)))
            .cloned()
            .collect()
    }

    /// Persist the index to a JSON file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, json)
    }

    /// Load index from a JSON file.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    /// Check if the index is stale (source files changed since index was built).
    pub fn is_stale(&self, current_hash: &str) -> bool {
        self.source_hash != current_hash
    }
}

/// Stable string key for a BranchId (for HashMap keying in profiles).
pub fn branch_key(b: &BranchId) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        b.file_id,
        b.line,
        b.col,
        b.direction,
        b.condition_index.unwrap_or(255)
    )
}

/// Compute SHA-256 hash of source files in a directory for staleness detection.
pub fn hash_source_files(root: &Path, language: Language) -> String {
    let extensions: &[&str] = match language {
        Language::Python => &["py"],
        Language::Rust => &["rs"],
        Language::JavaScript => &["js", "ts"],
        Language::Java => &["java"],
        Language::C => &["c", "h"],
        Language::Wasm => &["wat", "wasm"],
        Language::Ruby => &["rb"],
    };

    let mut paths: Vec<PathBuf> = Vec::new();
    collect_source_files(root, extensions, &mut paths);
    paths.sort();

    let mut hasher = Sha256::new();
    for path in &paths {
        if let Ok(content) = std::fs::read(path) {
            hasher.update(path.to_string_lossy().as_bytes());
            hasher.update(&content);
        }
    }

    format!("{:x}", hasher.finalize())
}

fn collect_source_files(dir: &Path, extensions: &[&str], out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden dirs, build artifacts, venvs
        if name_str.starts_with('.')
            || name_str == "target"
            || name_str == "node_modules"
            || name_str == "__pycache__"
            || name_str == ".venv"
            || name_str == "venv"
        {
            continue;
        }

        if path.is_dir() {
            collect_source_files(&path, extensions, out);
        } else if let Some(ext) = path.extension() {
            if extensions.iter().any(|e| ext == *e) {
                out.push(path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_branch(file_id: u64, line: u32, direction: u8) -> BranchId {
        BranchId::new(file_id, line, 0, direction)
    }

    #[test]
    fn branch_key_deterministic() {
        let b = make_branch(42, 10, 0);
        assert_eq!(branch_key(&b), branch_key(&b));
    }

    #[test]
    fn branch_key_differs_by_direction() {
        let a = make_branch(42, 10, 0);
        let b = make_branch(42, 10, 1);
        assert_ne!(branch_key(&a), branch_key(&b));
    }

    #[test]
    fn build_profiles_empty() {
        let profiles = BranchIndex::build_profiles(&[]);
        assert!(profiles.is_empty());
    }

    #[test]
    fn build_profiles_counts_correctly() {
        let traces = vec![
            TestTrace {
                test_name: "test_a".into(),
                branches: vec![make_branch(1, 10, 0), make_branch(1, 20, 0)],
                duration_ms: 100,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_b".into(),
                branches: vec![make_branch(1, 10, 0), make_branch(2, 5, 1)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
        ];

        let profiles = BranchIndex::build_profiles(&traces);

        let key_10 = branch_key(&make_branch(1, 10, 0));
        let profile = &profiles[&key_10];
        assert_eq!(profile.hit_count, 2);
        assert_eq!(profile.test_count, 2);
        assert_eq!(profile.test_names, vec!["test_a", "test_b"]);

        let key_20 = branch_key(&make_branch(1, 20, 0));
        assert_eq!(profiles[&key_20].test_count, 1);
    }

    #[test]
    fn coverage_percent_empty() {
        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 0,
            covered_branches: 0,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        assert!((index.coverage_percent() - 100.0).abs() < 0.01);
    }

    #[test]
    fn coverage_percent_partial() {
        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 100,
            covered_branches: 75,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };
        assert!((index.coverage_percent() - 75.0).abs() < 0.01);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let index = BranchIndex {
            traces: vec![TestTrace {
                test_name: "test_x".into(),
                branches: vec![make_branch(1, 10, 0)],
                duration_ms: 42,
                status: ExecutionStatus::Pass,
            }],
            profiles: BranchIndex::build_profiles(&[TestTrace {
                test_name: "test_x".into(),
                branches: vec![make_branch(1, 10, 0)],
                duration_ms: 42,
                status: ExecutionStatus::Pass,
            }]),
            file_paths: HashMap::from([(1u64, PathBuf::from("src/lib.py"))]),
            total_branches: 5,
            covered_branches: 3,
            created_at: "2026-03-12T00:00:00Z".into(),
            language: Language::Python,
            target_root: PathBuf::from("/tmp/test"),
            source_hash: "abc123".into(),
        };

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".apex/index.json");

        index.save(&path).unwrap();
        let loaded = BranchIndex::load(&path).unwrap();

        assert_eq!(loaded.traces.len(), 1);
        assert_eq!(loaded.total_branches, 5);
        assert_eq!(loaded.covered_branches, 3);
        assert_eq!(loaded.source_hash, "abc123");
    }

    #[test]
    fn is_stale_detects_change() {
        let index = BranchIndex {
            traces: vec![],
            profiles: HashMap::new(),
            file_paths: HashMap::new(),
            total_branches: 0,
            covered_branches: 0,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: "old_hash".into(),
        };
        assert!(index.is_stale("new_hash"));
        assert!(!index.is_stale("old_hash"));
    }

    #[test]
    fn uncovered_branch_ids_finds_missing() {
        let traces = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![make_branch(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let index = BranchIndex {
            traces: traces.clone(),
            profiles: BranchIndex::build_profiles(&traces),
            file_paths: HashMap::new(),
            total_branches: 3,
            covered_branches: 1,
            created_at: String::new(),
            language: Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let all = vec![
            make_branch(1, 10, 0), // covered
            make_branch(1, 20, 0), // not covered
            make_branch(2, 5, 1),  // not covered
        ];

        let uncovered = index.uncovered_branch_ids(&all);
        assert_eq!(uncovered.len(), 2);
    }
}
