use crate::types::{branch_key, BranchIndex};
use apex_core::types::BranchId;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Flaky detection
// ---------------------------------------------------------------------------

/// A flaky test: same test, different branch sets across runs.
#[derive(Debug, Clone, Serialize)]
pub struct FlakyTest {
    pub test_name: String,
    /// Branches that appear in some runs but not others.
    pub divergent_branches: Vec<DivergentBranch>,
    /// Number of runs where divergence was observed.
    pub divergent_runs: usize,
    pub total_runs: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DivergentBranch {
    pub branch: BranchId,
    pub file_path: Option<PathBuf>,
    /// How many of N runs hit this branch.
    pub hit_ratio: String,
}

/// Analyze multiple traces of the same tests to find nondeterminism.
pub fn detect_flaky_tests(
    runs: &[Vec<crate::TestTrace>],
    file_paths: &HashMap<u64, PathBuf>,
) -> Vec<FlakyTest> {
    if runs.is_empty() {
        return vec![];
    }

    // Group traces by test name across runs
    let mut test_runs: HashMap<&str, Vec<HashSet<String>>> = HashMap::new();

    for run in runs {
        for trace in run {
            let keys: HashSet<String> = trace.branches.iter().map(branch_key).collect();
            test_runs
                .entry(&trace.test_name)
                .or_default()
                .push(keys);
        }
    }

    let mut flaky = Vec::new();

    for (test_name, branch_sets) in &test_runs {
        if branch_sets.len() < 2 {
            continue;
        }

        // Find branches that aren't in every run
        let union: HashSet<&String> = branch_sets.iter().flat_map(|s| s.iter()).collect();
        let intersection: HashSet<&String> = branch_sets[0]
            .iter()
            .filter(|k| branch_sets.iter().all(|s| s.contains(*k)))
            .collect();

        let divergent_keys: Vec<&String> = union.difference(&intersection).copied().collect();

        if !divergent_keys.is_empty() {
            let total_runs = branch_sets.len();
            let divergent_branches: Vec<DivergentBranch> = divergent_keys
                .iter()
                .map(|key| {
                    let hits = branch_sets.iter().filter(|s| s.contains(*key)).count();
                    // Parse branch from key (file_id:line:col:direction:condition)
                    let parts: Vec<&str> = key.split(':').collect();
                    let file_id: u64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
                    let line: u32 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let direction: u8 = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);

                    DivergentBranch {
                        branch: BranchId::new(file_id, line, 0, direction),
                        file_path: file_paths.get(&file_id).cloned(),
                        hit_ratio: format!("{}/{}", hits, total_runs),
                    }
                })
                .collect();

            flaky.push(FlakyTest {
                test_name: test_name.to_string(),
                divergent_branches,
                divergent_runs: total_runs,
                total_runs,
            });
        }
    }

    flaky.sort_by(|a, b| b.divergent_branches.len().cmp(&a.divergent_branches.len()));
    flaky
}

// ---------------------------------------------------------------------------
// Complexity analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct FunctionComplexity {
    pub file_path: PathBuf,
    pub function_name: String,
    pub line: u32,
    /// Total branches in this function (static complexity).
    pub static_complexity: usize,
    /// Branches actually exercised by tests.
    pub exercised_complexity: usize,
    /// Ratio: exercised / static.
    pub exercise_ratio: f64,
    /// Classification based on the ratio.
    pub classification: String,
}

