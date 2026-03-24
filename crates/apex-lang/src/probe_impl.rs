//! Per-language environment probes for APEX.
//!
//! Each `probe_*` function returns the typed `*Env` struct from
//! `apex_core::probe`, wrapping existing detection logic from the language
//! runners instead of duplicating it.

use apex_core::probe::{
    CCppEnv, DotnetEnv, EnvironmentProbe, GoEnv, JsEnv, JvmEnv, PythonEnv, RubyEnv, RustEnv,
    SwiftEnv,
};
use apex_core::types::Language;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Module-level helpers (private)
// ---------------------------------------------------------------------------

/// Run `cmd args` and return the first line of stdout, or empty string on
/// failure.
fn get_version(cmd: &str, args: &[&str]) -> String {
    std::process::Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                // Some tools write version to stderr (e.g. java -version)
                let se = String::from_utf8_lossy(&o.stderr).trim().to_string();
                if se.is_empty() { None } else { Some(se.lines().next().unwrap_or("").trim().to_string()) }
            } else {
                Some(s.lines().next().unwrap_or("").trim().to_string())
            }
        })
        .unwrap_or_default()
}

/// Run `cmd args` and return first-line of stdout/stderr as `Some`, or `None`
/// when the command is absent or exits non-zero.
fn check_tool(cmd: &str, args: &[&str]) -> Option<String> {
    let o = std::process::Command::new(cmd).args(args).output().ok()?;
    if !o.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
        Some(stderr.lines().next().unwrap_or("").trim().to_string())
    } else {
        Some(stdout.lines().next().unwrap_or("").trim().to_string())
    }
}

/// Return `true` when `name` is found on PATH.
fn which(name: &str) -> bool {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

/// Probe Python environment at `target`.
pub fn probe_python(target: &Path) -> PythonEnv {
    use super::python::PythonRunner;

    let venv = PythonRunner::<apex_core::command::RealCommandRunner>::find_venv_python(target);
    let interpreter = venv
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                PythonRunner::<apex_core::command::RealCommandRunner>::resolve_python(),
            )
        });

    let version = get_version(interpreter.to_str().unwrap_or("python3"), &["--version"]);
    let pep668 =
        PythonRunner::<apex_core::command::RealCommandRunner>::is_externally_managed(target);
    let pkg_mgr =
        PythonRunner::<apex_core::command::RealCommandRunner>::detect_package_manager(target);
    let coverage_tool = check_tool("coverage", &["--version"]);

    // Determine primary test runner label
    let test_runner = detect_python_test_runner(target);

    PythonEnv {
        interpreter,
        version,
        venv: venv.map(PathBuf::from),
        coverage_tool,
        test_runner,
        package_manager: Some(format!("{:?}", pkg_mgr).to_lowercase()),
        pep668_managed: pep668,
    }
}

fn detect_python_test_runner(target: &Path) -> Option<String> {
    // pytest indicators
    if target.join("pytest.ini").exists() {
        return Some("pytest".into());
    }
    if target.join("pyproject.toml").exists() {
        let content = std::fs::read_to_string(target.join("pyproject.toml")).unwrap_or_default();
        if content.contains("[tool.pytest") {
            return Some("pytest".into());
        }
    }
    if target.join("setup.cfg").exists() {
        let content = std::fs::read_to_string(target.join("setup.cfg")).unwrap_or_default();
        if content.contains("[tool:pytest]") || content.contains("[pytest]") {
            return Some("pytest".into());
        }
    }
    // Check if pytest is available on PATH
    if check_tool("pytest", &["--version"]).is_some() {
        return Some("pytest".into());
    }
    Some("unittest".into())
}

// ---------------------------------------------------------------------------
// JavaScript / TypeScript
// ---------------------------------------------------------------------------

