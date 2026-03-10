# APEX Analytics Layer — Bug & Security Detection Design

## Goal

Add a pluggable analytics layer to APEX that detects bugs, security vulnerabilities, and code quality issues by cross-referencing coverage data with multiple detection techniques. Raw findings are produced by mechanical detectors; an LLM agent interprets and prioritizes them.

## Architecture

**Approach:** Offline interpretation (Approach 1). Detectors run after coverage measurement, produce `Vec<Finding>`, findings are serialized into the enriched agent JSON report, and the external agent loop (Claude Code) interprets them.

**Detector model:** Trait-based plugin system (`Detector` trait). Each detector is a separate struct implementing a common interface. The `DetectorPipeline` orchestrates all enabled detectors concurrently via `tokio::join_all`.

**Integration points:**
- `apex run --strategy agent` — tier 1 detectors run automatically, findings included in JSON
- `apex audit` — dedicated subcommand, runs full detector battery
- `apex.toml [detect]` — configuration for detector selection and tuning

## Tech Stack

- New crate: `apex-detect` (pure Rust, no heavy deps in core)
- External tools invoked as subprocesses: cargo-geiger, cargo-audit, cargo-clippy, pip-audit, npm-audit
- SARIF import for external static analysis (semgrep, CodeQL)
- Optional: Anthropic API for LLM oracle detector

---

## Core Abstractions

### Detector Trait

```rust
// apex-detect/src/lib.rs

#[async_trait]
pub trait Detector: Send + Sync {
    /// Human-readable name (e.g., "sanitizer", "unsafe-reachability")
    fn name(&self) -> &str;

    /// Run analysis given coverage + execution context.
    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>>;
}
```

### AnalysisContext

Shared context passed to all detectors:

```rust
#[derive(Debug, Clone)]
pub struct AnalysisContext {
    pub target_root: PathBuf,
    pub language: Language,
    pub oracle: Arc<CoverageOracle>,
    pub file_paths: HashMap<u64, PathBuf>,
    pub known_bugs: Vec<BugReport>,         // snapshot from apex-core::types, no dep on apex-agent
    pub source_cache: HashMap<PathBuf, String>,
    pub fuzz_corpus: Option<PathBuf>,       // path to fuzz corpus if available
    pub config: DetectConfig,               // from apex.toml [detect]
}
```

**Why `Vec<BugReport>` instead of `BugLedger`:** `BugReport` lives in `apex-core::types` (lightweight). A hypothetical `BugLedger` would live in `apex-agent` (heavyweight crate with LLM deps). Using a simple `Vec<BugReport>` snapshot avoids pulling `apex-agent` as a dependency of `apex-detect`.

### Finding

Every detector produces the same output type:

```rust
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
    pub covered: bool,           // is this code path covered by tests?
    pub suggestion: String,
    pub explanation: Option<String>,  // LLM-generated (populated by LlmOracleDetector)
    pub fix: Option<Fix>,            // LLM-generated (populated by LlmOracleDetector)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity { Critical, High, Medium, Low, Info }

impl Severity {
    /// Numeric rank for sorting: lower = more severe (Critical=0, Info=4).
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

### SecuritySummary

```rust
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

### AnalysisReport

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalysisReport {
    pub findings: Vec<Finding>,
    pub detector_status: Vec<(String, bool)>,
}

