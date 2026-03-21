//! `apex wrap` — run a user's test command with coverage injection.
//!
//! Usage:
//! ```text
//! apex wrap --lang python -- pytest -q
//! apex wrap --lang go -- go test ./...
//! apex wrap -- cargo test          # auto-detect from command
//! ```

use apex_core::types::Language;
use apex_instrument::wrap::{detect_language_from_command, inject_coverage};
use clap::Parser;
use color_eyre::{eyre::eyre, Result};
use std::path::PathBuf;
use tracing::info;

use crate::LangArg;

/// CLI arguments for `apex wrap`.
#[derive(Parser)]
pub struct WrapArgs {
    /// Language of the project (auto-detected from command if omitted).
    #[arg(long, short, value_enum)]
    pub lang: Option<LangArg>,

    /// Directory to write coverage output files.
    #[arg(long, short, default_value = ".apex-coverage")]
    pub output_dir: PathBuf,

    /// The test command and its arguments (everything after `--`).
    #[arg(trailing_var_arg = true, required = true)]
    pub cmd: Vec<String>,
}

/// Execute the wrapped test command with coverage instrumentation.
pub async fn run_wrap(args: WrapArgs) -> Result<()> {
    if args.cmd.is_empty() {
        return Err(eyre!(
            "No command specified. Usage: apex wrap [--lang <lang>] -- <test-command>"
        ));
    }

    // Resolve language: explicit flag wins, otherwise auto-detect.
    let lang: Language = match args.lang {
        Some(l) => l.into(),
        None => detect_language_from_command(&args.cmd).ok_or_else(|| {
            eyre!(
                "Cannot auto-detect language from command {:?}. Use --lang to specify.",
                args.cmd.first().unwrap_or(&String::new())
            )
        })?,
    };

    // Ensure output directory exists.
    std::fs::create_dir_all(&args.output_dir)?;

    let injection = inject_coverage(lang, &args.cmd, &args.output_dir);

    info!(
        lang = %lang,
        cmd = ?injection.args,
        env = ?injection.env_vars,
        output_dir = %args.output_dir.display(),
        "Running wrapped command with coverage injection"
    );

    // Build the child process.
    let program = injection
        .args
        .first()
        .ok_or_else(|| eyre!("Injected command is empty"))?;

    let mut child = tokio::process::Command::new(program);
    child.args(&injection.args[1..]);
    for (k, v) in &injection.env_vars {
        child.env(k, v);
    }

    let status = child.status().await?;

    if status.success() {
        info!(
            output_dir = %args.output_dir.display(),
            "Test command succeeded — coverage data written"
        );
    } else {
        let code = status.code().unwrap_or(-1);
        eprintln!(
            "apex wrap: test command exited with code {code}"
        );
        // Still exit with the child's code so CI pipelines propagate failure.
        std::process::exit(code);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_args_parsing() {
        // Simulate: apex wrap --lang python -- pytest -q
        let args = WrapArgs::try_parse_from([
            "wrap", "--lang", "python", "--", "pytest", "-q",
        ])
        .unwrap();
        assert!(matches!(args.lang, Some(LangArg::Python)));
        assert_eq!(args.cmd, vec!["pytest", "-q"]);
    }

    #[test]
    fn test_wrap_args_auto_detect() {
        // Simulate: apex wrap -- cargo test
        let args =
            WrapArgs::try_parse_from(["wrap", "--", "cargo", "test"]).unwrap();
        assert!(args.lang.is_none());
        assert_eq!(args.cmd, vec!["cargo", "test"]);
    }

    #[test]
    fn test_wrap_args_custom_output_dir() {
        let args = WrapArgs::try_parse_from([
            "wrap",
            "--output-dir",
            "/tmp/my-cov",
            "--",
            "go",
            "test",
            "./...",
        ])
        .unwrap();
        assert_eq!(args.output_dir, PathBuf::from("/tmp/my-cov"));
    }

    #[test]
    fn test_wrap_args_default_output_dir() {
        let args =
            WrapArgs::try_parse_from(["wrap", "--", "npm", "test"]).unwrap();
        assert_eq!(args.output_dir, PathBuf::from(".apex-coverage"));
    }

    #[test]
    fn test_wrap_args_requires_cmd() {
        let result = WrapArgs::try_parse_from(["wrap"]);
        assert!(result.is_err());
    }
}