/// Analyze exercised vs static complexity per function.
pub fn analyze_complexity(
    index: &BranchIndex,
    target_root: &Path,
) -> Vec<FunctionComplexity> {
    let mut results = Vec::new();

    // Read source files and find function boundaries
    for (file_id, rel_path) in &index.file_paths {
        let full_path = target_root.join(rel_path);
        let source = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lines: Vec<&str> = source.lines().collect();
        let functions = extract_functions(&lines, index.language);

        // Get branches in this file from profiles
        let file_profiles: Vec<_> = index
            .profiles
            .values()
            .filter(|p| p.branch.file_id == *file_id)
            .collect();

        for (func_name, func_start, func_end) in &functions {
            let in_function: Vec<_> = file_profiles
                .iter()
                .filter(|p| p.branch.line >= *func_start && p.branch.line <= *func_end)
                .collect();

            let static_count = in_function.len();
            let exercised_count = in_function.iter().filter(|p| p.hit_count > 0).count();

            if static_count == 0 {
                continue;
            }

            let ratio = exercised_count as f64 / static_count as f64;
            let classification = if ratio >= 0.9 {
                "fully-exercised"
            } else if ratio >= 0.5 {
                "partially-tested"
            } else if ratio > 0.0 {
                "under-tested"
            } else {
                "dead"
            };

            results.push(FunctionComplexity {
                file_path: rel_path.clone(),
                function_name: func_name.clone(),
                line: *func_start,
                static_complexity: static_count,
                exercised_complexity: exercised_count,
                exercise_ratio: ratio,
                classification: classification.into(),
            });
        }
    }

    results.sort_by(|a, b| {
        a.exercise_ratio
            .partial_cmp(&b.exercise_ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    results
}

/// Extract function names and line ranges from source code.
fn extract_functions(
    lines: &[&str],
    language: apex_core::types::Language,
) -> Vec<(String, u32, u32)> {
    let mut functions = Vec::new();

    let func_pattern: &[&str] = match language {
        apex_core::types::Language::Python => &["def "],
        apex_core::types::Language::Rust => &["fn "],
        apex_core::types::Language::JavaScript => &["function ", "=> {"],
        apex_core::types::Language::Java => &["void ", "public ", "private ", "protected "],
        apex_core::types::Language::Ruby => &["def "],
        _ => &["fn "],
    };

    let mut current_func: Option<(String, u32)> = None;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let line_num = (i + 1) as u32;

        let is_func_start = func_pattern.iter().any(|p| trimmed.contains(p))
            && !trimmed.starts_with('#')
            && !trimmed.starts_with("//")
            && !trimmed.starts_with("///");

        if is_func_start {
            // Close previous function
            if let Some((name, start)) = current_func.take() {
                functions.push((name, start, line_num - 1));
            }

            // Extract function name
            let name = extract_func_name(trimmed, language);
            current_func = Some((name, line_num));
        }
    }

    // Close last function
    if let Some((name, start)) = current_func {
        functions.push((name, start, lines.len() as u32));
    }

    functions
}

fn extract_func_name(line: &str, language: apex_core::types::Language) -> String {
    match language {
        apex_core::types::Language::Python => {
            // "def foo(...):"
            line.trim()
                .strip_prefix("def ")
                .and_then(|s| s.split('(').next())
                .unwrap_or("unknown")
                .trim()
                .to_string()
        }
        apex_core::types::Language::Rust => {
            // "pub async fn foo(...)"
            let s = line.trim();
            let after_fn = s
                .find("fn ")
                .map(|i| &s[i + 3..])
                .unwrap_or("unknown");
            after_fn
                .split(|c: char| c == '(' || c == '<' || c.is_whitespace())
                .next()
                .unwrap_or("unknown")
                .to_string()
        }
        _ => {
            // Generic: find first identifier-like token after keyword
            let tokens: Vec<&str> = line.split_whitespace().collect();
            tokens
                .iter()
                .find(|t| {
                    t.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false)
                        && !["pub", "async", "fn", "def", "function", "void", "public", "private", "protected", "static"]
                            .contains(t)
                })
                .map(|t| t.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_'))
                .unwrap_or("unknown")
                .to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Documentation generation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDoc {
    pub file_path: PathBuf,
    pub function_name: String,
    pub line: u32,
    pub paths: Vec<ExecutionPath>,
    pub total_tests: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutionPath {
    /// Branches taken in this path.
    pub branch_count: usize,
    /// Representative test that exercises this path.
    pub representative_test: String,
    /// Percentage of tests that follow this path.
    pub frequency_pct: f64,
    /// Number of tests following this path.
    pub test_count: usize,
}

/// Generate behavioral documentation from execution traces.
pub fn generate_docs(
    index: &BranchIndex,
    target_root: &Path,
) -> Vec<FunctionDoc> {
    let mut docs = Vec::new();

    for (file_id, rel_path) in &index.file_paths {
        let full_path = target_root.join(rel_path);
        let source = match std::fs::read_to_string(&full_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lines: Vec<&str> = source.lines().collect();
        let functions = extract_functions(&lines, index.language);

        for (func_name, func_start, func_end) in &functions {
            // For each test, compute its "path signature" within this function
            // (set of branches taken in this function's line range)
            let mut path_groups: HashMap<Vec<String>, Vec<&str>> = HashMap::new();

            for trace in &index.traces {
                let func_branches: Vec<String> = trace
                    .branches
                    .iter()
                    .filter(|b| {
                        b.file_id == *file_id && b.line >= *func_start && b.line <= *func_end
                    })
                    .map(branch_key)
                    .collect();

                if func_branches.is_empty() {
                    continue; // test doesn't touch this function
                }

                let mut sorted = func_branches;
                sorted.sort();
                path_groups
                    .entry(sorted)
                    .or_default()
                    .push(&trace.test_name);
            }

            if path_groups.is_empty() {
                continue;
            }

            let total_tests: usize = path_groups.values().map(|v| v.len()).sum();
            let mut paths: Vec<ExecutionPath> = path_groups
                .iter()
                .map(|(branches, tests)| {
                    ExecutionPath {
                        branch_count: branches.len(),
                        representative_test: tests[0].to_string(),
                        frequency_pct: (tests.len() as f64 / total_tests as f64) * 100.0,
                        test_count: tests.len(),
                    }
                })
                .collect();

            paths.sort_by(|a, b| {
                b.test_count
                    .cmp(&a.test_count)
                    .then(a.branch_count.cmp(&b.branch_count))
            });

            docs.push(FunctionDoc {
                file_path: rel_path.clone(),
                function_name: func_name.clone(),
                line: *func_start,
                paths,
                total_tests,
            });
        }
    }

    docs.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line.cmp(&b.line))
    });

    docs
}

// ---------------------------------------------------------------------------
// Attack surface analysis
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct AttackSurfaceReport {
    pub entry_pattern: String,
    pub entry_tests: usize,
    pub reachable_branches: usize,
    pub reachable_files: usize,
    pub total_branches: usize,
    pub attack_surface_pct: f64,
    pub reachable_file_details: Vec<ReachableFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReachableFile {
    pub file_path: PathBuf,
    pub reachable_branches: usize,
    pub total_branches_in_file: usize,
    pub coverage_pct: f64,
}

/// Map attack surface from entry-point test reachability.
pub fn analyze_attack_surface(
    index: &BranchIndex,
    entry_pattern: &str,
) -> AttackSurfaceReport {
    // Filter tests matching entry pattern
    let entry_traces: Vec<_> = index
        .traces
        .iter()
        .filter(|t| t.test_name.contains(entry_pattern))
        .collect();

    // Union of all branches reachable from entry-point tests
    let reachable: HashSet<String> = entry_traces
        .iter()
        .flat_map(|t| t.branches.iter().map(branch_key))
        .collect();

    // Group by file
    let mut file_reachable: HashMap<u64, HashSet<String>> = HashMap::new();
    for trace in &entry_traces {
        for branch in &trace.branches {
            file_reachable
                .entry(branch.file_id)
                .or_default()
                .insert(branch_key(branch));
        }
    }

    // Total branches per file from all profiles
    let mut file_totals: HashMap<u64, usize> = HashMap::new();
    for profile in index.profiles.values() {
        *file_totals.entry(profile.branch.file_id).or_default() += 1;
    }

    let mut reachable_files: Vec<ReachableFile> = file_reachable
        .iter()
        .map(|(file_id, branches)| {
            let total = file_totals.get(file_id).copied().unwrap_or(0);
            let path = index
                .file_paths
                .get(file_id)
                .cloned()
                .unwrap_or_else(|| PathBuf::from(format!("<{:016x}>", file_id)));
            ReachableFile {
                file_path: path,
                reachable_branches: branches.len(),
                total_branches_in_file: total,
                coverage_pct: if total > 0 {
                    (branches.len() as f64 / total as f64) * 100.0
                } else {
                    0.0
                },
            }
        })
        .collect();

    reachable_files.sort_by(|a, b| b.reachable_branches.cmp(&a.reachable_branches));

    let attack_surface_pct = if index.total_branches > 0 {
        (reachable.len() as f64 / index.total_branches as f64) * 100.0
    } else {
        0.0
    };

    AttackSurfaceReport {
        entry_pattern: entry_pattern.to_string(),
        entry_tests: entry_traces.len(),
        reachable_branches: reachable.len(),
        reachable_files: file_reachable.len(),
        total_branches: index.total_branches,
        attack_surface_pct,
        reachable_file_details: reachable_files,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TestTrace;
    use apex_core::types::ExecutionStatus;

    fn br(file_id: u64, line: u32, dir: u8) -> BranchId {
        BranchId::new(file_id, line, 0, dir)
    }

    #[test]
    fn flaky_detect_no_divergence() {
        let run1 = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let run2 = vec![TestTrace {
            test_name: "test_a".into(),
            branches: vec![br(1, 10, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];

        let flaky = detect_flaky_tests(&[run1, run2], &HashMap::new());
        assert!(flaky.is_empty());
    }

    #[test]
    fn flaky_detect_finds_divergence() {
        let run1 = vec![TestTrace {
            test_name: "test_flaky".into(),
            branches: vec![br(1, 10, 0), br(1, 20, 0)],
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];
        let run2 = vec![TestTrace {
            test_name: "test_flaky".into(),
            branches: vec![br(1, 10, 0), br(1, 20, 1)], // direction changed!
            duration_ms: 50,
            status: ExecutionStatus::Pass,
        }];

        let flaky = detect_flaky_tests(&[run1, run2], &HashMap::new());
        assert_eq!(flaky.len(), 1);
        assert_eq!(flaky[0].test_name, "test_flaky");
        assert!(flaky[0].divergent_branches.len() >= 1);
    }

    #[test]
    fn flaky_detect_empty_runs() {
        let flaky = detect_flaky_tests(&[], &HashMap::new());
        assert!(flaky.is_empty());
    }

    #[test]
    fn extract_func_name_python() {
        let name = extract_func_name("def process_order(order):", apex_core::types::Language::Python);
        assert_eq!(name, "process_order");
    }

    #[test]
    fn extract_func_name_rust() {
        let name = extract_func_name("pub async fn handle_request(req: Request) -> Response {", apex_core::types::Language::Rust);
        assert_eq!(name, "handle_request");
    }

    #[test]
    fn extract_func_name_python_no_args() {
        let name = extract_func_name("def setup():", apex_core::types::Language::Python);
        assert_eq!(name, "setup");
    }

    #[test]
    fn attack_surface_empty_pattern() {
        let index = BranchIndex {
            traces: vec![TestTrace {
                test_name: "test_internal".into(),
                branches: vec![br(1, 10, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            }],
            profiles: BranchIndex::build_profiles(&[TestTrace {
                test_name: "test_internal".into(),
                branches: vec![br(1, 10, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            }]),
            file_paths: HashMap::from([(1, PathBuf::from("src/lib.py"))]),
            total_branches: 5,
            covered_branches: 1,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.entry_tests, 0);
        assert_eq!(report.reachable_branches, 0);
    }

    #[test]
    fn attack_surface_matches_pattern() {
        let traces = vec![
            TestTrace {
                test_name: "test_api_login".into(),
                branches: vec![br(1, 10, 0), br(2, 5, 0)],
                duration_ms: 50,
                status: ExecutionStatus::Pass,
            },
            TestTrace {
                test_name: "test_internal_helper".into(),
                branches: vec![br(3, 20, 0)],
                duration_ms: 30,
                status: ExecutionStatus::Pass,
            },
        ];

        let index = BranchIndex {
            profiles: BranchIndex::build_profiles(&traces),
            traces,
            file_paths: HashMap::from([
                (1, PathBuf::from("src/api.py")),
                (2, PathBuf::from("src/auth.py")),
                (3, PathBuf::from("src/internal.py")),
            ]),
            total_branches: 10,
            covered_branches: 3,
            created_at: String::new(),
            language: apex_core::types::Language::Python,
            target_root: PathBuf::new(),
            source_hash: String::new(),
        };

        let report = analyze_attack_surface(&index, "test_api");
        assert_eq!(report.entry_tests, 1);
        assert_eq!(report.reachable_branches, 2);
        assert_eq!(report.reachable_files, 2);
    }
}
