//! A deterministic [`CommandRunner`] for integration tests.
//!
//! Unlike [`MockCommandRunner`](crate::command::MockCommandRunner) which uses
//! expect/returning, [`FixtureRunner`] is data-driven: callers register
//! (program, args) patterns mapped to canned [`CommandOutput`] values.

use crate::command::{CommandOutput, CommandRunner, CommandSpec};
use crate::error::{ApexError, Result};
use async_trait::async_trait;

/// A deterministic `CommandRunner` for integration tests.
/// Maps (program, args) patterns to canned outputs.
pub struct FixtureRunner {
    fixtures: Vec<Fixture>,
}

struct Fixture {
    program: String,
    args: Option<Vec<String>>,
    output: CommandOutput,
}

impl FixtureRunner {
    pub fn new() -> Self {
        FixtureRunner {
            fixtures: Vec::new(),
        }
    }

    /// Match any command with this program name.
    pub fn on(mut self, program: &str, output: CommandOutput) -> Self {
        self.fixtures.push(Fixture {
            program: program.into(),
            args: None,
            output,
        });
        self
    }

    /// Match commands with this program AND these args (prefix match).
    pub fn on_args(mut self, program: &str, args: &[&str], output: CommandOutput) -> Self {
        self.fixtures.push(Fixture {
            program: program.into(),
            args: Some(args.iter().map(|s| s.to_string()).collect()),
            output,
        });
        self
    }
}

impl Default for FixtureRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CommandRunner for FixtureRunner {
    async fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput> {
        // Try most-specific match first (with args), then program-only.
        // Iterate in reverse so later .on() calls take priority.
        for fixture in self.fixtures.iter().rev() {
            if fixture.program != spec.program {
                continue;
            }
            if let Some(ref expected_args) = fixture.args {
                if spec.args.len() >= expected_args.len()
                    && spec.args[..expected_args.len()] == **expected_args
                {
                    return Ok(fixture.output.clone());
                }
            }
        }
        // Try program-only matches.
        for fixture in self.fixtures.iter().rev() {
            if fixture.program == spec.program && fixture.args.is_none() {
                return Ok(fixture.output.clone());
            }
        }
        Err(ApexError::Detect(format!(
            "FixtureRunner: no fixture for `{} {}`",
            spec.program,
            spec.args.join(" ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn matches_exact_program() {
        let runner = FixtureRunner::new().on("cargo", CommandOutput::success(b"ok".to_vec()));
        let spec = CommandSpec::new("cargo", "/tmp");
        let result = runner.run_command(&spec).await.unwrap();
        assert_eq!(result.stdout, b"ok");
    }

    #[tokio::test]
    async fn matches_program_with_args() {
        let runner = FixtureRunner::new().on_args(
            "cargo",
            &["audit", "--json"],
            CommandOutput::success(b"{\"vulnerabilities\":{\"found\":0,\"list\":[]}}".to_vec()),
        );
        let spec = CommandSpec::new("cargo", "/tmp").args(["audit", "--json"]);
        let result = runner.run_command(&spec).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn args_match_is_prefix() {
        let runner = FixtureRunner::new().on_args(
            "cargo",
            &["test"],
            CommandOutput::success(b"pass".to_vec()),
        );
        // Extra args after prefix should still match.
        let spec = CommandSpec::new("cargo", "/tmp").args(["test", "--lib", "-v"]);
        let result = runner.run_command(&spec).await.unwrap();
        assert_eq!(result.stdout, b"pass");
    }

    #[tokio::test]
    async fn unmatched_command_returns_error() {
        let runner = FixtureRunner::new();
        let spec = CommandSpec::new("unknown", "/tmp");
        let result = runner.run_command(&spec).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn specific_args_take_priority_over_program_only() {
        let runner = FixtureRunner::new()
            .on("cargo", CommandOutput::success(b"generic".to_vec()))
            .on_args(
                "cargo",
                &["test"],
                CommandOutput::success(b"specific".to_vec()),
            );

        let test_spec = CommandSpec::new("cargo", "/tmp").args(["test"]);
        let result = runner.run_command(&test_spec).await.unwrap();
        assert_eq!(result.stdout, b"specific");

        let build_spec = CommandSpec::new("cargo", "/tmp").args(["build"]);
        let result = runner.run_command(&build_spec).await.unwrap();
        assert_eq!(result.stdout, b"generic");
    }

    #[tokio::test]
    async fn failure_output() {
        let runner = FixtureRunner::new().on("bad", CommandOutput::failure(1, b"error".to_vec()));
        let spec = CommandSpec::new("bad", "/tmp");
        let result = runner.run_command(&spec).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert_eq!(result.stderr, b"error");
    }

    #[test]
    fn default_creates_empty() {
        let runner = FixtureRunner::default();
        assert!(runner.fixtures.is_empty());
    }
}