/// Probe JavaScript/TypeScript environment at `target`.
pub fn probe_javascript(target: &Path) -> JsEnv {
    use super::js_env::{self, JsEnvironment, JsTestRunner};

    let env = JsEnvironment::detect(target);

    // Determine runtime
    let (runtime_name, runtime_version) = if target.join("bun.lockb").exists()
        || target.join("bunfig.toml").exists()
    {
        let v = get_version("bun", &["--version"]);
        ("bun".to_string(), v)
    } else if target.join("deno.json").exists() || target.join("deno.jsonc").exists() {
        let v = get_version("deno", &["--version"]);
        ("deno".to_string(), v)
    } else {
        let v = get_version("node", &["--version"]);
        ("node".to_string(), v)
    };

    // Package manager
    let package_manager = if let Some(ref e) = env {
        js_env::install_command(e).to_string()
    } else {
        // Fallback: inspect lockfiles
        if target.join("yarn.lock").exists() {
            "yarn".to_string()
        } else if target.join("pnpm-lock.yaml").exists() {
            "pnpm".to_string()
        } else {
            "npm".to_string()
        }
    };

    // Test runner
    let test_runner = if let Some(ref e) = env {
        let (bin, args) = js_env::test_command(e);
        let runner_name = args.first().cloned().unwrap_or(bin);
        Some(match e.test_runner {
            JsTestRunner::Jest => "jest".to_string(),
            JsTestRunner::Mocha => "mocha".to_string(),
            JsTestRunner::Vitest => "vitest".to_string(),
            JsTestRunner::BunTest => "bun-test".to_string(),
            JsTestRunner::DenoTest => "deno-test".to_string(),
            JsTestRunner::NpmScript => runner_name,
        })
    } else {
        None
    };

    // Coverage tool: v8 for node >= 16, istanbul otherwise
    let coverage_tool = if runtime_name == "node" {
        let major: Option<u32> = runtime_version
            .trim_start_matches('v')
            .split('.')
            .next()
            .and_then(|s| s.parse().ok());
        if major.map(|m| m >= 16).unwrap_or(false) {
            Some("v8".to_string())
        } else {
            Some("istanbul".to_string())
        }
    } else if runtime_name == "bun" {
        Some("bun-coverage".to_string())
    } else if runtime_name == "deno" {
        Some("deno-coverage".to_string())
    } else {
        None
    };

    JsEnv {
        runtime: runtime_name,
        version: runtime_version,
        package_manager,
        test_runner,
        coverage_tool,
    }
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

/// Probe Rust environment at `target`.
pub fn probe_rust(_target: &Path) -> RustEnv {
    // rustc --version → "rustc 1.76.0 (07dca489a 2024-02-04)"
    let version_line = get_version("rustc", &["--version"]);
    let version = version_line
        .split_whitespace()
        .nth(1)
        .unwrap_or(&version_line)
        .to_string();

    // toolchain: e.g. "stable-aarch64-apple-darwin" via rustup show active-toolchain
    let toolchain = get_version("rustup", &["show", "active-toolchain"])
        .split_whitespace()
        .next()
        .unwrap_or("stable")
        .to_string();

    let llvm_cov = check_tool("cargo-llvm-cov", &["--version"]).or_else(|| {
        // cargo-llvm-cov may be invoked as subcommand
        check_tool("cargo", &["llvm-cov", "--version"])
    });

    let nextest = check_tool("cargo-nextest", &["--version"]).or_else(|| {
        check_tool("cargo", &["nextest", "--version"])
    });

    RustEnv {
        toolchain: if toolchain.is_empty() {
            "stable".into()
        } else {
            toolchain
        },
        version,
        llvm_cov,
        nextest,
    }
}

// ---------------------------------------------------------------------------
// Go
// ---------------------------------------------------------------------------

/// Probe Go environment at `target`.
pub fn probe_go(_target: &Path) -> GoEnv {
    let version_line = get_version("go", &["version"]);
    // "go version go1.21.5 linux/amd64" → "go1.21.5"
    let version = version_line
        .split_whitespace()
        .nth(2)
        .unwrap_or(&version_line)
        .to_string();

    GoEnv {
        version,
        go_cover: which("go"),
    }
}

// ---------------------------------------------------------------------------
// JVM (Java + Kotlin)
// ---------------------------------------------------------------------------

/// Probe JVM environment at `target`.
pub fn probe_jvm(target: &Path) -> JvmEnv {
    // java -version writes to stderr
    let java_version = check_tool("java", &["-version"]).map(|s| {
        // First line: `java version "21.0.1"` or `openjdk version "21.0.1"`
        s.split('"').nth(1).unwrap_or(&s).to_string()
    });

    let kotlin_version = check_tool("kotlinc", &["-version"]).map(|s| {
        // "kotlinc-jvm 1.9.22 (JRE 21.0.1+...)"
        s.split_whitespace().nth(1).unwrap_or(&s).to_string()
    });

    let build_tool = Some(super::java::detect_build_tool(target).to_string());

    // Coverage: kover (Kotlin) or jacoco (Java)
    let coverage_tool = if kotlin_version.is_some() {
        if super::kotlin::detect_kover_plugin(target) {
            Some("kover".to_string())
        } else {
            Some("jacoco".to_string())
        }
    } else {
        Some("jacoco".to_string())
    };

    JvmEnv {
        java_version,
        kotlin_version,
        build_tool,
        coverage_tool,
    }
}

// ---------------------------------------------------------------------------
// Ruby
// ---------------------------------------------------------------------------

/// Probe Ruby environment at `target`.
pub fn probe_ruby(target: &Path) -> RubyEnv {
    use super::ruby::RubyRunner;

    let ruby_bin = RubyRunner::<apex_core::command::RealCommandRunner>::resolve_ruby();
    let version = get_version(ruby_bin, &["--version"])
        .split_whitespace()
        .nth(1)
        .unwrap_or("")
        .to_string();

    let test_runner = if target.join("spec").exists() || target.join(".rspec").exists() {
        Some("rspec".to_string())
    } else {
        Some("minitest".to_string())
    };

    // Version manager
    let version_manager = if std::process::Command::new("mise")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        Some("mise".to_string())
    } else if std::process::Command::new("rbenv")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        Some("rbenv".to_string())
    } else {
        None
    };

    // simplecov is always "available" as a gem; check if it's in Gemfile
    let coverage_tool = if target.join("Gemfile").exists() {
        let content = std::fs::read_to_string(target.join("Gemfile")).unwrap_or_default();
        if content.contains("simplecov") {
            Some("simplecov".to_string())
        } else {
            None
        }
    } else {
        None
    };

    RubyEnv {
        version,
        test_runner,
        coverage_tool,
        version_manager,
    }
}

