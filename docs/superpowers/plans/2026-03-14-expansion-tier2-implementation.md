<!-- status: DONE -->

# Expansion Tier 2 Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement 7 Tier 2 expansion capabilities (C29, C49, C48, C26, C33, C28, C51) — wiring existing infrastructure where available, building new detectors/analyzers where not.

**Architecture:** Group A (C29, C49, C48) wraps existing taint/risk/compliance infrastructure with CLI commands. Group B (C26, C33, C28, C51) creates new standalone analyzers in apex-detect with CLI integration.

**Tech Stack:** Rust, clap, async-trait, tokio, serde_json, regex

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `crates/apex-cli/src/lib.rs` | Add 7 CLI subcommands |
| Create | `crates/apex-detect/src/api_coverage.rs` | C26: OpenAPI spec vs code endpoint matching |
| Create | `crates/apex-detect/src/service_map.rs` | C33: Static analysis of service dependencies |
| Create | `crates/apex-detect/src/schema_check.rs` | C28: SQL migration risk analysis |
| Create | `crates/apex-detect/src/test_data.rs` | C51: Schema-driven test data generation |
| Modify | `crates/apex-detect/src/lib.rs` | Export new modules |
| Modify | `crates/apex-detect/src/finding.rs` | Add new FindingCategory variants |

---

## Chunk 1: Group A — Wire Existing Infrastructure

### Task 1: C29 — Data Flow CLI (`apex data-flow`)

Wire the existing taint analysis in apex-cpg into a CLI command.

**Files:**
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Add DataFlow to Commands enum and args**

```rust
    /// Trace data flow from input sources to output sinks, classify PII/sensitive data.
    DataFlow(DataFlowArgs),
```

```rust
#[derive(Parser)]
pub struct DataFlowArgs {
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    /// Classify flows containing PII (email, phone, SSN, etc.).
    #[arg(long)]
    pub classify_pii: bool,
    /// Maximum taint analysis depth.
    #[arg(long, default_value = "10")]
    pub max_depth: usize,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}
```

- [ ] **Step 2: Add match arm**

```rust
        Commands::DataFlow(args) => run_data_flow(args).await,
```

- [ ] **Step 3: Write handler**

```rust
async fn run_data_flow(args: DataFlowArgs) -> Result<()> {
    use apex_cpg::{builder, reaching_def, taint};

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    if lang != Language::Python {
        eprintln!("Warning: data-flow analysis currently supports Python only. Other languages will have limited results.");
    }

    // Build combined CPG
    let mut cpg = apex_cpg::Cpg::new();
    for (path, source) in &source_cache {
        let file_cpg = builder::build_python_cpg(source, &path.display().to_string());
        cpg.merge(file_cpg);
    }

    if cpg.node_count() == 0 {
        println!("No code found to analyze.");
        return Ok(());
    }

    // Add dataflow edges and find taint flows
    reaching_def::add_reaching_def_edges(&mut cpg);
    let flows = taint::find_taint_flows(&cpg, args.max_depth);

    match args.output_format {
        OutputFormat::Json => {
            // Serialize flows as JSON array
            let flow_data: Vec<serde_json::Value> = flows.iter().map(|f| {
                serde_json::json!({
                    "source": f.source,
                    "sink": f.sink,
                    "path_length": f.path.len(),
                    "variables": f.variable_chain,
                })
            }).collect();
            println!("{}", serde_json::to_string_pretty(&flow_data)?);
        }
        OutputFormat::Text => {
            if flows.is_empty() {
                println!("No taint flows detected.");
            } else {
                println!("\n{} taint flow(s) detected in {}\n", flows.len(), target_path.display());
                for (i, flow) in flows.iter().enumerate() {
                    let src_name = cpg.node(flow.source)
                        .map(|n| format!("{:?}", n))
                        .unwrap_or_else(|| format!("node:{}", flow.source));
                    let sink_name = cpg.node(flow.sink)
                        .map(|n| format!("{:?}", n))
                        .unwrap_or_else(|| format!("node:{}", flow.sink));
                    println!("  Flow {}: {} → {}", i + 1, src_name, sink_name);
                    if !flow.variable_chain.is_empty() {
                        println!("    Variables: {}", flow.variable_chain.join(" → "));
                    }
                    println!("    Path length: {} nodes", flow.path.len());
                    println!();
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p apex-cli 2>&1 | tail -5`

- [ ] **Step 5: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "feat(cli): add apex data-flow subcommand (C29)"
```

---

### Task 2: C49 — Blast Radius CLI (`apex blast-radius`)

Wire the existing risk assessment in apex-index into a CLI command.

**Files:**
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Add BlastRadius to Commands enum and args**

```rust
    /// Calculate change blast radius from branch index data.
    BlastRadius(BlastRadiusArgs),
```

```rust
#[derive(Parser)]
pub struct BlastRadiusArgs {
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    /// Comma-separated list of changed files (relative paths).
    #[arg(long, value_delimiter = ',')]
    pub changed_files: Vec<String>,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}
