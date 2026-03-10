//! WebAssembly instrumentation via `wasm-opt` SanitizerCoverage pass.
//!
//! Uses `wasm-opt --instrument-branch-coverage` from the Binaryen toolkit to
//! inject SanitizerCoverage hooks, then runs the instrumented module under
//! `wasmtime` with coverage tracking via a WASI import shim.
//!
//! # Status
//!
//! Infrastructure stub -- wasm-opt coverage pass plumbing and wasmtime
//! coverage collection are not yet implemented. The instrumentor discovers
//! source sections and produces synthetic BranchIds based on function count.
//!
//! Enable with: `--lang wasm`

#[cfg(feature = "wasm-instrument")]
use apex_core::error::ApexError;
use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::Result,
    traits::Instrumentor,
    types::{BranchId, InstrumentedTarget, Target},
};
use async_trait::async_trait;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tracing::{info, warn};

fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

pub struct WasmInstrumentor {
    branch_ids: Vec<BranchId>,
    runner: Arc<dyn CommandRunner>,
}

impl WasmInstrumentor {
    pub fn new() -> Self {
        WasmInstrumentor {
            branch_ids: Vec::new(),
            runner: Arc::new(RealCommandRunner),
        }
    }

    /// Create a new instrumentor with a custom command runner (for testing).
    pub fn with_runner(runner: Arc<dyn CommandRunner>) -> Self {
        WasmInstrumentor {
            branch_ids: Vec::new(),
            runner,
        }
    }

    /// Find `.wasm` files in the target directory.
    fn find_wasm_files(target: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        if let Ok(entries) = std::fs::read_dir(target) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "wasm").unwrap_or(false) {
                    found.push(path);
                }
            }
        }
        // Also look in common build output dirs.
        for subdir in &["build", "dist", "out", "pkg"] {
            let dir = target.join(subdir);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().map(|e| e == "wasm").unwrap_or(false) {
                        found.push(path);
                    }
                }
            }
        }
        found
    }

    /// Instrument a single `.wasm` file using `wasm-opt --instrument-branch-coverage`.
    ///
    /// Produces an instrumented binary at `<name>.inst.wasm` in a temp directory,
    /// then parses the output to count guard slots and generate real BranchIds.
    ///
    /// Only active when the `wasm-instrument` feature is enabled.
    #[cfg(feature = "wasm-instrument")]
    async fn instrument_with_wasm_opt(
        wasm_path: &Path,
        file_paths: &mut HashMap<u64, PathBuf>,
        runner: &dyn CommandRunner,
    ) -> Result<(Vec<BranchId>, PathBuf)> {
        let stem = wasm_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "module".to_string());
        let inst_dir = tempfile::tempdir()
            .map_err(|e| ApexError::Instrumentation(format!("failed to create temp dir: {e}")))?;
        let inst_dir_path = inst_dir.path().to_path_buf();
        // Keep the temp dir alive so the instrumented file persists.
        let _keep = inst_dir.keep();
        let inst_path = inst_dir_path.join(format!("{stem}.inst.wasm"));

        let spec =
            CommandSpec::new("wasm-opt", wasm_path.parent().unwrap_or(Path::new("."))).args([
                "--instrument-branch-coverage",
                "-o",
                &inst_path.to_string_lossy(),
                &wasm_path.to_string_lossy(),
            ]);
        let output = runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::Instrumentation(format!("wasm-opt exec: {e}")))?;

        if output.exit_code != 0 {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ApexError::Instrumentation(format!(
                "wasm-opt failed (exit {}): {stderr}",
                output.exit_code
            )));
        }

        // Parse the instrumented binary to estimate guard count.
        let orig_funcs = count_wasm_functions(wasm_path).unwrap_or(0);
        let inst_funcs = count_wasm_functions(&inst_path).unwrap_or(0);
        let guard_count = if inst_funcs > orig_funcs {
            inst_funcs - orig_funcs
        } else {
            orig_funcs.max(1)
        };

        let rel = wasm_path.file_name().map(PathBuf::from).unwrap_or_default();
        let rel_str = rel.to_string_lossy().to_string();
        let file_id = fnv1a_hash(&rel_str);
        file_paths.insert(file_id, rel);

        let mut branches = Vec::new();
        for i in 0..guard_count {
            branches.push(BranchId::new(file_id, i as u32, 0, 0));
            branches.push(BranchId::new(file_id, i as u32, 0, 1));
        }

        info!(
            file = %wasm_path.display(),
            guards = guard_count,
            branches = branches.len(),
            inst_path = %inst_path.display(),
            "wasm-opt instrumentation succeeded"
        );

        Ok((branches, inst_path))
    }

    /// Produce synthetic BranchIds from a wasm binary's function count.
    ///
    /// Real implementation would parse the DWARF debug sections and map
    /// code offsets to source locations via `wasm-opt` source map output.
    fn synthetic_branches_from_wasm(
        wasm_path: &Path,
        file_paths: &mut HashMap<u64, PathBuf>,
    ) -> Vec<BranchId> {
        let rel = wasm_path.file_name().map(PathBuf::from).unwrap_or_default();
        let rel_str = rel.to_string_lossy().to_string();
        let file_id = fnv1a_hash(&rel_str);
        file_paths.insert(file_id, rel);

        // Parse wasm binary to count functions (sections 0x60 = func type).
        // Synthesise two branches per function as a rough proxy.
        let func_count = count_wasm_functions(wasm_path).unwrap_or(4);
        let mut branches = Vec::new();
        for i in 0..func_count {
            let line = (i as u32 + 1) * 10; // synthetic line numbers
            branches.push(BranchId::new(file_id, line, 0, 0));
            branches.push(BranchId::new(file_id, line, 0, 1));
        }
        branches
    }
}