// ---------------------------------------------------------------------------
// C / C++
// ---------------------------------------------------------------------------

/// Probe C/C++ environment at `target`.
pub fn probe_c_cpp(target: &Path) -> CCppEnv {
    // Detect compiler: prefer clang, fall back to gcc
    let (compiler, version) = if which("clang") {
        let v = get_version("clang", &["--version"]);
        ("clang".to_string(), v)
    } else if which("gcc") {
        let v = get_version("gcc", &["--version"]);
        ("gcc".to_string(), v)
    } else {
        ("cc".to_string(), String::new())
    };

    // Build system: check common markers
    let build_system = if target.join("xmake.lua").exists() {
        Some("xmake".to_string())
    } else if target.join("CMakeLists.txt").exists() {
        Some("cmake".to_string())
    } else if target.join("meson.build").exists() {
        Some("meson".to_string())
    } else if target.join("Makefile").exists() || target.join("makefile").exists() {
        Some("make".to_string())
    } else if target.join("configure.ac").exists() || target.join("configure").exists() {
        Some("autoconf".to_string())
    } else {
        None
    };

    // Coverage tool
    let coverage_tool = if which("llvm-cov") {
        Some("llvm-cov".to_string())
    } else if which("gcov") {
        Some("gcov".to_string())
    } else {
        None
    };

    CCppEnv {
        compiler,
        version,
        build_system,
        coverage_tool,
    }
}

