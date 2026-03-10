use apex_core::{
    command::{CommandRunner, CommandSpec, RealCommandRunner},
    error::{ApexError, Result},
    traits::{LanguageRunner, TestRunOutput},
    types::Language,
};
use async_trait::async_trait;
use std::{path::Path, time::Instant};
use tracing::{info, warn};

pub struct JavaRunner<R: CommandRunner = RealCommandRunner> {
    runner: R,
}

impl JavaRunner {
    pub fn new() -> Self {
        JavaRunner {
            runner: RealCommandRunner,
        }
    }
}

impl<R: CommandRunner> JavaRunner<R> {
    pub fn with_runner(runner: R) -> Self {
        JavaRunner { runner }
    }
}

impl Default for JavaRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Detect whether the project uses Gradle or Maven.
fn detect_build_tool(target: &Path) -> &'static str {
    if target.join("build.gradle").exists() || target.join("build.gradle.kts").exists() {
        "gradle"
    } else {
        "maven"
    }
}

#[async_trait]
impl<R: CommandRunner> LanguageRunner for JavaRunner<R> {
    fn language(&self) -> Language {
        Language::Java
    }

    fn detect(&self, target: &Path) -> bool {
        target.join("pom.xml").exists()
            || target.join("build.gradle").exists()
            || target.join("build.gradle.kts").exists()
    }

    async fn install_deps(&self, target: &Path) -> Result<()> {
        info!(target = %target.display(), "installing Java dependencies");

        let build_tool = detect_build_tool(target);

        let spec = if build_tool == "gradle" {
            CommandSpec::new("./gradlew", target).args(["dependencies", "--quiet"])
        } else {
            CommandSpec::new("mvn", target).args(["-q", "dependency:resolve", "-DskipTests"])
        };

        let result = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("spawn {}: {e}", spec.program)))?;

        if result.exit_code != 0 {
            warn!(
                exit = result.exit_code,
                build_tool,
                "dependency installation returned non-zero (offline builds may still work)"
            );
        }

        Ok(())
    }

    async fn run_tests(&self, target: &Path, extra_args: &[String]) -> Result<TestRunOutput> {
        let build_tool = detect_build_tool(target);

        let mut cmd_parts: Vec<String> = if build_tool == "gradle" {
            vec!["./gradlew".into(), "test".into(), "--quiet".into()]
        } else {
            vec!["mvn".into(), "-q".into(), "test".into()]
        };

        cmd_parts.extend(extra_args.iter().cloned());

        info!(
            target = %target.display(),
            cmd = ?cmd_parts,
            "running Java tests"
        );

        let spec = CommandSpec::new(&cmd_parts[0], target).args(cmd_parts[1..].to_vec());

        let start = Instant::now();
        let output = self
            .runner
            .run_command(&spec)
            .await
            .map_err(|e| ApexError::LanguageRunner(format!("run tests: {e}")))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(TestRunOutput {
            exit_code: output.exit_code,
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::command::CommandOutput;
    use apex_core::traits::LanguageRunner;

    mockall::mock! {
        Cmd {}
        #[async_trait]
        impl CommandRunner for Cmd {
            async fn run_command(&self, spec: &CommandSpec) -> Result<CommandOutput>;
        }
    }

    #[test]
    fn detect_pom_xml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
        assert!(JavaRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_build_gradle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();
        assert!(JavaRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_build_gradle_kts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();
        assert!(JavaRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_empty_dir_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!JavaRunner::new().detect(dir.path()));
    }

    #[test]
    fn detect_build_tool_gradle() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();
        assert_eq!(detect_build_tool(dir.path()), "gradle");
    }

    #[test]
    fn detect_build_tool_gradle_kts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle.kts"), "").unwrap();
        assert_eq!(detect_build_tool(dir.path()), "gradle");
    }

    #[test]
    fn detect_build_tool_maven_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(detect_build_tool(dir.path()), "maven");
    }

    #[test]
    fn detect_build_tool_maven_with_pom() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "").unwrap();
        assert_eq!(detect_build_tool(dir.path()), "maven");
    }

    #[test]
    fn language_is_java() {
        assert_eq!(JavaRunner::new().language(), Language::Java);
    }

    // ---- Mock-based tests ----

    #[tokio::test]
    async fn install_deps_gradle_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./gradlew")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"BUILD SUCCESSFUL".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_maven_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "mvn")
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"BUILD SUCCESS".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_nonzero_exit_still_ok() {
        // Non-zero exit from deps install is just a warning, not an error.
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .times(1)
            .returning(|_| Ok(CommandOutput::failure(1, b"FAILURE".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn install_deps_command_error() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn mvn: not found".into(),
            })
        });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.install_deps(dir.path()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_tests_gradle_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("build.gradle"), "").unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "./gradlew" && spec.args.contains(&"test".to_string()))
            .times(1)
            .returning(|_| {
                Ok(CommandOutput::success(
                    b"BUILD SUCCESSFUL\n5 tests passed".to_vec(),
                ))
            });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(result.stdout.contains("5 tests passed"));
    }

    #[tokio::test]
    async fn run_tests_maven_failure() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.program == "mvn")
            .times(1)
            .returning(|_| {
                Ok(CommandOutput {
                    exit_code: 1,
                    stdout: b"Tests run: 3, Failures: 1".to_vec(),
                    stderr: b"BUILD FAILURE".to_vec(),
                })
            });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await.unwrap();
        assert_eq!(result.exit_code, 1);
        assert!(result.stdout.contains("Failures: 1"));
    }

    #[tokio::test]
    async fn run_tests_command_not_found() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command().times(1).returning(|_| {
            Err(ApexError::Subprocess {
                exit_code: -1,
                stderr: "spawn mvn: No such file or directory".into(),
            })
        });

        let runner = JavaRunner::with_runner(mock);
        let result = runner.run_tests(dir.path(), &[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn run_tests_with_extra_args() {
        let dir = tempfile::tempdir().unwrap();

        let mut mock = MockCmd::new();
        mock.expect_run_command()
            .withf(|spec| spec.args.iter().any(|a| a == "-Dtest=MyTest"))
            .times(1)
            .returning(|_| Ok(CommandOutput::success(b"ok".to_vec())));

        let runner = JavaRunner::with_runner(mock);
        let result = runner
            .run_tests(dir.path(), &["-Dtest=MyTest".into()])
            .await
            .unwrap();
        assert_eq!(result.exit_code, 0);
    }
}
