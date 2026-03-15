//! Shared C/C++ coverage instrumentor using gcov or llvm-cov.
//!
//! Parses gcov text output to extract line-level coverage data and converts
//! it to `BranchId` entries using `fnv1a_hash` for file identification.

use apex_core::{
    error::Result,
    hash::fnv1a_hash,
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Coverage instrumentor for C and C++ projects.
///
/// Supports two compilation paths:
/// - **gcov:** compile with `-fprofile-arcs -ftest-coverage`, parse `.gcov` files
/// - **llvm-cov:** compile with `-fprofile-instr-generate -fcoverage-mapping`,
///   use `llvm-cov export`
///
/// Currently implements gcov text output parsing.
pub struct CCoverageInstrumentor {
    branch_ids: Vec<BranchId>,
}

impl CCoverageInstrumentor {
    pub fn new() -> Self {
        CCoverageInstrumentor {
            branch_ids: Vec::new(),
        }
    }
}

impl Default for CCoverageInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

/// A parsed line from gcov output.
#[derive(Debug, Clone, PartialEq)]
pub enum GcovLine {
    /// Non-executable line (marked with `-:`).
    NonExecutable,
    /// Unexecuted line (marked with `#####:`).
    Unexecuted { line_number: u32, source: String },
    /// Executed line with a count.
    Executed {
        count: u64,
        line_number: u32,
        source: String,
    },
}

/// Parse a single gcov output line.
///
/// Format: `execution_count:line_number:source_text`
/// - `-:` prefix means non-executable
/// - `#####:` prefix means unexecuted (0 count)
/// - `N:` where N is a number means executed N times
pub fn parse_gcov_line(line: &str) -> Option<GcovLine> {
    let colon1 = line.find(':')?;
    let count_str = line[..colon1].trim();

    let rest = &line[colon1 + 1..];
    let colon2 = rest.find(':')?;
    let line_num_str = rest[..colon2].trim();
    let source = rest[colon2 + 1..].to_string();

    if count_str == "-" {
        return Some(GcovLine::NonExecutable);
    }

    let line_number: u32 = line_num_str.parse().ok()?;

    if count_str == "#####" {
        return Some(GcovLine::Unexecuted {
            line_number,
            source,
        });
    }

    let count: u64 = count_str.parse().ok()?;
    Some(GcovLine::Executed {
        count,
        line_number,
        source,
    })
}

/// Parse full gcov output for a single file.
/// Returns (all_branches, executed_branches, file_id).
pub fn parse_gcov_output(
    file_path: &str,
    gcov_text: &str,
) -> (Vec<BranchId>, Vec<BranchId>, u64) {
    let file_id = fnv1a_hash(file_path);
    let mut all_branches = Vec::new();
    let mut executed_branches = Vec::new();

    for line in gcov_text.lines() {
        match parse_gcov_line(line) {
            Some(GcovLine::Executed {
                count,
                line_number,
                ..
            }) => {
                let branch = BranchId::new(file_id, line_number, 0, 0);
                all_branches.push(branch.clone());
                if count > 0 {
                    executed_branches.push(branch);
                }
            }
            Some(GcovLine::Unexecuted { line_number, .. }) => {
                let branch = BranchId::new(file_id, line_number, 0, 0);
                all_branches.push(branch);
            }
            _ => {}
        }
    }

    (all_branches, executed_branches, file_id)
}

/// Scan a directory for .gcov files and parse them all.
pub fn scan_gcov_files(dir: &Path) -> (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut all = Vec::new();
    let mut executed = Vec::new();
    let mut file_paths = HashMap::new();

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gcov") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    // The .gcov filename is typically source.c.gcov
                    let source_name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown");
                    let (a, e, fid) = parse_gcov_output(source_name, &content);
                    file_paths.insert(fid, PathBuf::from(source_name));
                    all.extend(a);
                    executed.extend(e);
                }
            }
        }
    }

    (all, executed, file_paths)
}

#[async_trait]
impl Instrumentor for CCoverageInstrumentor {
    fn branch_ids(&self) -> &[BranchId] {
        &self.branch_ids
    }

    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        // Look for pre-existing .gcov files in the target directory
        let (all_branches, executed_branches, file_paths) = scan_gcov_files(&target.root);

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids: all_branches,
            executed_branch_ids: executed_branches,
            file_paths,
            work_dir: target.root.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_non_executable_line() {
        let result = parse_gcov_line("        -:    0:Source:hello.c");
        assert_eq!(result, Some(GcovLine::NonExecutable));
    }

