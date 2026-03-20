//! Unified LLVM source-based coverage backend.
//!
//! All compiled languages that use LLVM (C, C++, Rust, Swift) produce the
//! same `llvm-cov export --format=json` output. This module provides:
//!
//! 1. A single JSON parser ([`parse_llvm_cov_export`])
//! 2. Tool resolution ([`resolve_llvm_tools`])
//! 3. A unified pipeline struct ([`LlvmCoverageBackend`])
//!
//! ## Segment filtering
//!
//! Each segment is `[line, col, count, has_count, is_region_entry, is_gap_region]`.
//! We keep only segments where `has_count=true AND is_region_entry=true AND is_gap=false`.
//! This matches the Rust parser (the most correct of the three legacy parsers).
//!
//! ## Bool/int compatibility
//!
//! Different LLVM versions encode booleans as `true`/`false` or `0`/`1`.
//! The [`json_truthy`] helper handles both encodings uniformly.

use apex_core::{
    error::{ApexError, Result},
    hash::fnv1a_hash as fnv1a,
    types::BranchId,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// LlvmTools — resolved tool paths
// ---------------------------------------------------------------------------

/// Resolved paths (or commands) for LLVM coverage tools.
#[derive(Debug, Clone)]
pub struct LlvmTools {
    /// Command to invoke llvm-profdata, e.g. `"llvm-profdata"` or `"xcrun llvm-profdata"`.
    pub profdata: String,
    /// Command to invoke llvm-cov, e.g. `"llvm-cov"` or `"xcrun llvm-cov"`.
    pub llvm_cov: String,
}

/// Resolve LLVM tool paths by checking (in order):
///
/// 1. `LLVM_PROFDATA` / `LLVM_COV` environment variables (explicit override)
/// 2. `llvm-profdata` / `llvm-cov` directly on PATH
/// 3. `xcrun llvm-profdata` / `xcrun llvm-cov` (macOS Xcode / CommandLineTools)
///
/// Returns an error if neither tool can be found.
pub fn resolve_llvm_tools() -> Result<LlvmTools> {
    let profdata = resolve_single_tool("LLVM_PROFDATA", "llvm-profdata")?;
    let llvm_cov = resolve_single_tool("LLVM_COV", "llvm-cov")?;
    Ok(LlvmTools { profdata, llvm_cov })
}

/// Resolve a single LLVM tool by env var, PATH, or xcrun.
fn resolve_single_tool(env_var: &str, tool_name: &str) -> Result<String> {
    // 1. Environment variable override
    if let Ok(path) = std::env::var(env_var) {
        if !path.is_empty() {
            return Ok(path);
        }
    }

    // 2. Direct on PATH
    if tool_on_path(tool_name) {
        return Ok(tool_name.to_string());
    }

    // 3. xcrun (macOS)
    if tool_on_path("xcrun") && xcrun_has_tool(tool_name) {
        return Ok(format!("xcrun {tool_name}"));
    }

    Err(ApexError::Instrumentation(format!(
        "{tool_name} not found. Set {env_var} or ensure {tool_name} is on PATH. \
         On macOS, install Xcode CommandLineTools: xcode-select --install"
    )))
}

/// Check if a tool exists on PATH via `which`.
fn tool_on_path(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if xcrun can find a tool.
fn xcrun_has_tool(name: &str) -> bool {
    std::process::Command::new("xcrun")
        .args(["--find", name])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// LlvmCoverageBackend — unified pipeline
// ---------------------------------------------------------------------------

/// Unified LLVM source-based coverage pipeline for C/C++/Rust/Swift.
///
/// Manages output paths for profraw, profdata, and JSON files. Language-specific
/// compilation and test execution is handled externally; this struct handles
/// merge, export, and parsing.
pub struct LlvmCoverageBackend {
    /// Root directory of the target project.
    pub target_root: PathBuf,
    /// Resolved LLVM tool paths.
    pub tools: LlvmTools,
    /// Directory where `.profraw` files are collected.
    pub profraw_dir: PathBuf,
    /// Path for the merged `.profdata` file.
    pub profdata_path: PathBuf,
    /// Path for the exported JSON coverage report.
    pub json_path: PathBuf,
}

impl LlvmCoverageBackend {
    /// Create a new backend for the given target root.
    ///
    /// Resolves LLVM tools and sets up output paths under `<target>/.apex/`.
    pub fn new(target: &Path) -> Result<Self> {
        let tools = resolve_llvm_tools()?;
        let apex_dir = target.join(".apex");
        Ok(Self {
            target_root: target.to_path_buf(),
            tools,
            profraw_dir: apex_dir.join("profraw"),
            profdata_path: apex_dir.join("coverage.profdata"),
            json_path: apex_dir.join("coverage").join("llvm-cov.json"),
        })
    }

    /// Create a backend with pre-resolved tools (useful for testing).
    pub fn with_tools(target: &Path, tools: LlvmTools) -> Self {
        let apex_dir = target.join(".apex");
        Self {
            target_root: target.to_path_buf(),
            tools,
            profraw_dir: apex_dir.join("profraw"),
            profdata_path: apex_dir.join("coverage.profdata"),
            json_path: apex_dir.join("coverage").join("llvm-cov.json"),
        }
    }

    /// Merge `.profraw` files into a single `.profdata` file.
    ///
    /// Runs: `llvm-profdata merge -sparse <profraw_dir>/*.profraw -o <profdata_path>`
    pub async fn merge_profraw(&self) -> Result<()> {
        std::fs::create_dir_all(&self.profraw_dir).map_err(|e| {
            ApexError::Instrumentation(format!("create profraw dir: {e}"))
        })?;

        // Collect .profraw files
        let profraw_files: Vec<PathBuf> = std::fs::read_dir(&self.profraw_dir)
            .map_err(|e| ApexError::Instrumentation(format!("read profraw dir: {e}")))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "profraw"))
            .collect();

        if profraw_files.is_empty() {
            return Err(ApexError::Instrumentation(
                "no .profraw files found in profraw directory".into(),
            ));
        }

        // Build command: handle "xcrun llvm-profdata" as two tokens
        let (program, prefix_args) = split_command(&self.tools.profdata);

        let profraw_strs: Vec<String> = profraw_files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();

        let mut args: Vec<&str> = prefix_args.iter().map(|s| s.as_str()).collect();
        args.push("merge");
        args.push("-sparse");
        for s in &profraw_strs {
            args.push(s);
        }
        args.push("-o");
        let profdata_str = self.profdata_path.to_string_lossy().into_owned();
        args.push(&profdata_str);

        if let Some(parent) = self.profdata_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        let output = tokio::process::Command::new(program)
            .args(&args)
            .output()
            .await
            .map_err(|e| ApexError::Instrumentation(format!("llvm-profdata merge: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ApexError::Instrumentation(format!(
                "llvm-profdata merge failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr
            )));
        }

        Ok(())
    }

    /// Export coverage as JSON using `llvm-cov export`.
    ///
    /// Runs: `llvm-cov export <binary> -instr-profile=<profdata> --format=text`
    pub async fn export_json(&self, binary_path: &Path) -> Result<()> {
        let (program, prefix_args) = split_command(&self.tools.llvm_cov);

        let profdata_str = format!("-instr-profile={}", self.profdata_path.display());
        let binary_str = binary_path.to_string_lossy().into_owned();

        let mut args: Vec<&str> = prefix_args.iter().map(|s| s.as_str()).collect();
        args.extend(["export", &binary_str, &profdata_str, "--format=text"]);

        let output = tokio::process::Command::new(program)
            .args(&args)
            .current_dir(&self.target_root)
            .output()
            .await
            .map_err(|e| ApexError::Instrumentation(format!("llvm-cov export: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ApexError::Instrumentation(format!(
                "llvm-cov export failed (exit {}): {}",
                output.status.code().unwrap_or(-1),
                stderr
            )));
        }

        if let Some(parent) = self.json_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        std::fs::write(&self.json_path, &output.stdout).map_err(|e| {
            ApexError::Instrumentation(format!("write coverage JSON: {e}"))
        })?;

        Ok(())
    }

    /// Parse the exported JSON into branch IDs.
    ///
    /// Reads `self.json_path` and delegates to [`parse_llvm_cov_export`].
    #[allow(clippy::type_complexity)]
    pub fn parse(
        &self,
    ) -> std::result::Result<
        (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let json = std::fs::read_to_string(&self.json_path)?;
        parse_llvm_cov_export(&json, &self.target_root)
    }
}