// ---------------------------------------------------------------------------
// Swift
// ---------------------------------------------------------------------------

/// Probe Swift environment at `target`.
pub fn probe_swift(_target: &Path) -> SwiftEnv {
    let version_line = get_version("swift", &["--version"]);
    // "swift-driver version: 1.87.3 Apple Swift version 5.9.2 ..."
    // or "Swift version 5.9.2 (swift-5.9.2-RELEASE)"
    let version = version_line
        .split("version ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .unwrap_or(&version_line)
        .to_string();

    let spm = which("swift");
    let coverage = check_tool("xcrun", &["llvm-cov", "--version"]).is_some();

    SwiftEnv {
        version,
        spm,
        coverage,
    }
}

// ---------------------------------------------------------------------------
// C# / .NET
// ---------------------------------------------------------------------------

/// Probe .NET/C# environment at `target`.
pub fn probe_dotnet(target: &Path) -> DotnetEnv {
    let version = get_version("dotnet", &["--version"]);

    // Check coverlet
    let coverage_tool = if has_coverlet(target) {
        Some("coverlet".to_string())
    } else {
        None
    };

    DotnetEnv {
        version,
        coverage_tool,
    }
}

fn has_coverlet(target: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(target) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("csproj") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if content.contains("coverlet") || content.contains("Coverlet") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Full probe
// ---------------------------------------------------------------------------

/// Build a complete `EnvironmentProbe` for a target directory, populating
/// the field that corresponds to `lang`.
pub fn probe_all(target: &Path, lang: Language) -> EnvironmentProbe {
    let mut probe = EnvironmentProbe::empty(target);
    probe.primary_language = Some(lang.to_string());

    match lang {
        Language::Python => probe.python = Some(probe_python(target)),
        Language::JavaScript => probe.javascript = Some(probe_javascript(target)),
        Language::Rust => probe.rust = Some(probe_rust(target)),
        Language::Go => probe.go = Some(probe_go(target)),
        Language::Java | Language::Kotlin => probe.java = Some(probe_jvm(target)),
        Language::Ruby => probe.ruby = Some(probe_ruby(target)),
        Language::C | Language::Cpp => probe.c_cpp = Some(probe_c_cpp(target)),
        Language::Swift => probe.swift = Some(probe_swift(target)),
        Language::CSharp => probe.csharp = Some(probe_dotnet(target)),
        Language::Wasm => {} // no env struct for Wasm
    }

    probe
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::Language;
    use std::path::Path;

    // -----------------------------------------------------------------------
    // get_version helper
    // -----------------------------------------------------------------------

    #[test]
    fn get_version_nonexistent_command_returns_empty() {
        let v = get_version("__apex_no_such_binary__", &["--version"]);
        assert_eq!(v, "");
    }

    #[test]
    fn get_version_known_tool_returns_nonempty() {
        // `true` is always on PATH and exits 0, but produces no output —
        // use `echo` as an always-present command that writes stdout.
        let v = get_version("echo", &["hello"]);
        assert_eq!(v, "hello");
    }

    // -----------------------------------------------------------------------
    // check_tool helper
    // -----------------------------------------------------------------------

    #[test]
    fn check_tool_nonexistent_returns_none() {
        let r = check_tool("__apex_no_such_binary__", &["--version"]);
        assert!(r.is_none());
    }

    #[test]
    fn check_tool_present_tool_returns_some() {
        let r = check_tool("echo", &["world"]);
        assert!(r.is_some());
        assert_eq!(r.unwrap(), "world");
    }

    // -----------------------------------------------------------------------
    // probe_rust: returns a populated RustEnv
    // -----------------------------------------------------------------------

    #[test]
    fn probe_rust_returns_toolchain_string() {
        let env = probe_rust(Path::new("/tmp"));
        // In a CI environment rustc is present; version should be non-empty.
        // We can't assert exact values, just structural correctness.
        // If rustc is absent the version will be "".
        let _ = &env.toolchain; // just access it
        let _ = &env.version;
    }

    // -----------------------------------------------------------------------
    // probe_python: returns a PythonEnv with at least interpreter set
    // -----------------------------------------------------------------------

    #[test]
    fn probe_python_returns_populated_env() {
        let tmp = tempfile::tempdir().unwrap();
        let env = probe_python(tmp.path());
        // interpreter must be non-empty PathBuf
        assert!(!env.interpreter.as_os_str().is_empty());
    }

    // -----------------------------------------------------------------------
    // probe_go: version string from `go version`
    // -----------------------------------------------------------------------

    #[test]
    fn probe_go_returns_env() {
        let env = probe_go(Path::new("/tmp"));
        // go_cover is true only when `go` is on PATH; just check no panic
        let _ = env.go_cover;
        let _ = &env.version;
    }

    // -----------------------------------------------------------------------
    // probe_all: primary_language matches input
    // -----------------------------------------------------------------------

    #[test]
    fn probe_all_primary_language_matches_input() {
        let tmp = tempfile::tempdir().unwrap();
        let probe = probe_all(tmp.path(), Language::Python);
        assert_eq!(probe.primary_language.as_deref(), Some("python"));
    }

    // -----------------------------------------------------------------------
    // probe_all: Python language → python field is Some
    // -----------------------------------------------------------------------

    #[test]
    fn probe_all_python_populates_python_field() {
        let tmp = tempfile::tempdir().unwrap();
        let probe = probe_all(tmp.path(), Language::Python);
        assert!(probe.python.is_some());
        assert!(probe.javascript.is_none());
        assert!(probe.rust.is_none());
    }

    // -----------------------------------------------------------------------
    // probe_all: Rust language → rust field is Some
    // -----------------------------------------------------------------------

    #[test]
    fn probe_all_rust_populates_rust_field() {
        let tmp = tempfile::tempdir().unwrap();
        let probe = probe_all(tmp.path(), Language::Rust);
        assert!(probe.rust.is_some());
        assert!(probe.python.is_none());
    }

    // -----------------------------------------------------------------------
    // probe_all: Kotlin → java field (shared JvmEnv)
    // -----------------------------------------------------------------------

    #[test]
    fn probe_all_kotlin_populates_java_field() {
        let tmp = tempfile::tempdir().unwrap();
        let probe = probe_all(tmp.path(), Language::Kotlin);
        assert!(probe.java.is_some());
    }

    // -----------------------------------------------------------------------
    // probe_all: Wasm → no crash, no language field set
    // -----------------------------------------------------------------------

    #[test]
    fn probe_all_unknown_language_wasm_no_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let probe = probe_all(tmp.path(), Language::Wasm);
        assert!(probe.python.is_none());
        assert!(probe.rust.is_none());
        assert!(probe.javascript.is_none());
        assert_eq!(probe.primary_language.as_deref(), Some("wasm"));
    }

    // -----------------------------------------------------------------------
    // probe_all: C → c_cpp field
    // -----------------------------------------------------------------------

    #[test]
    fn probe_all_c_populates_c_cpp_field() {
        let tmp = tempfile::tempdir().unwrap();
        let probe = probe_all(tmp.path(), Language::C);
        assert!(probe.c_cpp.is_some());
    }

    // -----------------------------------------------------------------------
    // probe_dotnet: version field populated (dotnet may not be on PATH)
    // -----------------------------------------------------------------------

    #[test]
    fn probe_dotnet_does_not_panic() {
        let tmp = tempfile::tempdir().unwrap();
        let env = probe_dotnet(tmp.path());
        // coverage_tool None is fine when no csproj present
        assert!(env.coverage_tool.is_none());
    }
}
