use crate::error::Result;
use crate::types::{
    BranchId, ExecutionResult, ExplorationContext, InputSeed, InstrumentedTarget, Language,
    SnapshotId, SynthesizedTest, Target, TestCandidate,
};

/// A strategy that proposes inputs to drive coverage.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait Strategy: Send + Sync {
    fn name(&self) -> &str;
    async fn suggest_inputs(&self, ctx: &ExplorationContext) -> Result<Vec<InputSeed>>;
    async fn observe(&self, result: &ExecutionResult) -> Result<()>;
}

/// An execution environment that runs a seed and returns coverage data.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait Sandbox: Send + Sync {
    async fn run(&self, input: &InputSeed) -> Result<ExecutionResult>;
    async fn snapshot(&self) -> Result<SnapshotId>;
    async fn restore(&self, id: SnapshotId) -> Result<()>;
    fn language(&self) -> Language;
}

/// Instruments a target to emit branch coverage data.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait Instrumentor: Send + Sync {
    async fn instrument(&self, target: &Target) -> Result<InstrumentedTarget>;
    fn branch_ids(&self) -> &[BranchId];
}

/// Synthesizes concrete test files from `TestCandidate`s.
#[cfg_attr(test, mockall::automock)]
pub trait TestSynthesizer: Send + Sync {
    fn synthesize(&self, candidates: &[TestCandidate]) -> Result<Vec<SynthesizedTest>>;
    fn language(&self) -> Language;
}

/// Detects, installs, and runs the test suite for a given language.
#[cfg_attr(test, mockall::automock)]
#[async_trait::async_trait]
pub trait LanguageRunner: Send + Sync {
    fn language(&self) -> Language;
    fn detect(&self, target: &std::path::Path) -> bool;
    async fn install_deps(&self, target: &std::path::Path) -> Result<()>;
    async fn run_tests(
        &self,
        target: &std::path::Path,
        extra_args: &[String],
    ) -> Result<TestRunOutput>;
}

#[derive(Debug, Clone)]
pub struct TestRunOutput {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
}