    #[test]
    fn parse_unexecuted_line() {
        let result = parse_gcov_line("    #####:    5:    return -1;");
        assert!(matches!(
            result,
            Some(GcovLine::Unexecuted {
                line_number: 5,
                ..
            })
        ));
    }

    #[test]
    fn parse_executed_line() {
        let result = parse_gcov_line("       10:    3:    x = x + 1;");
        assert!(matches!(
            result,
            Some(GcovLine::Executed {
                count: 10,
                line_number: 3,
                ..
            })
        ));
    }

    #[test]
    fn parse_executed_high_count() {
        let result = parse_gcov_line("  1000000:   42:    loop_body();");
        assert!(matches!(
            result,
            Some(GcovLine::Executed {
                count: 1000000,
                line_number: 42,
                ..
            })
        ));
    }

    #[test]
    fn parse_gcov_output_mixed() {
        let gcov = "\
        -:    0:Source:test.c\n\
        -:    1:#include <stdio.h>\n\
       10:    2:int main() {\n\
        5:    3:    int x = 0;\n\
    #####:    4:    if (x > 0) {\n\
    #####:    5:        printf(\"positive\");\n\
        5:    6:    }\n\
        5:    7:    return 0;\n\
        -:    8:}";

        let (all, executed, _file_id) = parse_gcov_output("test.c", gcov);
        assert_eq!(all.len(), 6); // lines 2,3,4,5,6,7
        assert_eq!(executed.len(), 4); // lines 2,3,6,7
    }

    #[test]
    fn parse_gcov_output_empty() {
        let (all, executed, _) = parse_gcov_output("empty.c", "");
        assert!(all.is_empty());
        assert!(executed.is_empty());
    }

    #[test]
    fn parse_gcov_output_all_executed() {
        let gcov = "\
        1:    1:int main() {\n\
        1:    2:    return 0;\n\
        -:    3:}";
        let (all, executed, _) = parse_gcov_output("main.c", gcov);
        assert_eq!(all.len(), 2);
        assert_eq!(executed.len(), 2);
    }

    #[test]
    fn parse_gcov_output_uses_fnv1a() {
        let (all, _, file_id) = parse_gcov_output("src/lib.c", "    1:    1:code");
        assert_eq!(file_id, fnv1a_hash("src/lib.c"));
        assert_eq!(all[0].file_id, file_id);
    }

    #[test]
    fn scan_gcov_files_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let (all, executed, paths) = scan_gcov_files(tmp.path());
        assert!(all.is_empty());
        assert!(executed.is_empty());
        assert!(paths.is_empty());
    }

    #[test]
    fn scan_gcov_files_with_gcov_file() {
        let tmp = tempfile::tempdir().unwrap();
        let gcov_content = "    1:    1:int main() {\n    1:    2:    return 0;\n";
        std::fs::write(tmp.path().join("main.c.gcov"), gcov_content).unwrap();

        let (all, executed, paths) = scan_gcov_files(tmp.path());
        assert_eq!(all.len(), 2);
        assert_eq!(executed.len(), 2);
        assert_eq!(paths.len(), 1);
        // The file stem of "main.c.gcov" is "main.c"
        assert!(paths.values().any(|p| p.to_str() == Some("main.c")));
    }

    #[test]
    fn instrumentor_default() {
        let instr = CCoverageInstrumentor::default();
        // Just verify it constructs without panic
        let _ = instr;
    }

    #[test]
    fn parse_gcov_line_invalid() {
        assert!(parse_gcov_line("not a valid line").is_none());
    }

    #[tokio::test]
    async fn instrumentor_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let target = Target {
            root: tmp.path().to_path_buf(),
            language: apex_core::types::Language::C,
            test_command: Vec::new(),
        };
        let instr = CCoverageInstrumentor::new();
        let result = instr.instrument(&target).await.unwrap();
        assert!(result.branch_ids.is_empty());
        assert!(result.executed_branch_ids.is_empty());
    }
}
