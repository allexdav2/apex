<!-- status: DONE -->

# Expansion Tier 1 Integration Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the 4 existing-but-unintegrated Tier 1 expansion detectors (C35 secret-scan, C30 license-scan, C50 flag-hygiene, C27 api-diff) into the pipeline, config, and CLI so they are fully usable.

**Architecture:** All detector code already exists. This plan adds: (1) pipeline registration in `from_config()`, (2) `default_enabled()` entries, (3) a `FeatureFlagHygiene` FindingCategory variant, (4) dedicated CLI subcommands for `secret-scan`, `license-scan`, `flag-hygiene`, and `api-diff`, (5) test fixes for the new detector count.

**Tech Stack:** Rust, clap, async-trait, tokio

---

## File Structure

| Action | File | Responsibility |
|--------|------|---------------|
| Modify | `crates/apex-detect/src/finding.rs` | Add `FeatureFlagHygiene` and `ApiBreakingChange` to `FindingCategory` |
| Modify | `crates/apex-detect/src/config.rs` | Add `license-scan`, `flag-hygiene` to `default_enabled()` |
| Modify | `crates/apex-detect/src/pipeline.rs` | Register `SecretScanDetector`, `LicenseScanDetector`, `FlagHygieneDetector` in `from_config()` |
| Modify | `crates/apex-detect/src/detectors/flag_hygiene.rs` | Use `FindingCategory::FeatureFlagHygiene` instead of `SecuritySmell` |
| Modify | `crates/apex-cli/src/lib.rs` | Add `SecretScan`, `LicenseScan`, `FlagHygiene`, `ApiDiff` subcommands + handlers |

---

## Chunk 1: FindingCategory + Config + Pipeline Registration

### Task 1: Add FindingCategory variants

**Files:**
- Modify: `crates/apex-detect/src/finding.rs:50-65`

- [ ] **Step 1: Add FeatureFlagHygiene and ApiBreakingChange variants**

In `FindingCategory` enum (line 52), add two new variants after `LicenseViolation`:

```rust
pub enum FindingCategory {
    MemorySafety,
    UndefinedBehavior,
    Injection,
    PanicPath,
    UnsafeCode,
    DependencyVuln,
    LogicBug,
    SecuritySmell,
    PathTraversal,
    InsecureConfig,
    HardcodedSecret,
    LicenseViolation,
    FeatureFlagHygiene,
    ApiBreakingChange,
}
```

- [ ] **Step 2: Run tests to verify compilation**

Run: `cargo test -p apex-detect --lib finding -- --no-run 2>&1 | tail -5`
Expected: compiles successfully

- [ ] **Step 3: Commit**

```bash
git add crates/apex-detect/src/finding.rs
git commit -m "feat: add FeatureFlagHygiene and ApiBreakingChange finding categories"
```

---

### Task 2: Update FlagHygieneDetector to use new category

**Files:**
- Modify: `crates/apex-detect/src/detectors/flag_hygiene.rs:171`

- [ ] **Step 1: Replace SecuritySmell with FeatureFlagHygiene**

Change line 171 from:
```rust
                category: FindingCategory::SecuritySmell,
```
to:
```rust
                category: FindingCategory::FeatureFlagHygiene,
```

- [ ] **Step 2: Run flag_hygiene tests**

Run: `cargo test -p apex-detect flag_hygiene`
Expected: all 11 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/apex-detect/src/detectors/flag_hygiene.rs
git commit -m "refactor: use FeatureFlagHygiene category for flag-hygiene detector"
```

---

### Task 3: Add license-scan and flag-hygiene to default_enabled

**Files:**
- Modify: `crates/apex-detect/src/config.rs:3-31`

- [ ] **Step 1: Add entries to default_enabled()**

Add `"license-scan".into()` and `"flag-hygiene".into()` to the `default_enabled()` vec. Place them after the existing `"secret-scan"` entry (line 14):

```rust
fn default_enabled() -> Vec<String> {
    vec![
        "unsafe".into(),
        "deps".into(),
        "panic".into(),
        "static".into(),
        "security".into(),
        "secrets".into(),
        "path-normalize".into(),
        "timeout".into(),
        "session-security".into(),
        "secret-scan".into(),
        "license-scan".into(),
        "flag-hygiene".into(),
        "discarded-async-result".into(),
        // ... rest unchanged
    ]
}
```

- [ ] **Step 2: Update the test that checks default count**

In `config.rs` tests, update `empty_toml_gives_defaults` (line 198-200):
```rust
    #[test]
    fn empty_toml_gives_defaults() {
        let cfg: DetectConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.enabled.len(), 27);  // was 25, +2 new
        assert_eq!(cfg.severity_threshold, "low");
    }