impl AnalysisReport {
    pub fn security_summary(&self) -> SecuritySummary { /* count by severity */ }
}
```

**Key design decisions:**
- `covered: bool` on every finding — uncovered + finding = higher priority
- Evidence is typed — the interpreting agent reasons about evidence kind
- FindingCategory is a fixed enum — enables structured filtering/aggregation
- Severity is independent of category — a `PanicPath` can be Critical (in auth code) or Low (in debug logging)
- All types derive `Serialize`/`Deserialize` for JSON round-tripping

---

## Detector Implementations

### Tier 1 — Ships First (no external API keys, fast)

#### 1. UnsafeReachabilityDetector (Rust-only)

- Runs `cargo geiger --output-format json`
- Cross-references unsafe blocks with coverage oracle reachability
- Severity: uncovered + reachable unsafe = High; covered + tested = Info
- Category: `UnsafeCode`
- Evidence: `UnsafeBlock { file, line_range, reason }`

#### 2. DependencyAuditDetector (all languages)

- Rust: `cargo audit --json`
- Python: `pip-audit --format json`
- JS: `npm audit --json`
- Maps CVEs to `DependencyVuln` findings
- Cross-references: if vulnerable function is on covered path, severity bumps
- Evidence: CVE ID, CVSS score, affected/fixed versions

#### 3. SanitizerReplayDetector (Rust/C)

- SAND-inspired (ICSE 2025): replay coverage-increasing fuzz corpus inputs through sanitizer builds
- Compiles target with `-fsanitize=address,undefined` (reuses `build_with_sancov` pattern)
- Only replays top N% interesting inputs (configurable, default 1%)
- Categories: `MemorySafety`, `UndefinedBehavior`
- Evidence: `SanitizerReport { sanitizer, stderr }`
- Only runs when fuzz corpus exists

#### 4. StaticAnalysisDetector (multi-language)

- Rust: `cargo clippy --message-format json` — parses diagnostics
- Multi-lang: imports SARIF files from `--sarif-input <path>`
- Cross-references warnings with coverage data
- Clippy lint ID mapped to FindingCategory (e.g., `clippy::unwrap_used` -> PanicPath)
- Evidence: `StaticAnalysis { tool, rule_id, sarif }`

#### 5. PanicPatternDetector (Rust-focused, works for all)

- Source-level pattern scan — zero external tools
- Patterns: `.unwrap()`, `.expect()`, `panic!()`, `unreachable!()`, `todo!()`, unchecked indexing, unchecked arithmetic
- Cross-references with coverage: pattern on uncovered path = Medium; on covered path without panic test = High
- Category: `PanicPath`
- Evidence: `CoverageGap` + source context

### Tier 2 — Higher Value, More Complexity

#### 6. LlmOracleDetector (the "explain & fix" agent)

- The interpretation layer. Takes ALL other detectors' findings + coverage gaps and produces human-readable explanations with actionable fixes.
- **Three responsibilities:**
  1. **Explain** — For each finding, generate a plain-language explanation: what the issue is, why it matters, what could go wrong in production. No jargon — a junior dev should understand it.
  2. **Fix** — For each finding, suggest a concrete fix. Not "add validation" but actual code: a patch, a dependency upgrade command, or a test case to write. The fix goes into a new `explanation` and `fix` field on the Finding.
  3. **Correlate** — Cross-reference findings from different detectors. E.g., "clippy warns about unwrap() at line 89, AND this line is on an uncovered path, AND cargo-audit shows the dep used here has a CVE — this is a compound risk."
- Batches by function (one prompt per function, not per branch)
- Prompt includes: source context, coverage status, all findings for that function, related dependency info
- Populates the `explanation: Option<String>` and `fix: Option<Fix>` fields on each Finding (these fields are defined in the Finding struct above, initially `None`, set by this detector)
- Gated: `--llm-analyze` flag, requires `ANTHROPIC_API_KEY`
- Categories: `SecuritySmell`, `LogicBug` (plus re-categorizes other findings when LLM disagrees)
- Uses cheaper model by default (configurable)
- **Runs last** — after all other detectors, so it has full context

#### 7. DifferentialDetector

- Compares behavior: current version vs `--diff-base <git-ref>`
- Runs both versions on fuzz corpus + test inputs in sandbox
- Behavioral difference on same input = `LogicBug`
- Evidence: `DiffBehavior { input, expected, actual }`
- Checks out old version in git worktree

#### 8. PropertyDetector

- User-defined invariants in `apex.toml [[detect.properties]]`
- Generates property-based test inputs using fuzz corpus as seed
- Checks invariants, reports violations as `LogicBug`
- Initial: string equality, exit code, regex on output
- Future: proptest strategy integration

---

## DetectorPipeline

```rust
// apex-detect/src/pipeline.rs

