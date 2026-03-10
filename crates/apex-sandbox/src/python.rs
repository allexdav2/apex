use apex_core::{
    error::{ApexError, Result},
    traits::Sandbox,
    types::{BranchId, ExecutionResult, ExecutionStatus, InputSeed, Language, SnapshotId},
};
use apex_coverage::CoverageOracle;
use async_trait::async_trait;
use serde::Deserialize;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};
use tracing::{debug, warn};

// ---------------------------------------------------------------------------
// Coverage JSON wire types (mirrors apex-instrument; intentionally duplicated)
// ---------------------------------------------------------------------------

fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[derive(Deserialize)]
struct ApexCoverageJson {
    files: HashMap<String, FileData>,
}

#[derive(Deserialize)]
struct FileData {
    executed_branches: Vec<[i64; 2]>,
    #[allow(dead_code)]
    missing_branches: Vec<[i64; 2]>,
    #[allow(dead_code)]
    all_branches: Vec<[i64; 2]>,
}

// ---------------------------------------------------------------------------
// PythonTestSandbox
// ---------------------------------------------------------------------------

/// Runs a Python test candidate under `coverage.py`, then computes which
/// previously-uncovered branches were newly hit.
///
/// `InputSeed.data` must be UTF-8 Python source code.
#[allow(dead_code)]
pub struct PythonTestSandbox {
    oracle: Arc<CoverageOracle>,
    /// Maps file_id (FNV-1a of repo-relative path) → repo-relative PathBuf.
    file_paths: Arc<HashMap<u64, PathBuf>>,
    target_dir: PathBuf,
    timeout_ms: u64,
}

impl PythonTestSandbox {
    pub fn new(
        oracle: Arc<CoverageOracle>,
        file_paths: Arc<HashMap<u64, PathBuf>>,
        target_dir: PathBuf,
    ) -> Self {
        PythonTestSandbox {
            oracle,
            file_paths,
            target_dir,
            timeout_ms: 30_000,
        }
    }

