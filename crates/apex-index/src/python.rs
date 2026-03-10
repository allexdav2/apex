use crate::types::{hash_source_files, BranchIndex, TestTrace};
use apex_core::types::{BranchId, ExecutionStatus, Language};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// coverage.py JSON schema (reused from apex-instrument)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ApexCoverageJson {
    files: HashMap<String, FileData>,
}

#[derive(Debug, Deserialize)]
struct FileData {
    executed_branches: Vec<[i64; 2]>,
    #[allow(dead_code)]
    missing_branches: Vec<[i64; 2]>,
    #[allow(dead_code)]
    all_branches: Vec<[i64; 2]>,
}

/// FNV-1a 64-bit hash (must match apex-instrument's implementation).
fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build a BranchIndex for a Python project by running each test individually
/// under coverage.py and collecting per-test branch data.
pub async fn build_python_index(
    target_root: &Path,
    parallelism: usize,
) -> Result<BranchIndex, Box<dyn std::error::Error + Send + Sync>> {
    let target_root = std::fs::canonicalize(target_root)?;
    info!(target = %target_root.display(), "building Python branch index");

    // 1. Enumerate tests
    let test_names = enumerate_tests(&target_root).await?;
    info!(count = test_names.len(), "discovered tests");

    if test_names.is_empty() {
        return Ok(empty_index(&target_root));
    }

    // 2. Run full suite once to get total branch set
    let (all_branches, file_paths) = run_full_coverage(&target_root).await?;
    info!(total = all_branches.len(), "total branches discovered");

    // 3. Run each test individually and collect traces
    let traces = run_per_test_coverage(&target_root, &test_names, parallelism).await?;

    // 4. Build profiles and index
    let profiles = BranchIndex::build_profiles(&traces);
    let covered_branches = profiles.len();
    let source_hash = hash_source_files(&target_root, Language::Python);

    let index = BranchIndex {
        traces,
        profiles,
        file_paths,
        total_branches: all_branches.len(),
        covered_branches,
        created_at: chrono_now(),
        language: Language::Python,
        target_root: target_root.clone(),
        source_hash,
    };

    info!(
        total = index.total_branches,
        covered = index.covered_branches,
        tests = index.traces.len(),
        "index built: {:.1}% coverage",
        index.coverage_percent()
    );

    Ok(index)
}

// ---------------------------------------------------------------------------
// Test enumeration
// ---------------------------------------------------------------------------

async fn enumerate_tests(
    target_root: &Path,
) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
    let output = tokio::process::Command::new("python3")
        .args(["-m", "pytest", "--collect-only", "-q", "--no-header"])
        .current_dir(target_root)
        .output()
        .await?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut tests = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        // pytest --collect-only -q outputs lines like "tests/test_foo.py::test_bar"
        if line.contains("::") && !line.starts_with("=") && !line.starts_with("-") {
            tests.push(line.to_string());
        }
    }

    if !output.status.success() && tests.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(stderr = %stderr, "pytest --collect-only failed");
    }

    Ok(tests)
}

// ---------------------------------------------------------------------------
// Full-suite coverage (for total branch set)
// ---------------------------------------------------------------------------

async fn run_full_coverage(
    target_root: &Path,
) -> Result<
    (Vec<BranchId>, HashMap<u64, PathBuf>),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let data_file = target_root.join(".apex_index_full_cov");
    let json_out = target_root.join(".apex_index_full_cov.json");

    // Run coverage on full suite
    let status = tokio::process::Command::new("python3")
        .args([
            "-m", "coverage", "run", "--branch",
            &format!("--data-file={}", data_file.display()),
            "-m", "pytest", "-q", "--tb=no", "--no-header",
        ])
        .current_dir(target_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;

    if !status.success() {
        debug!("full suite returned non-zero (coverage data may still exist)");
    }

    // Export to JSON
    let _ = tokio::process::Command::new("python3")
        .args([
            "-m", "coverage", "json",
            &format!("--data-file={}", data_file.display()),
            "-o", &json_out.to_string_lossy(),
        ])
        .current_dir(target_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    let (branches, file_paths) = parse_coverage_all_branches(&json_out, target_root)?;

    // Cleanup temp files
    let _ = std::fs::remove_file(&data_file);
    let _ = std::fs::remove_file(&json_out);

    Ok((branches, file_paths))
}

// ---------------------------------------------------------------------------
// Per-test coverage
// ---------------------------------------------------------------------------

async fn run_per_test_coverage(
    target_root: &Path,
    test_names: &[String],
    parallelism: usize,
) -> Result<Vec<TestTrace>, Box<dyn std::error::Error + Send + Sync>> {
    use tokio::sync::Semaphore;
    use std::sync::Arc;

    let semaphore = Arc::new(Semaphore::new(parallelism.max(1)));
    let mut handles = Vec::with_capacity(test_names.len());

    for (i, test_name) in test_names.iter().enumerate() {
        let sem = semaphore.clone();
        let root = target_root.to_path_buf();
        let name = test_name.clone();
        let idx = i;

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await.unwrap();
            run_single_test(&root, &name, idx).await
        });
        handles.push(handle);
    }

    let mut traces = Vec::with_capacity(test_names.len());
    for handle in handles {
        match handle.await? {
            Ok(trace) => traces.push(trace),
            Err(e) => warn!(error = %e, "failed to collect trace for one test"),
        }
    }

    Ok(traces)
}