```

Also add a test that the new detectors are in defaults:
```rust
    #[test]
    fn default_config_has_expansion_tier1_detectors() {
        let cfg = DetectConfig::default();
        assert!(cfg.enabled.contains(&"secret-scan".to_string()));
        assert!(cfg.enabled.contains(&"license-scan".to_string()));
        assert!(cfg.enabled.contains(&"flag-hygiene".to_string()));
    }
```

- [ ] **Step 3: Run config tests**

Run: `cargo test -p apex-detect config`
Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/apex-detect/src/config.rs
git commit -m "feat: enable license-scan and flag-hygiene detectors by default"
```

---

### Task 4: Register detectors in pipeline.rs from_config()

**Files:**
- Modify: `crates/apex-detect/src/pipeline.rs:24-103`

- [ ] **Step 1: Add SecretScanDetector, LicenseScanDetector, FlagHygieneDetector registrations**

After the existing `"secrets"` block (line 43-44) and before the Rust self-analysis detectors comment (line 49), add:

```rust
        if cfg.enabled.contains(&"secret-scan".into()) {
            detectors.push(Box::new(SecretScanDetector::new()));
        }
        if cfg.enabled.contains(&"license-scan".into()) {
            detectors.push(Box::new(LicenseScanDetector::enterprise()));
        }
        if cfg.enabled.contains(&"flag-hygiene".into()) {
            detectors.push(Box::new(FlagHygieneDetector::default_max_age()));
        }
```

Note: `SecretScanDetector::new()` takes no args. `LicenseScanDetector::enterprise()` uses enterprise policy by default. `FlagHygieneDetector::default_max_age()` — verify this constructor exists.

- [ ] **Step 2: Update pipeline test for default detector count**

In `pipeline.rs` tests, update `from_config_enables_all_by_default` (line 361-365):
```rust
    #[test]
    fn from_config_enables_all_by_default() {
        let cfg = DetectConfig::default();
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Rust);
        assert_eq!(pipeline.detectors.len(), 18);  // was 15, +3 new
    }
```

Also update `from_config_all_non_rust_language` (line 735-744):
```rust
    #[test]
    fn from_config_all_non_rust_language() {
        let cfg = DetectConfig::default();
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Python);
        assert_eq!(pipeline.detectors.len(), 17);  // was 14, +3 new
    }
```

Add tests for new detector registration:
```rust
    #[test]
    fn from_config_only_secret_scan() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["secret-scan".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Python);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "secret-scan");
    }

    #[test]
    fn from_config_only_license_scan() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["license-scan".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Python);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "license-scan");
    }

    #[test]
    fn from_config_only_flag_hygiene() {
        let mut cfg = DetectConfig::default();
        cfg.enabled = vec!["flag-hygiene".into()];
        let pipeline = DetectorPipeline::from_config(&cfg, Language::Python);
        assert_eq!(pipeline.detectors.len(), 1);
        assert_eq!(pipeline.detectors[0].name(), "flag-hygiene");
    }
```

- [ ] **Step 3: Run pipeline tests**

Run: `cargo test -p apex-detect pipeline`
Expected: all tests pass

- [ ] **Step 4: Run full apex-detect test suite**

Run: `cargo test -p apex-detect`
Expected: all tests pass (300+ tests)

- [ ] **Step 5: Commit**

```bash
git add crates/apex-detect/src/pipeline.rs
git commit -m "feat: register secret-scan, license-scan, flag-hygiene in detector pipeline"
```

---

## Chunk 2: CLI Subcommands

### Task 5: Add SecretScan CLI subcommand

**Files:**
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Add SecretScan to Commands enum**

After `Features(FeaturesArgs)` in the `Commands` enum, add:

```rust
    /// Scan source code for leaked secrets (API keys, tokens, passwords).
    SecretScan(SecretScanArgs),
```

- [ ] **Step 2: Add SecretScanArgs struct**

After the last `*Args` struct definition, add:

```rust
#[derive(Parser)]
pub struct SecretScanArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Shannon entropy threshold for detecting high-entropy strings.
    #[arg(long, default_value = "4.5")]
    pub entropy_threshold: f64,

    /// Output format: text or json.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}
```

- [ ] **Step 3: Add match arm and handler**

In the `match cli.command` block, add:

```rust
        Commands::SecretScan(args) => run_secret_scan(args, cfg).await,
```

Add the handler function:

