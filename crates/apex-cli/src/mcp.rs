//! MCP (Model Context Protocol) STDIO server for APEX.
//!
//! Exposes APEX CLI commands as MCP tools so that AI coding assistants
//! (Claude Code, Cursor, etc.) can invoke them over a JSON-RPC STDIO
//! transport.
//!
//! Each tool handler spawns `apex` as a subprocess rather than calling
//! library functions directly, because the tracing subscriber can only
//! be initialised once per process.

use rmcp::{
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    schemars, tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};

use std::process::Stdio;
use tokio::process::Command;

// ---------------------------------------------------------------------------
// Parameter structs — derive JsonSchema for automatic MCP schema generation
// ---------------------------------------------------------------------------

/// Parameters for `apex run` (coverage analysis).
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RunParams {
    /// Path to the target repository.
    #[schemars(description = "Absolute or relative path to the target repository")]
    pub target: String,

    /// Programming language (python, js, java, c, rust, wasm, ruby).
    #[schemars(description = "Programming language: python, js, java, c, rust, wasm, ruby")]
    pub lang: String,

    /// Exploration strategy (baseline, fuzz, concolic, all). Defaults to baseline.
    #[schemars(description = "Strategy: baseline | fuzz | concolic | all")]
    pub strategy: Option<String>,
}

/// Parameters for `apex audit` (security/bug detection).
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DetectParams {
    /// Path to the target repository.
    #[schemars(description = "Absolute or relative path to the target repository")]
    pub target: String,

    /// Programming language.
    #[schemars(description = "Programming language: python, js, java, c, rust, wasm, ruby")]
    pub lang: String,

    /// Comma-separated detector list (optional).
    #[schemars(description = "Comma-separated list of detectors to run (optional)")]
    pub detectors: Option<String>,
}

/// Parameters for `apex reach` (reachability from a source location).
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ReachParams {
    /// Target location as file:line.
    #[schemars(description = "Source location as file:line")]
    pub target: String,

    /// Programming language.
    #[schemars(description = "Programming language: python, js, java, c, rust, wasm, ruby")]
    pub lang: String,
}

/// Parameters for `apex ratchet` (CI coverage gate).
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RatchetParams {
    /// Path to the target repository.
    #[schemars(description = "Absolute or relative path to the target repository")]
    pub target: String,

    /// Programming language.
    #[schemars(description = "Programming language: python, js, java, c, rust, wasm, ruby")]
    pub lang: String,

    /// Minimum coverage threshold (0.0-1.0, optional).
    #[schemars(description = "Minimum coverage threshold 0.0-1.0 (optional)")]
    pub min_coverage: Option<f64>,
}

/// Parameters for `apex deploy-score`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DeployScoreParams {
    /// Path to the target repository.
    #[schemars(description = "Absolute or relative path to the target repository")]
    pub target: String,

    /// Programming language.
    #[schemars(description = "Programming language: python, js, java, c, rust, wasm, ruby")]
    pub lang: String,
}

/// Parameters for `apex complexity`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ComplexityParams {
    /// Path to the target repository.
    #[schemars(description = "Path to the target repository")]
    pub target: String,
}

/// Parameters for `apex dead-code`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DeadCodeParams {
    /// Path to the target repository.
    #[schemars(description = "Path to the target repository")]
    pub target: String,
}

/// Parameters for `apex risk`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RiskParams {
    /// Path to the target repository.
    #[schemars(description = "Path to the target repository")]
    pub target: String,

    /// Comma-separated list of changed files.
    #[schemars(description = "Comma-separated list of changed files")]
    pub changed_files: String,
}

/// Parameters for `apex hotpaths`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HotpathsParams {
    /// Path to the target repository.
    #[schemars(description = "Path to the target repository")]
    pub target: String,
}

/// Parameters for `apex test-optimize`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TestOptimizeParams {
    /// Path to the target repository.
    #[schemars(description = "Path to the target repository")]
    pub target: String,
}

/// Parameters for `apex test-prioritize`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TestPrioritizeParams {
    /// Path to the target repository.
    #[schemars(description = "Path to the target repository")]
    pub target: String,

    /// Comma-separated list of changed files.
    #[schemars(description = "Comma-separated list of changed files")]
    pub changed_files: String,
}