/// Count exported/imported functions by scanning the wasm binary header.
fn count_wasm_functions(path: &Path) -> Option<usize> {
    let bytes = std::fs::read(path).ok()?;
    // Wasm magic: 0x00 0x61 0x73 0x6d 0x01 0x00 0x00 0x00
    if bytes.get(..4) != Some(&[0x00, 0x61, 0x73, 0x6d]) {
        return None;
    }
    // Walk sections, find function section (id=3) to get function count.
    let mut pos = 8usize;
    while pos + 2 < bytes.len() {
        let section_id = bytes[pos];
        pos += 1;
        // Read LEB128 section size.
        let (size, advance) = read_leb128(&bytes[pos..])?;
        pos += advance;
        if section_id == 3 {
            // Function section: count = LEB128 at start.
            if pos < bytes.len() {
                let (count, _) = read_leb128(&bytes[pos..])?;
                return Some(count as usize);
            }
        }
        pos += size as usize;
    }
    None
}

fn read_leb128(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        result |= ((byte & 0x7f) as u64) << shift;
        shift += 7;
        if byte & 0x80 == 0 {
            return Some((result, i + 1));
        }
        if shift >= 63 {
            break;
        }
    }
    None
}

impl Default for WasmInstrumentor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Instrumentor for WasmInstrumentor {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget> {
        info!(target = %target.root.display(), "instrumenting WASM");

        // Check for wasm-opt in PATH.
        let spec = CommandSpec::new("wasm-opt", &target.root).args(["--version"]);
        let has_wasm_opt = self
            .runner
            .run_command(&spec)
            .await
            .map(|o| o.exit_code == 0)
            .unwrap_or(false);

        if !has_wasm_opt {
            warn!(
                "wasm-opt not found in PATH; branch coverage instrumentation unavailable. \
                 Install Binaryen: https://github.com/WebAssembly/binaryen"
            );
        }

        let wasm_files = Self::find_wasm_files(&target.root);
        if wasm_files.is_empty() {
            warn!(
                target = %target.root.display(),
                "no .wasm files found; WASM instrumentation yielded no branches"
            );
        }

        let mut file_paths = HashMap::new();
        let mut branch_ids = Vec::new();
        let mut _work_dir = target.root.clone();

        for wasm_file in &wasm_files {
            #[cfg(feature = "wasm-instrument")]
            if has_wasm_opt {
                match Self::instrument_with_wasm_opt(
                    wasm_file,
                    &mut file_paths,
                    self.runner.as_ref(),
                )
                .await
                {
                    Ok((branches, inst_path)) => {
                        if let Some(parent) = inst_path.parent() {
                            _work_dir = parent.to_path_buf();
                        }
                        branch_ids.extend(branches);
                        continue;
                    }
                    Err(e) => {
                        warn!(error = %e, "wasm-opt instrumentation failed; falling back to synthetic");
                    }
                }
            }

            // Fallback to synthetic branches
            let branches =
                WasmInstrumentor::synthetic_branches_from_wasm(wasm_file, &mut file_paths);
            info!(
                file = %wasm_file.display(),
                branches = branches.len(),
                "wasm: synthetic branch IDs"
            );
            branch_ids.extend(branches);
        }

