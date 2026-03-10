//! RUSTC_WRAPPER logic for SanCov-instrumented builds.
//!
//! Generates the rustc flags needed to enable SanCov instrumentation.

/// SanCov instrumentation mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SanCovMode {
    /// trace-pc-guard: function call per edge. Most flexible.
    TracePcGuard,
    /// inline-8bit-counters: one `inc` instruction per edge. 2-5x faster.
    Inline8BitCounters,
    /// inline-bool-flag: one store per edge. Fastest, binary only.
    InlineBoolFlag,
}

/// Generate rustc flags for SanCov instrumentation.
pub fn sancov_rustc_flags(mode: SanCovMode, trace_compares: bool) -> Vec<String> {
    let mut flags = vec![
        "-C".into(),
        "passes=sancov-module".into(),
        "-C".into(),
        "llvm-args=-sanitizer-coverage-level=3".into(),
        "-C".into(),
        "codegen-units=1".into(),
    ];

    match mode {
        SanCovMode::TracePcGuard => {
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-trace-pc-guard".into());
        }
        SanCovMode::Inline8BitCounters => {
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-inline-8bit-counters".into());
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-pc-table".into());
        }
        SanCovMode::InlineBoolFlag => {
            flags.push("-C".into());
            flags.push("llvm-args=-sanitizer-coverage-inline-bool-flag".into());
        }
    }

    if trace_compares {
        flags.push("-C".into());
        flags.push("llvm-args=-sanitizer-coverage-trace-compares".into());
    }

    flags
}

/// Generate a complete RUSTC_WRAPPER shell command string.
pub fn wrapper_command(rustc_path: &str, mode: SanCovMode, trace_compares: bool) -> String {
    let flags = sancov_rustc_flags(mode, trace_compares);
    let flag_str = flags.join(" ");
    format!("{rustc_path} {flag_str} \"$@\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_pc_guard_flags() {
        let flags = sancov_rustc_flags(SanCovMode::TracePcGuard, false);
        assert!(flags.contains(&"passes=sancov-module".to_string()));
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-trace-pc-guard".to_string()));
        assert!(!flags.iter().any(|f| f.contains("trace-compares")));
    }

    #[test]
    fn inline_8bit_counters_flags() {
        let flags = sancov_rustc_flags(SanCovMode::Inline8BitCounters, false);
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-inline-8bit-counters".to_string()));
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-pc-table".to_string()));
    }

    #[test]
    fn inline_bool_flag_flags() {
        let flags = sancov_rustc_flags(SanCovMode::InlineBoolFlag, false);
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-inline-bool-flag".to_string()));
    }

    #[test]
    fn trace_compares_added() {
        let flags = sancov_rustc_flags(SanCovMode::TracePcGuard, true);
        assert!(flags.contains(&"llvm-args=-sanitizer-coverage-trace-compares".to_string()));
    }

    #[test]
    fn codegen_units_1() {
        let flags = sancov_rustc_flags(SanCovMode::TracePcGuard, false);
        assert!(flags.contains(&"codegen-units=1".to_string()));
    }

    #[test]
    fn wrapper_command_format() {
        let cmd = wrapper_command("rustc", SanCovMode::TracePcGuard, false);
        assert!(cmd.starts_with("rustc"));
        assert!(cmd.contains("sancov-module"));
        assert!(cmd.ends_with("\"$@\""));
    }
}
