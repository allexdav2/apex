<!-- status: DONE --># APEX Agent Loop Redesign Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the broken internal AgentCluster loop with a CLI-driven architecture where `--strategy agent` outputs rich JSON gap reports for Claude Code to consume, and add `--strategy driller` as a standalone CLI path.

**Architecture:** APEX binary becomes measurement + strategy execution only. The `"agent"` match arm produces a prioritized JSON gap report (with bang_for_buck scoring, difficulty classification, source context). Claude Code drives the agent loop externally. Internal strategies (fuzz, driller, concolic) remain available as standalone CLI commands.

**Tech Stack:** Rust, serde_json, clap, tracing

**Spec:** `docs/superpowers/specs/2026-03-11-apex-agent-loop-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/apex-core/src/agent_report.rs` | **Create** | `AgentGapReport`, `GapEntry`, `BlockedEntry`, `GapDifficulty` serde structs |
| `crates/apex-core/src/lib.rs` | Modify | Export `agent_report` module |
| `crates/apex-cli/src/main.rs` | Modify | Replace `"agent"` match arm, add `"driller"` match arm, add `print_agent_json_report()`, remove `run_agent_strategy()` |
| `crates/apex-core/src/traits.rs` | Modify | Remove `trait Agent` |
| `crates/apex-agent/src/orchestrator.rs` | Modify | Remove `run_agent_cycle()`, `agent` field, agent-related methods. Keep `AgentCluster` for fuzz/driller strategy orchestration but rename concept |
| `crates/apex-agent/src/lib.rs` | Modify | Update exports |
| `crates/apex-cli/tests/integration_test.rs` | Modify | Add tests for agent JSON report |

---

## Chunk 1: Agent Report Types + JSON Output

### Task 1: Create AgentGapReport types