/// Split a command string like `"xcrun llvm-profdata"` into program + prefix args.
fn split_command(cmd: &str) -> (&str, Vec<String>) {
    let parts: Vec<&str> = cmd.split_whitespace().collect();
    if parts.len() <= 1 {
        (cmd.trim(), Vec::new())
    } else {
        (
            parts[0],
            parts[1..].iter().map(|s| s.to_string()).collect(),
        )
    }
}

// ---------------------------------------------------------------------------
// Unified JSON parser
// ---------------------------------------------------------------------------

/// Interpret a JSON value as a boolean, supporting both `true`/`false` literals
/// and integer `0`/`1` (different LLVM versions use different encodings).
pub fn json_truthy(val: &serde_json::Value) -> bool {
    match val {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    }
}

/// Parse `llvm-cov export --format=json` output into branch coverage data.
///
/// The JSON schema (identical for C, C++, Rust, and Swift):
/// ```json
/// {
///   "data": [{
///     "files": [{
///       "filename": "src/main.rs",
///       "segments": [
///         [line, col, count, has_count, is_region_entry, is_gap_region]
///       ]
///     }]
///   }]
/// }
/// ```
///
/// ## Filtering
///
/// Only segments with all 6 fields where `has_count=true AND is_region_entry=true
/// AND is_gap=false` are treated as coverable units. Each such segment maps to
/// a single `BranchId` with `direction=0`.
///
/// - `count > 0` means executed
/// - `count == 0` means coverable but not executed
///
/// ## Compatibility
///
/// Boolean fields may be encoded as `true`/`false` or `0`/`1` depending on
/// LLVM version. The [`json_truthy`] helper handles both.
///
/// ## File filtering
///
/// Files whose absolute path does not start with `target_root` are skipped
/// (e.g. stdlib, external deps).
#[allow(clippy::type_complexity)]
pub fn parse_llvm_cov_export(
    json_str: &str,
    target_root: &Path,
) -> std::result::Result<
    (Vec<BranchId>, Vec<BranchId>, HashMap<u64, PathBuf>),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let v: serde_json::Value = serde_json::from_str(json_str)?;

    let mut branch_ids: Vec<BranchId> = Vec::new();
    let mut executed_ids: Vec<BranchId> = Vec::new();
    let mut file_paths: HashMap<u64, PathBuf> = HashMap::new();

    let data = v["data"]
        .as_array()
        .ok_or("missing or invalid 'data' array")?;

    for entry in data {
        let files = entry["files"]
            .as_array()
            .ok_or("missing or invalid 'files' array")?;

        for file in files {
            let filename = file["filename"]
                .as_str()
                .ok_or("missing or invalid 'filename'")?;

            // Skip files outside the target root (stdlib, deps, etc.)
            let abs = Path::new(filename);
            let rel = match abs.strip_prefix(target_root) {
                Ok(r) => r.to_path_buf(),
                Err(_) => continue,
            };

            let fid = fnv1a(&rel.to_string_lossy());
            file_paths.entry(fid).or_insert_with(|| rel.clone());

            let segments = file["segments"]
                .as_array()
                .ok_or("missing or invalid 'segments' array")?;

            for seg in segments {
                let arr = seg.as_array().ok_or("segment is not an array")?;
                if arr.len() < 6 {
                    continue;
                }

                let line = arr[0].as_u64().unwrap_or(0) as u32;
                let col = arr[1].as_u64().unwrap_or(0).min(u16::MAX as u64) as u16;
                let count = arr[2].as_u64().unwrap_or(0);
                let has_count = json_truthy(&arr[3]);
                let is_entry = json_truthy(&arr[4]);
                let is_gap = json_truthy(&arr[5]);

                if !has_count || !is_entry || is_gap {
                    continue;
                }

                let bid = BranchId::new(fid, line, col, 0);
                branch_ids.push(bid.clone());
                if count > 0 {
                    executed_ids.push(bid);
                }
            }
        }
    }

    // Deduplicate (stable order within each file).
    branch_ids.sort_by_key(|b| (b.file_id, b.line, b.col));
    branch_ids.dedup();
    executed_ids.sort_by_key(|b| (b.file_id, b.line, b.col));
    executed_ids.dedup();

    Ok((branch_ids, executed_ids, file_paths))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- json_truthy ---------------------------------------------------------

    #[test]
    fn json_truthy_with_bool_true() {
        assert!(json_truthy(&serde_json::Value::Bool(true)));
    }

    #[test]
    fn json_truthy_with_bool_false() {
        assert!(!json_truthy(&serde_json::Value::Bool(false)));
    }

    #[test]
    fn json_truthy_with_int_one() {
        assert!(json_truthy(&serde_json::json!(1)));
    }

    #[test]
    fn json_truthy_with_int_zero() {
        assert!(!json_truthy(&serde_json::json!(0)));
    }

    #[test]
    fn json_truthy_with_negative_int() {
        // -1 is truthy (non-zero)
        assert!(json_truthy(&serde_json::json!(-1)));
    }

    #[test]
    fn json_truthy_with_null() {
        assert!(!json_truthy(&serde_json::Value::Null));
    }

    #[test]
    fn json_truthy_with_string() {
        assert!(!json_truthy(&serde_json::json!("true")));
    }

    // -- resolve_llvm_tools --------------------------------------------------

    #[test]
    fn resolve_llvm_tools_finds_tools_or_returns_clear_error() {
        // On most dev machines at least one path will work.
        // If neither is available, the error should mention what's missing.
        match resolve_llvm_tools() {
            Ok(tools) => {
                assert!(!tools.profdata.is_empty());
                assert!(!tools.llvm_cov.is_empty());
            }
            Err(e) => {
                let msg = format!("{e}");
                assert!(
                    msg.contains("not found"),
                    "error should say 'not found', got: {msg}"
                );
            }
        }
    }

    #[test]
    fn resolve_single_tool_respects_env_var() {
        // Temporarily set an env var
        std::env::set_var("TEST_LLVM_TOOL_OVERRIDE", "/custom/llvm-profdata");
        let result = resolve_single_tool("TEST_LLVM_TOOL_OVERRIDE", "llvm-profdata");
        std::env::remove_var("TEST_LLVM_TOOL_OVERRIDE");
        assert_eq!(result.unwrap(), "/custom/llvm-profdata");
    }

    #[test]
    fn resolve_single_tool_empty_env_var_skipped() {
        std::env::set_var("TEST_LLVM_EMPTY_VAR", "");
        let result = resolve_single_tool("TEST_LLVM_EMPTY_VAR", "nonexistent_tool_xyz_99999");
        std::env::remove_var("TEST_LLVM_EMPTY_VAR");
        // Should fail because the tool doesn't exist and env var is empty
        assert!(result.is_err());
    }

    // -- split_command -------------------------------------------------------

    #[test]
    fn split_command_single_word() {
        let (prog, args) = split_command("llvm-profdata");
        assert_eq!(prog, "llvm-profdata");
        assert!(args.is_empty());
    }

    #[test]
    fn split_command_xcrun_prefix() {
        let (prog, args) = split_command("xcrun llvm-profdata");
        assert_eq!(prog, "xcrun");
        assert_eq!(args, vec!["llvm-profdata"]);
    }

    // -- parse_llvm_cov_export -----------------------------------------------

    /// Minimal realistic LLVM coverage JSON fixture.
    fn sample_json(root: &str) -> String {
        format!(
            r#"{{
  "data": [
    {{
      "files": [
        {{
          "filename": "{root}/src/main.rs",
          "segments": [
            [5, 1, 10, true, true, false],
            [8, 5, 0, true, true, false],
            [12, 1, 3, true, true, false],
            [15, 1, 0, false, false, false],
            [20, 1, 1, true, false, false],
            [25, 1, 0, true, true, true]
          ]
        }},
        {{
          "filename": "{root}/src/lib.rs",
          "segments": [
            [3, 1, 5, true, true, false],
            [7, 1, 0, true, true, false]
          ]
        }},
        {{
          "filename": "/rustc/abc123/library/core/src/ops.rs",
          "segments": [
            [1, 1, 100, true, true, false]
          ]
        }}
      ]
    }}
  ]
}}"#
        )
    }

    #[test]
    fn parse_basic_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = sample_json(root.to_str().unwrap());

        let (all, exec, fps) = parse_llvm_cov_export(&json, root).unwrap();

        // main.rs: line 5 (covered), line 8 (uncovered), line 12 (covered)
        //   line 15: !has_count -> skip
        //   line 20: !is_region_entry -> skip
        //   line 25: is_gap -> skip
        // lib.rs: line 3 (covered), line 7 (uncovered)
        // ops.rs: external -> skip
        assert_eq!(all.len(), 5);
        assert_eq!(exec.len(), 3); // lines 5, 12, 3
        assert_eq!(fps.len(), 2); // main.rs, lib.rs
    }

    #[test]
    fn parse_skips_external_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = sample_json(root.to_str().unwrap());

        let (_, _, fps) = parse_llvm_cov_export(&json, root).unwrap();

        for path in fps.values() {
            let s = path.to_string_lossy();
            assert!(!s.contains("ops.rs"), "should skip external file: {s}");
        }
    }

    #[test]
    fn parse_deduplication() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{root}/src/dup.rs",
      "segments": [
        [1, 1, 5, true, true, false],
        [1, 1, 5, true, true, false]
      ]
    }}]
  }}]
}}"#,
            root = root.to_str().unwrap()
        );

        let (all, exec, _) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(exec.len(), 1);
    }

    #[test]
    fn parse_empty_data() {
        let json = r#"{"data": [{"files": []}]}"#;
        let (all, exec, fps) =
            parse_llvm_cov_export(json, Path::new("/nonexistent")).unwrap();
        assert_eq!(all.len(), 0);
        assert_eq!(exec.len(), 0);
        assert_eq!(fps.len(), 0);
    }

    #[test]
    fn parse_gap_region_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/gap.rs",
      "segments": [
        [1, 1, 5, true, true, true],
        [2, 1, 5, true, true, false]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, exec, _) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 1, "gap region should be skipped");
        assert_eq!(exec.len(), 1);
    }

    #[test]
    fn parse_not_region_entry_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/noentry.rs",
      "segments": [
        [1, 1, 5, true, false, false],
        [2, 1, 5, false, true, false]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, _, _) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 0);
    }

    #[test]
    fn parse_short_segment_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{root}/src/short.rs",
      "segments": [[1, 2, 3, true, true]]
    }}]
  }}]
}}"#,
            root = root.to_str().unwrap()
        );

        let (all, _, _) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 0, "5-field segment should be skipped (need 6)");
    }

    #[test]
    fn parse_integer_booleans() {
        // Some LLVM versions use 0/1 instead of true/false
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/intbool.rs",
      "segments": [
        [10, 5, 3, 1, 1, 0],
        [15, 1, 0, 1, 1, 0],
        [20, 1, 5, 0, 1, 0],
        [25, 1, 5, 1, 0, 0],
        [30, 1, 5, 1, 1, 1]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );

        let (all, exec, _) = parse_llvm_cov_export(&json, root).unwrap();
        // Only lines 10 and 15 pass all filters (has_count=1, is_entry=1, is_gap=0)
        assert_eq!(all.len(), 2);
        assert_eq!(exec.len(), 1); // only line 10 has count > 0
    }

    #[test]
    fn parse_all_zero_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/zero.rs",
      "segments": [
        [1, 1, 0, true, true, false],
        [5, 1, 0, true, true, false]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, exec, fps) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(exec.len(), 0);
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn parse_multiple_data_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [
    {{
      "files": [{{
        "filename": "{root}/src/a.rs",
        "segments": [[1, 1, 1, true, true, false]]
      }}]
    }},
    {{
      "files": [{{
        "filename": "{root}/src/b.rs",
        "segments": [[2, 1, 0, true, true, false]]
      }}]
    }}
  ]
}}"#,
            root = root.display()
        );
        let (all, exec, fps) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(exec.len(), 1);
        assert_eq!(fps.len(), 2);
    }

    #[test]
    fn parse_col_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/col.rs",
      "segments": [[10, 42, 1, true, true, false]]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, _, _) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].line, 10);
        assert_eq!(all[0].col, 42);
        assert_eq!(all[0].direction, 0);
    }

    #[test]
    fn parse_col_clamped_to_u16_max() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/bigcol.rs",
      "segments": [[1, 70000, 1, true, true, false]]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, _, _) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].col, u16::MAX);
    }

    #[test]
    fn parse_invalid_json() {
        let result = parse_llvm_cov_export("not json", Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_data_key() {
        let result = parse_llvm_cov_export(r#"{"not_data": []}"#, Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_files_key() {
        let result =
            parse_llvm_cov_export(r#"{"data": [{"not_files": []}]}"#, Path::new("/tmp"));
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_filename() {
        let result = parse_llvm_cov_export(
            r#"{"data": [{"files": [{"no_filename": true, "segments": []}]}]}"#,
            Path::new("/tmp"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn parse_missing_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{"data": [{{"files": [{{"filename": "{}/src/a.rs"}}]}}]}}"#,
            root.display()
        );
        let result = parse_llvm_cov_export(&json, root);
        assert!(result.is_err());
    }

    #[test]
    fn parse_segment_not_array() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/bad.rs",
      "segments": ["not_an_array"]
    }}]
  }}]
}}"#,
            root.display()
        );
        let result = parse_llvm_cov_export(&json, root);
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/empty_seg.rs",
      "segments": []
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, exec, fps) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 0);
        assert_eq!(exec.len(), 0);
        // File is registered even with empty segments
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn parse_file_paths_deduplicated() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [
    {{
      "files": [{{
        "filename": "{root}/src/same.rs",
        "segments": [[1, 1, 1, true, true, false]]
      }}]
    }},
    {{
      "files": [{{
        "filename": "{root}/src/same.rs",
        "segments": [[2, 1, 0, true, true, false]]
      }}]
    }}
  ]
}}"#,
            root = root.display()
        );
        let (all, _, fps) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(fps.len(), 1);
    }

    #[test]
    fn parse_empty_object() {
        let result = parse_llvm_cov_export("{}", Path::new("/tmp"));
        assert!(result.is_err());
    }

    // -- LlvmCoverageBackend ------------------------------------------------

    #[test]
    fn backend_with_tools_sets_paths() {
        let tools = LlvmTools {
            profdata: "llvm-profdata".to_string(),
            llvm_cov: "llvm-cov".to_string(),
        };
        let backend = LlvmCoverageBackend::with_tools(Path::new("/my/project"), tools);

        assert_eq!(backend.target_root, PathBuf::from("/my/project"));
        assert_eq!(
            backend.profraw_dir,
            PathBuf::from("/my/project/.apex/profraw")
        );
        assert_eq!(
            backend.profdata_path,
            PathBuf::from("/my/project/.apex/coverage.profdata")
        );
        assert_eq!(
            backend.json_path,
            PathBuf::from("/my/project/.apex/coverage/llvm-cov.json")
        );
    }

    #[test]
    fn backend_parse_reads_json_file() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let tools = LlvmTools {
            profdata: "llvm-profdata".to_string(),
            llvm_cov: "llvm-cov".to_string(),
        };
        let backend = LlvmCoverageBackend::with_tools(root, tools);

        // Write a coverage JSON file at the expected path
        std::fs::create_dir_all(backend.json_path.parent().unwrap()).unwrap();
        let json = sample_json(root.to_str().unwrap());
        std::fs::write(&backend.json_path, &json).unwrap();

        let (all, exec, fps) = backend.parse().unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(exec.len(), 3);
        assert_eq!(fps.len(), 2);
    }

    #[test]
    fn backend_parse_missing_file_errors() {
        let tools = LlvmTools {
            profdata: "llvm-profdata".to_string(),
            llvm_cov: "llvm-cov".to_string(),
        };
        let backend =
            LlvmCoverageBackend::with_tools(Path::new("/nonexistent/project"), tools);

        let result = backend.parse();
        assert!(result.is_err());
    }

    #[test]
    fn parse_mixed_bool_and_int_encoding() {
        // Mix of bool and int encoding in the same file (shouldn't happen but
        // the parser should handle it gracefully).
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/mixed.rs",
      "segments": [
        [1, 1, 5, true, true, false],
        [2, 1, 3, 1, 1, 0],
        [3, 1, 0, true, 1, false],
        [4, 1, 0, 1, true, 0]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, exec, _) = parse_llvm_cov_export(&json, root).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(exec.len(), 2); // lines 1 and 2 have count > 0
    }

    #[test]
    fn parse_direction_always_zero() {
        // Verify the unified parser always uses direction=0 (no dual-direction)
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let json = format!(
            r#"{{
  "data": [{{
    "files": [{{
      "filename": "{}/src/dir.rs",
      "segments": [
        [1, 1, 5, true, true, false],
        [2, 1, 0, true, true, false]
      ]
    }}]
  }}]
}}"#,
            root.display()
        );
        let (all, exec, _) = parse_llvm_cov_export(&json, root).unwrap();

        for bid in &all {
            assert_eq!(bid.direction, 0, "all branches should have direction=0");
        }
        for bid in &exec {
            assert_eq!(bid.direction, 0, "executed branches should have direction=0");
        }
        // Uncovered line (count=0) should NOT appear in executed
        assert_eq!(all.len(), 2);
        assert_eq!(exec.len(), 1);
    }
}