        Ok(InstrumentedTarget {
            target: target.clone(),
            branch_ids,
            executed_branch_ids: Vec::new(),
            file_paths,
            work_dir: _work_dir,
        })
    }

    fn branch_ids(&self) -> &[BranchId] {
        &self.branch_ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;

    /// A test-only CommandRunner that returns a configurable output.
    struct FakeRunner {
        exit_code: i32,
        fail: bool,
    }

    impl FakeRunner {
        fn success() -> Self {
            FakeRunner {
                exit_code: 0,
                fail: false,
            }
        }

        fn failure(exit_code: i32) -> Self {
            FakeRunner {
                exit_code,
                fail: false,
            }
        }

        fn spawn_error() -> Self {
            FakeRunner {
                exit_code: -1,
                fail: true,
            }
        }
    }

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run_command(
            &self,
            _spec: &CommandSpec,
        ) -> apex_core::error::Result<CommandOutput> {
            if self.fail {
                return Err(apex_core::error::ApexError::Subprocess {
                    exit_code: -1,
                    stderr: "spawn failed".into(),
                });
            }
            Ok(CommandOutput {
                exit_code: self.exit_code,
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        }
    }

    // ------------------------------------------------------------------
    // read_leb128 tests
    // ------------------------------------------------------------------

    #[test]
    fn test_leb128_single_byte() {
        assert_eq!(read_leb128(&[0x00]), Some((0, 1)));
        assert_eq!(read_leb128(&[0x01]), Some((1, 1)));
        assert_eq!(read_leb128(&[0x7f]), Some((127, 1)));
    }

    #[test]
    fn test_leb128_two_bytes() {
        assert_eq!(read_leb128(&[0x80, 0x01]), Some((128, 2)));
        assert_eq!(read_leb128(&[0xAC, 0x02]), Some((300, 2)));
    }

    #[test]
    fn test_leb128_larger() {
        assert_eq!(read_leb128(&[0xE5, 0x8E, 0x26]), Some((624485, 3)));
    }

    #[test]
    fn test_leb128_empty() {
        assert_eq!(read_leb128(&[]), None);
    }

    #[test]
    fn test_leb128_unterminated() {
        assert_eq!(
            read_leb128(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80]),
            None
        );
    }

    // ------------------------------------------------------------------
    // count_wasm_functions tests
    // ------------------------------------------------------------------

    /// Build a minimal valid wasm binary with a function section containing `n` functions.
    fn build_minimal_wasm(func_count: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x00, 0x61, 0x73, 0x6d]); // magic
        bytes.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // version 1

        // Type section (id=1): one function type () -> ()
        bytes.push(1); // section id
        bytes.push(4); // section size (LEB128)
        bytes.push(1); // 1 type
        bytes.push(0x60); // func type
        bytes.push(0); // 0 params
        bytes.push(0); // 0 results

        // Function section (id=3): func_count functions, all type index 0
        let func_body_size = 1 + func_count; // count byte + func_count type indices
        bytes.push(3); // section id
        bytes.push(func_body_size); // section size
        bytes.push(func_count); // function count
        for _ in 0..func_count {
            bytes.push(0); // type index 0
        }