**Files:**
- Create: `crates/apex-core/src/agent_report.rs`
- Modify: `crates/apex-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

In `crates/apex-core/src/agent_report.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_gap_report_serializes_to_json() {
        let report = AgentGapReport {
            summary: GapSummary {
                total_branches: 100,
                covered_branches: 80,
                coverage_pct: 0.80,
                files_total: 5,
                files_fully_covered: 2,
            },
            gaps: vec![GapEntry {
                file: "src/main.rs".into(),
                function: Some("handle_request".into()),
                branch_line: 42,
                branch_condition: Some("match status {".into()),
                source_context: vec!["    match status {".into(), "        200 => ok(),".into()],
                uncovered_branches: 3,
                coverage_pct: 0.75,
                closest_existing_test: None,
                bang_for_buck: 0.85,
                difficulty: GapDifficulty::Medium,
                difficulty_reason: "needs mock HTTP client".into(),
                suggested_approach: "Test each match arm with mock responses".into(),
            }],
            blocked: vec![BlockedEntry {
                file: "src/rpc.rs".into(),
                uncovered_branches: 50,
                reason: "gRPC server required".into(),
            }],
        };

        let json = serde_json::to_string_pretty(&report).unwrap();
        assert!(json.contains("\"coverage_pct\": 0.8"));
        assert!(json.contains("\"bang_for_buck\": 0.85"));
        assert!(json.contains("\"difficulty\": \"medium\""));
        assert!(json.contains("\"gRPC server required\""));
    }

    #[test]
    fn gap_difficulty_ordering() {
        assert!(GapDifficulty::Easy < GapDifficulty::Medium);
        assert!(GapDifficulty::Medium < GapDifficulty::Hard);
        assert!(GapDifficulty::Hard < GapDifficulty::Blocked);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-core agent_gap_report 2>&1 | tail -5`
Expected: FAIL — module doesn't exist yet

- [ ] **Step 3: Write the implementation**

In `crates/apex-core/src/agent_report.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Rich gap report designed for external agent consumption (Claude Code).
/// Produced by `--strategy agent --output-format json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentGapReport {
    pub summary: GapSummary,
    pub gaps: Vec<GapEntry>,
    pub blocked: Vec<BlockedEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapSummary {
    pub total_branches: usize,
    pub covered_branches: usize,
    pub coverage_pct: f64,
    pub files_total: usize,
    pub files_fully_covered: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GapEntry {
    pub file: PathBuf,
    pub function: Option<String>,
    pub branch_line: u32,
    pub branch_condition: Option<String>,
    pub source_context: Vec<String>,
    pub uncovered_branches: usize,
    pub coverage_pct: f64,
    /// What fraction of the file's uncovered branches this gap represents.
    /// Higher = more coverage gain from testing this file. 0.0–1.0.
    pub bang_for_buck: f64,
    pub difficulty: GapDifficulty,
    pub difficulty_reason: String,
    pub suggested_approach: String,
    pub closest_existing_test: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockedEntry {
    pub file: PathBuf,
    pub uncovered_branches: usize,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GapDifficulty {
    Easy,
    Medium,
    Hard,
    Blocked,
}

// ... tests from Step 1 go here
```

Add to `crates/apex-core/src/lib.rs`:

```rust
pub mod agent_report;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p apex-core agent_gap_report 2>&1 | tail -10`
Expected: 2 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/agent_report.rs crates/apex-core/src/lib.rs
git commit -m "feat: add AgentGapReport types for rich JSON gap output"
```

---

### Task 2: Implement difficulty classifier

**Files:**
- Modify: `crates/apex-core/src/agent_report.rs`

The classifier examines source context lines to heuristically assign difficulty.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `agent_report.rs`:

```rust
#[test]
fn classify_easy_branch() {
    let lines = vec!["    if x > 0 {".to_string(), "        return true;".to_string()];
    let (diff, reason) = classify_difficulty(&lines);
    assert_eq!(diff, GapDifficulty::Easy);
    assert!(reason.contains("simple conditional"));
}

#[test]
fn classify_medium_needs_setup() {
    let lines = vec!["    match config.mode {".to_string(), "        Mode::Debug => {".to_string()];
    let (diff, reason) = classify_difficulty(&lines);
    assert_eq!(diff, GapDifficulty::Medium);
}

#[test]
fn classify_hard_external_deps() {
    let lines = vec!["    let resp = client.get(url).await?;".to_string()];
    let (diff, reason) = classify_difficulty(&lines);
    assert_eq!(diff, GapDifficulty::Hard);
    assert!(reason.contains("async") | reason.contains("external") | reason.contains("network"));
}

#[test]
fn classify_hard_unsafe() {
    let lines = vec!["    unsafe { ptr::write(dest, val) }".to_string()];
    let (diff, reason) = classify_difficulty(&lines);
    assert_eq!(diff, GapDifficulty::Hard);
}

#[test]
fn classify_hard_ffi() {
    let lines = vec!["    extern \"C\" fn callback(data: *mut c_void) {".to_string()];
    let (diff, reason) = classify_difficulty(&lines);
    assert_eq!(diff, GapDifficulty::Hard);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-core classify_ 2>&1 | tail -5`
Expected: FAIL — `classify_difficulty` not defined

- [ ] **Step 3: Write the implementation**

Add to `crates/apex-core/src/agent_report.rs` (before tests module):

```rust
/// Heuristic difficulty classification based on source context keywords.
pub fn classify_difficulty(source_lines: &[String]) -> (GapDifficulty, String) {
    let joined = source_lines.join(" ").to_lowercase();

    // Hard indicators: external deps, unsafe, FFI, network, process spawning
    let hard_patterns: &[(&str, &str)] = &[
        ("unsafe", "unsafe code block"),
        ("extern", "FFI / extern function"),
        (".await", "async operation — needs runtime + possibly mock"),
        ("tokio::spawn", "async task spawning"),
        ("command::new", "process spawning"),
        ("std::process", "process spawning"),
        ("tcplistener", "network listener"),
        ("tcpstream", "network connection"),
        ("udpsocket", "network socket"),
        ("hyper::", "HTTP framework"),
        ("reqwest::", "HTTP client"),
        ("tonic::", "gRPC framework"),
        ("grpc", "gRPC integration"),
    ];

    for (pattern, reason) in hard_patterns {
        if joined.contains(pattern) {
            return (GapDifficulty::Hard, format!("needs external deps: {reason}"));
        }
    }

    // Medium indicators: match arms, error handling, complex setup
    let medium_patterns: &[(&str, &str)] = &[
        ("match ", "match expression — needs test per arm"),
        ("unwrap_or", "error fallback path"),
        ("map_err", "error mapping"),
        ("ok_or", "option-to-result conversion"),
        ("config.", "configuration-dependent branch"),
        ("env::", "environment-dependent branch"),
        ("std::fs::", "filesystem operation"),
        ("file::open", "file I/O"),
    ];

    for (pattern, reason) in medium_patterns {
        if joined.contains(pattern) {
            return (GapDifficulty::Medium, format!("needs setup: {reason}"));
        }
    }

    // Default: easy
    (GapDifficulty::Easy, "simple conditional".to_string())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p apex-core classify_ 2>&1 | tail -10`
Expected: 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/agent_report.rs
git commit -m "feat: add heuristic difficulty classifier for gap entries"
```

---

### Task 3: Implement bang-for-buck scorer and suggested approach

**Files:**
- Modify: `crates/apex-core/src/agent_report.rs`

- [ ] **Step 1: Write the failing test**

Add to tests module:

```rust
#[test]
fn bang_for_buck_high_when_many_uncovered_in_file() {
    // File has 20 uncovered out of 100 total = 0.2 density
    let score = compute_bang_for_buck(20, 100);
    assert!((score - 0.2).abs() < 0.01);
}

#[test]
fn bang_for_buck_capped_at_one() {
    // Edge case: all uncovered
    let score = compute_bang_for_buck(50, 50);
    assert!((score - 1.0).abs() < 0.01);
}

#[test]
fn bang_for_buck_zero_when_no_uncovered() {
    let score = compute_bang_for_buck(0, 100);
    assert!((score - 0.0).abs() < 0.01);
}

#[test]
fn suggested_approach_for_easy() {
    let approach = suggest_approach(GapDifficulty::Easy, &["if x > 0 {".into()]);
    assert!(approach.contains("unit test"));
}

#[test]
fn suggested_approach_for_hard_fuzz() {
    let approach = suggest_approach(GapDifficulty::Hard, &["unsafe { parse_bytes(buf) }".into()]);
    assert!(approach.contains("fuzz") || approach.contains("binary"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-core bang_for_buck 2>&1 | tail -5`
Expected: FAIL — functions not defined

- [ ] **Step 3: Write the implementation**

Add to `crates/apex-core/src/agent_report.rs`:

```rust
/// Bang-for-buck: fraction of file's branches that are uncovered.
/// Higher = more coverage improvement potential from testing this file.
pub fn compute_bang_for_buck(uncovered_in_file: usize, total_in_file: usize) -> f64 {
    if total_in_file == 0 {
        return 0.0;
    }
    (uncovered_in_file as f64 / total_in_file as f64).min(1.0)
}

/// Generate a one-line test approach suggestion based on difficulty and source.
pub fn suggest_approach(difficulty: GapDifficulty, source_lines: &[String]) -> String {
    let joined = source_lines.join(" ").to_lowercase();

    match difficulty {
        GapDifficulty::Easy => {
            "Write a unit test with direct function call and assertion".to_string()
        }
        GapDifficulty::Medium => {
            if joined.contains("match ") {
                "Test each match arm with controlled input variants".to_string()
            } else if joined.contains("err") {
                "Test error path by providing invalid input".to_string()
            } else if joined.contains("config") || joined.contains("env") {
                "Test with different configuration/environment values".to_string()
            } else {
                "Write test with appropriate setup and mocks".to_string()
            }
        }
        GapDifficulty::Hard => {
            if joined.contains("unsafe") || joined.contains("extern") {
                "Use --strategy fuzz for binary-level exploration".to_string()
            } else if joined.contains(".await") || joined.contains("grpc") || joined.contains("tcp") {
                "Use --strategy driller or mock the async boundary".to_string()
            } else {
                "Consider --strategy fuzz or integration test with full harness".to_string()
            }
        }
        GapDifficulty::Blocked => {
            "Cannot unit-test — needs integration harness or external service".to_string()
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p apex-core bang_for_buck suggested_approach 2>&1 | tail -10`
Expected: 5 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/agent_report.rs
git commit -m "feat: add bang-for-buck scorer and approach suggestions"
```

---

### Task 4: Implement function name extraction from source context

**Files:**
- Modify: `crates/apex-core/src/agent_report.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn extract_function_name_from_context() {
    let lines = vec![
        "fn handle_request(req: Request) -> Response {".to_string(),
        "    if req.method == Method::GET {".to_string(),
        "        return ok();".to_string(),
    ];
    assert_eq!(extract_enclosing_function(&lines), Some("handle_request".to_string()));
}

#[test]
fn extract_function_name_async() {
    let lines = vec![
        "    async fn process(data: &[u8]) -> Result<()> {".to_string(),
        "        let parsed = parse(data)?;".to_string(),
    ];
    assert_eq!(extract_enclosing_function(&lines), Some("process".to_string()));
}

#[test]
fn extract_function_name_pub() {
    let lines = vec![
        "pub(crate) fn validate(input: &str) -> bool {".to_string(),
        "    !input.is_empty()".to_string(),
    ];
    assert_eq!(extract_enclosing_function(&lines), Some("validate".to_string()));
}

#[test]
fn extract_function_name_none_when_absent() {
    let lines = vec![
        "    let x = 42;".to_string(),
        "    println!(\"{x}\");".to_string(),
    ];
    assert_eq!(extract_enclosing_function(&lines), None);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-core extract_function 2>&1 | tail -5`
Expected: FAIL

- [ ] **Step 3: Write the implementation**

```rust
/// Extract the enclosing function name from source context lines.
/// Scans for `fn <name>(` pattern, handling pub/async/unsafe prefixes.
pub fn extract_enclosing_function(source_lines: &[String]) -> Option<String> {
    for line in source_lines {
        let trimmed = line.trim();
        // Find "fn " and extract the identifier before "("
        if let Some(fn_pos) = trimmed.find("fn ") {
            let after_fn = &trimmed[fn_pos + 3..];
            // Skip if it's inside a string or comment
            if fn_pos > 0 {
                let before = &trimmed[..fn_pos];
                if before.contains("//") || before.contains('"') {
                    continue;
                }
            }
            if let Some(paren_pos) = after_fn.find('(') {
                let name = after_fn[..paren_pos].trim();
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return Some(name.to_string());
                }
            }
        }
    }
    None
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p apex-core extract_function 2>&1 | tail -10`
Expected: 4 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/agent_report.rs
git commit -m "feat: extract enclosing function name from source context"
```

---

### Task 5: Build AgentGapReport from CoverageOracle

**Files:**
- Modify: `crates/apex-core/src/agent_report.rs`

This is the core function that assembles the full report from oracle data.

- [ ] **Step 1: Write the failing test**

```rust
use crate::types::{BranchId, Language};
use std::collections::HashMap;

#[test]
fn build_report_from_oracle_data() {
    // Simulate oracle data: 2 files, one with uncovered branches
    let file_paths: HashMap<u64, PathBuf> = [
        (1, PathBuf::from("src/main.rs")),
        (2, PathBuf::from("src/lib.rs")),
    ].into();

    let all_branches = vec![
        BranchId { file_id: 1, line: 10, col: 5, direction: 0, discriminator: 0, condition_index: None },
        BranchId { file_id: 1, line: 10, col: 5, direction: 1, discriminator: 0, condition_index: None },
        BranchId { file_id: 1, line: 20, col: 5, direction: 0, discriminator: 0, condition_index: None },
        BranchId { file_id: 2, line: 5, col: 1, direction: 0, discriminator: 0, condition_index: None },
    ];

    let covered_ids: Vec<BranchId> = vec![
        BranchId { file_id: 1, line: 10, col: 5, direction: 0, discriminator: 0, condition_index: None },
        BranchId { file_id: 2, line: 5, col: 1, direction: 0, discriminator: 0, condition_index: None },
    ];

    let uncovered = vec![
        BranchId { file_id: 1, line: 10, col: 5, direction: 1, discriminator: 0, condition_index: None },
        BranchId { file_id: 1, line: 20, col: 5, direction: 0, discriminator: 0, condition_index: None },
    ];

    // Source lines keyed by (file_id, line)
    let source_cache: HashMap<(u64, u32), String> = [
        ((1, 10), "    if x > 0 {".to_string()),
        ((1, 20), "    match mode {".to_string()),
    ].into();

    let report = build_agent_gap_report(
        all_branches.len(),
        covered_ids.len(),
        &uncovered,
        &file_paths,
        &source_cache,
    );

    assert_eq!(report.summary.total_branches, 4);
    assert_eq!(report.summary.covered_branches, 2);
    assert!((report.summary.coverage_pct - 0.5).abs() < 0.01);
    assert_eq!(report.summary.files_total, 2);
    assert_eq!(report.summary.files_fully_covered, 1); // file 2 is fully covered
    assert_eq!(report.gaps.len(), 2);
    assert!(report.gaps[0].bang_for_buck >= report.gaps[1].bang_for_buck); // sorted by b4b desc
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-core build_report_from 2>&1 | tail -5`
Expected: FAIL

- [ ] **Step 3: Write the implementation**

```rust
use crate::types::BranchId;
use std::collections::HashMap;

/// Build a complete AgentGapReport from oracle data.
///
/// `source_cache` maps (file_id, line) → source line text.
/// Gaps are sorted by `bang_for_buck` descending.
pub fn build_agent_gap_report(
    total_branches: usize,
    covered_branches: usize,
    uncovered: &[BranchId],
    file_paths: &HashMap<u64, PathBuf>,
    source_cache: &HashMap<(u64, u32), String>,
) -> AgentGapReport {
    let coverage_pct = if total_branches == 0 {
        1.0
    } else {
        covered_branches as f64 / total_branches as f64
    };

    // Count uncovered per file and total per file
    let mut uncovered_per_file: HashMap<u64, Vec<&BranchId>> = HashMap::new();
    for b in uncovered {
        uncovered_per_file.entry(b.file_id).or_default().push(b);
    }

    // Count total branches per file (from all_branches count we don't have directly,
    // so approximate: uncovered + covered. We'll use file-level uncovered count only.)
    // For accurate per-file totals, we'd need all branches grouped by file.
    // Approximation: use uncovered count as the numerator for bang_for_buck.

    // Unique file IDs across all file_paths
    let files_total = file_paths.len();
    let files_with_uncovered: std::collections::HashSet<u64> =
        uncovered.iter().map(|b| b.file_id).collect();
    let files_fully_covered = files_total.saturating_sub(files_with_uncovered.len());

    // Build gap entries grouped by (file_id, line) to deduplicate
    let mut seen: std::collections::HashSet<(u64, u32)> = std::collections::HashSet::new();
    let mut gaps = Vec::new();

    for branch in uncovered {
        let key = (branch.file_id, branch.line);
        if !seen.insert(key) {
            continue; // already have an entry for this line
        }

        let file = file_paths
            .get(&branch.file_id)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(format!("file_{}", branch.file_id)));

        // Gather source context: the branch line + surrounding lines (±3)
        let mut context_lines = Vec::new();
        for offset in -3i32..=3 {
            let l = (branch.line as i32 + offset) as u32;
            if let Some(src) = source_cache.get(&(branch.file_id, l)) {
                context_lines.push(src.clone());
            }
        }

        let (difficulty, difficulty_reason) = classify_difficulty(&context_lines);
        let approach = suggest_approach(difficulty, &context_lines);
        let function = extract_enclosing_function(&context_lines);

        let file_uncovered_count = uncovered_per_file
            .get(&branch.file_id)
            .map(|v| v.len())
            .unwrap_or(0);

        let bang = compute_bang_for_buck(file_uncovered_count, total_branches);

        let branch_condition = source_cache
            .get(&(branch.file_id, branch.line))
            .cloned();

        // Per-file coverage: (total_in_file - uncovered_in_file) / total_in_file
        // We don't have total_in_file, so use 1.0 - (uncovered/total) as approximation
        let file_cov = 1.0 - (file_uncovered_count as f64 / total_branches.max(1) as f64);

        gaps.push(GapEntry {
            file,
            function,
            branch_line: branch.line,
            branch_condition,
            source_context: context_lines,
            uncovered_branches: file_uncovered_count,
            coverage_pct: file_cov,
            bang_for_buck: bang,
            difficulty,
            difficulty_reason,
            suggested_approach: approach,
            closest_existing_test: None, // v1: not implemented
        });
    }

    // Sort by bang_for_buck descending
    gaps.sort_by(|a, b| b.bang_for_buck.partial_cmp(&a.bang_for_buck).unwrap_or(std::cmp::Ordering::Equal));

    AgentGapReport {
        summary: GapSummary {
            total_branches,
            covered_branches,
            coverage_pct,
            files_total,
            files_fully_covered,
        },
        gaps,
        blocked: Vec::new(), // v1: blocked detection not yet implemented
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p apex-core build_report 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/agent_report.rs
git commit -m "feat: build_agent_gap_report assembles rich JSON from oracle data"
```

---

## Chunk 2: CLI Wiring

### Task 6: Add print_agent_json_report() to CLI

**Files:**
- Modify: `crates/apex-cli/src/main.rs`

- [ ] **Step 1: Add the function**

Add after the existing `print_json_gap_report()` function (around line 485):

```rust
/// Print rich agent-format JSON gap report for external agent consumption.
fn print_agent_json_report(
    oracle: &CoverageOracle,
    file_paths: &HashMap<u64, PathBuf>,
    target_path: &std::path::Path,
) {
    use apex_core::agent_report::build_agent_gap_report;

    let all_branch_count = oracle.all_branches().len();
    let covered_count = oracle.covered_branches().len();
    let uncovered: Vec<_> = oracle.uncovered_branches().collect();

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
                // Cache ±5 lines around the branch
                let start = (branch.line as usize).saturating_sub(6);
                let end = (branch.line as usize + 5).min(lines.len());
                for (i, line) in lines[start..end].iter().enumerate() {
                    let line_num = (start + i + 1) as u32;
                    source_cache.entry((branch.file_id, line_num))
                        .or_insert_with(|| line.to_string());
                }
            }
        }
    }

    let report = build_agent_gap_report(
        all_branch_count,
        covered_count,
        &uncovered,
        file_paths,
        &source_cache,
    );

    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(e) => eprintln!("{{\"error\": \"failed to serialize report: {e}\"}}"),
    }
}
```

- [ ] **Step 2: Run workspace build to verify it compiles**

Run: `cargo build -p apex-cli 2>&1 | tail -5`
Expected: Compiles (function exists but not yet called)

- [ ] **Step 3: Commit**

```bash
git add crates/apex-cli/src/main.rs
git commit -m "feat: add print_agent_json_report() for rich JSON output"
```

---

### Task 7: Replace "agent" match arm and add "driller"

**Files:**
- Modify: `crates/apex-cli/src/main.rs`

- [ ] **Step 1: Replace the "agent" match arm body**

Find the `"agent"` match arm (around line 245):

```rust
        "agent" => {
            run_agent_strategy(
                Arc::clone(&oracle),
                &instrumented,
                coverage_target,
                rounds,
            )
            .await?;
        }
```

Replace with:

```rust
        "agent" => {
            // Agent strategy: baseline measurement already done by instrumentation above.
            // The rich JSON gap report is produced in the output section below.
            info!("agent strategy: baseline measurement complete, producing gap report");
        }
```

- [ ] **Step 2: Add "driller" match arm**

Find the strategy match block. Add before `_ => {}`:

```rust
        "driller" => {
            let solver = apex_symbolic::PortfolioSolver::new(vec![], std::time::Duration::from_secs(30));
            let driller = apex_agent::driller::DrillerStrategy::new(
                Arc::new(std::sync::Mutex::new(solver)),
                4096,
            );
            // Run driller as a standalone strategy with the existing fuzz infrastructure
            let cmd = fuzz_command(&args, &target_path);
            fuzz::run_single_strategy(
                Arc::clone(&oracle),
                &instrumented,
                Box::new(driller),
                coverage_target,
                fuzz_iters,
                rounds,
                args.output.clone(),
                cmd,
                cfg.clone(),
            )
            .await?;
        }
```

**Note:** If `fuzz::run_single_strategy` doesn't exist, we need to check what functions `apex-fuzz` exports. The driller needs a runner. Read `crates/apex-fuzz/src/lib.rs` to see available functions. If only `run_fuzz_strategy` and `run_all_strategies` exist, add the driller to `run_all_strategies` or create a thin wrapper. For v1, the driller can be wired into the `"all"` path or we skip this and note it as future work.

**Alternative if no single-strategy runner exists:** Wire driller into the `"all"` strategy path, and add `"driller"` as an alias:

```rust
        "driller" => {
            info!("driller strategy: running SMT-driven path exploration");
            // For v1, driller runs through the 'all' strategies path
            let cmd = fuzz_command(&args, &target_path);
            fuzz::run_all_strategies(
                Arc::clone(&oracle),
                &instrumented,
                coverage_target,
                fuzz_iters,
                rounds,
                args.output.clone(),
                cmd,
                cfg.clone(),
            )
            .await?;
        }
```

- [ ] **Step 3: Update the gap report output section**

Find the output section (around line 258):

```rust
    // 4. Output gap report
    match output_format {
        OutputFormat::Json => print_json_gap_report(&oracle, &instrumented.file_paths, &target_path),
        OutputFormat::Text => print_gap_report(&oracle, &instrumented.file_paths, &target_path),
    }
```

Replace with:

```rust
    // 4. Output gap report
    let is_agent = args.strategy == "agent";
    match (output_format, is_agent) {
        (OutputFormat::Json, true) => {
            print_agent_json_report(&oracle, &instrumented.file_paths, &target_path);
        }
        (OutputFormat::Json, false) => {
            print_json_gap_report(&oracle, &instrumented.file_paths, &target_path);
        }
        (OutputFormat::Text, _) => {
            print_gap_report(&oracle, &instrumented.file_paths, &target_path);
        }
    }
```

**Note:** `args.strategy` needs to be accessible here. It currently is since it's used in the match above. Store it before the match if needed:

```rust
    let strategy_name = args.strategy.as_str();
```

- [ ] **Step 4: Run workspace build**

Run: `cargo build -p apex-cli 2>&1 | tail -10`
Expected: Compiles

- [ ] **Step 5: Run existing tests**

Run: `cargo test -p apex-cli 2>&1 | tail -10`
Expected: All existing tests pass (the tests don't exercise the agent match arm)

- [ ] **Step 6: Commit**

```bash
git add crates/apex-cli/src/main.rs
git commit -m "feat: wire --strategy agent for JSON output, add --strategy driller"
```

---

### Task 8: Remove run_agent_strategy() and clean up imports

**Files:**
- Modify: `crates/apex-cli/src/main.rs`

- [ ] **Step 1: Delete the run_agent_strategy() function**

Find and remove the entire `run_agent_strategy()` function (around lines 538-610). It starts with:
```rust
async fn run_agent_strategy(
```
and ends about 72 lines later.

- [ ] **Step 2: Clean up unused imports**

Remove from imports at the top of `main.rs`:
```rust
use apex_agent::{AgentCluster, OrchestratorConfig};
use apex_concolic::PythonConcolicStrategy;
```

Keep:
```rust
use apex_core::traits::{Sandbox, Strategy};
```
only if still used elsewhere. Check if `Sandbox` and `Strategy` are used outside `run_agent_strategy`. If not, remove those imports too.

- [ ] **Step 3: Run build and tests**

Run: `cargo build -p apex-cli 2>&1 | tail -5 && cargo test -p apex-cli 2>&1 | tail -10`
Expected: Compiles and all tests pass

- [ ] **Step 4: Commit**

```bash
git add crates/apex-cli/src/main.rs
git commit -m "refactor: remove run_agent_strategy() — agent loop now external"
```

---

## Chunk 3: Trait and Orchestrator Cleanup

### Task 9: Remove trait Agent from core

**Files:**
- Modify: `crates/apex-core/src/traits.rs`
- Modify: `crates/apex-core/src/types.rs` (if TestCandidate only used by Agent)

- [ ] **Step 1: Remove trait Agent**

In `crates/apex-core/src/traits.rs`, remove:

```rust
pub trait Agent: Send + Sync {
    async fn generate_tests(&self, gap: &CoverageGapReport, ctx: &SourceContext) -> Result<Vec<TestCandidate>>;
    async fn refine_test(&self, candidate: &TestCandidate, failure: &ExecutionResult) -> Result<TestCandidate>;
}
```

Also remove the associated `use` imports for `CoverageGapReport`, `SourceContext`, `TestCandidate` if they were only used by `trait Agent`.

- [ ] **Step 2: Check if TestCandidate is used elsewhere**

Run: `grep -r "TestCandidate" crates/ --include="*.rs" -l`

If only used in `traits.rs` and `orchestrator.rs` (which we're cleaning up), leave it in `types.rs` for now — it may be useful later. If used nowhere, remove it.

- [ ] **Step 3: Run build**

Run: `cargo build --workspace 2>&1 | tail -10`
Expected: May fail in crates that reference `trait Agent`. Fix each one.

- [ ] **Step 4: Fix any compilation errors**

Common fixes:
- `orchestrator.rs` references `Agent` — we'll clean that in Task 10
- Any mock `Agent` in tests — remove those test modules

- [ ] **Step 5: Commit** (after Task 10 if they need to go together)

---

### Task 10: Remove agent cycle from orchestrator

**Files:**
- Modify: `crates/apex-agent/src/orchestrator.rs`
- Modify: `crates/apex-agent/src/lib.rs`

- [ ] **Step 1: Remove agent-related fields and methods from AgentCluster**

In `orchestrator.rs`:

1. Remove `agent: Option<Box<dyn Agent>>` field from `AgentCluster` struct
2. Remove `with_agent()` builder method
3. Remove `run_agent_cycle()` method entirely
4. In `run()`, remove the stall-detection escalation to `run_agent_cycle()` — replace with just logging:
   ```rust
   if stall_count >= self.config.stall_threshold {
       warn!("coverage stalled after {} iterations with no improvement", stall_count);
       break;
   }
   ```
5. Remove `MAX_AGENT_ROUNDS` and `MAX_REFINEMENT_ROUNDS` constants
6. Remove `use crate::source::*` if only used by `run_agent_cycle`

- [ ] **Step 2: Update lib.rs exports**

In `crates/apex-agent/src/lib.rs`, keep all module exports. The struct is still used by fuzz/all strategy paths. Just ensure removed items (`Agent` references) don't cause errors.

- [ ] **Step 3: Fix orchestrator tests**

The orchestrator has 50+ tests. Tests that reference `with_agent()`, `run_agent_cycle()`, or mock `Agent` implementations need to be removed. Tests for `run()`, stall detection, config, and strategy execution should still pass.

Strategy for test cleanup:
- Keep: construction, config, builder (minus `with_agent`), coverage target, deadline
- Remove: any test that creates a mock `Agent`, calls `run_agent_cycle`, or tests agent refinement loops
- Update: stall detection tests to expect `break` instead of agent cycle escalation

- [ ] **Step 4: Run build and tests**

Run: `cargo build --workspace 2>&1 | tail -10 && cargo test --workspace 2>&1 | tail -20`
Expected: All pass

- [ ] **Step 5: Commit**

```bash
git add crates/apex-core/src/traits.rs crates/apex-agent/src/orchestrator.rs crates/apex-agent/src/lib.rs
git commit -m "refactor: remove trait Agent and agent cycle from orchestrator"
```

---

### Task 11: Add integration test for agent JSON report

**Files:**
- Modify: `crates/apex-cli/tests/integration_test.rs`

- [ ] **Step 1: Write the test**

```rust
#[test]
fn agent_json_report_structure() {
    use apex_core::agent_report::{AgentGapReport, GapDifficulty};

    // Build a minimal oracle with known branches
    let oracle = CoverageOracle::new(4);
    let b1 = BranchId { file_id: 1, line: 10, col: 1, direction: 0, discriminator: 0, condition_index: None };
    let b2 = BranchId { file_id: 1, line: 10, col: 1, direction: 1, discriminator: 0, condition_index: None };
    let b3 = BranchId { file_id: 1, line: 20, col: 1, direction: 0, discriminator: 0, condition_index: None };
    let b4 = BranchId { file_id: 2, line: 5, col: 1, direction: 0, discriminator: 0, condition_index: None };

    oracle.register_branches(&[b1.clone(), b2.clone(), b3.clone(), b4.clone()]);
    // Mark b1 and b4 as covered
    oracle.mark_covered(&b1);
    oracle.mark_covered(&b4);

    let file_paths: HashMap<u64, PathBuf> = [
        (1, PathBuf::from("src/main.rs")),
        (2, PathBuf::from("src/lib.rs")),
    ].into();

    let uncovered: Vec<_> = oracle.uncovered_branches().collect();
    let source_cache = HashMap::new(); // no source for this test

    let report = apex_core::agent_report::build_agent_gap_report(
        4, 2, &uncovered, &file_paths, &source_cache,
    );

    // Verify structure
    assert_eq!(report.summary.total_branches, 4);
    assert_eq!(report.summary.covered_branches, 2);
    assert!((report.summary.coverage_pct - 0.5).abs() < 0.01);
    assert!(!report.gaps.is_empty());

    // Verify it serializes cleanly
    let json = serde_json::to_string(&report).unwrap();
    let parsed: AgentGapReport = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.summary.total_branches, 4);
}
```

**Note:** Adapt the test to match the actual `CoverageOracle` API. The oracle may not have `register_branches` / `mark_covered` / `uncovered_branches` with exactly these signatures. Read the oracle implementation to match the actual API. The test above is a template — the implementer must read `crates/apex-coverage/src/lib.rs` to get the real API.

- [ ] **Step 2: Run the test**

Run: `cargo test -p apex-cli agent_json_report 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/apex-cli/tests/integration_test.rs
git commit -m "test: add integration test for agent JSON gap report"
```

---

### Task 12: Final workspace verification

- [ ] **Step 1: Run full workspace build**

```bash
cargo build --workspace 2>&1 | tail -10
```
Expected: No errors

- [ ] **Step 2: Run full workspace tests**

```bash
cargo test --workspace 2>&1 | tail -20
```
Expected: All tests pass, no regressions

- [ ] **Step 3: Run clippy**

```bash
cargo clippy --workspace -- -D warnings 2>&1 | tail -10
```
Expected: No warnings

- [ ] **Step 4: Verify agent JSON output manually**

```bash
LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov \
LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata \
cargo run --bin apex --manifest-path /Users/ad/prj/bcov/Cargo.toml -- \
  run --target /Users/ad/prj/bcov --lang rust --strategy agent \
  --output-format json 2>/dev/null | python3 -m json.tool | head -30
```
Expected: Valid JSON with `summary`, `gaps`, and `blocked` fields

- [ ] **Step 5: Final commit if any fixups needed**

```bash
git add -A
git commit -m "chore: final fixups for agent loop redesign"
```