pub struct DetectorPipeline {
    detectors: Vec<Box<dyn Detector>>,
}

impl DetectorPipeline {
    pub fn from_config(cfg: &DetectConfig, lang: Language) -> Self {
        let mut detectors: Vec<Box<dyn Detector>> = Vec::new();

        // Tier 1 — pure source analysis (no subprocesses, safe to run concurrently)
        if cfg.enabled.contains("panic") {
            detectors.push(Box::new(PanicPatternDetector));
        }

        // Tier 1 — subprocess-based (run sequentially within their group, see below)
        if cfg.enabled.contains("unsafe") && lang == Language::Rust {
            detectors.push(Box::new(UnsafeReachabilityDetector));
        }
        if cfg.enabled.contains("deps") {
            detectors.push(Box::new(DependencyAuditDetector));
        }
        if cfg.enabled.contains("static") {
            detectors.push(Box::new(StaticAnalysisDetector::new(cfg)));
        }
        if cfg.enabled.contains("sanitizer") {
            detectors.push(Box::new(SanitizerReplayDetector::new(cfg)));
        }

        // Tier 2 (opt-in)
        if cfg.enabled.contains("llm") {
            detectors.push(Box::new(LlmOracleDetector::new(cfg)));
        }
        if cfg.enabled.contains("diff") {
            detectors.push(Box::new(DifferentialDetector::new(cfg)));
        }
        if cfg.enabled.contains("property") {
            detectors.push(Box::new(PropertyDetector::new(cfg)));
        }

        Self { detectors }
    }

    pub async fn run_all(&self, ctx: &AnalysisContext) -> AnalysisReport {
        // Split detectors into two groups:
        //   1. Pure analysis (PanicPattern, LlmOracle) — safe to run concurrently
        //   2. Cargo-subprocess (geiger, audit, clippy, sanitizer) — run sequentially
        //      to avoid Cargo.lock contention (concurrent `cargo` invocations fail)
        //
        // Pure detectors run concurrently via join_all.
        // Subprocess detectors run sequentially in their own task.
        // Both groups run in parallel with each other.

        let per_detector_timeout = ctx.config.per_detector_timeout_secs
            .map(|s| std::time::Duration::from_secs(s))
            .unwrap_or(std::time::Duration::from_secs(300)); // default 5 min

        let (pure, subprocess): (Vec<_>, Vec<_>) = self.detectors.iter()
            .partition(|d| !d.uses_cargo_subprocess());

        // Run pure detectors concurrently with timeout
        let pure_results = futures::future::join_all(
            pure.iter().map(|d| {
                let timeout = per_detector_timeout;
                async move {
                    let name = d.name().to_string();
                    match tokio::time::timeout(timeout, d.analyze(ctx)).await {
                        Ok(result) => (name, result),
                        Err(_) => (name.clone(), Err(ApexError::Timeout(timeout.as_millis() as u64))),
                    }
                }
            })
        );

        // Run subprocess detectors sequentially (Cargo.lock contention)
        let subprocess_results = async {
            let mut results = Vec::new();
            for d in &subprocess {
                let name = d.name().to_string();
                let result = match tokio::time::timeout(per_detector_timeout, d.analyze(ctx)).await {
                    Ok(r) => r,
                    Err(_) => Err(ApexError::Timeout(per_detector_timeout.as_millis() as u64)),
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
                    tracing::warn!(detector = %name, error = %e, "detector failed");
                    detector_status.push((name, false));
                }
            }
        }

        deduplicate(&mut findings);
        // Sort: most severe first, then uncovered before covered
        findings.sort_by_key(|f| (f.severity.rank(), f.covered as u8));

        AnalysisReport { findings, detector_status }
    }
}
```

### Detector Trait (extended)

The `Detector` trait gains one method for subprocess classification:

```rust
#[async_trait]
pub trait Detector: Send + Sync {
    fn name(&self) -> &str;
    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>>;