```rust
async fn run_secret_scan(args: SecretScanArgs, _cfg: &ApexConfig) -> Result<()> {
    use apex_detect::{AnalysisContext, DetectConfig, Detector};
    use apex_detect::detectors::secret_scan::SecretScanDetector;
    use std::sync::Arc;

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    let detector = SecretScanDetector::new();
    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: std::collections::HashMap::new(),
        known_bugs: vec![],
        source_cache,
        fuzz_corpus: None,
        config: DetectConfig::default(),
        runner: Arc::new(apex_core::command::RealCommandRunner),
        cpg: None,
        threat_model: Default::default(),
    };

    let findings = detector.analyze(&ctx).await?;
    print_findings(&findings, &args.output_format, &target_path);
    Ok(())
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p apex-cli 2>&1 | tail -5`
Expected: compiles (may have warnings, no errors)

- [ ] **Step 5: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "feat: add apex secret-scan CLI subcommand (C35)"
```

---

### Task 6: Add LicenseScan CLI subcommand

**Files:**
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Add LicenseScan to Commands enum**

```rust
    /// Scan dependencies for license compliance violations.
    LicenseScan(LicenseScanArgs),
```

- [ ] **Step 2: Add LicenseScanArgs struct**

```rust
#[derive(Parser)]
pub struct LicenseScanArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// License policy: permissive or enterprise.
    #[arg(long, default_value = "enterprise")]
    pub policy: String,

    /// Output format: text or json.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}
```

- [ ] **Step 3: Add match arm and handler**

```rust
        Commands::LicenseScan(args) => run_license_scan(args, cfg).await,
```

Handler:

```rust
async fn run_license_scan(args: LicenseScanArgs, _cfg: &ApexConfig) -> Result<()> {
    use apex_detect::{AnalysisContext, DetectConfig, Detector};
    use apex_detect::detectors::license_scan::{LicenseScanDetector, LicensePolicy};
    use std::sync::Arc;

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    let policy = match args.policy.as_str() {
        "permissive" => LicensePolicy::Permissive,
        _ => LicensePolicy::Enterprise,
    };
    let detector = LicenseScanDetector::new(policy);
    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: std::collections::HashMap::new(),
        known_bugs: vec![],
        source_cache,
        fuzz_corpus: None,
        config: DetectConfig::default(),
        runner: Arc::new(apex_core::command::RealCommandRunner),
        cpg: None,
        threat_model: Default::default(),
    };

    let findings = detector.analyze(&ctx).await?;
    print_findings(&findings, &args.output_format, &target_path);
    Ok(())
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p apex-cli 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "feat: add apex license-scan CLI subcommand (C30)"
```

---

### Task 7: Add FlagHygiene CLI subcommand

**Files:**
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Add FlagHygiene to Commands enum**

```rust
    /// Detect stale, always-on, or dead feature flags.
    FlagHygiene(FlagHygieneArgs),
```

- [ ] **Step 2: Add FlagHygieneArgs struct**

```rust
#[derive(Parser)]
pub struct FlagHygieneArgs {
    /// Path to the target repository.
    #[arg(long, short)]
    pub target: PathBuf,

    /// Programming language of the target.
    #[arg(long, short, value_enum)]
    pub lang: LangArg,

    /// Maximum flag age in days before marking stale.
    #[arg(long, default_value = "90")]
    pub max_age: u64,