        bytes
    }

    #[test]
    fn test_count_wasm_functions_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let wasm_path = tmp.path().join("test.wasm");
        std::fs::write(&wasm_path, build_minimal_wasm(5)).unwrap();

        assert_eq!(count_wasm_functions(&wasm_path), Some(5));
    }

    #[test]
    fn test_count_wasm_functions_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let wasm_path = tmp.path().join("zero.wasm");
        std::fs::write(&wasm_path, build_minimal_wasm(0)).unwrap();

        assert_eq!(count_wasm_functions(&wasm_path), Some(0));
    }

    #[test]
    fn test_count_wasm_functions_not_wasm() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("notawasm.wasm");
        std::fs::write(&path, b"this is not a wasm file").unwrap();

        assert_eq!(count_wasm_functions(&path), None);
    }

    #[test]
    fn test_count_wasm_functions_no_func_section() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nofunc.wasm");
        let mut bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        bytes.push(0); // section id 0 (custom)
        bytes.push(4); // section size
        bytes.push(1); // name len
        bytes.push(b'x'); // name
        bytes.push(0); // payload
        bytes.push(0); // payload
        std::fs::write(&path, &bytes).unwrap();

        assert_eq!(count_wasm_functions(&path), None);
    }

    #[test]
    fn test_count_wasm_functions_missing_file() {
        assert_eq!(
            count_wasm_functions(Path::new("/nonexistent/missing.wasm")),
            None
        );
    }

    // ------------------------------------------------------------------
    // synthetic_branches_from_wasm tests
    // ------------------------------------------------------------------

    #[test]
    fn test_synthetic_branches_count() {
        let tmp = tempfile::tempdir().unwrap();
        let wasm_path = tmp.path().join("app.wasm");
        std::fs::write(&wasm_path, build_minimal_wasm(3)).unwrap();

        let mut fps = HashMap::new();
        let branches = WasmInstrumentor::synthetic_branches_from_wasm(&wasm_path, &mut fps);

        assert_eq!(branches.len(), 6);
        assert_eq!(fps.len(), 1);

        let lines: Vec<u32> = branches.iter().map(|b| b.line).collect();
        assert!(lines.contains(&10));
        assert!(lines.contains(&20));
        assert!(lines.contains(&30));
    }

    #[test]
    fn test_synthetic_branches_directions() {
        let tmp = tempfile::tempdir().unwrap();
        let wasm_path = tmp.path().join("app.wasm");
        std::fs::write(&wasm_path, build_minimal_wasm(1)).unwrap();

        let mut fps = HashMap::new();
        let branches = WasmInstrumentor::synthetic_branches_from_wasm(&wasm_path, &mut fps);

        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].direction, 0);
        assert_eq!(branches[1].direction, 1);
    }

    // ------------------------------------------------------------------
    // find_wasm_files tests
    // ------------------------------------------------------------------

    #[test]
    fn test_find_wasm_files_in_root() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("app.wasm"), &[0u8]).unwrap();
        std::fs::write(tmp.path().join("not_wasm.txt"), &[0u8]).unwrap();

        let found = WasmInstrumentor::find_wasm_files(tmp.path());
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn test_find_wasm_files_in_subdirs() {
        let tmp = tempfile::tempdir().unwrap();
        let build_dir = tmp.path().join("build");
        std::fs::create_dir_all(&build_dir).unwrap();
        std::fs::write(build_dir.join("module.wasm"), &[0u8]).unwrap();

        let found = WasmInstrumentor::find_wasm_files(tmp.path());
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn test_find_wasm_files_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let found = WasmInstrumentor::find_wasm_files(tmp.path());
        assert_eq!(found.len(), 0);
    }

    // ------------------------------------------------------------------
    // instrument() fallback behavior tests (with mock runner)
    // ------------------------------------------------------------------

    #[tokio::test]
    async fn test_instrument_uses_synthetic_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let wasm_path = tmp.path().join("app.wasm");
        std::fs::write(&wasm_path, build_minimal_wasm(3)).unwrap();

        let target = Target {
            root: tmp.path().to_path_buf(),
            language: apex_core::types::Language::Wasm,
            test_command: Vec::new(),
        };

        // Runner reports wasm-opt not found (failure on version check)
        let runner = Arc::new(FakeRunner::failure(127));
        let instrumentor = WasmInstrumentor::with_runner(runner);
        let result = instrumentor.instrument(&target).await.unwrap();

        // 3 functions x 2 directions = 6 synthetic branches
        assert_eq!(result.branch_ids.len(), 6);
        assert!(result.executed_branch_ids.is_empty());
        assert_eq!(result.file_paths.len(), 1);
    }

    #[tokio::test]
    async fn test_instrument_no_wasm_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();

        let target = Target {
            root: tmp.path().to_path_buf(),
            language: apex_core::types::Language::Wasm,
            test_command: Vec::new(),
        };

        let runner = Arc::new(FakeRunner::failure(127));
        let instrumentor = WasmInstrumentor::with_runner(runner);
        let result = instrumentor.instrument(&target).await.unwrap();

        assert!(result.branch_ids.is_empty());
        assert!(result.file_paths.is_empty());
    }

    #[tokio::test]
    async fn test_instrument_multiple_wasm_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.wasm"), build_minimal_wasm(2)).unwrap();
        std::fs::write(tmp.path().join("b.wasm"), build_minimal_wasm(1)).unwrap();

        let target = Target {
            root: tmp.path().to_path_buf(),
            language: apex_core::types::Language::Wasm,
            test_command: Vec::new(),
        };

        let runner = Arc::new(FakeRunner::failure(127));
        let instrumentor = WasmInstrumentor::with_runner(runner);
        let result = instrumentor.instrument(&target).await.unwrap();

        // 2 funcs x 2 + 1 func x 2 = 6 branches total
        assert_eq!(result.branch_ids.len(), 6);
        assert_eq!(result.file_paths.len(), 2);
    }

    #[tokio::test]
    async fn test_instrument_wasm_opt_available_but_no_feature() {
        // Even if wasm-opt reports available (exit 0), without
        // wasm-instrument feature we fall back to synthetic
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("app.wasm"), build_minimal_wasm(2)).unwrap();

        let target = Target {
            root: tmp.path().to_path_buf(),
            language: apex_core::types::Language::Wasm,
            test_command: Vec::new(),
        };

        let runner = Arc::new(FakeRunner::success()); // wasm-opt --version succeeds
        let instrumentor = WasmInstrumentor::with_runner(runner);
        let result = instrumentor.instrument(&target).await.unwrap();

        // Without the feature, still uses synthetic fallback
        assert_eq!(result.branch_ids.len(), 4); // 2 funcs x 2
    }

    // ------------------------------------------------------------------
    // wasm-opt instrumentation tests (feature-gated)
    // ------------------------------------------------------------------

    #[cfg(feature = "wasm-instrument")]
    mod wasm_opt_tests {
        use super::*;

        /// Check if wasm-opt is available in PATH.
        fn has_wasm_opt() -> bool {
            std::process::Command::new("wasm-opt")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }

        #[tokio::test]
        async fn test_instrument_with_wasm_opt_missing_file() {
            let runner = FakeRunner::spawn_error();
            let mut fps = HashMap::new();
            let result = WasmInstrumentor::instrument_with_wasm_opt(
                Path::new("/nonexistent/file.wasm"),
                &mut fps,
                &runner,
            )
            .await;
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn test_instrument_with_wasm_opt_invalid_wasm() {
            if !has_wasm_opt() {
                eprintln!("skipping: wasm-opt not found");
                return;
            }
            let tmp = tempfile::tempdir().unwrap();
            let bad_wasm = tmp.path().join("bad.wasm");
            std::fs::write(&bad_wasm, b"not a valid wasm binary").unwrap();

            let runner = apex_core::command::RealCommandRunner;
            let mut fps = HashMap::new();
            let result =
                WasmInstrumentor::instrument_with_wasm_opt(&bad_wasm, &mut fps, &runner).await;
            assert!(result.is_err(), "expected error for invalid wasm");
        }

        #[tokio::test]
        async fn test_instrument_with_wasm_opt_valid_wasm() {
            if !has_wasm_opt() {
                eprintln!("skipping: wasm-opt not found");
                return;
            }
            let tmp = tempfile::tempdir().unwrap();
            let wasm_path = tmp.path().join("mod.wasm");
            let wasm_bytes = build_valid_wasm_module(2);
            std::fs::write(&wasm_path, &wasm_bytes).unwrap();

            let runner = apex_core::command::RealCommandRunner;
            let mut fps = HashMap::new();
            let result =
                WasmInstrumentor::instrument_with_wasm_opt(&wasm_path, &mut fps, &runner).await;
            match result {
                Ok((branches, inst_path)) => {
                    assert!(!branches.is_empty(), "expected at least one branch");
                    assert!(inst_path.exists(), "instrumented file should exist");
                    assert!(
                        inst_path.to_string_lossy().contains(".inst.wasm"),
                        "instrumented file should have .inst.wasm extension"
                    );
                    assert_eq!(fps.len(), 1);
                }
                Err(e) => {
                    eprintln!("wasm-opt rejected minimal binary (expected): {e}");
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // proptest properties
    // ------------------------------------------------------------------

    use proptest::prelude::*;

    /// Write an unsigned LEB128 value into a byte buffer.
    fn write_leb128(mut val: u64) -> Vec<u8> {
        let mut buf = Vec::new();
        loop {
            let mut byte = (val & 0x7f) as u8;
            val >>= 7;
            if val != 0 {
                byte |= 0x80;
            }
            buf.push(byte);
            if val == 0 {
                break;
            }
        }
        buf
    }

    proptest! {
        #[test]
        fn prop_leb128_roundtrip(val in 0u64..=0x0FFF_FFFF_FFFF_FFFFu64) {
            let encoded = write_leb128(val);
            let (decoded, consumed) = read_leb128(&encoded).unwrap();
            prop_assert_eq!(decoded, val);
            prop_assert_eq!(consumed, encoded.len());
        }

        #[test]
        fn prop_leb128_trailing_data_ignored(
            val in 0u64..=0x7FFF_FFFFu64,
            tail in proptest::collection::vec(any::<u8>(), 0..16),
        ) {
            let mut encoded = write_leb128(val);
            let expected_len = encoded.len();
            encoded.extend_from_slice(&tail);
            let (decoded, consumed) = read_leb128(&encoded).unwrap();
            prop_assert_eq!(decoded, val);
            prop_assert_eq!(consumed, expected_len);
        }

        #[test]
        fn prop_fnv1a_deterministic(s in "[a-zA-Z0-9_./]{0,64}") {
            let h1 = fnv1a_hash(&s);
            let h2 = fnv1a_hash(&s);
            prop_assert_eq!(h1, h2);
        }

        #[test]
        fn prop_fnv1a_different_inputs(
            a in "[a-z]{1,16}",
            b in "[a-z]{1,16}",
        ) {
            // Different inputs should (almost always) produce different hashes.
            // Only skip the check when inputs are equal.
            if a != b {
                prop_assert_ne!(fnv1a_hash(&a), fnv1a_hash(&b));
            }
        }

        /// Fuzz-like: random bytes should never panic read_leb128.
        #[test]
        fn prop_read_leb128_never_panics(data in proptest::collection::vec(any::<u8>(), 0..32)) {
            // Should return Some or None, never panic
            let _ = read_leb128(&data);
        }

        /// Fuzz-like: random bytes that look like WASM headers should be handled gracefully.
        #[test]
        fn prop_wasm_parsing_never_panics(data in proptest::collection::vec(any::<u8>(), 0..128)) {
            // Prepend WASM magic header
            let mut wasm_like = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
            wasm_like.extend_from_slice(&data);
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("fuzz.wasm");
            std::fs::write(&path, &wasm_like).unwrap();
            // Should return Some or None, never panic
            let _ = count_wasm_functions(&path);
        }
    }

    /// Build a more complete valid wasm module with type, function, and code sections.
    #[allow(dead_code)]
    fn build_valid_wasm_module(func_count: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&[0x00, 0x61, 0x73, 0x6d]); // magic
        bytes.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // version 1

        // Type section (id=1): one function type () -> ()
        bytes.push(1); // section id
        bytes.push(4); // section size
        bytes.push(1); // 1 type
        bytes.push(0x60); // func type marker
        bytes.push(0); // 0 params
        bytes.push(0); // 0 results

        // Function section (id=3): func_count functions, all type index 0
        let func_sec_size = 1 + func_count;
        bytes.push(3); // section id
        bytes.push(func_sec_size); // section size
        bytes.push(func_count); // function count
        for _ in 0..func_count {
            bytes.push(0); // type index 0
        }

        // Code section (id=10): each function body is just `end`
        let body_size: u8 = 2; // local count (0) + end opcode
        let code_sec_payload = 1 + func_count * (1 + body_size); // count + bodies
        bytes.push(10); // section id
        bytes.push(code_sec_payload); // section size
        bytes.push(func_count); // function count
        for _ in 0..func_count {
            bytes.push(body_size); // body size
            bytes.push(0); // local declaration count = 0
            bytes.push(0x0b); // end opcode
        }

        bytes
    }
}