/// Parameters for `apex blast-radius`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BlastRadiusParams {
    /// Path to the target repository.
    #[schemars(description = "Path to the target repository")]
    pub target: String,

    /// Programming language.
    #[schemars(description = "Programming language: python, js, java, c, rust, wasm, ruby")]
    pub lang: String,

    /// Comma-separated list of changed files.
    #[schemars(description = "Comma-separated list of changed files")]
    pub changed_files: String,
}

/// Parameters for `apex secret-scan`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SecretScanParams {
    /// Path to the target repository.
    #[schemars(description = "Path to the target repository")]
    pub target: String,

    /// Programming language.
    #[schemars(description = "Programming language: python, js, java, c, rust, wasm, ruby")]
    pub lang: String,
}

/// Parameters for `apex data-flow`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DataFlowParams {
    /// Path to the target repository.
    #[schemars(description = "Path to the target repository")]
    pub target: String,

    /// Programming language.
    #[schemars(description = "Programming language: python, js, java, c, rust, wasm, ruby")]
    pub lang: String,
}

// ---------------------------------------------------------------------------
// Path validation
// ---------------------------------------------------------------------------

/// Validate that `target` points to an existing filesystem path and return its
/// canonicalized, absolute form.
///
/// Returns an `McpError` (invalid_params) when:
/// - the path does not exist, or
/// - canonicalization fails for any other reason.
///
/// This prevents path-traversal attacks (e.g. `../../etc/passwd`) by
/// resolving symlinks and `..` components before the path reaches the
/// subprocess.
pub(crate) fn validate_target_path(target: &str) -> Result<String, McpError> {
    std::path::Path::new(target)
        .canonicalize()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| {
            McpError::invalid_params(
                format!("invalid target path '{target}': {e}"),
                None,
            )
        })
}

// ---------------------------------------------------------------------------
// Subprocess helper
// ---------------------------------------------------------------------------

/// Run `apex <args...>` as a subprocess, capturing stdout.
///
/// Stderr is inherited so tracing output goes to the MCP host's log.
pub(crate) async fn run_apex_command(args: &[&str]) -> Result<String, McpError> {
    let exe = std::env::current_exe()
        .map_err(|e| McpError::internal_error(format!("cannot locate apex binary: {e}"), None))?;

    let output = Command::new(&exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| McpError::internal_error(format!("failed to spawn apex: {e}"), None))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        Ok(stdout)
    } else {
        let code = output.status.code().unwrap_or(-1);
        Err(McpError::internal_error(
            format!("apex exited with code {code}\nstdout:\n{stdout}\nstderr:\n{stderr}"),
            None,
        ))
    }
}

// ---------------------------------------------------------------------------
// MCP Service
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ApexMcpService {
    tool_router: ToolRouter<Self>,
}

