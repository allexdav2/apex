//! Git helper utilities for APEX.
//!
//! Thin wrappers around the `git` CLI for diff analysis, file retrieval,
//! and blame queries. Uses [`CommandRunner`] so tests can inject mocks.

use crate::command::{CommandRunner, CommandSpec};
use crate::error::{ApexError, Result};
use std::path::{Path, PathBuf};

/// Return files changed between `git_ref` and HEAD.
pub async fn changed_files_since(
    runner: &dyn CommandRunner,
    repo: &Path,
    git_ref: &str,
) -> Result<Vec<PathBuf>> {
    let spec = CommandSpec::new("git", repo)
        .args(["diff", "--name-only", git_ref, "HEAD"])
        .timeout(15_000);
    let output = runner
        .run_command(&spec)
        .await
        .map_err(|e| ApexError::Detect(format!("git diff --name-only: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| PathBuf::from(l.trim()))
        .collect())
}

/// Read a file's contents at a specific git ref.
pub async fn file_at_ref(
    runner: &dyn CommandRunner,
    repo: &Path,
    git_ref: &str,
    file: &Path,
) -> Result<String> {
    let file_spec = format!("{git_ref}:{}", file.display());
    let spec = CommandSpec::new("git", repo)
        .args(["show", &file_spec])
        .timeout(10_000);
    let output = runner
        .run_command(&spec)
        .await
        .map_err(|e| ApexError::Detect(format!("git show {file_spec}: {e}")))?;
    if output.exit_code != 0 {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApexError::Detect(format!(
            "git show {file_spec} failed: {stderr}"
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Get the author date of the most recent commit that touched a specific line.
///
/// Returns an ISO-8601 date string (e.g. "2024-06-15").
pub async fn blame_date(
    runner: &dyn CommandRunner,
    repo: &Path,
    file: &Path,
    line: u32,
) -> Result<String> {
    let line_range = format!("{line},{line}");
    let spec = CommandSpec::new("git", repo)
        .args([
            "blame",
            "-L",
            &line_range,
            "--porcelain",
            &file.display().to_string(),
        ])
        .timeout(10_000);
    let output = runner
        .run_command(&spec)
        .await
        .map_err(|e| ApexError::Detect(format!("git blame: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse porcelain output for "author-time <epoch>"
    for blame_line in stdout.lines() {
        if let Some(epoch_str) = blame_line.strip_prefix("author-time ") {
            if let Ok(epoch) = epoch_str.trim().parse::<i64>() {
                // Convert epoch to ISO date
                let secs = epoch;
                let days = secs / 86400;
                // Simple epoch-to-date: 1970-01-01 + days
                let date = epoch_to_date(days);
                return Ok(date);
            }
        }
    }

    Err(ApexError::Detect(format!(
        "git blame: no author-time found for {}:{}",
        file.display(),
        line
    )))
}

/// Convert days-since-epoch to YYYY-MM-DD string.
fn epoch_to_date(days_since_epoch: i64) -> String {
    // Algorithm from Howard Hinnant's civil_from_days
    let z = days_since_epoch + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandOutput;
    use crate::fixture_runner::FixtureRunner;

    #[tokio::test]
    async fn changed_files_since_parses_output() {
        let runner = FixtureRunner::new().on(
            "git",
            CommandOutput::success(b"src/main.rs\nsrc/lib.rs\n".to_vec()),
        );
        let files = changed_files_since(&runner, Path::new("/repo"), "HEAD~3")
            .await
            .unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0], PathBuf::from("src/main.rs"));
        assert_eq!(files[1], PathBuf::from("src/lib.rs"));
    }

    #[tokio::test]
    async fn changed_files_since_empty() {
        let runner = FixtureRunner::new().on("git", CommandOutput::success(b"".to_vec()));
        let files = changed_files_since(&runner, Path::new("/repo"), "main")
            .await
            .unwrap();
        assert!(files.is_empty());
    }

    #[tokio::test]
    async fn file_at_ref_returns_content() {
        let runner =
            FixtureRunner::new().on("git", CommandOutput::success(b"fn main() {}\n".to_vec()));
        let content = file_at_ref(
            &runner,
            Path::new("/repo"),
            "main",
            Path::new("src/main.rs"),
        )
        .await
        .unwrap();
        assert_eq!(content, "fn main() {}\n");
    }

    #[tokio::test]
    async fn file_at_ref_failure() {
        let runner = FixtureRunner::new().on(
            "git",
            CommandOutput::failure(128, b"fatal: not a git repository".to_vec()),
        );
        let result =
            file_at_ref(&runner, Path::new("/repo"), "main", Path::new("missing.rs")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn blame_date_parses_porcelain() {
        // 1718409600 = 2024-06-15 00:00:00 UTC
        let porcelain = b"abc123 1 1 1\nauthor John\nauthor-mail <j@example.com>\nauthor-time 1718409600\nauthor-tz +0000\n";
        let runner = FixtureRunner::new().on("git", CommandOutput::success(porcelain.to_vec()));
        let date = blame_date(&runner, Path::new("/repo"), Path::new("src/lib.rs"), 42)
            .await
            .unwrap();
        assert_eq!(date, "2024-06-15");
    }

    #[tokio::test]
    async fn blame_date_missing_author_time() {
        let runner = FixtureRunner::new().on(
            "git",
            CommandOutput::success(b"abc123 1 1 1\nauthor John\n".to_vec()),
        );
        let result = blame_date(&runner, Path::new("/repo"), Path::new("src/lib.rs"), 1).await;
        assert!(result.is_err());
    }

    #[test]
    fn epoch_to_date_known_dates() {
        assert_eq!(epoch_to_date(0), "1970-01-01");
        // 2024-06-15 = day 19889
        assert_eq!(epoch_to_date(19889), "2024-06-15");
        // 2026-03-14 = day 20526
        assert_eq!(epoch_to_date(20526), "2026-03-14");
    }
}
