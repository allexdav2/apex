<!-- status: DONE --># APEX Analytics Layer Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a pluggable detection layer (`apex-detect` crate) that finds bugs and security issues by cross-referencing coverage data with static analysis, dependency audits, and pattern scanning.

**Architecture:** Trait-based detectors produce `Vec<Finding>`, orchestrated by `DetectorPipeline` which runs pure-analysis detectors concurrently and cargo-subprocess detectors sequentially. Findings enrich the existing `AgentGapReport` via optional fields.

**Tech Stack:** Rust, tokio, serde, uuid. External tools: cargo-clippy, cargo-audit, cargo-geiger (invoked as subprocesses).

**Spec:** `docs/superpowers/specs/2026-03-11-apex-analytics-layer-design.md`

---

## File Structure

```
crates/apex-detect/                  # NEW CRATE
  Cargo.toml
  src/
    lib.rs                           # Re-exports, Detector trait
    finding.rs                       # Finding, Severity, FindingCategory, Evidence, Fix types
    config.rs                        # DetectConfig + sub-configs
    context.rs                       # AnalysisContext
    pipeline.rs                      # DetectorPipeline orchestration + dedup
    report.rs                        # AnalysisReport, SecuritySummary
    detectors/
      mod.rs                         # Re-exports all detectors
      panic_pattern.rs               # PanicPatternDetector (source scan)
      dep_audit.rs                   # DependencyAuditDetector (cargo-audit subprocess)
      unsafe_reach.rs                # UnsafeReachabilityDetector (cargo-geiger subprocess)
      static_analysis.rs             # StaticAnalysisDetector (clippy subprocess + SARIF)

crates/apex-core/src/
  error.rs                           # MODIFY: add Detect(String) variant
  config.rs                          # MODIFY: add `detect: DetectConfig` to ApexConfig
  agent_report.rs                    # MODIFY: add optional findings + security_summary

crates/apex-cli/src/
  main.rs                            # MODIFY: add `apex audit` subcommand, wire detectors into `run`
```

---

## Chunk 1: Foundation Types

### Task 1: Create `apex-detect` crate skeleton

**Files:**
- Create: `crates/apex-detect/Cargo.toml`
- Create: `crates/apex-detect/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Create Cargo.toml for the new crate**

```toml
[package]
name = "apex-detect"
version = "0.1.0"
edition = "2021"

[dependencies]
apex-core = { path = "../apex-core" }
apex-coverage = { path = "../apex-coverage" }
async-trait = "0.1"
futures = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
tokio = { workspace = true }
toml = "0.8"
tracing = "0.1"
uuid = { version = "1", features = ["v4", "serde"] }
regex = "1"

[dev-dependencies]
mockall = { workspace = true }
tempfile = "3"
tokio = { workspace = true, features = ["test-util"] }
```

- [ ] **Step 2: Create empty lib.rs**

Start with an empty crate. Modules will be added incrementally as each task creates them.

```rust
// Modules added incrementally by subsequent tasks.
```

- [ ] **Step 3: Add apex-detect to workspace members**

In root `Cargo.toml`, add `"crates/apex-detect"` to the `members` array, after `"crates/apex-fuzz"`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p apex-detect`
Expected: Errors about missing modules (that's fine, they'll be created next)

- [ ] **Step 5: Commit**

```bash
git add crates/apex-detect/Cargo.toml crates/apex-detect/src/lib.rs Cargo.toml
git commit -m "feat(detect): scaffold apex-detect crate"
```

---

### Task 2: Finding types (`finding.rs`)

**Files:**
- Create: `crates/apex-detect/src/finding.rs`

This is the core data model. Every detector produces `Vec<Finding>`.

- [ ] **Step 1: Write tests for Severity ordering and Finding basics**

At the bottom of `finding.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn severity_rank_ordering() {
        assert_eq!(Severity::Critical.rank(), 0);
        assert_eq!(Severity::High.rank(), 1);
        assert_eq!(Severity::Medium.rank(), 2);
        assert_eq!(Severity::Low.rank(), 3);
        assert_eq!(Severity::Info.rank(), 4);
    }

    #[test]
    fn severity_ord_matches_rank() {
        assert!(Severity::Critical < Severity::High);
        assert!(Severity::High < Severity::Medium);
        assert!(Severity::Medium < Severity::Low);
        assert!(Severity::Low < Severity::Info);
    }

    #[test]
    fn finding_serializes_to_json() {
        let f = Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity: Severity::High,
            category: FindingCategory::PanicPath,
            file: PathBuf::from("src/main.rs"),
            line: Some(42),
            title: "unwrap on error path".into(),
            description: "desc".into(),
            evidence: vec![],
            covered: false,
            suggestion: "add error test".into(),
            explanation: None,
            fix: None,
        };
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"severity\":\"high\""));
        assert!(json.contains("\"category\":\"panic_path\""));
        assert!(json.contains("\"covered\":false"));
    }

    #[test]
    fn fix_variants_serialize() {
        let patch = Fix::CodePatch {
            file: "src/lib.rs".into(),
            diff: "+check".into(),
        };
        let json = serde_json::to_string(&patch).unwrap();
        assert!(json.contains("\"type\":\"code_patch\""));

        let upgrade = Fix::DependencyUpgrade {
            package: "openssl".into(),
            to: "0.10.55".into(),
        };
        let json = serde_json::to_string(&upgrade).unwrap();
        assert!(json.contains("\"type\":\"dependency_upgrade\""));
    }

    #[test]
    fn evidence_variants_serialize() {
        let e = Evidence::SanitizerReport {
            sanitizer: "asan".into(),
            stderr: "heap-buffer-overflow".into(),
        };
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"type\":\"sanitizer_report\""));
        assert!(json.contains("asan"));
    }
}
```

- [ ] **Step 2: Add `pub mod finding;` to lib.rs**

In `crates/apex-detect/src/lib.rs`, add:
```rust
pub mod finding;
```