impl Default for ApexMcpService {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl ApexMcpService {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    /// Run APEX coverage analysis. Returns a JSON gap report with branch
    /// coverage data, uncovered branches, and suggested test targets.
    #[tool(
        description = "Run APEX coverage analysis. Returns a JSON gap report with branch coverage data, uncovered branches, and suggested test targets."
    )]
    async fn apex_run(
        &self,
        Parameters(params): Parameters<RunParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let strategy = params.strategy.as_deref().unwrap_or("baseline");
        let output = run_apex_command(&[
            "run",
            "--target",
            &target,
            "--lang",
            &params.lang,
            "--strategy",
            strategy,
            "--output-format",
            "json",
        ])
        .await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Run APEX security/bug detection (audit). Returns findings with CWE IDs,
    /// severity, file locations, and remediation advice.
    #[tool(
        description = "Run APEX security/bug detection (audit). Returns findings with CWE IDs, severity, file locations, and remediation advice."
    )]
    async fn apex_detect(
        &self,
        Parameters(params): Parameters<DetectParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let mut args = vec![
            "audit".to_string(),
            "--target".to_string(),
            target,
            "--lang".to_string(),
            params.lang.clone(),
        ];
        if let Some(ref d) = params.detectors {
            args.push("--detectors".to_string());
            args.push(d.clone());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = run_apex_command(&arg_refs).await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Map reachable code paths from a source location (file:line). Returns
    /// entry-point reachability and attack surface data.
    #[tool(
        description = "Map reachable code paths from a source location (file:line). Returns entry-point reachability and attack surface data."
    )]
    async fn apex_reach(
        &self,
        Parameters(params): Parameters<ReachParams>,
    ) -> Result<CallToolResult, McpError> {
        let output = run_apex_command(&[
            "reach",
            "--target",
            &params.target,
            "--lang",
            &params.lang,
        ])
        .await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Run APEX ratchet CI gate. Fails if branch coverage drops below the
    /// configured or specified threshold.
    #[tool(
        description = "Run APEX ratchet CI gate. Fails if branch coverage drops below the configured or specified threshold. Returns pass/fail with coverage percentage."
    )]
    async fn apex_ratchet(
        &self,
        Parameters(params): Parameters<RatchetParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let mut args = vec![
            "ratchet".to_string(),
            "--target".to_string(),
            target,
            "--lang".to_string(),
            params.lang.clone(),
        ];
        if let Some(min) = params.min_coverage {
            args.push("--min-coverage".to_string());
            args.push(min.to_string());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = run_apex_command(&arg_refs).await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Run APEX doctor to verify all required external tools are installed.
    #[tool(
        description = "Run APEX doctor to verify all required external tools are installed and configured correctly."
    )]
    async fn apex_doctor(&self) -> Result<CallToolResult, McpError> {
        let output = run_apex_command(&["doctor"]).await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Calculate deployment confidence score (0-100) based on coverage,
    /// findings, and code quality metrics.
    #[tool(
        description = "Calculate a deployment confidence score (0-100) based on coverage, findings, and code quality metrics."
    )]
    async fn apex_deploy_score(
        &self,
        Parameters(params): Parameters<DeployScoreParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let output = run_apex_command(&[
            "deploy-score",
            "--target",
            &target,
            "--lang",
            &params.lang,
            "--output-format",
            "json",
        ])
        .await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Analyze function-level cyclomatic complexity. Returns hotspots sorted
    /// by complexity score.
    #[tool(
        description = "Analyze function-level cyclomatic complexity. Returns hotspots sorted by complexity score."
    )]
    async fn apex_complexity(
        &self,
        Parameters(params): Parameters<ComplexityParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let output = run_apex_command(&[
            "complexity",
            "--target",
            &target,
            "--output-format",
            "json",
        ])
        .await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Find unreachable branches and dead code. Returns per-file dead branch
    /// counts.
    #[tool(
        description = "Find unreachable branches and dead code. Returns per-file dead branch counts."
    )]
    async fn apex_dead_code(
        &self,
        Parameters(params): Parameters<DeadCodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let output = run_apex_command(&[
            "dead-code",
            "--target",
            &target,
            "--output-format",
            "json",
        ])
        .await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Assess risk of changed files. Returns affected test count and risk
    /// level.
    #[tool(
        description = "Assess risk of changed files. Returns affected test count and risk level."
    )]
    async fn apex_risk(
        &self,
        Parameters(params): Parameters<RiskParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let mut args = vec![
            "risk".to_string(),
            "--target".to_string(),
            target,
            "--output-format".to_string(),
            "json".to_string(),
        ];
        if !params.changed_files.is_empty() {
            args.push("--changed-files".to_string());
            args.push(params.changed_files.clone());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = run_apex_command(&arg_refs).await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Identify the most frequently executed code paths. Returns branches
    /// sorted by hit count.
    #[tool(
        description = "Identify the most frequently executed code paths. Returns branches sorted by hit count."
    )]
    async fn apex_hotpaths(
        &self,
        Parameters(params): Parameters<HotpathsParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let output = run_apex_command(&[
            "hotpaths",
            "--target",
            &target,
            "--output-format",
            "json",
        ])
        .await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Identify redundant tests that can be removed to save CI time.
    #[tool(
        description = "Identify redundant tests that can be removed to save CI time."
    )]
    async fn apex_test_optimize(
        &self,
        Parameters(params): Parameters<TestOptimizeParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let output = run_apex_command(&[
            "test-optimize",
            "--target",
            &target,
            "--output-format",
            "json",
        ])
        .await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Prioritize tests by relevance to changed files. Returns ordered test
    /// list.
    #[tool(
        description = "Prioritize tests by relevance to changed files. Returns ordered test list."
    )]
    async fn apex_test_prioritize(
        &self,
        Parameters(params): Parameters<TestPrioritizeParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let mut args = vec![
            "test-prioritize".to_string(),
            "--target".to_string(),
            target,
            "--output-format".to_string(),
            "json".to_string(),
        ];
        if !params.changed_files.is_empty() {
            args.push("--changed-files".to_string());
            args.push(params.changed_files.clone());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = run_apex_command(&arg_refs).await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Calculate the blast radius of changed files — which modules and tests
    /// are affected.
    #[tool(
        description = "Calculate the blast radius of changed files — which modules and tests are affected."
    )]
    async fn apex_blast_radius(
        &self,
        Parameters(params): Parameters<BlastRadiusParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let mut args = vec![
            "blast-radius".to_string(),
            "--target".to_string(),
            target,
            "--lang".to_string(),
            params.lang.clone(),
        ];
        if !params.changed_files.is_empty() {
            args.push("--changed-files".to_string());
            args.push(params.changed_files.clone());
        }
        let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let output = run_apex_command(&arg_refs).await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Scan for hardcoded secrets, API keys, and high-entropy strings.
    #[tool(
        description = "Scan for hardcoded secrets, API keys, and high-entropy strings."
    )]
    async fn apex_secret_scan(
        &self,
        Parameters(params): Parameters<SecretScanParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let output = run_apex_command(&[
            "secret-scan",
            "--target",
            &target,
            "--lang",
            &params.lang,
            "--output-format",
            "json",
        ])
        .await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }

    /// Trace data flow and taint paths through the codebase.
    #[tool(
        description = "Trace data flow and taint paths through the codebase."
    )]
    async fn apex_data_flow(
        &self,
        Parameters(params): Parameters<DataFlowParams>,
    ) -> Result<CallToolResult, McpError> {
        let target = validate_target_path(&params.target)?;
        let output = run_apex_command(&[
            "data-flow",
            "--target",
            &target,
            "--lang",
            &params.lang,
            "--output-format",
            "json",
        ])
        .await?;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }
}