    /// Output format: text or json.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}
```

- [ ] **Step 3: Add match arm and handler**

```rust
        Commands::FlagHygiene(args) => run_flag_hygiene(args, cfg).await,
```

Handler:

```rust
async fn run_flag_hygiene(args: FlagHygieneArgs, _cfg: &ApexConfig) -> Result<()> {
    use apex_detect::{AnalysisContext, DetectConfig, Detector};
    use apex_detect::detectors::flag_hygiene::FlagHygieneDetector;
    use std::sync::Arc;

    let lang: Language = args.lang.into();
    let target_path = args.target.canonicalize()?;
    let source_cache = build_source_cache(&target_path, lang);

    let detector = FlagHygieneDetector::default_max_age();
    let ctx = AnalysisContext {
        target_root: target_path.clone(),
        language: lang,
        oracle: Arc::new(CoverageOracle::new()),
        file_paths: std::collections::HashMap::new(),
        known_bugs: vec![],
        source_cache,
        fuzz_corpus: None,
        config: DetectConfig::default(),
        runner: Arc::new(apex_core::command::RealCommandRunner),
        cpg: None,
        threat_model: Default::default(),
    };

    let findings = detector.analyze(&ctx).await?;
    print_findings(&findings, &args.output_format, &target_path);
    Ok(())
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p apex-cli 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "feat: add apex flag-hygiene CLI subcommand (C50)"
```

---

### Task 8: Add ApiDiff CLI subcommand

**Files:**
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Add ApiDiff to Commands enum**

```rust
    /// Detect breaking changes between two OpenAPI spec versions.
    ApiDiff(ApiDiffArgs),
```

- [ ] **Step 2: Add ApiDiffArgs struct**

```rust
#[derive(Parser)]
pub struct ApiDiffArgs {
    /// Path to the old/baseline OpenAPI spec (JSON).
    #[arg(long)]
    pub old: PathBuf,

    /// Path to the new/current OpenAPI spec (JSON).
    #[arg(long)]
    pub new: PathBuf,

    /// Output format: text or json.
    #[arg(long, default_value = "text")]
    pub output_format: OutputFormat,
}
```

- [ ] **Step 3: Add match arm and handler**

```rust
        Commands::ApiDiff(args) => run_api_diff(args).await,
```

Handler:

```rust
async fn run_api_diff(args: ApiDiffArgs) -> Result<()> {
    use apex_detect::api_diff::ApiDiffer;

    let old_spec = std::fs::read_to_string(&args.old)?;
    let new_spec = std::fs::read_to_string(&args.new)?;

    let report = ApiDiffer::diff(&old_spec, &new_spec)?;

    match args.output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        OutputFormat::Text => {
            println!("API Diff: {} → {}\n", args.old.display(), args.new.display());
            println!("Breaking changes:     {}", report.breaking_count);
            println!("Non-breaking changes: {}", report.non_breaking_count);
            println!("Deprecations:         {}", report.deprecation_count);
            if report.breaking_count > 0 {
                println!("\n--- Breaking Changes ---");
                for change in &report.changes {
                    if matches!(change.kind, apex_detect::api_diff::ChangeKind::Breaking) {
                        println!("  ✗ {} {} — {}", change.method, change.path, change.description);
                    }
                }
            }
        }
    }

    if report.breaking_count > 0 {
        std::process::exit(1);
    }

    Ok(())
}
```

Note: `ApiDiffer::diff(&str, &str)` takes JSON strings, parses internally. `ApiDiffReport` has public fields: `changes`, `breaking_count`, `non_breaking_count`, `deprecation_count`.

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p apex-cli 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 5: Run full workspace tests**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "feat: add apex api-diff CLI subcommand (C27)"
```

---

### Task 9: Add shared print_findings helper (if not already present)

**Files:**
- Modify: `crates/apex-cli/src/lib.rs`

- [ ] **Step 1: Check if print_findings already exists**

Search for an existing `print_findings` or equivalent helper in the CLI. If the `run_audit` function handles printing inline, extract a shared helper:

```rust
fn print_findings(findings: &[apex_detect::Finding], format: &OutputFormat, target: &std::path::Path) {
    if findings.is_empty() {
        println!("No findings.");
        return;
    }

    match format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(findings).unwrap_or_default());
        }
        OutputFormat::Text => {
            println!("\n{} finding(s) in {}\n", findings.len(), target.display());
            for f in findings {
                let sev = format!("{:?}", f.severity).to_uppercase();
                let file_loc = match f.line {
                    Some(l) => format!("{}:{}", f.file.display(), l),
                    None => f.file.display().to_string(),
                };
                println!("[{sev}] {file_loc}");
                println!("  {}", f.title);
                if !f.suggestion.is_empty() {
                    println!("  → {}", f.suggestion);
                }
                println!();
            }
        }
    }
}
```

If a similar helper already exists in the audit handler, reuse it. Skip creating a new one.

- [ ] **Step 2: Run workspace tests**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: all tests pass

- [ ] **Step 3: Commit (if changes made)**

```bash
git add crates/apex-cli/src/lib.rs
git commit -m "refactor: extract shared print_findings helper for CLI subcommands"
```

---

### Task 10: Update expansion plan status

**Files:**
- Modify: `plans/apex-expansion-plan.md` (or STATUS.md)

- [ ] **Step 1: Update STATUS.md**

Change the expansion plan entry from `16% (5/32)` to reflect the newly integrated capabilities. The 4 Tier 1 items (C35, C30, C50, C27) are now integrated into pipeline + CLI. Update:

```
| FUTURE | `apex-expansion-plan.md` | 28% (9/32) | Tier 1 integrated: C27, C30, C35, C50 |
```

- [ ] **Step 2: Commit**

```bash
git add plans/STATUS.md
git commit -m "docs: update expansion plan progress — Tier 1 integrated (9/32)"
```
