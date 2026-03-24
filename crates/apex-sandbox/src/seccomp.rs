/// Apply a restrictive seccomp-bpf filter to the current process.
///
/// Call this AFTER fork, BEFORE exec of the target binary. The filter
/// allows the minimal set of syscalls needed to run a fuzz target:
/// read, write, open/openat, close, mmap, mprotect, exit/exit_group,
/// brk, futex, fstat, lseek, stat, shared-memory (for SanCov bitmap).
///
/// Network syscalls (socket, connect, bind, listen, accept) and process
/// spawning (fork, clone) are denied. Any unrecognised syscall kills the
/// process via SECCOMP_RET_KILL_PROCESS.
///
/// This function is a no-op on non-Linux platforms and on Linux when the
/// `seccomp` Cargo feature is not enabled.
pub fn apply_seccomp_filter() -> apex_core::error::Result<()> {
    _apply()
}

// ---------------------------------------------------------------------------
// Linux + seccomp feature
// ---------------------------------------------------------------------------

#[cfg(all(target_os = "linux", feature = "seccomp"))]
fn _apply() -> apex_core::error::Result<()> {
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter, SeccompRule};
    use std::collections::BTreeMap;

    let rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::from([
        // I/O
        (libc::SYS_read, vec![]),
        (libc::SYS_write, vec![]),
        (libc::SYS_readv, vec![]),
        (libc::SYS_writev, vec![]),
        (libc::SYS_pread64, vec![]),
        (libc::SYS_pwrite64, vec![]),
        // File operations
        (libc::SYS_open, vec![]),
        (libc::SYS_openat, vec![]),
        (libc::SYS_close, vec![]),
        (libc::SYS_fstat, vec![]),
        (libc::SYS_stat, vec![]),
        (libc::SYS_lstat, vec![]),
        (libc::SYS_newfstatat, vec![]),
        (libc::SYS_lseek, vec![]),
        (libc::SYS_access, vec![]),
        (libc::SYS_faccessat, vec![]),
        (libc::SYS_getcwd, vec![]),
        (libc::SYS_getdents64, vec![]),
        (libc::SYS_ioctl, vec![]),
        (libc::SYS_fcntl, vec![]),
        (libc::SYS_dup, vec![]),
        (libc::SYS_dup2, vec![]),
        (libc::SYS_dup3, vec![]),
        (libc::SYS_pipe, vec![]),
        (libc::SYS_pipe2, vec![]),
        // Memory
        (libc::SYS_mmap, vec![]),
        (libc::SYS_mprotect, vec![]),
        (libc::SYS_munmap, vec![]),
        (libc::SYS_madvise, vec![]),
        (libc::SYS_brk, vec![]),
        (libc::SYS_mremap, vec![]),
        // Process / thread
        (libc::SYS_exit, vec![]),
        (libc::SYS_exit_group, vec![]),
        (libc::SYS_futex, vec![]),
        (libc::SYS_nanosleep, vec![]),
        (libc::SYS_clock_nanosleep, vec![]),
        (libc::SYS_getpid, vec![]),
        (libc::SYS_gettid, vec![]),
        (libc::SYS_getuid, vec![]),
        (libc::SYS_getgid, vec![]),
        (libc::SYS_geteuid, vec![]),
        (libc::SYS_getegid, vec![]),
        (libc::SYS_getrlimit, vec![]),
        (libc::SYS_setrlimit, vec![]),
        (libc::SYS_prctl, vec![]),
        (libc::SYS_arch_prctl, vec![]),
        (libc::SYS_sched_yield, vec![]),
        // Signal handling
        (libc::SYS_rt_sigaction, vec![]),
        (libc::SYS_rt_sigprocmask, vec![]),
        (libc::SYS_rt_sigreturn, vec![]),
        (libc::SYS_sigaltstack, vec![]),
        // Shared memory (for AFL/SanCov bitmap)
        (libc::SYS_shmat, vec![]),
        (libc::SYS_shmget, vec![]),
        (libc::SYS_shmdt, vec![]),
        (libc::SYS_shmctl, vec![]),
        // Time
        (libc::SYS_gettimeofday, vec![]),
        (libc::SYS_clock_gettime, vec![]),
        (libc::SYS_time, vec![]),
    ]);

    let target_arch = std::env::consts::ARCH
        .try_into()
        .map_err(|_| apex_core::error::ApexError::Sandbox("unsupported arch for seccomp".into()))?;

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::KillProcess,
        SeccompAction::Allow,
        target_arch,
    )
    .map_err(|e| apex_core::error::ApexError::Sandbox(format!("seccomp filter build: {e}")))?;

    let program: BpfProgram = filter
        .try_into()
        .map_err(|e| apex_core::error::ApexError::Sandbox(format!("seccomp bpf compile: {e}")))?;

    seccompiler::apply_filter(&program)
        .map_err(|e| apex_core::error::ApexError::Sandbox(format!("seccomp apply: {e}")))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Stub — Linux without feature, or non-Linux
// ---------------------------------------------------------------------------

#[cfg(not(all(target_os = "linux", feature = "seccomp")))]
fn _apply() -> apex_core::error::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The filter function must be callable without panicking.
    /// On non-Linux (or when the seccomp feature is absent) it is a no-op.
    /// On Linux with the feature it may return Ok or an Err depending on the
    /// kernel — but it must never panic.
    #[test]
    fn apply_seccomp_filter_does_not_panic() {
        // Do NOT apply the filter inside the test-runner process; that would
        // lock down the test runner. We test the public API surface only.
        let _ = apply_seccomp_filter;
        // Verify the stub/no-op path on the current platform.
        #[cfg(not(all(target_os = "linux", feature = "seccomp")))]
        assert!(apply_seccomp_filter().is_ok());
    }
}