- [ ] **Step 3: Implement Finding, Severity, FindingCategory, Evidence, Fix types**

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use apex_core::types::BranchId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: Uuid,
    pub detector: String,
    pub severity: Severity,
    pub category: FindingCategory,
    pub file: PathBuf,
    pub line: Option<u32>,
    pub title: String,
    pub description: String,
    pub evidence: Vec<Evidence>,
    pub covered: bool,
    pub suggestion: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<Fix>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl Severity {
    pub fn rank(&self) -> u8 {
        match self {
            Severity::Critical => 0,
            Severity::High => 1,
            Severity::Medium => 2,
            Severity::Low => 3,
            Severity::Info => 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    MemorySafety,
    UndefinedBehavior,
    Injection,
    PanicPath,
    UnsafeCode,
    DependencyVuln,
    LogicBug,
    SecuritySmell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Evidence {
    CoverageGap { branch_id: BranchId, line: u32 },
    SanitizerReport { sanitizer: String, stderr: String },
    StaticAnalysis { tool: String, rule_id: String, sarif: serde_json::Value },
    UnsafeBlock { file: PathBuf, line_range: (u32, u32), reason: String },
    DiffBehavior { input: Vec<u8>, expected: String, actual: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Fix {
    CodePatch { file: PathBuf, diff: String },
    DependencyUpgrade { package: String, to: String },
    TestCase { file: PathBuf, code: String },
    ConfigChange { description: String },
    Manual { steps: Vec<String> },
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-detect -- finding`
Expected: All 5 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/apex-detect/src/finding.rs crates/apex-detect/src/lib.rs
git commit -m "feat(detect): add Finding, Severity, Evidence, Fix types"
```

---

### Task 3: DetectConfig (`config.rs`)

**Files:**
- Create: `crates/apex-detect/src/config.rs`

- [ ] **Step 1: Add `pub mod config;` to lib.rs**

In `crates/apex-detect/src/lib.rs`, add:
```rust
pub mod config;
```

- [ ] **Step 2: Write tests for config defaults and deserialization**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_tier1_detectors() {
        let cfg = DetectConfig::default();
        assert!(cfg.enabled.contains(&"panic".to_string()));
        assert!(cfg.enabled.contains(&"deps".to_string()));
        assert!(cfg.enabled.contains(&"unsafe".to_string()));
        assert!(cfg.enabled.contains(&"static".to_string()));
    }

    #[test]
    fn default_timeout_is_none() {
        let cfg = DetectConfig::default();
        assert!(cfg.per_detector_timeout_secs.is_none());
    }

    #[test]
    fn deserialize_from_toml() {
        let toml_str = r#"
enabled = ["panic", "deps"]
severity_threshold = "high"
per_detector_timeout_secs = 60

[sanitizer]
replay_top_percent = 5
sanitizers = ["address"]

[static]
clippy_extra_args = ["-W", "clippy::pedantic"]
"#;
        let cfg: DetectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.enabled, vec!["panic", "deps"]);
        assert_eq!(cfg.severity_threshold, "high");
        assert_eq!(cfg.per_detector_timeout_secs, Some(60));
        assert_eq!(cfg.sanitizer.replay_top_percent, 5);
        assert_eq!(cfg.static_analysis.clippy_extra_args, vec!["-W".to_string(), "clippy::pedantic".to_string()]);
    }

    #[test]
    fn empty_toml_gives_defaults() {
        let cfg: DetectConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.enabled.len(), 4);
        assert_eq!(cfg.severity_threshold, "low");
    }
}
```

- [ ] **Step 3: Implement DetectConfig and sub-config structs**

```rust
use serde::{Deserialize, Serialize};

fn default_enabled() -> Vec<String> {
    vec!["unsafe".into(), "deps".into(), "panic".into(), "static".into()]
}

fn default_severity() -> String {
    "low".into()
}

fn default_replay_top_percent() -> u8 {
    1
}

fn default_sanitizers() -> Vec<String> {
    vec!["address".into(), "undefined".into()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectConfig {
    #[serde(default = "default_enabled")]
    pub enabled: Vec<String>,
    #[serde(default = "default_severity")]
    pub severity_threshold: String,
    #[serde(default)]
    pub per_detector_timeout_secs: Option<u64>,
    #[serde(default)]
    pub sanitizer: SanitizerConfig,
    #[serde(default, rename = "static")]
    pub static_analysis: StaticAnalysisConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub diff: DiffConfig,
    #[serde(default)]
    pub properties: Vec<PropertyConfig>,
}

impl Default for DetectConfig {
    fn default() -> Self {
        DetectConfig {
            enabled: default_enabled(),
            severity_threshold: default_severity(),
            per_detector_timeout_secs: None,
            sanitizer: SanitizerConfig::default(),
            static_analysis: StaticAnalysisConfig::default(),
            llm: LlmConfig::default(),
            diff: DiffConfig::default(),
            properties: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SanitizerConfig {
    #[serde(default = "default_replay_top_percent")]
    pub replay_top_percent: u8,
    #[serde(default = "default_sanitizers")]
    pub sanitizers: Vec<String>,
}

impl Default for SanitizerConfig {
    fn default() -> Self {
        SanitizerConfig {
            replay_top_percent: default_replay_top_percent(),
            sanitizers: default_sanitizers(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct StaticAnalysisConfig {
    pub clippy_extra_args: Vec<String>,
    pub sarif_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub enabled: bool,
    pub batch_size: usize,
    pub model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig {
            enabled: false,
            batch_size: 10,
            model: "claude-sonnet-4-6".into(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DiffConfig {
    pub base_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyConfig {
    pub name: String,
    pub check: String,
    pub target: String,
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-detect -- config`
Expected: All 4 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/apex-detect/src/config.rs crates/apex-detect/src/lib.rs
git commit -m "feat(detect): add DetectConfig with sub-configs"
```

---

### Task 4: AnalysisContext (`context.rs`) and AnalysisReport (`report.rs`)

**Files:**
- Create: `crates/apex-detect/src/context.rs`
- Create: `crates/apex-detect/src/report.rs`

- [ ] **Step 1: Add `pub mod context; pub mod report;` to lib.rs**

In `crates/apex-detect/src/lib.rs`, add:
```rust
pub mod context;
pub mod report;
```

- [ ] **Step 2: Write AnalysisContext**

Note: `CoverageOracle` does not derive `Debug`, so `AnalysisContext` cannot derive it either. We implement `Debug` manually, skipping the oracle field.

```rust
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use apex_core::types::{BugReport, Language};
use apex_coverage::CoverageOracle;

use crate::config::DetectConfig;

#[derive(Clone)]
pub struct AnalysisContext {
    pub target_root: PathBuf,
    pub language: Language,
    pub oracle: Arc<CoverageOracle>,
    pub file_paths: HashMap<u64, PathBuf>,
    pub known_bugs: Vec<BugReport>,
    pub source_cache: HashMap<PathBuf, String>,
    pub fuzz_corpus: Option<PathBuf>,
    pub config: DetectConfig,
}

impl fmt::Debug for AnalysisContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalysisContext")
            .field("target_root", &self.target_root)
            .field("language", &self.language)
            .field("file_paths", &self.file_paths.len())
            .field("source_cache", &self.source_cache.len())
            .field("fuzz_corpus", &self.fuzz_corpus)
            .finish()
    }
}
```

No tests needed — this is a pure data struct. It will be tested through pipeline integration tests.

- [ ] **Step 3: Write tests for SecuritySummary and AnalysisReport**

In `report.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Finding, FindingCategory, Severity};
    use std::path::PathBuf;

    fn make_finding(severity: Severity) -> Finding {
        Finding {
            id: uuid::Uuid::nil(),
            detector: "test".into(),
            severity,
            category: FindingCategory::PanicPath,
            file: PathBuf::from("test.rs"),
            line: Some(1),
            title: "t".into(),
            description: "d".into(),
            evidence: vec![],
            covered: false,
            suggestion: "s".into(),
            explanation: None,
            fix: None,
        }
    }

    #[test]
    fn security_summary_counts_severities() {
        let report = AnalysisReport {
            findings: vec![
                make_finding(Severity::Critical),
                make_finding(Severity::High),
                make_finding(Severity::High),
                make_finding(Severity::Medium),
                make_finding(Severity::Low),
                make_finding(Severity::Info),
            ],
            detector_status: vec![("test".into(), true)],
        };
        let summary = report.security_summary();
        assert_eq!(summary.critical, 1);
        assert_eq!(summary.high, 2);
        assert_eq!(summary.medium, 1);
        assert_eq!(summary.low, 1);
    }

    #[test]
    fn empty_report_gives_zero_summary() {
        let report = AnalysisReport {
            findings: vec![],
            detector_status: vec![],
        };
        let summary = report.security_summary();
        assert_eq!(summary.critical, 0);
        assert_eq!(summary.high, 0);
        assert!(summary.top_risk.is_none());
    }

    #[test]
    fn security_summary_serializes() {
        let summary = SecuritySummary {
            critical: 1,
            high: 2,
            medium: 3,
            low: 4,
            detectors_run: vec!["panic".into()],
            top_risk: Some("src/main.rs — uncovered panic".into()),
        };
        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"critical\":1"));
        assert!(json.contains("\"top_risk\""));
    }
}
```

- [ ] **Step 4: Implement AnalysisReport and SecuritySummary**

```rust
use serde::{Deserialize, Serialize};

use crate::finding::{Finding, Severity};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub findings: Vec<Finding>,
    pub detector_status: Vec<(String, bool)>,
}

impl AnalysisReport {
    pub fn security_summary(&self) -> SecuritySummary {
        let mut critical = 0;
        let mut high = 0;
        let mut medium = 0;
        let mut low = 0;

        for f in &self.findings {
            match f.severity {
                Severity::Critical => critical += 1,
                Severity::High => high += 1,
                Severity::Medium => medium += 1,
                Severity::Low => low += 1,
                Severity::Info => {}
            }
        }

        let detectors_run: Vec<String> = self
            .detector_status
            .iter()
            .map(|(name, _)| name.clone())
            .collect();

        let top_risk = self
            .findings
            .iter()
            .filter(|f| f.severity.rank() <= Severity::High.rank())
            .min_by_key(|f| (f.severity.rank(), f.covered as u8))
            .map(|f| format!("{} — {}", f.file.display(), f.title));

        SecuritySummary {
            critical,
            high,
            medium,
            low,
            detectors_run,
            top_risk,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecuritySummary {
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub detectors_run: Vec<String>,
    pub top_risk: Option<String>,
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p apex-detect -- report`
Expected: All 3 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/apex-detect/src/context.rs crates/apex-detect/src/report.rs crates/apex-detect/src/lib.rs
git commit -m "feat(detect): add AnalysisContext, AnalysisReport, SecuritySummary"
```

---

### Task 5: Detector trait + DetectorPipeline (`lib.rs`, `pipeline.rs`)

**Files:**
- Modify: `crates/apex-detect/src/lib.rs`
- Create: `crates/apex-detect/src/pipeline.rs`
- Create: `crates/apex-detect/src/detectors/mod.rs`

- [ ] **Step 1: Add `pub mod pipeline; pub mod detectors;` and Detector trait to lib.rs**

Replace the existing `lib.rs` content with the full version (all prior modules + trait):

```rust
pub mod finding;
pub mod config;
pub mod context;
pub mod pipeline;
pub mod report;
pub mod detectors;

pub use config::DetectConfig;
pub use context::AnalysisContext;
pub use finding::{Evidence, Finding, FindingCategory, Fix, Severity};
pub use pipeline::DetectorPipeline;
pub use report::{AnalysisReport, SecuritySummary};

use apex_core::error::Result;
use async_trait::async_trait;

/// A pluggable detector that analyzes code for bugs/security issues.
#[async_trait]
pub trait Detector: Send + Sync {
    /// Human-readable name (e.g., "panic-pattern", "dependency-audit").
    fn name(&self) -> &str;

    /// Run analysis given coverage + execution context.
    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>>;

    /// Does this detector invoke `cargo` subprocesses?
    /// If true, the pipeline runs it sequentially to avoid Cargo.lock contention.
    fn uses_cargo_subprocess(&self) -> bool {
        false
    }
}
```

- [ ] **Step 2: Create empty detectors/mod.rs**

```rust
// Detector modules added by Tasks 8-11.
```

- [ ] **Step 3: Write pipeline tests**

In `pipeline.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Finding, FindingCategory, Severity};
    use std::path::PathBuf;

    fn make_finding(detector: &str, file: &str, line: u32, severity: Severity, category: FindingCategory) -> Finding {
        Finding {
            id: uuid::Uuid::new_v4(),
            detector: detector.into(),
            severity,
            category,
            file: PathBuf::from(file),
            line: Some(line),
            title: format!("{detector} finding"),
            description: "desc".into(),
            evidence: vec![],
            covered: false,
            suggestion: "fix it".into(),
            explanation: None,
            fix: None,
        }
    }

    #[test]
    fn deduplicate_merges_same_location_and_category() {
        let mut findings = vec![
            make_finding("a", "src/main.rs", 10, Severity::Medium, FindingCategory::PanicPath),
            make_finding("b", "src/main.rs", 10, Severity::High, FindingCategory::PanicPath),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 1);
        // Keeps highest severity (High < Medium in rank)
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn deduplicate_keeps_different_categories_separate() {
        let mut findings = vec![
            make_finding("a", "src/main.rs", 10, Severity::Medium, FindingCategory::PanicPath),
            make_finding("b", "src/main.rs", 10, Severity::High, FindingCategory::UnsafeCode),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn deduplicate_keeps_different_lines_separate() {
        let mut findings = vec![
            make_finding("a", "src/main.rs", 10, Severity::Medium, FindingCategory::PanicPath),
            make_finding("a", "src/main.rs", 20, Severity::Medium, FindingCategory::PanicPath),
        ];
        deduplicate(&mut findings);
        assert_eq!(findings.len(), 2);
    }

    #[test]
    fn deduplicate_empty_is_noop() {
        let mut findings: Vec<Finding> = vec![];
        deduplicate(&mut findings);
        assert!(findings.is_empty());
    }
}
```

- [ ] **Step 4: Implement DetectorPipeline and deduplicate**

```rust
use std::collections::HashMap;
use std::time::Duration;

use apex_core::error::{ApexError, Result};
use tracing::warn;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::report::AnalysisReport;
use crate::Detector;

pub struct DetectorPipeline {
    detectors: Vec<Box<dyn Detector>>,
}

impl DetectorPipeline {
    pub fn new(detectors: Vec<Box<dyn Detector>>) -> Self {
        Self { detectors }
    }

    pub async fn run_all(&self, ctx: &AnalysisContext) -> AnalysisReport {
        let per_detector_timeout = ctx
            .config
            .per_detector_timeout_secs
            .map(Duration::from_secs)
            .unwrap_or(Duration::from_secs(300));

        let (pure, subprocess): (Vec<_>, Vec<_>) = self
            .detectors
            .iter()
            .partition(|d| !d.uses_cargo_subprocess());

        // Pure detectors run concurrently
        let pure_futs = pure.iter().map(|d| {
            let timeout = per_detector_timeout;
            async move {
                let name = d.name().to_string();
                match tokio::time::timeout(timeout, d.analyze(ctx)).await {
                    Ok(result) => (name, result),
                    Err(_) => (
                        name,
                        Err(ApexError::Timeout(timeout.as_millis() as u64)),
                    ),
                }
            }
        });
        let pure_results = futures::future::join_all(pure_futs);

        // Subprocess detectors run sequentially (Cargo.lock contention)
        let subprocess_results = async {
            let mut results = Vec::new();
            for d in &subprocess {
                let name = d.name().to_string();
                let result =
                    match tokio::time::timeout(per_detector_timeout, d.analyze(ctx)).await {
                        Ok(r) => r,
                        Err(_) => Err(ApexError::Timeout(
                            per_detector_timeout.as_millis() as u64,
                        )),
                    };
                results.push((name, result));
            }
            results
        };

        let (pure_res, sub_res) = tokio::join!(pure_results, subprocess_results);

        let mut findings = Vec::new();
        let mut detector_status = Vec::new();

        for (name, result) in pure_res.into_iter().chain(sub_res) {
            match result {
                Ok(f) => {
                    detector_status.push((name, true));
                    findings.extend(f);
                }
                Err(e) => {
                    warn!(detector = %name, error = %e, "detector failed");
                    detector_status.push((name, false));
                }
            }
        }

        deduplicate(&mut findings);
        findings.sort_by_key(|f| (f.severity.rank(), f.covered as u8));

        AnalysisReport {
            findings,
            detector_status,
        }
    }
}

pub fn deduplicate(findings: &mut Vec<Finding>) {
    let mut seen: HashMap<(std::path::PathBuf, Option<u32>, FindingCategory), usize> =
        HashMap::new();
    let mut merged = Vec::new();

    for finding in findings.drain(..) {
        let key = (
            finding.file.clone(),
            finding.line,
            finding.category.clone(),
        );
        if let Some(&idx) = seen.get(&key) {
            if finding.severity.rank() < merged[idx].severity.rank() {
                let existing: &mut Finding = &mut merged[idx];
                existing.severity = finding.severity;
                existing.title.clone_from(&finding.title);
                existing.description.clone_from(&finding.description);
            }
            merged[idx].evidence.extend(finding.evidence);
        } else {
            seen.insert(key, merged.len());
            merged.push(finding);
        }
    }

    *findings = merged;
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p apex-detect -- pipeline`
Expected: All 4 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/apex-detect/src/lib.rs crates/apex-detect/src/pipeline.rs crates/apex-detect/src/detectors/mod.rs
git commit -m "feat(detect): add Detector trait, DetectorPipeline with dedup"
```

---

### Task 6: Add `ApexError::Detect` variant and `detect` field to `ApexConfig`

**Files:**
- Modify: `crates/apex-core/src/error.rs:4-46`
- Modify: `crates/apex-core/src/config.rs:11-22`
- Modify: `crates/apex-core/Cargo.toml`

- [ ] **Step 1: Add `Detect(String)` variant to `ApexError`**

In `crates/apex-core/src/error.rs`, add after the `Timeout` variant (line 43):

```rust
    #[error("Detector error: {0}")]
    Detect(String),
```

- [ ] **Step 2: Add test for new variant**

In the `tests` module of `error.rs`, add:

```rust
    #[test]
    fn display_detect() {
        let e = ApexError::Detect("cargo-audit failed".into());
        let msg = e.to_string();
        assert!(msg.contains("Detector error"));
        assert!(msg.contains("cargo-audit failed"));
    }
```

- [ ] **Step 3: Add lightweight `DetectConfig` to `apex-core/src/config.rs`**

`apex-detect` depends on `apex-core`, so `apex-core` cannot depend on `apex-detect`. Instead, we add a lightweight `DetectConfig` directly in `apex-core` that captures the TOML shape. The full `DetectConfig` with sub-structs lives in `apex-detect` and can accept `From<apex_core::config::DetectConfig>`.

In `crates/apex-core/src/config.rs`, add after `LoggingConfig`:

```rust
// ---------------------------------------------------------------------------
// Detection / Analytics
// ---------------------------------------------------------------------------

/// Lightweight detection config stored in apex.toml.
/// The full DetectConfig with sub-structs lives in `apex-detect`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DetectConfig {
    pub enabled: Vec<String>,
    #[serde(default = "default_detect_severity")]
    pub severity_threshold: String,
    pub per_detector_timeout_secs: Option<u64>,
}

fn default_detect_severity() -> String {
    "low".into()
}
```

And add `pub detect: DetectConfig` to `ApexConfig` struct (after `logging`):

```rust
pub struct ApexConfig {
    pub coverage: CoverageConfig,
    pub fuzz: FuzzConfig,
    pub concolic: ConcolicConfig,
    pub agent: AgentConfig,
    pub sandbox: SandboxConfig,
    pub symbolic: SymbolicConfig,
    pub instrument: InstrumentConfig,
    pub logging: LoggingConfig,
    pub detect: DetectConfig,
}
```

- [ ] **Step 4: Update existing config test**

In `default_config_has_expected_values` test, add:

```rust
        assert_eq!(cfg.detect.severity_threshold, "low");
        assert!(cfg.detect.per_detector_timeout_secs.is_none());
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p apex-core`
Expected: All tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/apex-core/src/error.rs crates/apex-core/src/config.rs
git commit -m "feat(core): add ApexError::Detect variant and DetectConfig to ApexConfig"
```

---

### Task 7: Add optional findings to `AgentGapReport`

**Files:**
- Modify: `crates/apex-core/src/agent_report.rs:12-16`

- [ ] **Step 1: Add optional fields to AgentGapReport**

In `crates/apex-core/src/agent_report.rs`, modify the `AgentGapReport` struct to add two new fields:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentGapReport {
    pub summary: GapSummary,
    pub gaps: Vec<GapEntry>,
    pub blocked: Vec<BlockedEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_summary: Option<serde_json::Value>,
}
```

Note: We use `serde_json::Value` rather than concrete types to avoid `apex-core` depending on `apex-detect`. The CLI serializes `Vec<Finding>` and `SecuritySummary` from `apex-detect` into `Value` before setting these fields.

- [ ] **Step 2: Update the `build_agent_gap_report` function**

At line 264, update the return statement to include the new fields:

```rust
    AgentGapReport {
        summary: GapSummary { ... },
        gaps,
        blocked: Vec::new(),
        findings: None,
        security_summary: None,
    }
```

- [ ] **Step 3: Update test fixtures**

In the test `agent_gap_report_serializes_to_json`, add the new fields:

```rust
        let report = AgentGapReport {
            // ... existing fields ...
            findings: None,
            security_summary: None,
        };
```

Also update the `build_report_from_oracle_data` test: the returned report will now have `findings: None` and `security_summary: None`, which is correct.

- [ ] **Step 4: Add test for backward-compatible deserialization**

```rust
    #[test]
    fn agent_gap_report_deserializes_without_findings() {
        // JSON without findings/security_summary fields (old format)
        let json = r#"{
            "summary": {"total_branches": 10, "covered_branches": 5, "coverage_pct": 0.5, "files_total": 1, "files_fully_covered": 0},
            "gaps": [],
            "blocked": []
        }"#;
        let report: AgentGapReport = serde_json::from_str(json).unwrap();
        assert!(report.findings.is_none());
        assert!(report.security_summary.is_none());
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p apex-core`
Expected: All tests pass

- [ ] **Step 6: Run full workspace test to check nothing broke**

Run: `cargo test --workspace`
Expected: All existing tests still pass

- [ ] **Step 7: Commit**

```bash
git add crates/apex-core/src/agent_report.rs
git commit -m "feat(core): add optional findings/security_summary to AgentGapReport"
```

---

## Chunk 2: Tier 1 Detectors

### Task 8: PanicPatternDetector

**Files:**
- Create: `crates/apex-detect/src/detectors/panic_pattern.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs`

This is the simplest detector — pure source scan, no subprocesses.

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DetectConfig;
    use crate::context::AnalysisContext;
    use apex_core::types::Language;
    use apex_coverage::CoverageOracle;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn make_ctx(source_files: HashMap<PathBuf, String>) -> AnalysisContext {
        AnalysisContext {
            target_root: PathBuf::from("/tmp/test"),
            language: Language::Rust,
            oracle: Arc::new(CoverageOracle::new()),
            file_paths: HashMap::new(),
            known_bugs: vec![],
            source_cache: source_files,
            fuzz_corpus: None,
            config: DetectConfig::default(),
        }
    }

    #[tokio::test]
    async fn detects_unwrap() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/main.rs"),
            "fn foo() {\n    let x = bar().unwrap();\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let detector = PanicPatternDetector;
        let findings = detector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::PanicPath);
        assert!(findings[0].title.contains("unwrap"));
        assert_eq!(findings[0].line, Some(2));
    }

    #[tokio::test]
    async fn detects_expect() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn foo() {\n    x.expect(\"oops\");\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("expect"));
    }

    #[tokio::test]
    async fn detects_panic_macro() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn foo() {\n    panic!(\"boom\");\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 1);
        assert!(findings[0].title.contains("panic!"));
    }

    #[tokio::test]
    async fn detects_todo_and_unreachable() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn foo() {\n    todo!();\n}\nfn bar() {\n    unreachable!();\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert_eq!(findings.len(), 2);
    }

    #[tokio::test]
    async fn ignores_comments() {
        let mut files = HashMap::new();
        files.insert(
            PathBuf::from("src/lib.rs"),
            "fn foo() {\n    // x.unwrap() is bad\n    let y = 1;\n}\n".into(),
        );
        let ctx = make_ctx(files);
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[tokio::test]
    async fn empty_source_cache_produces_no_findings() {
        let ctx = make_ctx(HashMap::new());
        let findings = PanicPatternDetector.analyze(&ctx).await.unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn does_not_use_cargo_subprocess() {
        assert!(!PanicPatternDetector.uses_cargo_subprocess());
    }
}
```

- [ ] **Step 2: Implement PanicPatternDetector**

```rust
use apex_core::error::Result;
use async_trait::async_trait;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Evidence, Finding, FindingCategory, Severity};
use crate::Detector;

pub struct PanicPatternDetector;

/// Patterns that indicate potential panic paths in source code.
const PANIC_PATTERNS: &[(&str, &str)] = &[
    (".unwrap()", "unwrap() call — panics on None/Err"),
    (".expect(", "expect() call — panics on None/Err with message"),
    ("panic!(", "panic!() macro — explicit panic"),
    ("todo!(", "todo!() macro — unimplemented code"),
    ("unreachable!(", "unreachable!() macro — should-not-reach path"),
    ("unimplemented!(", "unimplemented!() macro"),
];

#[async_trait]
impl Detector for PanicPatternDetector {
    fn name(&self) -> &str {
        "panic-pattern"
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut findings = Vec::new();

        for (path, source) in &ctx.source_cache {
            for (line_num, line) in source.lines().enumerate() {
                let trimmed = line.trim();

                // Skip comments
                if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                    continue;
                }

                for (pattern, description) in PANIC_PATTERNS {
                    if trimmed.contains(pattern) {
                        let line_1based = (line_num + 1) as u32;

                        findings.push(Finding {
                            id: Uuid::new_v4(),
                            detector: self.name().into(),
                            severity: Severity::Medium,
                            category: FindingCategory::PanicPath,
                            file: path.clone(),
                            line: Some(line_1based),
                            title: format!("{description} at line {line_1based}"),
                            description: format!(
                                "Pattern `{pattern}` found in {}:{}",
                                path.display(),
                                line_1based
                            ),
                            evidence: vec![],
                            covered: false, // TODO: cross-reference with oracle
                            suggestion: "Handle error explicitly or add test for panic path".into(),
                            explanation: None,
                            fix: None,
                        });
                        break; // One finding per line max
                    }
                }
            }
        }

        Ok(findings)
    }
}
```

- [ ] **Step 3: Register in detectors/mod.rs**

```rust
pub mod panic_pattern;

pub use panic_pattern::PanicPatternDetector;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-detect -- panic_pattern`
Expected: All 7 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/apex-detect/src/detectors/panic_pattern.rs crates/apex-detect/src/detectors/mod.rs
git commit -m "feat(detect): add PanicPatternDetector — source-level panic scan"
```

---

### Task 9: DependencyAuditDetector

**Files:**
- Create: `crates/apex-detect/src/detectors/dep_audit.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs`

Runs `cargo audit --json` as a subprocess and parses output.

- [ ] **Step 1: Write tests with mock JSON fixture**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_cargo_audit_json_with_vulns() {
        let json = r#"{
            "vulnerabilities": {
                "found": 1,
                "list": [{
                    "advisory": {
                        "id": "RUSTSEC-2023-0044",
                        "title": "openssl: X.509 bypass",
                        "severity": "high",
                        "url": "https://rustsec.org/advisories/RUSTSEC-2023-0044",
                        "description": "desc"
                    },
                    "package": {
                        "name": "openssl",
                        "version": "0.10.38"
                    },
                    "versions": {
                        "patched": [">=0.10.55"]
                    }
                }]
            }
        }"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::DependencyVuln);
        assert!(findings[0].title.contains("RUSTSEC-2023-0044"));
        assert_eq!(findings[0].severity, Severity::High);
    }

    #[test]
    fn parse_cargo_audit_json_no_vulns() {
        let json = r#"{"vulnerabilities": {"found": 0, "list": []}}"#;
        let findings = parse_cargo_audit_output(json).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_cargo_audit_invalid_json() {
        let result = parse_cargo_audit_output("not json");
        assert!(result.is_err());
    }

    #[test]
    fn uses_cargo_subprocess_returns_true() {
        let d = DependencyAuditDetector;
        assert!(d.uses_cargo_subprocess());
    }
}
```

- [ ] **Step 2: Implement DependencyAuditDetector**

```rust
use apex_core::error::{ApexError, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Fix, Severity};
use crate::Detector;

pub struct DependencyAuditDetector;

#[async_trait]
impl Detector for DependencyAuditDetector {
    fn name(&self) -> &str {
        "dependency-audit"
    }

    fn uses_cargo_subprocess(&self) -> bool {
        true
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let output = tokio::process::Command::new("cargo")
            .args(["audit", "--json"])
            .current_dir(&ctx.target_root)
            .output()
            .await
            .map_err(|e| ApexError::Detect(format!("cargo-audit: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_cargo_audit_output(&stdout)
    }
}

pub fn parse_cargo_audit_output(json_str: &str) -> Result<Vec<Finding>> {
    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| ApexError::Detect(format!("cargo-audit JSON parse: {e}")))?;

    let mut findings = Vec::new();

    let vulns = parsed
        .get("vulnerabilities")
        .and_then(|v| v.get("list"))
        .and_then(|v| v.as_array());

    if let Some(vuln_list) = vulns {
        for vuln in vuln_list {
            let advisory = &vuln["advisory"];
            let id = advisory["id"].as_str().unwrap_or("unknown");
            let title = advisory["title"].as_str().unwrap_or("unknown vulnerability");
            let sev_str = advisory["severity"].as_str().unwrap_or("medium");
            let pkg_name = vuln["package"]["name"].as_str().unwrap_or("unknown");
            let pkg_version = vuln["package"]["version"].as_str().unwrap_or("?");
            let patched = vuln["versions"]["patched"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let severity = match sev_str {
                "critical" => Severity::Critical,
                "high" => Severity::High,
                "medium" => Severity::Medium,
                "low" => Severity::Low,
                _ => Severity::Medium,
            };

            let fix = if !patched.is_empty() {
                Some(Fix::DependencyUpgrade {
                    package: pkg_name.into(),
                    to: patched.into(),
                })
            } else {
                None
            };

            findings.push(Finding {
                id: Uuid::new_v4(),
                detector: "dependency-audit".into(),
                severity,
                category: FindingCategory::DependencyVuln,
                file: PathBuf::from("Cargo.toml"),
                line: None,
                title: format!("{pkg_name} {pkg_version} ({id})"),
                description: format!("{title}"),
                evidence: vec![],
                covered: true, // Dep vulns are always "reachable"
                suggestion: if !patched.is_empty() {
                    format!("Upgrade {pkg_name} to {patched}")
                } else {
                    "No patched version available — consider alternative crate".into()
                },
                explanation: None,
                fix,
            });
        }
    }

    Ok(findings)
}
```

- [ ] **Step 3: Register in detectors/mod.rs**

Add:
```rust
pub mod dep_audit;
pub use dep_audit::DependencyAuditDetector;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-detect -- dep_audit`
Expected: All 4 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/apex-detect/src/detectors/dep_audit.rs crates/apex-detect/src/detectors/mod.rs
git commit -m "feat(detect): add DependencyAuditDetector — cargo-audit integration"
```

---

### Task 10: UnsafeReachabilityDetector

**Files:**
- Create: `crates/apex-detect/src/detectors/unsafe_reach.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs`

Runs `cargo geiger --output-format json` and cross-references with source cache.

- [ ] **Step 1: Write tests with mock JSON fixture**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_geiger_json_with_unsafe() {
        let json = r#"{
            "packages": [{
                "package": {"name": "mylib", "version": "0.1.0"},
                "unsafety": {
                    "used": {
                        "functions": {"unsafe_": 2},
                        "exprs": {"unsafe_": 5}
                    },
                    "unused": {
                        "functions": {"unsafe_": 0},
                        "exprs": {"unsafe_": 0}
                    }
                }
            }]
        }"#;
        let findings = parse_geiger_output(json, "mylib").unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::UnsafeCode);
        assert!(findings[0].title.contains("unsafe"));
    }

    #[test]
    fn parse_geiger_json_no_unsafe() {
        let json = r#"{
            "packages": [{
                "package": {"name": "mylib", "version": "0.1.0"},
                "unsafety": {
                    "used": {"functions": {"unsafe_": 0}, "exprs": {"unsafe_": 0}},
                    "unused": {"functions": {"unsafe_": 0}, "exprs": {"unsafe_": 0}}
                }
            }]
        }"#;
        let findings = parse_geiger_output(json, "mylib").unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn uses_cargo_subprocess_returns_true() {
        assert!(UnsafeReachabilityDetector.uses_cargo_subprocess());
    }
}
```

- [ ] **Step 2: Implement UnsafeReachabilityDetector**

```rust
use apex_core::error::{ApexError, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use uuid::Uuid;

use crate::context::AnalysisContext;
use crate::finding::{Finding, FindingCategory, Severity};
use crate::Detector;

pub struct UnsafeReachabilityDetector;

#[async_trait]
impl Detector for UnsafeReachabilityDetector {
    fn name(&self) -> &str {
        "unsafe-reachability"
    }

    fn uses_cargo_subprocess(&self) -> bool {
        true
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let output = tokio::process::Command::new("cargo")
            .args(["geiger", "--output-format", "json", "--all-features"])
            .current_dir(&ctx.target_root)
            .output()
            .await
            .map_err(|e| ApexError::Detect(format!("cargo-geiger: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // cargo-geiger may not be installed — degrade gracefully
            if stderr.contains("no such command") || stderr.contains("not found") {
                tracing::info!("cargo-geiger not installed, skipping unsafe analysis");
                return Ok(vec![]);
            }
            return Err(ApexError::Detect(format!(
                "cargo-geiger failed:\n{stderr}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Extract the package name from the target root's Cargo.toml
        let pkg_name = ctx
            .target_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        parse_geiger_output(&stdout, pkg_name)
    }
}

pub fn parse_geiger_output(json_str: &str, target_pkg: &str) -> Result<Vec<Finding>> {
    let parsed: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| ApexError::Detect(format!("geiger JSON parse: {e}")))?;

    let mut findings = Vec::new();

    let packages = parsed
        .get("packages")
        .and_then(|p| p.as_array());

    if let Some(pkgs) = packages {
        for pkg in pkgs {
            let name = pkg["package"]["name"].as_str().unwrap_or("");
            // Only report on the target package, not all deps
            if !name.eq_ignore_ascii_case(target_pkg) && !target_pkg.is_empty() {
                continue;
            }

            let used = &pkg["unsafety"]["used"];
            let unsafe_fns = used["functions"]["unsafe_"].as_u64().unwrap_or(0);
            let unsafe_exprs = used["exprs"]["unsafe_"].as_u64().unwrap_or(0);

            if unsafe_fns > 0 || unsafe_exprs > 0 {
                findings.push(Finding {
                    id: Uuid::new_v4(),
                    detector: "unsafe-reachability".into(),
                    severity: Severity::Medium,
                    category: FindingCategory::UnsafeCode,
                    file: PathBuf::from("Cargo.toml"),
                    line: None,
                    title: format!(
                        "{name}: {unsafe_fns} unsafe fn(s), {unsafe_exprs} unsafe expr(s)"
                    ),
                    description: format!(
                        "Package {name} uses {unsafe_fns} unsafe functions and {unsafe_exprs} unsafe expressions"
                    ),
                    evidence: vec![],
                    covered: false,
                    suggestion: "Audit unsafe blocks for memory safety, add targeted fuzz tests".into(),
                    explanation: None,
                    fix: None,
                });
            }
        }
    }

    Ok(findings)
}
```

- [ ] **Step 3: Register in detectors/mod.rs**

Add:
```rust
pub mod unsafe_reach;
pub use unsafe_reach::UnsafeReachabilityDetector;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-detect -- unsafe_reach`
Expected: All 3 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/apex-detect/src/detectors/unsafe_reach.rs crates/apex-detect/src/detectors/mod.rs
git commit -m "feat(detect): add UnsafeReachabilityDetector — cargo-geiger integration"
```

---

### Task 11: StaticAnalysisDetector (Clippy)

**Files:**
- Create: `crates/apex-detect/src/detectors/static_analysis.rs`
- Modify: `crates/apex-detect/src/detectors/mod.rs`

Runs `cargo clippy --message-format json` and parses diagnostics.

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_clippy_diagnostic() {
        let line = r#"{"reason":"compiler-message","message":{"code":{"code":"clippy::unwrap_used"},"level":"warning","message":"used `unwrap()` on a `Result`","spans":[{"file_name":"src/main.rs","line_start":42,"line_end":42,"column_start":5,"column_end":20}]}}"#;
        let findings = parse_clippy_line(line);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].category, FindingCategory::PanicPath);
        assert_eq!(findings[0].line, Some(42));
        assert!(findings[0].title.contains("clippy::unwrap_used"));
    }

    #[test]
    fn parse_clippy_non_diagnostic_line() {
        let line = r#"{"reason":"build-script-executed"}"#;
        let findings = parse_clippy_line(line);
        assert!(findings.is_empty());
    }

    #[test]
    fn clippy_code_to_category_mapping() {
        assert_eq!(clippy_code_to_category("clippy::unwrap_used"), FindingCategory::PanicPath);
        assert_eq!(clippy_code_to_category("clippy::cast_possible_truncation"), FindingCategory::UndefinedBehavior);
        assert_eq!(clippy_code_to_category("clippy::some_other_lint"), FindingCategory::SecuritySmell);
    }

    #[test]
    fn uses_cargo_subprocess_returns_true() {
        let d = StaticAnalysisDetector::default();
        assert!(d.uses_cargo_subprocess());
    }
}
```

- [ ] **Step 2: Implement StaticAnalysisDetector**

```rust
use apex_core::error::{ApexError, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use uuid::Uuid;

use crate::config::StaticAnalysisConfig;
use crate::context::AnalysisContext;
use crate::finding::{Evidence, Finding, FindingCategory, Severity};
use crate::Detector;

#[derive(Default)]
pub struct StaticAnalysisDetector {
    pub extra_args: Vec<String>,
}

impl StaticAnalysisDetector {
    pub fn new(config: &StaticAnalysisConfig) -> Self {
        StaticAnalysisDetector {
            extra_args: config.clippy_extra_args.clone(),
        }
    }
}

#[async_trait]
impl Detector for StaticAnalysisDetector {
    fn name(&self) -> &str {
        "static-analysis"
    }

    fn uses_cargo_subprocess(&self) -> bool {
        true
    }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        let mut args = vec![
            "clippy".to_string(),
            "--message-format".to_string(),
            "json".to_string(),
            "--".to_string(),
        ];
        args.extend(self.extra_args.clone());

        let output = tokio::process::Command::new("cargo")
            .args(&args)
            .current_dir(&ctx.target_root)
            .output()
            .await
            .map_err(|e| ApexError::Detect(format!("cargo-clippy: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut findings = Vec::new();

        for line in stdout.lines() {
            findings.extend(parse_clippy_line(line));
        }

        Ok(findings)
    }
}

pub fn clippy_code_to_category(code: &str) -> FindingCategory {
    if code.contains("unwrap") || code.contains("expect_used") || code.contains("panic") {
        FindingCategory::PanicPath
    } else if code.contains("cast") || code.contains("truncat") || code.contains("overflow") {
        FindingCategory::UndefinedBehavior
    } else if code.contains("unsafe") {
        FindingCategory::UnsafeCode
    } else {
        FindingCategory::SecuritySmell
    }
}

pub fn parse_clippy_line(line: &str) -> Vec<Finding> {
    let parsed: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return vec![],
    };

    if parsed.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
        return vec![];
    }

    let message = &parsed["message"];
    let code = message["code"]["code"].as_str().unwrap_or("");
    if code.is_empty() {
        return vec![];
    }

    let msg_text = message["message"].as_str().unwrap_or("");
    let level = message["level"].as_str().unwrap_or("warning");

    let spans = message["spans"].as_array();
    let (file, line_num) = spans
        .and_then(|s| s.first())
        .map(|span| {
            let f = span["file_name"].as_str().unwrap_or("unknown");
            let l = span["line_start"].as_u64().unwrap_or(0) as u32;
            (PathBuf::from(f), Some(l))
        })
        .unwrap_or((PathBuf::from("unknown"), None));

    let severity = match level {
        "error" => Severity::High,
        "warning" => Severity::Medium,
        _ => Severity::Low,
    };

    let category = clippy_code_to_category(code);

    vec![Finding {
        id: Uuid::new_v4(),
        detector: "static-analysis".into(),
        severity,
        category,
        file,
        line: line_num,
        title: format!("{code}: {msg_text}"),
        description: msg_text.into(),
        evidence: vec![Evidence::StaticAnalysis {
            tool: "clippy".into(),
            rule_id: code.into(),
            sarif: serde_json::Value::Null,
        }],
        covered: false,
        suggestion: format!("Address clippy lint: {code}"),
        explanation: None,
        fix: None,
    }]
}
```

- [ ] **Step 3: Register in detectors/mod.rs**

Add:
```rust
pub mod static_analysis;
pub use static_analysis::StaticAnalysisDetector;
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-detect -- static_analysis`
Expected: All 4 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/apex-detect/src/detectors/static_analysis.rs crates/apex-detect/src/detectors/mod.rs
git commit -m "feat(detect): add StaticAnalysisDetector — clippy integration"
```

---

## Chunk 3: Pipeline Factory + CLI Integration

### Task 12: Pipeline factory (`from_config`)

**Files:**
- Modify: `crates/apex-detect/src/pipeline.rs`

- [ ] **Step 1: Add `from_config` constructor test**

In pipeline.rs tests, add:

```rust
    #[test]
    fn from_config_enables_panic_by_default() {
        let cfg = DetectConfig::default();
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        // Default enables: panic, deps, unsafe, static
        assert_eq!(pipeline.detectors.len(), 4);
    }

    #[test]
    fn from_config_respects_enabled_list() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["panic".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "panic-pattern");
    }

    #[test]
    fn from_config_skips_unsafe_for_non_rust() {
        let cfg = DetectConfig::default();
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Python);
        // Should not include unsafe-reachability for Python
        assert!(pipeline.detectors.iter().all(|d| d.name() != "unsafe-reachability"));
    }
```

- [ ] **Step 2: Implement `from_config`**

Add to `DetectorPipeline`:

```rust
use apex_core::types::Language;
use crate::config::DetectConfig;
use crate::detectors::*;

impl DetectorPipeline {
    pub fn from_config(cfg: &DetectConfig, lang: Language) -> Self {
        let mut detectors: Vec<Box<dyn Detector>> = Vec::new();

        if cfg.enabled.contains(&"panic".to_string()) {
            detectors.push(Box::new(PanicPatternDetector));
        }
        if cfg.enabled.contains(&"unsafe".to_string()) && lang == Language::Rust {
            detectors.push(Box::new(UnsafeReachabilityDetector));
        }
        if cfg.enabled.contains(&"deps".to_string()) {
            detectors.push(Box::new(DependencyAuditDetector));
        }
        if cfg.enabled.contains(&"static".to_string()) {
            detectors.push(Box::new(StaticAnalysisDetector::new(&cfg.static_analysis)));
        }

        Self { detectors }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-detect -- pipeline`
Expected: All 7 tests pass (4 existing + 3 new)

- [ ] **Step 4: Commit**

```bash
git add crates/apex-detect/src/pipeline.rs
git commit -m "feat(detect): add DetectorPipeline::from_config factory"
```

---

### Task 13: `apex audit` CLI subcommand

**Files:**
- Modify: `crates/apex-cli/Cargo.toml`
- Modify: `crates/apex-cli/src/main.rs`

- [ ] **Step 1: Add `apex-detect` dependency to apex-cli**

In `crates/apex-cli/Cargo.toml`, add to `[dependencies]`:

```toml
apex-detect = { path = "../apex-detect" }
```

- [ ] **Step 2: Add Audit subcommand to CLI definition**

In `main.rs`, add to the `Commands` enum:

```rust
    /// Run security and bug detection analysis.
    Audit(AuditArgs),
```

And add the args struct:

```rust
#[derive(Parser)]
struct AuditArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    lang: LangArg,

    /// Comma-separated list of detectors to run.
    #[arg(long, value_delimiter = ',')]
    detectors: Option<Vec<String>>,

    /// Minimum severity to report.
    #[arg(long, default_value = "low")]
    severity_threshold: String,

    /// Output format: text (human-readable) or json (machine-readable).
    #[arg(long, default_value = "text")]
    output_format: OutputFormat,
}
```

- [ ] **Step 3: Implement the audit command handler**

Add a new function. Note: takes `&ApexConfig` to match existing CLI patterns (`run`, `ratchet` both take `&ApexConfig`).

```rust
async fn run_audit(args: AuditArgs, cfg: &ApexConfig) -> Result<()> {
    use apex_detect::{AnalysisContext, DetectConfig, DetectorPipeline, Severity};
    use apex_coverage::CoverageOracle;
    use std::sync::Arc;

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;

    // Build detect config from apex.toml + CLI overrides
    let mut detect_cfg = DetectConfig::default();
    // Apply settings from apex.toml [detect] section
    if !cfg.detect.enabled.is_empty() {
        detect_cfg.enabled = cfg.detect.enabled.clone();
    }
    detect_cfg.severity_threshold = cfg.detect.severity_threshold.clone();
    detect_cfg.per_detector_timeout_secs = cfg.detect.per_detector_timeout_secs;
    // CLI --detectors overrides config file
    if let Some(detectors) = args.detectors {
        detect_cfg.enabled = detectors;
    }

    // Build source cache by reading .rs/.py/.js files
    let source_cache = build_source_cache(&target_path, lang);

    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: std::collections::HashMap::new(),
        known_bugs: vec![],
        source_cache,
        fuzz_corpus: None,
        config: detect_cfg.clone(),
    };

    let pipeline = DetectorPipeline::from_config(&detect_cfg, lang);
    let report = pipeline.run_all(&ctx).await;

    let min_severity = match args.severity_threshold.as_str() {
        "critical" => Severity::Critical,
        "high" => Severity::High,
        "medium" => Severity::Medium,
        "low" => Severity::Low,
        _ => Severity::Info,
    };

    match args.output_format {
        OutputFormat::Json => {
            let json = serde_json::to_string_pretty(&report)?;
            println!("{json}");
        }
        OutputFormat::Text => {
            let summary = report.security_summary();
            println!("\nAPEX Security Audit — {}\n", target_path.display());
            println!(
                "  CRITICAL  {}      HIGH  {}      MEDIUM  {}      LOW  {}\n",
                summary.critical, summary.high, summary.medium, summary.low
            );

            for f in &report.findings {
                if f.severity.rank() > min_severity.rank() {
                    continue;
                }
                let sev = format!("{:?}", f.severity).to_uppercase();
                println!(
                    "{:<9} {}:{} — {}",
                    sev,
                    f.file.display(),
                    f.line.map(|l| l.to_string()).unwrap_or_default(),
                    f.title
                );
                println!("          [{}] {}", f.detector, f.description);
                println!("          Suggestion: {}\n", f.suggestion);
            }

            let status_line: String = report
                .detector_status
                .iter()
                .map(|(name, ok)| {
                    if *ok {
                        format!("{name} OK")
                    } else {
                        format!("{name} FAIL")
                    }
                })
                .collect::<Vec<_>>()
                .join("  ");
            println!("Detectors: {status_line}");
        }
    }

    Ok(())
}

fn build_source_cache(
    target: &std::path::Path,
    lang: Language,
) -> std::collections::HashMap<PathBuf, String> {
    let extensions: &[&str] = match lang {
        Language::Rust => &["rs"],
        Language::Python => &["py"],
        Language::JavaScript => &["js", "ts"],
        Language::Java => &["java"],
        Language::C => &["c", "h"],
        Language::Wasm => &["rs", "wat"],
    };

    let mut cache = std::collections::HashMap::new();

    if let Ok(entries) = walkdir(target, extensions) {
        for path in entries {
            if let Ok(content) = std::fs::read_to_string(&path) {
                // Use relative path
                let rel = path.strip_prefix(target).unwrap_or(&path).to_path_buf();
                cache.insert(rel, content);
            }
        }
    }

    cache
}

fn walkdir(root: &std::path::Path, extensions: &[&str]) -> std::io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk_recursive(root, extensions, &mut files)?;
    Ok(files)
}

fn walk_recursive(
    dir: &std::path::Path,
    extensions: &[&str],
    files: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    if dir.is_dir() {
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden dirs and target/
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name.starts_with('.') || name == "target" || name == "node_modules" {
                    continue;
                }
                walk_recursive(&path, extensions, files)?;
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if extensions.contains(&ext) {
                    files.push(path);
                }
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Wire it into main match**

In the main `match` on `cli.command`, add:

In `main()`, add to the existing match on `cli.command` (after `Commands::Doctor`):

```rust
        Commands::Audit(args) => run_audit(args, &cfg).await,
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build -p apex-cli`
Expected: Compiles successfully

- [ ] **Step 6: Commit**

```bash
git add crates/apex-cli/Cargo.toml crates/apex-cli/src/main.rs
git commit -m "feat(cli): add 'apex audit' subcommand"
```

---

### Task 14: Wire detectors into `apex run --strategy agent`

**Files:**
- Modify: `crates/apex-cli/src/main.rs`

- [ ] **Step 1: Refactor `print_agent_json_report` to return the report**

The existing function at `crates/apex-cli/src/main.rs:524` builds and prints the report in one shot. Refactor it into two functions: one that builds, one that prints. This allows us to mutate the report before printing.

Rename the existing function and split:

```rust
/// Build the rich agent-format gap report (without printing).
fn build_agent_report(
    oracle: &CoverageOracle,
    file_paths: &HashMap<u64, PathBuf>,
    target_path: &std::path::Path,
) -> apex_core::agent_report::AgentGapReport {
    use apex_core::agent_report::build_agent_gap_report;

    let total = oracle.total_count();
    let covered = oracle.covered_count();
    let uncovered = oracle.uncovered_branches();

    // Build source cache: read source lines for uncovered branch locations
    let mut source_cache: HashMap<(u64, u32), String> = HashMap::new();
    for branch in &uncovered {
        if source_cache.contains_key(&(branch.file_id, branch.line)) {
            continue;
        }
        if let Some(path) = file_paths.get(&branch.file_id) {
            let full_path = target_path.join(path);
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let lines: Vec<&str> = content.lines().collect();
                let start = (branch.line as usize).saturating_sub(6);
                let end = (branch.line as usize + 5).min(lines.len());
                for (i, line) in lines[start..end].iter().enumerate() {
                    let line_num = (start + i + 1) as u32;
                    source_cache
                        .entry((branch.file_id, line_num))
                        .or_insert_with(|| line.to_string());
                }
            }
        }
    }

    build_agent_gap_report(total, covered, &uncovered, file_paths, &source_cache)
}

/// Print rich agent-format JSON gap report for external agent consumption.
fn print_agent_json_report(
    oracle: &CoverageOracle,
    file_paths: &HashMap<u64, PathBuf>,
    target_path: &std::path::Path,
) {
    let report = build_agent_report(oracle, file_paths, target_path);
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("{{\"error\": \"failed to serialize report: {e}\"}}"),
    }
}
```

- [ ] **Step 2: Find the call site and wire in detector enrichment**

Find where `print_agent_json_report` is called in the agent strategy path. Replace that call with:

```rust
    // Build report, enrich with detectors, then print
    let mut report = build_agent_report(&oracle, &instrumented.file_paths, &target_path);

    // Run tier 1 detectors
    let detect_cfg = apex_detect::DetectConfig::default();
    let file_source_cache = build_source_cache(&target_path, lang);

    let detect_ctx = apex_detect::AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: oracle.clone(),
        file_paths: instrumented.file_paths.clone(),
        known_bugs: vec![],
        source_cache: file_source_cache,
        fuzz_corpus: None,
        config: detect_cfg.clone(),
    };

    let pipeline = apex_detect::DetectorPipeline::from_config(&detect_cfg, lang);
    let analysis = pipeline.run_all(&detect_ctx).await;

    report.findings = Some(serde_json::to_value(&analysis.findings).unwrap_or_default());
    report.security_summary = Some(serde_json::to_value(&analysis.security_summary()).unwrap_or_default());

    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("{{\"error\": \"failed to serialize report: {e}\"}}"),
    }
```

Note: The `build_source_cache` helper function is already defined in Task 13. It builds a `HashMap<PathBuf, String>` of all source files.

- [ ] **Step 3: Verify it compiles**

Run: `cargo build -p apex-cli`
Expected: Compiles

- [ ] **Step 4: Run workspace tests**

Run: `cargo test --workspace`
Expected: All tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/apex-cli/src/main.rs
git commit -m "feat(cli): enrich agent report with detector findings"
```

---

### Task 15: Final verification

- [ ] **Step 1: Run full workspace test suite**

Run: `cargo test --workspace`
Expected: All tests pass, no regressions

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Verify audit command works end-to-end**

Run: `cargo run --bin apex -- audit --target /Users/ad/prj/bcov --lang rust --output-format json 2>/dev/null | head -20`
Expected: JSON output with findings array

- [ ] **Step 4: Commit any final fixes if needed**

```bash
git add crates/apex-detect/ crates/apex-core/ crates/apex-cli/
git commit -m "chore(detect): final cleanup and linting"
```