    pub fn with_timeout(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    /// Parse the coverage JSON file and return (file_id, line, direction) tuples
    /// for all branches that were executed.
    fn executed_branches_from_json(&self, json_path: &Path) -> Result<Vec<BranchId>> {
        let content = std::fs::read_to_string(json_path)
            .map_err(|e| ApexError::Sandbox(format!("read coverage json: {e}")))?;

        let data: ApexCoverageJson = serde_json::from_str(&content)
            .map_err(|e| ApexError::Sandbox(format!("parse coverage json: {e}")))?;

        let mut branches = Vec::new();
        for (abs_path, fdata) in &data.files {
            // Normalise to repo-root-relative path — must match how the
            // instrumentor computed file_id for the oracle.
            let rel = Path::new(abs_path)
                .strip_prefix(&self.target_dir)
                .unwrap_or(Path::new(abs_path));
            let file_id = fnv1a_hash(&rel.to_string_lossy());

            for pair in &fdata.executed_branches {
                let from_line = pair[0].unsigned_abs() as u32;
                let direction = if pair[1] < 0 { 1u8 } else { 0u8 };
                branches.push(BranchId::new(file_id, from_line, 0, direction));
            }
        }
        Ok(branches)
    }
}

#[async_trait]
impl Sandbox for PythonTestSandbox {
    async fn run(&self, input: &InputSeed) -> Result<ExecutionResult> {
        let start = Instant::now();

        // Write candidate test code to a temporary .py file.
        // NamedTempFile must stay alive until pytest finishes.
        let tmp_dir =
            tempfile::tempdir().map_err(|e| ApexError::Sandbox(format!("tempdir: {e}")))?;

        let test_file = tmp_dir.path().join("test_apex_candidate.py");
        let code = std::str::from_utf8(&input.data)
            .map_err(|e| ApexError::Sandbox(format!("candidate not valid UTF-8: {e}")))?;
        std::fs::write(&test_file, code)
            .map_err(|e| ApexError::Sandbox(format!("write candidate: {e}")))?;

        // Paths for coverage data (unique per run via tmp_dir UUID).
        let cov_data = tmp_dir.path().join("cov.data");
        let cov_json = tmp_dir.path().join("cov.json");

        // Step 1: run pytest under coverage.py
        let run_output = tokio::time::timeout(
            std::time::Duration::from_millis(self.timeout_ms),
            tokio::process::Command::new("python3")
                .args([
                    "-m",
                    "coverage",
                    "run",
                    "--branch",
                    "--source=.",
                    &format!("--data-file={}", cov_data.display()),
                    "-m",
                    "pytest",
                    &test_file.to_string_lossy(),
                    "-x",
                    "-q",
                    "--tb=short",
                ])
                .current_dir(&self.target_dir)
                .output(),
        )
        .await;

        let duration_ms = start.elapsed().as_millis() as u64;

        let run_output = match run_output {
            Err(_) => {
                return Ok(ExecutionResult {
                    seed_id: input.id,
                    status: ExecutionStatus::Timeout,
                    new_branches: Vec::new(),
                    trace: None,
                    duration_ms,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            Ok(Err(e)) => return Err(ApexError::Sandbox(format!("spawn pytest: {e}"))),
            Ok(Ok(o)) => o,
        };

        let stdout = String::from_utf8_lossy(&run_output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&run_output.stderr).to_string();

        let status = match run_output.status.code() {
            Some(0) => ExecutionStatus::Pass,
            Some(1) => ExecutionStatus::Fail, // test failures
            Some(code) if code < 0 => ExecutionStatus::Crash,
            _ => ExecutionStatus::Fail,
        };

        // Step 2: export coverage to JSON (best-effort; may fail on syntax errors)
        let json_ok = tokio::process::Command::new("python3")
            .args([
                "-m",
                "coverage",
                "json",
                &format!("--data-file={}", cov_data.display()),
                "-o",
                &cov_json.to_string_lossy(),
            ])
            .current_dir(&self.target_dir)
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);

        // Step 3: compute coverage delta vs oracle
        let new_branches = if json_ok && cov_json.exists() {
            match self.executed_branches_from_json(&cov_json) {
                Ok(executed) => executed
                    .into_iter()
                    .filter(|b| {
                        matches!(
                            self.oracle.state_of(b),
                            Some(apex_core::types::BranchState::Uncovered)
                        )
                    })
                    .collect(),
                Err(e) => {
                    warn!(error = %e, "failed to parse candidate coverage JSON");
                    Vec::new()
                }
            }
        } else {
            debug!("coverage JSON not produced for this candidate");
            Vec::new()
        };

        Ok(ExecutionResult {
            seed_id: input.id,
            status,
            new_branches,
            trace: None,
            duration_ms,
            stdout,
            stderr,
        })
    }

    async fn snapshot(&self) -> Result<SnapshotId> {
        Err(ApexError::NotSupported(
            "PythonTestSandbox does not support snapshots".into(),
        ))
    }

    async fn restore(&self, _id: SnapshotId) -> Result<()> {
        Err(ApexError::NotSupported(
            "PythonTestSandbox does not support restore".into(),
        ))
    }

    fn language(&self) -> Language {
        Language::Python
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn make_oracle() -> Arc<CoverageOracle> {
        Arc::new(CoverageOracle::new())
    }

    fn make_file_paths() -> Arc<HashMap<u64, PathBuf>> {
        Arc::new(HashMap::new())
    }

    #[test]
    fn fnv1a_deterministic() {
        assert_eq!(fnv1a_hash("foo/bar.py"), fnv1a_hash("foo/bar.py"));
    }

    #[test]
    fn fnv1a_different_inputs_differ() {
        assert_ne!(fnv1a_hash("a.py"), fnv1a_hash("b.py"));
    }

    #[test]
    fn fnv1a_empty_string() {
        // Should not panic and should return the FNV offset basis.
        let h = fnv1a_hash("");
        assert_eq!(h, 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn new_sets_default_timeout() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"));
        assert_eq!(sb.timeout_ms, 30_000);
        assert_eq!(sb.target_dir, PathBuf::from("/proj"));
    }

    #[test]
    fn with_timeout_overrides() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"))
            .with_timeout(5_000);
        assert_eq!(sb.timeout_ms, 5_000);
    }

    #[test]
    fn language_returns_python() {
        use apex_core::traits::Sandbox;
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"));
        assert_eq!(sb.language(), Language::Python);
    }

    #[test]
    fn snapshot_not_supported() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            use apex_core::traits::Sandbox;
            sb.snapshot().await
        });
        assert!(err.is_err());
    }