async fn run_single_test(
    target_root: &Path,
    test_name: &str,
    idx: usize,
) -> Result<TestTrace, Box<dyn std::error::Error + Send + Sync>> {
    let data_file = target_root.join(format!(".apex_idx_test_{idx}"));
    let json_out = target_root.join(format!(".apex_idx_test_{idx}.json"));

    let start = std::time::Instant::now();

    let output = tokio::process::Command::new("python3")
        .args([
            "-m", "coverage", "run", "--branch",
            &format!("--data-file={}", data_file.display()),
            "-m", "pytest", "-q", "--tb=no", "--no-header", test_name,
        ])
        .current_dir(target_root)
        .output()
        .await?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let status = if output.status.success() {
        ExecutionStatus::Pass
    } else {
        ExecutionStatus::Fail
    };

    // Export to JSON
    let _ = tokio::process::Command::new("python3")
        .args([
            "-m", "coverage", "json",
            &format!("--data-file={}", data_file.display()),
            "-o", &json_out.to_string_lossy(),
        ])
        .current_dir(target_root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    let branches = parse_coverage_executed(&json_out, target_root).unwrap_or_default();

    debug!(
        test = test_name,
        branches = branches.len(),
        duration_ms,
        "collected trace"
    );

    // Cleanup
    let _ = std::fs::remove_file(&data_file);
    let _ = std::fs::remove_file(&json_out);

    Ok(TestTrace {
        test_name: test_name.to_string(),
        branches,
        duration_ms,
        status,
    })
}

// ---------------------------------------------------------------------------
// Coverage JSON parsing
// ---------------------------------------------------------------------------

/// Parse coverage JSON and return ALL branches (executed + missing).
fn parse_coverage_all_branches(
    json_path: &Path,
    repo_root: &Path,
) -> Result<(Vec<BranchId>, HashMap<u64, PathBuf>), Box<dyn std::error::Error + Send + Sync>> {
    let content = std::fs::read_to_string(json_path)?;
    let data: CoverageJsonRaw = serde_json::from_str(&content)?;
    let mut branches = Vec::new();
    let mut file_paths = HashMap::new();

    for (file_path, fdata) in &data.files {
        let rel = Path::new(file_path)
            .strip_prefix(repo_root)
            .unwrap_or(Path::new(file_path));
        let rel_str = rel.to_string_lossy();
        let file_id = fnv1a_hash(&rel_str);
        file_paths.insert(file_id, rel.to_path_buf());

        if let Some(executed) = fdata.get("executed_branches") {
            if let Some(arr) = executed.as_array() {
                for pair in arr {
                    if let Some(pair_arr) = pair.as_array() {
                        if pair_arr.len() == 2 {
                            let from = pair_arr[0].as_i64().unwrap_or(0);
                            let to = pair_arr[1].as_i64().unwrap_or(0);
                            let direction = if to < 0 { 1u8 } else { 0u8 };
                            branches.push(BranchId::new(file_id, from.unsigned_abs() as u32, 0, direction));
                        }
                    }
                }
            }
        }
        if let Some(missing) = fdata.get("missing_branches") {
            if let Some(arr) = missing.as_array() {
                for pair in arr {
                    if let Some(pair_arr) = pair.as_array() {
                        if pair_arr.len() == 2 {
                            let from = pair_arr[0].as_i64().unwrap_or(0);
                            let to = pair_arr[1].as_i64().unwrap_or(0);
                            let direction = if to < 0 { 1u8 } else { 0u8 };
                            branches.push(BranchId::new(file_id, from.unsigned_abs() as u32, 0, direction));
                        }
                    }
                }
            }
        }
    }

    Ok((branches, file_paths))
}

/// Raw coverage.py JSON envelope: {"files": {path: {data}}, "totals": {...}}
#[derive(Debug, Deserialize)]
struct CoverageJsonRaw {
    #[serde(default)]
    files: HashMap<String, HashMap<String, serde_json::Value>>,
}

/// Parse coverage JSON and return only EXECUTED branches.
fn parse_coverage_executed(
    json_path: &Path,
    repo_root: &Path,
) -> Result<Vec<BranchId>, Box<dyn std::error::Error + Send + Sync>> {
    let content = std::fs::read_to_string(json_path)?;

    // Try APEX format first (from apex_instrument.py)
    if let Ok(data) = serde_json::from_str::<ApexCoverageJson>(&content) {
        return Ok(parse_apex_format(&data, repo_root));
    }

    // Fall back to raw coverage.py JSON format
    let data: CoverageJsonRaw = serde_json::from_str(&content)?;
    let mut branches = Vec::new();

    for (file_path, fdata) in &data.files {
        let rel = Path::new(file_path)
            .strip_prefix(repo_root)
            .unwrap_or(Path::new(file_path));
        let file_id = fnv1a_hash(&rel.to_string_lossy());

        if let Some(executed) = fdata.get("executed_branches") {
            if let Some(arr) = executed.as_array() {
                for pair in arr {
                    if let Some(pair_arr) = pair.as_array() {
                        if pair_arr.len() == 2 {
                            let from = pair_arr[0].as_i64().unwrap_or(0);
                            let to = pair_arr[1].as_i64().unwrap_or(0);
                            let direction = if to < 0 { 1u8 } else { 0u8 };
                            branches.push(BranchId::new(
                                file_id,
                                from.unsigned_abs() as u32,
                                0,
                                direction,
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(branches)
}

fn parse_apex_format(data: &ApexCoverageJson, repo_root: &Path) -> Vec<BranchId> {
    let mut branches = Vec::new();
    for (file_path, fdata) in &data.files {
        let rel = Path::new(file_path)
            .strip_prefix(repo_root)
            .unwrap_or(Path::new(file_path));
        let file_id = fnv1a_hash(&rel.to_string_lossy());

        for pair in &fdata.executed_branches {
            let from = pair[0].unsigned_abs() as u32;
            let direction = if pair[1] < 0 { 1u8 } else { 0u8 };
            branches.push(BranchId::new(file_id, from, 0, direction));
        }
    }
    branches
}

fn chrono_now() -> String {
    // Simple ISO 8601 without chrono dependency
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", dur.as_secs())
}

fn empty_index(target_root: &Path) -> BranchIndex {
    BranchIndex {
        traces: vec![],
        profiles: HashMap::new(),
        file_paths: HashMap::new(),
        total_branches: 0,
        covered_branches: 0,
        created_at: chrono_now(),
        language: Language::Python,
        target_root: target_root.to_path_buf(),
        source_hash: hash_source_files(target_root, Language::Python),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_matches_instrument_crate() {
        // Must match apex-instrument's FNV-1a implementation
        assert_eq!(fnv1a_hash(""), 0xcbf2_9ce4_8422_2325);
        let h = fnv1a_hash("src/app.py");
        assert_ne!(h, 0);
    }

    #[test]
    fn parse_apex_format_works() {
        let json = r#"{
            "files": {
                "src/app.py": {
                    "executed_branches": [[10, 12], [20, -1]],
                    "missing_branches": [[10, -1]],
                    "all_branches": [[10, 12], [20, -1], [10, -1]]
                }
            }
        }"#;

        let data: ApexCoverageJson = serde_json::from_str(json).unwrap();
        let branches = parse_apex_format(&data, Path::new("/tmp"));
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].direction, 0); // 12 > 0 → true branch
        assert_eq!(branches[1].direction, 1); // -1 < 0 → false branch
    }

    #[test]
    fn parse_coverage_executed_apex_format() {
        let tmp = tempfile::tempdir().unwrap();
        let json_path = tmp.path().join("cov.json");
        let json = r#"{
            "files": {
                "mod.py": {
                    "executed_branches": [[5, 8], [10, -1]],
                    "missing_branches": [],
                    "all_branches": [[5, 8], [10, -1]]
                }
            }
        }"#;
        std::fs::write(&json_path, json).unwrap();

        let branches = parse_coverage_executed(&json_path, tmp.path()).unwrap();
        assert_eq!(branches.len(), 2);
    }

    #[test]
    fn parse_coverage_executed_missing_file() {
        let result = parse_coverage_executed(Path::new("/nonexistent.json"), Path::new("/tmp"));
        assert!(result.is_err());
    }
}