    /// Does this detector invoke `cargo` subprocesses? If true, the pipeline
    /// runs it sequentially with other subprocess detectors to avoid Cargo.lock
    /// contention. Default: false.
    fn uses_cargo_subprocess(&self) -> bool { false }
}
```

Detectors that return `true`: `UnsafeReachabilityDetector` (cargo-geiger), `DependencyAuditDetector` (cargo-audit), `StaticAnalysisDetector` (cargo-clippy), `SanitizerReplayDetector` (cargo build).

### Deduplication

```rust
fn deduplicate(findings: &mut Vec<Finding>) {
    // Group by (file, line, category). For each group:
    // - Keep the finding with the highest severity (lowest rank)
    // - Merge evidence arrays from all duplicates into the kept finding
    let mut seen: HashMap<(PathBuf, Option<u32>, FindingCategory), usize> = HashMap::new();
    let mut merged = Vec::new();

    for finding in findings.drain(..) {
        let key = (finding.file.clone(), finding.line, finding.category.clone());
        if let Some(&idx) = seen.get(&key) {
            // Merge: keep higher severity, combine evidence
            if finding.severity.rank() < merged[idx].severity.rank() {
                merged[idx].severity = finding.severity;
                merged[idx].title = finding.title;
                merged[idx].description = finding.description;
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

---

## Report Enrichment

### Agent JSON Report (enriched)

Two new **optional** top-level fields added to existing `AgentGapReport`:

```rust
// apex-core/src/agent_report.rs — existing struct, new fields only
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentGapReport {
    pub summary: GapSummary,
    pub gaps: Vec<GapEntry>,
    pub blocked: Vec<BlockedEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings: Option<Vec<Finding>>,          // NEW — from apex-detect
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_summary: Option<SecuritySummary>, // NEW — from apex-detect
}
```

`Option<>` ensures backward compatibility: existing JSON without these fields still deserializes correctly.

```json
{
  "summary": { "total_branches": 1406, "covered_branches": 1355, ... },
  "gaps": [ ... ],
  "findings": [
    {
      "id": "...",
      "detector": "unsafe-reachability",
      "severity": "high",
      "category": "unsafe_code",
      "file": "src/parser.rs",
      "line": 142,
      "title": "Reachable unsafe block with no test coverage",
      "description": "raw pointer dereference in parse_raw(), reachable from public API",
      "evidence": [{ "type": "UnsafeBlock", "line_range": [140, 155], "reason": "ptr::read" }],
      "covered": false,
      "suggestion": "Add test exercising parse_raw() with malformed input",
      "explanation": "ptr::read() on user-supplied bytes without length check. If input < size_of::<Header>(), reads uninitialized memory. Reachable from public parse() API.",
      "fix": {
        "type": "CodePatch",
        "file": "src/parser.rs",
        "diff": "--- a/src/parser.rs\n+++ b/src/parser.rs\n@@ -140,6 +140,9 @@\n pub fn parse_raw(buf: &[u8]) -> Result<Header, ParseError> {\n+    if buf.len() < std::mem::size_of::<Header>() {\n+        return Err(ParseError::BufferTooShort);\n+    }\n     unsafe { std::ptr::read(buf.as_ptr() as *const Header) }"
      }
    }
  ],
  "security_summary": {
    "critical": 0,
    "high": 3,
    "medium": 7,
    "low": 12,
    "detectors_run": ["unsafe-reachability", "dependency-audit", "panic-pattern", "clippy"],
    "top_risk": "src/parser.rs — 2 uncovered unsafe blocks on reachable paths"
  }
}
```

### Human-Readable Audit Output

Without `--llm-analyze`:
```
APEX Security Audit — /path/to/project

  CRITICAL  0      HIGH  3      MEDIUM  7      LOW  12

HIGH  src/parser.rs:142 — Reachable unsafe block, no test coverage
      [unsafe-reachability] raw pointer dereference in parse_raw()
      Suggestion: Add test with malformed input targeting parse_raw()

HIGH  src/crypto.rs:89 — Uncovered error path in key validation
      [panic-pattern] .unwrap() on decrypt() result, no test for Err case
      Suggestion: Test decrypt() with invalid key material

HIGH  Cargo.toml — openssl 0.10.38 (RUSTSEC-2023-0044, CVSS 7.5)
      [dependency-audit] Fix available: upgrade to >= 0.10.55

MEDIUM  src/eval.rs:67 — Clippy warning on covered+untested path
        [static-analysis] clippy::cast_possible_truncation
        Evidence: u64 -> u32 cast, branch at line 67 not covered
...

Detectors: unsafe-reachability OK  deps OK  panic-pattern OK  clippy OK  sanitizer SKIP (no corpus)
```

With `--llm-analyze` (LLM agent explains and offers fixes):
```
APEX Security Audit — /path/to/project

  CRITICAL  0      HIGH  3      MEDIUM  7      LOW  12

HIGH  src/parser.rs:142 — Reachable unsafe block, no test coverage
      [unsafe-reachability] raw pointer dereference in parse_raw()

      Explanation: The parse_raw() function uses ptr::read() to interpret
      user-supplied bytes as a Header struct. If the input buffer is shorter
      than size_of::<Header>(), this reads uninitialized memory. The function
      is called from the public parse() API with no length check beforehand.
      No existing test exercises this path with short input.

      Fix (code patch):
        --- a/src/parser.rs
        +++ b/src/parser.rs
        @@ -140,6 +140,9 @@
         pub fn parse_raw(buf: &[u8]) -> Result<Header, ParseError> {
        +    if buf.len() < std::mem::size_of::<Header>() {
        +        return Err(ParseError::BufferTooShort);
        +    }
             unsafe { std::ptr::read(buf.as_ptr() as *const Header) }

      Fix (test case):
        #[test]
        fn parse_raw_short_input() {
            assert!(matches!(parse_raw(&[0u8; 2]), Err(ParseError::BufferTooShort)));
        }

HIGH  Cargo.toml — openssl 0.10.38 (RUSTSEC-2023-0044, CVSS 7.5)
      [dependency-audit]

      Explanation: This version of openssl-rs has a known vulnerability where
      X.509 certificate verification can be bypassed with a crafted certificate
      chain. If your application verifies TLS certificates (it does — see
      src/client.rs:34), an attacker could MITM connections.

      Fix: cargo update -p openssl --precise 0.10.55
...

Detectors: unsafe-reachability OK  deps OK  panic-pattern OK  clippy OK  llm-oracle OK
```

---

## CLI Integration

### `apex audit` subcommand

```
apex audit --target <path> --lang <lang>
    [--detectors <list>]         # comma-separated, default from apex.toml
    [--severity-threshold <sev>] # only report >= this severity (default: low)
    [--sarif-input <path>]       # import external SARIF files
    [--diff-base <git-ref>]      # enable differential detector
    [--llm-analyze]              # enable LLM oracle (requires ANTHROPIC_API_KEY)
    [--output-format text|json]  # default: text
    [--output <dir>]             # save findings to directory
```

### `apex run --strategy agent` enrichment

- Tier 1 detectors run automatically after coverage measurement
- Findings included in agent JSON output alongside gaps
- No extra flags needed
- Sanitizer replay only if fuzz corpus exists

---

## Configuration (`apex.toml`)

```toml
[detect]
enabled = ["unsafe", "deps", "panic", "static"]
severity_threshold = "low"
per_detector_timeout_secs = 300   # 5 min per detector, 0 = no timeout

[detect.sanitizer]
replay_top_percent = 1
sanitizers = ["address", "undefined"]

[detect.static]
clippy_extra_args = ["-W", "clippy::pedantic"]
sarif_paths = []

[detect.llm]
enabled = false
batch_size = 10
model = "claude-sonnet-4-6"

[detect.diff]
base_ref = ""

[[detect.properties]]
name = "parse-roundtrip"
check = "parse(display(v)) == v"
target = "src/display.rs"
```

### DetectConfig struct

```rust
// apex-detect/src/config.rs

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DetectConfig {
    #[serde(default = "default_enabled")]
    pub enabled: Vec<String>,
    #[serde(default = "default_severity")]
    pub severity_threshold: String,
    #[serde(default)]
    pub per_detector_timeout_secs: Option<u64>,

    #[serde(default)]
    pub sanitizer: SanitizerConfig,
    #[serde(default)]
    pub r#static: StaticConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub diff: DiffConfig,
    #[serde(default)]
    pub properties: Vec<PropertyConfig>,
}

fn default_enabled() -> Vec<String> {
    vec!["unsafe".into(), "deps".into(), "panic".into(), "static".into()]
}
fn default_severity() -> String { "low".into() }
```

This struct lives in `apex-detect` (not `apex-core`) since it's only consumed by the detection pipeline. The CLI loads it from `apex.toml` and passes it into `AnalysisContext`.

---

## Crate Structure

```
crates/apex-detect/
  src/
    lib.rs              # Detector trait, Finding, AnalysisContext, re-exports
    finding.rs          # Finding, Severity, FindingCategory, Evidence types
    pipeline.rs         # DetectorPipeline orchestration + dedup
    config.rs           # DetectConfig deserialization from apex.toml
    detectors/
      mod.rs
      unsafe_reach.rs   # UnsafeReachabilityDetector
      dep_audit.rs      # DependencyAuditDetector
      sanitizer.rs      # SanitizerReplayDetector
      static_analysis.rs # StaticAnalysisDetector (clippy + SARIF import)
      panic_pattern.rs  # PanicPatternDetector
      llm_oracle.rs     # LlmOracleDetector (tier 2)
      differential.rs   # DifferentialDetector (tier 2)
      property.rs       # PropertyDetector (tier 2)
```

Dependencies: `apex-core` (types, error, BugReport), `apex-coverage` (CoverageOracle), `apex-sandbox` (for sanitizer replay). No dependency on `apex-fuzz`, `apex-concolic`, or `apex-agent`.

### Error Handling

Add to `apex-core/src/error.rs`:

```rust
#[derive(Debug, Error)]
pub enum ApexError {
    // ... existing variants ...

    #[error("Detector error: {0}")]
    Detect(String),
}
```

All `Detector::analyze()` implementations return `Result<Vec<Finding>>` using `ApexError`. Subprocess failures map to `ApexError::Detect(format!("{tool}: {stderr}"))`. Timeouts map to `ApexError::Timeout(ms)`.

---

## Testing Strategy

- Each detector has unit tests with mock `AnalysisContext` (fake oracle, fake source cache)
- `PanicPatternDetector` and `UnsafeReachabilityDetector` are pure source analysis — easiest to test
- `DependencyAuditDetector` and `StaticAnalysisDetector` mock subprocess output (JSON fixtures)
- `SanitizerReplayDetector` needs integration test with actual ASan build
- `DetectorPipeline` tested with mock detectors returning canned findings — verifies dedup, sorting, error handling
- `LlmOracleDetector` tested with mock HTTP responses

---

## Implementation Priority

1. `Finding` + `Detector` trait + `DetectorPipeline` + `AnalysisContext` (foundation)
2. `PanicPatternDetector` (zero deps, immediate value)
3. `DependencyAuditDetector` (cargo-audit integration)
4. `UnsafeReachabilityDetector` (cargo-geiger integration)
5. `StaticAnalysisDetector` (clippy + SARIF)
6. `apex audit` CLI subcommand
7. Agent report enrichment (findings + security_summary in JSON)
8. `SanitizerReplayDetector` (needs build infrastructure)
9. `LlmOracleDetector` (tier 2)
10. `DifferentialDetector` (tier 2)
11. `PropertyDetector` (tier 2)