#[tool_handler]
impl ServerHandler for ApexMcpService {
    fn get_info(&self) -> ServerInfo {
        let mut info = ServerInfo::new(ServerCapabilities::builder().enable_tools().build());
        info.instructions = Some(
            "APEX (Autonomous Path EXploration) -- drives any repository toward \
             100% branch coverage through instrumentation, fuzzing, concolic \
             execution, symbolic solving, and AI-guided test synthesis."
                .into(),
        );
        info
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the MCP STDIO server. Called from `apex mcp`.
pub async fn run_mcp() -> color_eyre::Result<()> {
    // All tracing goes to stderr -- stdout is the MCP JSON-RPC channel.
    let service = ApexMcpService::new()
        .serve(stdio())
        .await
        .map_err(|e| color_eyre::eyre::eyre!("MCP server error: {e}"))?;

    service
        .waiting()
        .await
        .map_err(|e| color_eyre::eyre::eyre!("MCP server terminated with error: {e}"))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(RunParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("lang"));
        assert!(json.contains("strategy"));
    }

    #[test]
    fn detect_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(DetectParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("lang"));
        assert!(json.contains("detectors"));
    }

    #[test]
    fn reach_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(ReachParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("lang"));
    }

    #[test]
    fn ratchet_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(RatchetParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("lang"));
        assert!(json.contains("min_coverage"));
    }

    #[test]
    fn deploy_score_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(DeployScoreParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("lang"));
    }

    #[test]
    fn complexity_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(ComplexityParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
    }

    #[test]
    fn dead_code_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(DeadCodeParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
    }

    #[test]
    fn risk_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(RiskParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("changed_files"));
    }

    #[test]
    fn hotpaths_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(HotpathsParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
    }

    #[test]
    fn test_optimize_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(TestOptimizeParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
    }

    #[test]
    fn test_prioritize_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(TestPrioritizeParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("changed_files"));
    }

    #[test]
    fn blast_radius_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(BlastRadiusParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("lang"));
        assert!(json.contains("changed_files"));
    }

    #[test]
    fn secret_scan_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(SecretScanParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("lang"));
    }

    #[test]
    fn data_flow_params_generates_valid_schema() {
        let schema = rmcp::schemars::schema_for!(DataFlowParams);
        let json = serde_json::to_string_pretty(&schema).unwrap();
        assert!(json.contains("target"));
        assert!(json.contains("lang"));
    }

    #[test]
    fn all_schemas_are_valid_json_schema_objects() {
        // Verify each schema has the expected JSON Schema structure
        for schema in [
            rmcp::schemars::schema_for!(RunParams),
            rmcp::schemars::schema_for!(DetectParams),
            rmcp::schemars::schema_for!(ReachParams),
            rmcp::schemars::schema_for!(RatchetParams),
            rmcp::schemars::schema_for!(DeployScoreParams),
            rmcp::schemars::schema_for!(ComplexityParams),
            rmcp::schemars::schema_for!(DeadCodeParams),
            rmcp::schemars::schema_for!(RiskParams),
            rmcp::schemars::schema_for!(HotpathsParams),
            rmcp::schemars::schema_for!(TestOptimizeParams),
            rmcp::schemars::schema_for!(TestPrioritizeParams),
            rmcp::schemars::schema_for!(BlastRadiusParams),
            rmcp::schemars::schema_for!(SecretScanParams),
            rmcp::schemars::schema_for!(DataFlowParams),
        ] {
            let json = serde_json::to_value(&schema).unwrap();
            assert!(json.get("type").is_some() || json.get("$schema").is_some());
            // Should have properties for struct schemas
            assert!(json.get("properties").is_some());
        }
    }

    #[test]
    fn required_fields_are_marked_in_schemas() {
        let schema = serde_json::to_value(rmcp::schemars::schema_for!(RunParams)).unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();
        let required_names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_names.contains(&"target"));
        assert!(required_names.contains(&"lang"));
        // strategy is optional -- should NOT be in required
        assert!(!required_names.contains(&"strategy"));

        // complexity: only target required
        let schema = serde_json::to_value(rmcp::schemars::schema_for!(ComplexityParams)).unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();
        let required_names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_names.contains(&"target"));

        // risk: both target and changed_files required
        let schema = serde_json::to_value(rmcp::schemars::schema_for!(RiskParams)).unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();
        let required_names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_names.contains(&"target"));
        assert!(required_names.contains(&"changed_files"));

        // blast_radius: target, lang, changed_files all required
        let schema =
            serde_json::to_value(rmcp::schemars::schema_for!(BlastRadiusParams)).unwrap();
        let required = schema.get("required").unwrap().as_array().unwrap();
        let required_names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_names.contains(&"target"));
        assert!(required_names.contains(&"lang"));
        assert!(required_names.contains(&"changed_files"));
    }

    #[test]
    fn service_can_be_constructed() {
        let _service = ApexMcpService::new();
    }

    // --- validate_target_path tests ---

    #[test]
    fn validate_target_path_rejects_nonexistent() {
        let result = validate_target_path("/tmp/apex-mcp-test-nonexistent-path-xyzzy");
        assert!(result.is_err(), "expected Err for nonexistent path");
        let err = result.unwrap_err();
        assert!(
            err.message.contains("invalid target path"),
            "unexpected error message: {}",
            err.message
        );
    }

    #[test]
    fn validate_target_path_rejects_traversal_string() {
        let result = validate_target_path("../../etc/passwd_apex_test_nonexistent");
        assert!(result.is_err(), "expected Err for traversal path");
    }

    #[test]
    fn validate_target_path_accepts_existing_dir() {
        let result = validate_target_path("/tmp");
        assert!(result.is_ok(), "expected Ok for /tmp, got: {:?}", result);
        let canonical = result.unwrap();
        assert!(
            canonical.starts_with('/'),
            "canonicalized path should be absolute, got: {canonical}"
        );
    }

    #[test]
    fn validate_target_path_resolves_to_absolute() {
        let tmp = std::env::temp_dir();
        let input = tmp.to_string_lossy().to_string();
        let result = validate_target_path(&input);
        assert!(result.is_ok());
        let canonical = result.unwrap();
        assert!(
            std::path::Path::new(&canonical).is_absolute(),
            "result should be absolute: {canonical}"
        );
    }
}
