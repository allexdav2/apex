//! AI agent orchestration for APEX — multi-agent ensemble strategies,
//! test generation, and coverage-driven refinement loops.

pub mod driller;
pub mod ensemble;
pub mod exchange;
pub mod ledger;
pub mod monitor;
pub mod orchestrator;
pub mod source;

pub use ledger::BugLedger;
pub use orchestrator::{AgentCluster, OrchestratorConfig};
pub use source::{build_uncovered_with_lines, extract_source_contexts};
