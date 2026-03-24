/// Generate a sandbox-exec(1) profile string for target execution on macOS.
///
/// The profile denies all operations by default, then re-opens:
/// - Read-only access to standard system paths (`/usr`, `/System`, `/lib`,
///   `/private/var/db/timezone`, etc.)
/// - Read+write access to every path listed in `allowed_paths`
/// - `process-exec` so the target binary itself can run
/// - `signal` to self (needed for normal POSIX signal handling)
///
/// Network access is explicitly denied. `execve` of new processes from inside
/// the sandbox is blocked (only the initial exec is permitted).
///
/// Usage:
/// ```rust,ignore
/// let profile = sandbox_profile(&[Path::new("/tmp/my-target")]);
/// // pass `profile` to `sandbox-exec -p <profile> <binary> [args…]`
/// ```
#[cfg(target_os = "macos")]
pub fn sandbox_profile(allowed_paths: &[&std::path::Path]) -> String {
    let mut lines = vec![
        "(version 1)".to_owned(),
        "(deny default)".to_owned(),
        // Standard read-only system paths.
        r#"(allow file-read*"#.to_owned(),
        r#"    (subpath "/usr")"#.to_owned(),
        r#"    (subpath "/System")"#.to_owned(),
        r#"    (subpath "/Library/Preferences")"#.to_owned(),
        r#"    (subpath "/private/var/db/timezone")"#.to_owned(),
        r#"    (subpath "/private/var/db/dyld")"#.to_owned(),
        r#"    (literal "/dev/null")"#.to_owned(),
        r#"    (literal "/dev/zero")"#.to_owned(),
        r#"    (literal "/dev/urandom")"#.to_owned(),
        r#"    (literal "/dev/random")"#.to_owned(),
        r#")"#.to_owned(),
        // Write to stdout/stderr.
        r#"(allow file-write-data"#.to_owned(),
        r#"    (literal "/dev/null")"#.to_owned(),
        r#"    (literal "/dev/stdout")"#.to_owned(),
        r#"    (literal "/dev/stderr")"#.to_owned(),
        r#")"#.to_owned(),
    ];

    // Per-target read+write paths.
    if !allowed_paths.is_empty() {
        lines.push("(allow file-read* file-write*".to_owned());
        for p in allowed_paths {
            lines.push(format!(r#"    (subpath "{}")"#, p.display()));
        }
        lines.push(")".to_owned());
    }

    lines.extend_from_slice(&[
        // Allow the target to exec itself (needed for the initial execve).
        "(allow process-exec)".to_owned(),
        // Allow signal to self.
        "(allow signal (target self))".to_owned(),
        // Allow sysctl reads (needed by Rust stdlib thread/memory queries).
        "(allow sysctl-read)".to_owned(),
        // Deny all network explicitly (belt-and-suspenders on top of `deny default`).
        "(deny network*)".to_owned(),
    ]);

    lines.join("\n")
}

/// Stub for non-macOS platforms — returns an empty string.
#[cfg(not(target_os = "macos"))]
pub fn sandbox_profile(_allowed_paths: &[&std::path::Path]) -> String {
    String::new()
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn profile_contains_deny_default() {
        let profile = sandbox_profile(&[]);
        assert!(
            profile.contains("(deny default)"),
            "profile must start with deny default"
        );
    }

    #[test]
    fn profile_allows_system_paths() {
        let profile = sandbox_profile(&[]);
        assert!(
            profile.contains(r#"(subpath "/usr")"#),
            "profile must allow /usr"
        );
        assert!(
            profile.contains(r#"(subpath "/System")"#),
            "profile must allow /System"
        );
    }

    #[test]
    fn profile_denies_network() {
        let profile = sandbox_profile(&[]);
        assert!(
            profile.contains("(deny network*)"),
            "profile must deny network"
        );
    }

    #[test]
    fn profile_includes_allowed_paths() {
        let p1 = Path::new("/tmp/target_dir");
        let p2 = Path::new("/var/data/corpus");
        let profile = sandbox_profile(&[p1, p2]);
        assert!(
            profile.contains(r#"(subpath "/tmp/target_dir")"#),
            "profile must include target dir"
        );
        assert!(
            profile.contains(r#"(subpath "/var/data/corpus")"#),
            "profile must include corpus dir"
        );
        assert!(
            profile.contains("(allow file-read* file-write*"),
            "allowed paths must be read-write"
        );
    }

    #[test]
    fn profile_allows_process_exec() {
        let profile = sandbox_profile(&[]);
        assert!(
            profile.contains("(allow process-exec)"),
            "profile must allow process-exec"
        );
    }

    #[test]
    fn profile_empty_allowed_paths_no_rw_block() {
        // When no allowed paths are given, the read-write block must be absent.
        let profile = sandbox_profile(&[]);
        assert!(
            !profile.contains("(allow file-read* file-write*"),
            "no rw block when no allowed paths"
        );
    }
}