    #[test]
    fn restore_not_supported() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/proj"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let err = rt.block_on(async {
            use apex_core::traits::Sandbox;
            sb.restore(SnapshotId::new()).await
        });
        assert!(err.is_err());
    }

    #[test]
    fn executed_branches_from_json_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().to_path_buf();
        let json_path = tmp.path().join("cov.json");

        let json = format!(
            r#"{{
  "files": {{
    "{}/src/app.py": {{
      "executed_branches": [[10, 12], [20, -1]],
      "missing_branches": [[30, 35]],
      "all_branches": [[10, 12], [20, -1], [30, 35]]
    }}
  }}
}}"#,
            target_dir.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), target_dir);
        let branches = sb.executed_branches_from_json(&json_path).unwrap();

        assert_eq!(branches.len(), 2);
        // [10, 12] → line 10, direction 0 (positive to_line)
        assert_eq!(branches[0].line, 10);
        assert_eq!(branches[0].direction, 0);
        // [20, -1] → line 20, direction 1 (negative to_line)
        assert_eq!(branches[1].line, 20);
        assert_eq!(branches[1].direction, 1);
    }

    #[test]
    fn executed_branches_from_json_strips_prefix() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().to_path_buf();
        let json_path = tmp.path().join("cov.json");

        let json = format!(
            r#"{{
  "files": {{
    "{}/src/mod.py": {{
      "executed_branches": [[1, 2]],
      "missing_branches": [],
      "all_branches": [[1, 2]]
    }}
  }}
}}"#,
            target_dir.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), target_dir);
        let branches = sb.executed_branches_from_json(&json_path).unwrap();

        // file_id should be based on relative path "src/mod.py"
        let expected_fid = fnv1a_hash("src/mod.py");
        assert_eq!(branches[0].file_id, expected_fid);
    }

    #[test]
    fn executed_branches_from_json_no_prefix_match() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");

        // File path does NOT share target_dir prefix
        let json = r#"{
  "files": {
    "/other/path/src/app.py": {
      "executed_branches": [[5, 10]],
      "missing_branches": [],
      "all_branches": [[5, 10]]
    }
  }
}"#;
        std::fs::write(&json_path, json).unwrap();

        let sb = PythonTestSandbox::new(
            make_oracle(),
            make_file_paths(),
            PathBuf::from("/nonexistent"),
        );
        let branches = sb.executed_branches_from_json(&json_path).unwrap();

        // Should still parse — uses full path as fallback
        assert_eq!(branches.len(), 1);
        let expected_fid = fnv1a_hash("/other/path/src/app.py");
        assert_eq!(branches[0].file_id, expected_fid);
    }

    #[test]
    fn executed_branches_from_json_empty_files() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        std::fs::write(&json_path, r#"{"files": {}}"#).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/x"));
        let branches = sb.executed_branches_from_json(&json_path).unwrap();
        assert!(branches.is_empty());
    }

    #[test]
    fn executed_branches_from_json_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        std::fs::write(&json_path, "not json at all").unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/x"));
        let result = sb.executed_branches_from_json(&json_path);
        assert!(result.is_err());
    }

    #[test]
    fn executed_branches_from_json_missing_file() {
        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), PathBuf::from("/x"));
        let result = sb.executed_branches_from_json(Path::new("/no/such/file.json"));
        assert!(result.is_err());
    }

    #[test]
    fn executed_branches_from_json_multiple_files() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().to_path_buf();
        let json_path = tmp.path().join("cov.json");

        let json = format!(
            r#"{{
  "files": {{
    "{td}/a.py": {{
      "executed_branches": [[1, 2]],
      "missing_branches": [],
      "all_branches": [[1, 2]]
    }},
    "{td}/b.py": {{
      "executed_branches": [[3, 4], [5, -1]],
      "missing_branches": [],
      "all_branches": [[3, 4], [5, -1]]
    }}
  }}
}}"#,
            td = target_dir.display()
        );
        std::fs::write(&json_path, &json).unwrap();

        let sb = PythonTestSandbox::new(make_oracle(), make_file_paths(), target_dir);
        let branches = sb.executed_branches_from_json(&json_path).unwrap();

        assert_eq!(branches.len(), 3);
    }
}