```

- [ ] **Step 2: Write handler**

```rust
async fn run_blast_radius(args: BlastRadiusArgs) -> Result<()> {
    use apex_index::analysis::assess_risk;

    let target_path = args.target.canonicalize()?;
    let index_path = target_path.join(".apex").join("index.json");

    if !index_path.exists() {
        eprintln!("No branch index found at {}. Run `apex index` first.", index_path.display());
        std::process::exit(1);
    }

    let index_data = std::fs::read_to_string(&index_path)?;
    let index: apex_index::BranchIndex = serde_json::from_str(&index_data)?;

    let assessment = assess_risk(&index, &args.changed_files);

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&assessment)?);
        }
        OutputFormat::Text => {
            println!("\nBlast Radius Assessment for {}\n", target_path.display());
            println!("Risk Level:        {}", assessment.level);
            println!("Risk Score:        {}/100", assessment.score);
            println!("Affected Tests:    {}", assessment.affected_tests);
            println!("Changed Branches:  {}", assessment.changed_branches);
            println!("Covered:           {}", assessment.covered_changed);
            println!("Uncovered:         {}", assessment.uncovered_changed);
            println!("Coverage:          {:.1}%", assessment.coverage_of_changed);
            if !assessment.reasons.is_empty() {
                println!("\nReasons:");
                for reason in &assessment.reasons {
                    println!("  • {}", reason);
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 3: Verify and commit**

Run: `cargo check -p apex-cli 2>&1 | tail -5`

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "feat(cli): add apex blast-radius subcommand (C49)"
```

---

### Task 3: C48 — Compliance Export CLI (`apex compliance-export`)

Wire existing ASVS/SSDF/STRIDE reporting into a CLI command.

**Files:**
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Add ComplianceExport to Commands enum and args**

```rust
    /// Export compliance evidence packages (ASVS, SSDF, STRIDE).
    ComplianceExport(ComplianceExportArgs),
```

```rust
#[derive(Parser)]
pub struct ComplianceExportArgs {
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    /// Compliance framework: asvs, ssdf, stride, or all.
    #[arg(long, default_value = "all")]
    pub framework: String,
    /// ASVS compliance level (L1, L2, L3). Only used with --framework asvs.
    #[arg(long, default_value = "L1")]
    pub level: String,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
    /// Write output to a file.
    #[arg(long, short)]
    pub output: Option<PathBuf>,
}
```

- [ ] **Step 2: Write handler**

The handler should:
1. Run the detector pipeline to get findings (reuse audit logic)
2. Extract detector IDs from findings
3. Generate ASVS report via `generate_asvs_report()`
4. Generate SSDF report via `generate_ssdf_report()`
5. Generate STRIDE matrix via `analyze_stride()` on combined source
6. Output combined compliance report

```rust
async fn run_compliance_export(args: ComplianceExportArgs, cfg: &ApexConfig) -> Result<()> {
    use apex_detect::compliance::{asvs, ssdf};
    use apex_detect::{AnalysisContext, DetectConfig, DetectorPipeline};
    use std::sync::Arc;

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    // Run detectors to get findings
    let detect_cfg = DetectConfig::default();
    let pipeline = DetectorPipeline::from_config(&detect_cfg, lang);
    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: std::collections::HashMap::new(),
        known_bugs: vec![],
        source_cache: source_cache.clone(),
        fuzz_corpus: None,
        config: detect_cfg,
        runner: Arc::new(apex_core::command::RealCommandRunner),
        cpg: None,
        threat_model: Default::default(),
        reverse_path_engine: None,
    };

    let report = pipeline.run_all(&ctx).await;
    let detector_ids: Vec<String> = report.findings.iter()
        .map(|f| f.detector.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mut output_buf = String::new();
    use std::fmt::Write;

    let frameworks: Vec<&str> = if args.framework == "all" {
        vec!["asvs", "ssdf", "stride"]
    } else {
        vec![args.framework.as_str()]
    };

    for fw in &frameworks {
        match *fw {
            "asvs" => {
                let level = match args.level.to_uppercase().as_str() {
                    "L2" => asvs::AsvsLevel::L2,
                    "L3" => asvs::AsvsLevel::L3,
                    _ => asvs::AsvsLevel::L1,
                };
                let asvs_report = asvs::generate_asvs_report(&detector_ids, level);
                writeln!(output_buf, "=== ASVS Compliance Report ===\n").ok();
                writeln!(output_buf, "Total requirements: {}", asvs_report.coverage.total).ok();
                writeln!(output_buf, "Automated:          {}", asvs_report.coverage.automated).ok();
                writeln!(output_buf, "Verified:           {}", asvs_report.coverage.verified).ok();
                writeln!(output_buf, "Failed:             {}", asvs_report.coverage.failed).ok();
                writeln!(output_buf, "Manual required:    {}", asvs_report.coverage.manual_required).ok();
                writeln!(output_buf).ok();
            }
            "ssdf" => {
                let ssdf_report = ssdf::generate_ssdf_report();
                writeln!(output_buf, "=== SSDF Compliance Report ===\n").ok();
                writeln!(output_buf, "Total tasks:   {}", ssdf_report.total_count).ok();
                writeln!(output_buf, "Satisfied:     {}", ssdf_report.satisfied_count).ok();
                writeln!(output_buf).ok();
                for task in &ssdf_report.tasks {
                    let status = if task.apex_satisfies { "✓" } else { "✗" };
                    writeln!(output_buf, "  {} {} — {}", status, task.id, task.description).ok();
                }
                writeln!(output_buf).ok();
            }
            "stride" => {
                let combined_source: String = source_cache.values().cloned().collect::<Vec<_>>().join("\n");
                let stride_matrix = apex_detect::threat::stride::analyze_stride(&combined_source);
                writeln!(output_buf, "=== STRIDE Threat Analysis ===\n").ok();
                for entry in &stride_matrix.entries {
                    writeln!(output_buf, "  {:?} (Risk: {:?})", entry.category, entry.risk_level).ok();
                    for m in &entry.mitigations_found {
                        writeln!(output_buf, "    ✓ {}", m).ok();
                    }
                    for m in &entry.mitigations_missing {
                        writeln!(output_buf, "    ✗ {}", m).ok();
                    }
                }
                writeln!(output_buf).ok();
            }
            other => {
                writeln!(output_buf, "Unknown framework: {}", other).ok();
            }
        }
    }

    if let Some(out_path) = &args.output {
        std::fs::write(out_path, &output_buf)?;
        eprintln!("Wrote compliance report to {}", out_path.display());
    } else {
        print!("{output_buf}");
    }

    Ok(())
}
```

- [ ] **Step 3: Verify and commit**

Run: `cargo check -p apex-cli 2>&1 | tail -5`

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "feat(cli): add apex compliance-export subcommand (C48)"
```

---

## Chunk 2: Group B — New Analyzers

### Task 4: Add new FindingCategory variants

**Files:**
- Modify: `crates/apex-detect/src/finding.rs`

- [ ] **Step 1: Add variants for Group B capabilities**

Add after `ApiBreakingChange`:

```rust
    ApiSpecCoverage,
    ServiceDependency,
    SchemaMigrationRisk,
    TestDataQuality,
```

- [ ] **Step 2: Update SARIF CWE mapping**

In `crates/apex-detect/src/sarif.rs`, add CWE mappings for new categories in the match block:
- `ApiSpecCoverage` → CWE-1059 (Insufficient Technical Documentation)
- `ServiceDependency` → CWE-1127 (Compilation with Insufficient Warnings)
- `SchemaMigrationRisk` → CWE-1066 (Missing Serialization Control)
- `TestDataQuality` → CWE-1007 (Insufficient Visual Distinction)

These are approximate CWE mappings — use the closest available.

- [ ] **Step 3: Commit**

```bash
git add crates/apex-detect/src/finding.rs crates/apex-detect/src/sarif.rs
git commit -m "feat(detect): add FindingCategory variants for Tier 2 Group B"
```

---

### Task 5: C26 — API Spec Coverage (`apex api-coverage`)

**Files:**
- Create: `crates/apex-detect/src/api_coverage.rs`
- Modify: `crates/apex-detect/src/lib.rs` (add `pub mod api_coverage`)
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Write api_coverage.rs**

Core logic: parse OpenAPI spec JSON, extract endpoints, scan source code for route handler patterns, cross-reference.

```rust
//! API Spec Coverage — compares OpenAPI spec against code.

use apex_core::error::{ApexError, Result};
use serde::Serialize;

/// Status of an API endpoint.
#[derive(Debug, Clone, Serialize)]
pub enum EndpointStatus {
    /// In spec and found in code.
    Implemented,
    /// In spec but not found in code.
    SpecOnly,
    /// Found in code but not in spec.
    CodeOnly,
}

/// A single endpoint entry in the coverage report.
#[derive(Debug, Clone, Serialize)]
pub struct EndpointCoverage {
    pub method: String,
    pub path: String,
    pub status: EndpointStatus,
}

/// Full coverage report.
#[derive(Debug, Clone, Serialize)]
pub struct ApiCoverageReport {
    pub endpoints: Vec<EndpointCoverage>,
    pub spec_count: usize,
    pub implemented_count: usize,
    pub spec_only_count: usize,
    pub code_only_count: usize,
}

/// Route patterns found in common frameworks.
const PYTHON_ROUTE_PATTERNS: &[&str] = &[
    r#"@app\.(get|post|put|delete|patch)\s*\(\s*['"](.*?)['"]\s*\)"#,
    r#"@router\.(get|post|put|delete|patch)\s*\(\s*['"](.*?)['"]\s*\)"#,
    r#"path\(\s*['"](.*?)['"]\s*,"#,
];

const JS_ROUTE_PATTERNS: &[&str] = &[
    r#"(?:app|router)\.(get|post|put|delete|patch)\s*\(\s*['"](.*?)['"]\s*,"#,
];

/// Compare an OpenAPI spec against source code to find coverage gaps.
pub fn analyze_coverage(
    spec_json: &str,
    source_cache: &std::collections::HashMap<std::path::PathBuf, String>,
    lang: apex_core::types::Language,
) -> Result<ApiCoverageReport> {
    let spec: serde_json::Value = serde_json::from_str(spec_json)
        .map_err(|e| ApexError::Detect(format!("invalid OpenAPI spec: {e}")))?;

    // Extract endpoints from spec
    let mut spec_endpoints: Vec<(String, String)> = Vec::new(); // (method, path)
    if let Some(paths) = spec.get("paths").and_then(|p| p.as_object()) {
        let methods = ["get", "post", "put", "delete", "patch", "head", "options"];
        for (path, item) in paths {
            for method in &methods {
                if item.get(method).is_some() {
                    spec_endpoints.push((method.to_uppercase(), path.clone()));
                }
            }
        }
    }

    // Extract route handlers from source code
    let patterns: &[&str] = match lang {
        apex_core::types::Language::Python => PYTHON_ROUTE_PATTERNS,
        apex_core::types::Language::JavaScript => JS_ROUTE_PATTERNS,
        _ => &[],
    };

    let compiled: Vec<regex::Regex> = patterns.iter()
        .filter_map(|p| regex::Regex::new(p).ok())
        .collect();

    let mut code_endpoints: Vec<(String, String)> = Vec::new();
    for source in source_cache.values() {
        for re in &compiled {
            for cap in re.captures_iter(source) {
                let method = cap.get(1).map(|m| m.as_str().to_uppercase()).unwrap_or_default();
                let path = cap.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
                if !path.is_empty() {
                    code_endpoints.push((method, path));
                }
            }
        }
    }

    // Cross-reference
    let mut endpoints = Vec::new();
    let mut implemented = 0usize;
    let mut spec_only = 0usize;

    for (method, path) in &spec_endpoints {
        let found = code_endpoints.iter().any(|(cm, cp)| {
            cm == method && paths_match(cp, path)
        });
        if found {
            endpoints.push(EndpointCoverage {
                method: method.clone(),
                path: path.clone(),
                status: EndpointStatus::Implemented,
            });
            implemented += 1;
        } else {
            endpoints.push(EndpointCoverage {
                method: method.clone(),
                path: path.clone(),
                status: EndpointStatus::SpecOnly,
            });
            spec_only += 1;
        }
    }

    let mut code_only = 0usize;
    for (method, path) in &code_endpoints {
        let in_spec = spec_endpoints.iter().any(|(sm, sp)| {
            sm == method && paths_match(path, sp)
        });
        if !in_spec {
            endpoints.push(EndpointCoverage {
                method: method.clone(),
                path: path.clone(),
                status: EndpointStatus::CodeOnly,
            });
            code_only += 1;
        }
    }

    Ok(ApiCoverageReport {
        spec_count: spec_endpoints.len(),
        implemented_count: implemented,
        spec_only_count: spec_only,
        code_only_count: code_only,
        endpoints,
    })
}

/// Normalize and compare paths (handle OpenAPI path params like {id}).
fn paths_match(code_path: &str, spec_path: &str) -> bool {
    let normalize = |p: &str| -> String {
        let re = regex::Regex::new(r"\{[^}]+\}").unwrap();
        re.replace_all(p, ":param").to_string()
    };
    let code_re = regex::Regex::new(r"<[^>]+>|:\w+").unwrap();
    let code_norm = code_re.replace_all(code_path, ":param").to_string();
    let spec_norm = normalize(spec_path);
    code_norm == spec_norm
}
```

- [ ] **Step 2: Export module and add CLI command**

In `crates/apex-detect/src/lib.rs`, add `pub mod api_coverage;`

Add CLI subcommand in `crates/apex-cli/src/lib.rs`:

```rust
    /// Compare OpenAPI spec against code to find undocumented or unimplemented endpoints.
    ApiCoverage(ApiCoverageArgs),
```

```rust
#[derive(Parser)]
pub struct ApiCoverageArgs {
    /// Path to OpenAPI spec file (JSON).
    #[arg(long)]
    pub spec: PathBuf,
    #[arg(long, short)]
    pub target: PathBuf,
    #[arg(long, short, value_enum)]
    pub lang: LangArg,
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}
```

Handler calls `apex_detect::api_coverage::analyze_coverage()` and prints results.

- [ ] **Step 3: Write tests in api_coverage.rs**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn sample_spec() -> &'static str {
        r#"{"paths":{"/users":{"get":{},"post":{}},"/users/{id}":{"get":{},"delete":{}}}}"#
    }

    #[test]
    fn detects_implemented_endpoints() {
        let mut source = HashMap::new();
        source.insert(PathBuf::from("app.py"),
            r#"@app.get("/users")\ndef list_users(): pass\n@app.post("/users")\ndef create_user(): pass"#.into());
        let report = analyze_coverage(sample_spec(), &source, apex_core::types::Language::Python).unwrap();
        assert_eq!(report.implemented_count, 2);
        assert_eq!(report.spec_only_count, 2); // /users/{id} GET and DELETE not in code
    }

    #[test]
    fn detects_spec_only_endpoints() {
        let source = HashMap::new();
        let report = analyze_coverage(sample_spec(), &source, apex_core::types::Language::Python).unwrap();
        assert_eq!(report.spec_only_count, 4);
        assert_eq!(report.implemented_count, 0);
    }

    #[test]
    fn paths_match_with_params() {
        assert!(paths_match("/users/:id", "/users/{id}"));
        assert!(paths_match("/users/<int:id>", "/users/{id}"));
        assert!(!paths_match("/users", "/posts"));
    }
}
```

- [ ] **Step 4: Verify and commit**

Run: `cargo test -p apex-detect api_coverage`

```bash
git add crates/apex-detect/src/api_coverage.rs crates/apex-detect/src/lib.rs crates/apex-cli/src/lib.rs
git commit -m "feat: add apex api-coverage analyzer and CLI (C26)"
```

---

### Task 6: C33 — Service Dependency Mapping (`apex service-map`)

**Files:**
- Create: `crates/apex-detect/src/service_map.rs`
- Modify: `crates/apex-detect/src/lib.rs`
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Write service_map.rs**

Static analysis to find HTTP client calls, gRPC stubs, message queue producers/consumers, DB connections.

```rust
//! Service Dependency Mapping — discovers runtime service dependencies from code.

use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub enum DependencyKind {
    Http,
    Grpc,
    MessageQueue,
    Database,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceDependency {
    pub kind: DependencyKind,
    pub target: String,         // URL, topic name, DB name, service name
    pub file: PathBuf,
    pub line: u32,
    pub evidence: String,       // the matching code snippet
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceMap {
    pub dependencies: Vec<ServiceDependency>,
    pub http_count: usize,
    pub grpc_count: usize,
    pub mq_count: usize,
    pub db_count: usize,
}

// HTTP client patterns
static HTTP_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| vec![
    Regex::new(r#"requests\.(get|post|put|delete|patch)\s*\(\s*['"f](.*?)['")\s]"#).unwrap(),
    Regex::new(r#"httpx\.(get|post|put|delete|patch)\s*\("#).unwrap(),
    Regex::new(r#"fetch\s*\(\s*[`'"](.*?)[`'"]\s*[,)]"#).unwrap(),
    Regex::new(r#"axios\.(get|post|put|delete|patch)\s*\("#).unwrap(),
    Regex::new(r#"HttpClient|reqwest::Client|hyper::Client"#).unwrap(),
]);

// Message queue patterns
static MQ_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| vec![
    Regex::new(r#"(?:producer|consumer)\.(send|subscribe|publish)\s*\(\s*['"](.*?)['"]\s*"#).unwrap(),
    Regex::new(r#"KafkaProducer|KafkaConsumer|NatsClient"#).unwrap(),
    Regex::new(r#"channel\.(basic_publish|basic_consume)\s*\("#).unwrap(),
]);

// Database patterns
static DB_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| vec![
    Regex::new(r#"(connect|create_engine|MongoClient|redis\.Redis)\s*\(\s*['"](.*?)['"]\s*"#).unwrap(),
    Regex::new(r#"DATABASE_URL|MONGO_URI|REDIS_URL"#).unwrap(),
]);

// gRPC patterns
static GRPC_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| vec![
    Regex::new(r#"grpc\.(insecure_channel|secure_channel)\s*\(\s*['"](.*?)['"]\s*"#).unwrap(),
    Regex::new(r#"Stub\s*\(|_grpc\.py|\.proto"#).unwrap(),
]);

pub fn analyze_service_map(
    source_cache: &HashMap<PathBuf, String>,
) -> ServiceMap {
    let mut deps = Vec::new();

    for (path, source) in source_cache {
        for (line_num, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            let line_1based = (line_num + 1) as u32;

            // Check HTTP
            for re in HTTP_PATTERNS.iter() {
                if let Some(cap) = re.captures(trimmed) {
                    let target = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
                    deps.push(ServiceDependency {
                        kind: DependencyKind::Http,
                        target,
                        file: path.clone(),
                        line: line_1based,
                        evidence: trimmed.to_string(),
                    });
                    break;
                }
            }
            // Check MQ
            for re in MQ_PATTERNS.iter() {
                if let Some(cap) = re.captures(trimmed) {
                    let target = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
                    deps.push(ServiceDependency {
                        kind: DependencyKind::MessageQueue,
                        target,
                        file: path.clone(),
                        line: line_1based,
                        evidence: trimmed.to_string(),
                    });
                    break;
                }
            }
            // Check DB
            for re in DB_PATTERNS.iter() {
                if let Some(cap) = re.captures(trimmed) {
                    let target = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
                    deps.push(ServiceDependency {
                        kind: DependencyKind::Database,
                        target,
                        file: path.clone(),
                        line: line_1based,
                        evidence: trimmed.to_string(),
                    });
                    break;
                }
            }
            // Check gRPC
            for re in GRPC_PATTERNS.iter() {
                if let Some(cap) = re.captures(trimmed) {
                    let target = cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string();
                    deps.push(ServiceDependency {
                        kind: DependencyKind::Grpc,
                        target,
                        file: path.clone(),
                        line: line_1based,
                        evidence: trimmed.to_string(),
                    });
                    break;
                }
            }
        }
    }

    let http_count = deps.iter().filter(|d| matches!(d.kind, DependencyKind::Http)).count();
    let grpc_count = deps.iter().filter(|d| matches!(d.kind, DependencyKind::Grpc)).count();
    let mq_count = deps.iter().filter(|d| matches!(d.kind, DependencyKind::MessageQueue)).count();
    let db_count = deps.iter().filter(|d| matches!(d.kind, DependencyKind::Database)).count();

    ServiceMap { dependencies: deps, http_count, grpc_count, mq_count, db_count }
}
```

- [ ] **Step 2: Add tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_python_requests() {
        let mut cache = HashMap::new();
        cache.insert(PathBuf::from("client.py"),
            r#"resp = requests.get("http://api.example.com/users")"#.into());
        let map = analyze_service_map(&cache);
        assert_eq!(map.http_count, 1);
        assert_eq!(map.dependencies[0].target, "http://api.example.com/users");
    }

    #[test]
    fn detects_database_connection() {
        let mut cache = HashMap::new();
        cache.insert(PathBuf::from("db.py"),
            r#"engine = create_engine("postgresql://localhost/mydb")"#.into());
        let map = analyze_service_map(&cache);
        assert_eq!(map.db_count, 1);
    }

    #[test]
    fn empty_source_returns_empty_map() {
        let cache = HashMap::new();
        let map = analyze_service_map(&cache);
        assert_eq!(map.dependencies.len(), 0);
    }
}
```

- [ ] **Step 3: Export module, add CLI, verify, commit**

```bash
git add crates/apex-detect/src/service_map.rs crates/apex-detect/src/lib.rs crates/apex-cli/src/lib.rs
git commit -m "feat: add apex service-map analyzer and CLI (C33)"
```

---

### Task 7: C28 — Schema Migration Safety (`apex schema-check`)

**Files:**
- Create: `crates/apex-detect/src/schema_check.rs`
- Modify: `crates/apex-detect/src/lib.rs`
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Write schema_check.rs**

Parse SQL migration files, detect risky operations (ALTER TABLE on large tables, DROP COLUMN, NOT NULL without default, type changes).

```rust
//! Schema Migration Safety — analyzes SQL migrations for risky operations.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum MigrationRisk {
    Safe,
    Caution,
    Dangerous,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationIssue {
    pub risk: MigrationRisk,
    pub line: u32,
    pub statement: String,
    pub description: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationReport {
    pub issues: Vec<MigrationIssue>,
    pub safe_count: usize,
    pub caution_count: usize,
    pub dangerous_count: usize,
}

static DROP_COLUMN: LazyLock<Regex> = LazyLock::new(||
    Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+DROP\s+COLUMN").unwrap());
static ADD_NOT_NULL: LazyLock<Regex> = LazyLock::new(||
    Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+ADD\s+(?:COLUMN\s+)?\S+\s+\S+\s+NOT\s+NULL(?!\s+DEFAULT)").unwrap());
static DROP_TABLE: LazyLock<Regex> = LazyLock::new(||
    Regex::new(r"(?i)DROP\s+TABLE").unwrap());
static RENAME_COLUMN: LazyLock<Regex> = LazyLock::new(||
    Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+RENAME\s+COLUMN").unwrap());
static CHANGE_TYPE: LazyLock<Regex> = LazyLock::new(||
    Regex::new(r"(?i)ALTER\s+TABLE\s+\S+\s+ALTER\s+COLUMN\s+\S+\s+(SET\s+DATA\s+)?TYPE").unwrap());
static CREATE_INDEX_NO_CONCURRENTLY: LazyLock<Regex> = LazyLock::new(||
    Regex::new(r"(?i)CREATE\s+(?:UNIQUE\s+)?INDEX\s+(?!CONCURRENTLY)").unwrap());
static TRUNCATE: LazyLock<Regex> = LazyLock::new(||
    Regex::new(r"(?i)TRUNCATE\s+").unwrap());

pub fn analyze_migration(sql: &str) -> MigrationReport {
    let mut issues = Vec::new();

    for (line_num, line) in sql.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("--") {
            continue;
        }
        let line_1based = (line_num + 1) as u32;

        if DROP_TABLE.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Dangerous,
                line: line_1based,
                statement: trimmed.to_string(),
                description: "DROP TABLE causes permanent data loss".into(),
                suggestion: "Rename table instead, drop after verification period".into(),
            });
        }
        if DROP_COLUMN.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Dangerous,
                line: line_1based,
                statement: trimmed.to_string(),
                description: "DROP COLUMN causes data loss and may break running queries".into(),
                suggestion: "Mark column as deprecated, stop reading it first, then drop in a later migration".into(),
            });
        }
        if ADD_NOT_NULL.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Dangerous,
                line: line_1based,
                statement: trimmed.to_string(),
                description: "Adding NOT NULL column without DEFAULT will fail on non-empty tables".into(),
                suggestion: "Add column as nullable, backfill, then set NOT NULL".into(),
            });
        }
        if CHANGE_TYPE.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Caution,
                line: line_1based,
                statement: trimmed.to_string(),
                description: "Column type change may require data cast and locks the table".into(),
                suggestion: "Add new column, migrate data, rename, drop old column".into(),
            });
        }
        if RENAME_COLUMN.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Caution,
                line: line_1based,
                statement: trimmed.to_string(),
                description: "Renaming column breaks queries using the old name".into(),
                suggestion: "Add new column, migrate data, update queries, then drop old column".into(),
            });
        }
        if CREATE_INDEX_NO_CONCURRENTLY.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Caution,
                line: line_1based,
                statement: trimmed.to_string(),
                description: "CREATE INDEX without CONCURRENTLY locks the table for writes".into(),
                suggestion: "Use CREATE INDEX CONCURRENTLY to avoid blocking writes".into(),
            });
        }
        if TRUNCATE.is_match(trimmed) {
            issues.push(MigrationIssue {
                risk: MigrationRisk::Dangerous,
                line: line_1based,
                statement: trimmed.to_string(),
                description: "TRUNCATE removes all data permanently".into(),
                suggestion: "Verify this is intentional and add a comment explaining why".into(),
            });
        }
    }

    let dangerous_count = issues.iter().filter(|i| i.risk == MigrationRisk::Dangerous).count();
    let caution_count = issues.iter().filter(|i| i.risk == MigrationRisk::Caution).count();
    let safe_count = issues.iter().filter(|i| i.risk == MigrationRisk::Safe).count();

    MigrationReport { issues, safe_count, caution_count, dangerous_count }
}
```

- [ ] **Step 2: Add tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_drop_column() {
        let sql = "ALTER TABLE users DROP COLUMN email;";
        let report = analyze_migration(sql);
        assert_eq!(report.dangerous_count, 1);
        assert!(report.issues[0].description.contains("data loss"));
    }

    #[test]
    fn detects_not_null_without_default() {
        let sql = "ALTER TABLE users ADD COLUMN age integer NOT NULL;";
        let report = analyze_migration(sql);
        assert_eq!(report.dangerous_count, 1);
    }

    #[test]
    fn allows_not_null_with_default() {
        let sql = "ALTER TABLE users ADD COLUMN age integer NOT NULL DEFAULT 0;";
        let report = analyze_migration(sql);
        assert_eq!(report.dangerous_count, 0);
    }

    #[test]
    fn detects_non_concurrent_index() {
        let sql = "CREATE INDEX idx_users_email ON users(email);";
        let report = analyze_migration(sql);
        assert_eq!(report.caution_count, 1);
    }

    #[test]
    fn allows_concurrent_index() {
        let sql = "CREATE INDEX CONCURRENTLY idx_users_email ON users(email);";
        let report = analyze_migration(sql);
        assert_eq!(report.caution_count, 0);
    }

    #[test]
    fn safe_migration_no_issues() {
        let sql = "ALTER TABLE users ADD COLUMN nickname text;";
        let report = analyze_migration(sql);
        assert_eq!(report.issues.len(), 0);
    }

    #[test]
    fn skips_comments() {
        let sql = "-- ALTER TABLE users DROP COLUMN email;";
        let report = analyze_migration(sql);
        assert_eq!(report.issues.len(), 0);
    }
}
```

- [ ] **Step 3: Export, CLI, verify, commit**

CLI handler reads SQL file(s) from `--migration` path, calls `analyze_migration()`, prints report.

```bash
git add crates/apex-detect/src/schema_check.rs crates/apex-detect/src/lib.rs crates/apex-cli/src/lib.rs
git commit -m "feat: add apex schema-check migration safety analyzer and CLI (C28)"
```

---

### Task 8: C51 — Test Data Generation (`apex test-data`)

**Files:**
- Create: `crates/apex-detect/src/test_data.rs`
- Modify: `crates/apex-detect/src/lib.rs`
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Write test_data.rs**

Parse SQL CREATE TABLE statements, generate INSERT statements with realistic data.

```rust
//! Test Data Generation — generates realistic test data from SQL schemas.

use regex::Regex;
use serde::Serialize;
use std::sync::LazyLock;

#[derive(Debug, Clone, Serialize)]
pub struct Column {
    pub name: String,
    pub col_type: String,
    pub nullable: bool,
    pub has_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Table {
    pub name: String,
    pub columns: Vec<Column>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedData {
    pub tables: Vec<Table>,
    pub sql: String,
}

static CREATE_TABLE: LazyLock<Regex> = LazyLock::new(||
    Regex::new(r"(?is)CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?(\S+)\s*\((.*?)\);").unwrap());

static COLUMN_DEF: LazyLock<Regex> = LazyLock::new(||
    Regex::new(r"(?i)^\s*(\w+)\s+(\w+(?:\([^)]*\))?)\s*(.*)$").unwrap());

pub fn parse_schema(sql: &str) -> Vec<Table> {
    let mut tables = Vec::new();

    for cap in CREATE_TABLE.captures_iter(sql) {
        let name = cap[1].trim_matches('"').trim_matches('`').to_string();
        let body = &cap[2];

        let mut columns = Vec::new();
        for col_line in body.split(',') {
            let trimmed = col_line.trim();
            // Skip constraints
            if trimmed.to_uppercase().starts_with("PRIMARY")
                || trimmed.to_uppercase().starts_with("FOREIGN")
                || trimmed.to_uppercase().starts_with("UNIQUE")
                || trimmed.to_uppercase().starts_with("CHECK")
                || trimmed.to_uppercase().starts_with("CONSTRAINT")
            {
                continue;
            }
            if let Some(col_cap) = COLUMN_DEF.captures(trimmed) {
                let col_name = col_cap[1].to_string();
                let col_type = col_cap[2].to_uppercase();
                let rest = col_cap[3].to_uppercase();
                let nullable = !rest.contains("NOT NULL");
                let has_default = rest.contains("DEFAULT");

                columns.push(Column { name: col_name, col_type, nullable, has_default });
            }
        }

        tables.push(Table { name, columns });
    }

    tables
}

pub fn generate_inserts(tables: &[Table], rows_per_table: usize) -> String {
    let mut sql = String::new();

    for table in tables {
        let cols: Vec<&str> = table.columns.iter()
            .filter(|c| !c.col_type.contains("SERIAL") && !c.has_default)
            .map(|c| c.name.as_str())
            .collect();

        if cols.is_empty() {
            continue;
        }

        for i in 0..rows_per_table {
            let values: Vec<String> = table.columns.iter()
                .filter(|c| !c.col_type.contains("SERIAL") && !c.has_default)
                .map(|c| generate_value(&c.col_type, &c.name, i, c.nullable))
                .collect();

            sql.push_str(&format!(
                "INSERT INTO {} ({}) VALUES ({});\n",
                table.name,
                cols.join(", "),
                values.join(", "),
            ));
        }
        sql.push('\n');
    }

    sql
}

fn generate_value(col_type: &str, col_name: &str, row: usize, nullable: bool) -> String {
    // Generate contextual data based on column name and type
    let name_lower = col_name.to_lowercase();

    if nullable && row % 7 == 0 {
        return "NULL".into();
    }

    if name_lower.contains("email") {
        return format!("'user{}@example.com'", row + 1);
    }
    if name_lower.contains("name") && !name_lower.contains("user") {
        let names = ["Alice", "Bob", "Charlie", "Diana", "Eve", "Frank"];
        return format!("'{}'", names[row % names.len()]);
    }
    if name_lower.contains("phone") {
        return format!("'+1555{:07}'", row + 1000000);
    }

    match col_type {
        t if t.contains("INT") || t.contains("SERIAL") => format!("{}", row + 1),
        t if t.contains("VARCHAR") || t.contains("TEXT") || t.contains("CHAR") => {
            format!("'{}_{}'", col_name, row + 1)
        }
        t if t.contains("BOOL") => if row % 2 == 0 { "TRUE" } else { "FALSE" }.into(),
        t if t.contains("TIMESTAMP") || t.contains("DATE") => {
            format!("'2026-01-{:02} 12:00:00'", (row % 28) + 1)
        }
        t if t.contains("NUMERIC") || t.contains("DECIMAL") || t.contains("FLOAT") || t.contains("DOUBLE") => {
            format!("{:.2}", (row as f64 + 1.0) * 9.99)
        }
        t if t.contains("UUID") => format!("'00000000-0000-0000-0000-{:012}'", row + 1),
        t if t.contains("JSON") => format!("'{{}}'"),
        _ => format!("'{}_{}'", col_name, row + 1),
    }
}
```

- [ ] **Step 2: Add tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_table() {
        let sql = r#"CREATE TABLE users (
            id SERIAL PRIMARY KEY,
            name VARCHAR(100) NOT NULL,
            email TEXT NOT NULL,
            age INTEGER,
            created_at TIMESTAMP DEFAULT NOW()
        );"#;
        let tables = parse_schema(sql);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].name, "users");
        assert_eq!(tables[0].columns.len(), 5);
    }

    #[test]
    fn generates_inserts() {
        let sql = "CREATE TABLE items (id SERIAL, name VARCHAR(50) NOT NULL, price NUMERIC);";
        let tables = parse_schema(sql);
        let inserts = generate_inserts(&tables, 3);
        assert!(inserts.contains("INSERT INTO items"));
        assert_eq!(inserts.lines().filter(|l| l.starts_with("INSERT")).count(), 3);
    }

    #[test]
    fn email_column_gets_email_values() {
        let sql = "CREATE TABLE users (email TEXT NOT NULL);";
        let tables = parse_schema(sql);
        let inserts = generate_inserts(&tables, 1);
        assert!(inserts.contains("@example.com"));
    }

    #[test]
    fn skips_serial_columns() {
        let sql = "CREATE TABLE t (id SERIAL, name TEXT);";
        let tables = parse_schema(sql);
        let inserts = generate_inserts(&tables, 1);
        assert!(!inserts.contains("id"));
    }
}
```

- [ ] **Step 3: Export, CLI, verify, commit**

CLI handler reads SQL schema file, calls `parse_schema()` + `generate_inserts()`, outputs SQL.

```bash
git add crates/apex-detect/src/test_data.rs crates/apex-detect/src/lib.rs crates/apex-cli/src/lib.rs
git commit -m "feat: add apex test-data generator and CLI (C51)"
```

---

## Chunk 3: Final Integration

### Task 9: Run full test suite and fix issues

- [ ] **Step 1: Run workspace tests**

Run: `cargo test --workspace 2>&1 | tail -20`

- [ ] **Step 2: Fix any failures**

Address compilation errors, test count mismatches, etc.

- [ ] **Step 3: Final commit if needed**

### Task 10: Update STATUS.md

- [ ] **Step 1: Update expansion plan progress**

Update internal `plans/STATUS.md` (gitignored) to reflect Tier 2 complete.

The expansion plan should move from `28% (9/32)` to `~50% (16/32)` — 7 new capabilities.

- [ ] **Step 2: Commit plan file status**

```bash
git add docs/superpowers/plans/2026-03-14-expansion-tier2-implementation.md
git commit -m "docs: mark expansion Tier 2 plan as DONE"
```
