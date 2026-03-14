# Reuse Fundamental Mechanisms from Competing Tools in APEX

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Integrate fundamental analysis mechanisms from Joern (CPG), CoverUp (LLM-guided coverage), EvoMaster (branch distance), and Angora (gradient descent) into APEX's architecture — replacing surface-level pattern matching with deep structural analysis.

**Evidence base:** Installed, ran, and studied source code of Joern v4.0.500, CoverUp, EvoMaster, SymCC, Angora, and Owi. Four parallel research agents produced detailed technical reports covering architecture, algorithms, and integration points.

**Architecture:** Extends existing crate structure via new trait implementations and one new crate (`apex-cpg`). No changes to the `Strategy`/`Sandbox`/`Instrumentor` trait interfaces. Each task is independently committable and testable.

**User direction:** "I am more interested in fundamental mechanism, not detecting os.call" — focus on reusable architectural mechanisms, not adding regex/substring patterns.

---

## Research Summary

| Tool | Mechanism Studied | Key Finding for APEX |
|------|-------------------|---------------------|
| **Joern** | Code Property Graph (AST+CFG+PDG in one graph) | REACHING_DEF edges enable taint analysis that substring matching can never replicate. 1,113 nodes / 6,453 edges for a 92-line file. Found all 3 vulns in test app via backward reachability. |
| **CoverUp** | LLM-guided test generation with coverage feedback | Closed-loop generate→run→measure→refine with `get_info` tool calling. AST-based segment extraction with line-number tagging. 3 retry attempts per gap. |
| **EvoMaster** | Continuous branch distance fitness | Korel-style distance heuristics: `x==42` with `x=40` scores 0.97 instead of binary 0. Per-target archive with subsumption. Taint-driven string specialization. |
| **Angora** | Gradient descent constraint solving | Partial derivatives via concrete execution (increment byte, measure distance change). Solves numeric constraints 10-100x faster than Z3. Magic bytes patching for string comparisons. |
| **SymCC** | Compiler-embedded symbolic execution | LLVM pass + runtime library. Short-circuiting optimization (skip symbolic computation when all operands concrete). Backend-agnostic constraint interface. |
| **Owi** | WASM parallel symbolic execution | Priority-based work-stealing scheduler. Rarity+depth+loop-penalty composite. Solver caching with negation inference. |

---

## Task 1: Continuous Branch Distance in CoverageOracle

**Why:** APEX's `CoverageOracle` uses a binary bitmap (covered/not-covered). EvoMaster's continuous branch distance provides a [0,1] fitness signal that guides mutation far more effectively. When `x == 42` and input produces `x = 40`, EvoMaster reports 0.97; APEX reports 0. This is the single highest-impact architectural change.

**Mechanism from:** EvoMaster's `HeuristicsForJumps.java` + `TruthnessUtils`

**Files:**
- Modify: `crates/apex-coverage/src/lib.rs` — add `BranchHeuristic` alongside binary bitmap
- Modify: `crates/apex-instrument/src/python.rs` — emit comparison operands in trace
- Modify: `crates/apex-fuzz/src/lib.rs` — use distance signal in corpus energy
- Modify: `crates/apex-agent/src/orchestrator.rs` — pass heuristics to strategies

- [ ] **Step 1: Define `BranchHeuristic` type**

Add to `apex-coverage`:
```rust
/// Continuous [0.0, 1.0] heuristic for a branch condition.
/// 1.0 = covered, 0.0 = maximally far from flipping.
#[derive(Debug, Clone, Copy)]
pub struct BranchHeuristic {
    pub branch_id: BranchId,
    pub score: f64,        // 0.0..=1.0
    pub operand_a: Option<i64>,
    pub operand_b: Option<i64>,
}
```

Distance functions (from EvoMaster's Korel approach):
- `a == b` → `1.0 - normalize(|a - b|)`  where `normalize(x) = x / (x + 1.0)`
- `a < b`  → if `a < b` then 1.0, else `1.0 - normalize(a - b + 1)`
- `a > b`  → if `a > b` then 1.0, else `1.0 - normalize(b - a + 1)`

- [ ] **Step 2: Add heuristic tracking to CoverageOracle**

Extend `CoverageOracle` with:
```rust
pub fn record_heuristic(&self, h: BranchHeuristic);
pub fn heuristics_for(&self, branch: &BranchId) -> Option<BranchHeuristic>;
pub fn best_heuristic(&self, branch: &BranchId) -> f64; // max score seen
```

- [ ] **Step 3: Extend Python instrumentor to emit operands**

Modify the tracer script (`apex_tracer.py`) to capture comparison operands:
```python
# In the trace callback, when a branch condition is evaluated:
# Emit: {"branch": [file_id, line, col, idx], "op": "==", "a": 40, "b": 42}
```

Parse these in `PythonConcolicStrategy` and feed to `oracle.record_heuristic()`.

- [ ] **Step 4: Use distance in FuzzStrategy corpus energy**

Modify `FuzzStrategy` to incorporate heuristic scores:
```rust
// Energy for corpus entry = base_energy * (1.0 + sum of near-miss heuristics)
// Near-miss = heuristic > 0.5 but < 1.0 (close to flipping but not yet covered)
```

- [ ] **Step 5: Write tests**

```rust
#[test]
fn distance_equality_exact_match() {
    assert_eq!(branch_distance_eq(42, 42), 1.0);
}

#[test]
fn distance_equality_close() {
    let d = branch_distance_eq(40, 42);
    assert!(d > 0.5 && d < 1.0); // close but not covered
}

#[test]
fn distance_equality_far() {
    let d = branch_distance_eq(0, 1000000);
    assert!(d < 0.01); // very far
}

#[test]
fn distance_less_than_satisfied() {
    assert_eq!(branch_distance_lt(5, 10), 1.0);
}

#[test]
fn distance_less_than_boundary() {
    let d = branch_distance_lt(10, 10);
    assert!(d < 1.0 && d > 0.5); // off by 1
}
```

- [ ] **Step 6: Run tests, verify pass**

- [ ] **Step 7: Commit**

---

## Task 2: Gradient Descent Constraint Solver

**Why:** APEX currently relies solely on Z3 (behind `z3-solver` feature flag, usually disabled). Angora's gradient descent solves numeric constraints 10-100x faster by treating branch conditions as distance functions minimized via partial derivatives. This requires only concrete executions — no SMT solver dependency.

**Mechanism from:** Angora's `fuzzer/src/search/gd.rs` + `grad.rs`

**Files:**
- Create: `crates/apex-symbolic/src/gradient.rs`
- Modify: `crates/apex-symbolic/src/traits.rs` — add `GradientSolver` implementing `Solver`
- Modify: `crates/apex-symbolic/src/portfolio.rs` — include gradient in portfolio

- [ ] **Step 1: Implement distance function for comparison predicates**

```rust
/// Compute distance to flipping a comparison.
/// Returns 0 when the branch flips.
pub fn comparison_distance(op: CmpOp, a: i64, b: i64) -> f64 {
    match op {
        CmpOp::Eq => (a - b).unsigned_abs() as f64,
        CmpOp::Ne => if a != b { 0.0 } else { 1.0 },
        CmpOp::Lt => if a < b { 0.0 } else { (a - b + 1) as f64 },
        CmpOp::Le => if a <= b { 0.0 } else { (a - b) as f64 },
        CmpOp::Gt => if a > b { 0.0 } else { (b - a + 1) as f64 },
        CmpOp::Ge => if a >= b { 0.0 } else { (b - a) as f64 },
    }
}
```

- [ ] **Step 2: Implement partial derivative computation**

```rust
/// Compute partial derivative of distance w.r.t. input variable.
/// Uses finite differences: perturb input by ±1, measure distance change.
pub struct PartialDerivative {
    pub var_name: String,
    pub gradient: f64,
    pub direction: Direction, // Positive or Negative
}

pub fn compute_gradient(
    variables: &[(String, i64)],  // current values
    op: CmpOp,
    target_operand: &str,  // which operand is input-derived
    other_value: i64,
) -> Vec<PartialDerivative> { ... }
```

- [ ] **Step 3: Implement descent algorithm**

```rust
pub struct GradientSolver {
    max_iterations: usize,  // default 100
    step_sizes: Vec<f64>,   // [1, 2, 4, 8, 16, ...]
}

impl GradientSolver {
    /// Attempt to solve constraint by gradient descent.
    /// Returns new variable values that satisfy the constraint, or None.
    pub fn solve_constraint(
        &self,
        op: CmpOp,
        variables: &mut [(String, i64)],
        target_operand_idx: usize,
        other_value: i64,
    ) -> Option<Vec<(String, i64)>> { ... }
}
```

- [ ] **Step 4: Implement `Solver` trait for `GradientSolver`**

The `Solver` trait expects SMTLIB2 strings. Parse simple comparison constraints:
```rust
impl Solver for GradientSolver {
    fn solve(&self, constraints: &[String], negate_last: bool) -> Result<Option<InputSeed>> {
        // Parse last constraint: "(> x 5)" → CmpOp::Gt, var="x", val=5
        // Run gradient descent
        // Encode solution as JSON InputSeed
    }
}
```

- [ ] **Step 5: Add to PortfolioSolver**

Add `GradientSolver` as the first solver in the portfolio (fast, no deps), falling back to Z3 for complex constraints.

- [ ] **Step 6: Write tests**

```rust
#[test]
fn gradient_solves_simple_equality() {
    let solver = GradientSolver::new(100);
    // Solve: x == 42, starting from x = 40
    let result = solver.solve_constraint(CmpOp::Eq, &mut [("x", 40)], 0, 42);
    assert_eq!(result.unwrap()[0].1, 42);
}

#[test]
fn gradient_solves_less_than() {
    let solver = GradientSolver::new(100);
    // Solve: x < 10, starting from x = 15
    let result = solver.solve_constraint(CmpOp::Lt, &mut [("x", 15)], 0, 10);
    assert!(result.unwrap()[0].1 < 10);
}

#[test]
fn distance_zero_means_solved() {
    assert_eq!(comparison_distance(CmpOp::Eq, 5, 5), 0.0);
    assert_eq!(comparison_distance(CmpOp::Lt, 3, 5), 0.0);
}
```

- [ ] **Step 7: Run tests, verify pass**

- [ ] **Step 8: Commit**

---

## Task 3: Native Code Property Graph (apex-cpg)

**Why:** APEX's `SecurityPatternDetector` does substring matching on source lines (`"subprocess.call("` as literal). Joern builds a Code Property Graph (AST + CFG + REACHING_DEF edges) and traces data flow from sources to sinks. For the same 92-line test app, Joern found all 3 vulnerabilities with complete flow paths; APEX's substring matcher missed `subprocess.run()` entirely and cannot trace data flow at all.

**Mechanism from:** Joern's CPG schema + REACHING_DEF pass + backward reachability

**Files:**
- Create: `crates/apex-cpg/Cargo.toml`
- Create: `crates/apex-cpg/src/lib.rs` — graph types
- Create: `crates/apex-cpg/src/builder.rs` — build CPG from Python AST
- Create: `crates/apex-cpg/src/reaching_def.rs` — MOP reaching definitions
- Create: `crates/apex-cpg/src/taint.rs` — backward reachability query
- Modify: `Cargo.toml` (workspace) — add `apex-cpg` member

- [ ] **Step 1: Define CPG node and edge types**

```rust
pub type NodeId = u32;

#[derive(Debug, Clone)]
pub enum NodeKind {
    Method { name: String, file: PathBuf, line: u32 },
    Parameter { name: String, index: u32 },
    Call { name: String, line: u32 },
    Identifier { name: String, line: u32 },
    Literal { value: String, line: u32 },
    Return { line: u32 },
    ControlStructure { kind: CtrlKind, line: u32 },
}

#[derive(Debug, Clone)]
pub enum EdgeKind {
    Ast,                           // parent → child
    Cfg,                           // control flow successor
    ReachingDef { variable: String }, // data dependency
    Argument { index: u32 },       // call → argument
}
```

- [ ] **Step 2: Build CPG from Python AST**

Use `tree-sitter-python` (already available via `apex-lang`) to parse Python and construct:
1. METHOD nodes for each `def`
2. CALL nodes for each function call
3. IDENTIFIER nodes for each name reference
4. CFG edges between sequential statements, branching at `if`/`while`/`for`
5. AST edges for parent-child relationships

- [ ] **Step 3: Implement reaching definitions (MOP algorithm)**

```rust
/// Compute reaching definitions using Meet-Over-all-Paths.
/// For each program point, determine which variable definitions can reach it.
pub fn compute_reaching_defs(cpg: &Cpg, method: NodeId) -> ReachingDefResult {
    // 1. Compute gen/kill sets for each node
    // 2. Iterate to fixpoint: out[n] = gen[n] ∪ (in[n] - kill[n])
    //                          in[n]  = ∪ out[p] for p in predecessors(n)
    // 3. Materialize as ReachingDef edges
}
```

- [ ] **Step 4: Implement backward taint reachability**

```rust
/// Given a set of sink nodes and source nodes, find all paths
/// from sources to sinks following REACHING_DEF edges backward.
pub fn reachable_by(
    cpg: &Cpg,
    sinks: &[NodeId],
    sources: &[NodeId],
    max_depth: usize,
) -> Vec<TaintFlow> {
    // BFS backward from each sink following incoming ReachingDef edges
    // Stop when a source is reached or max_depth exceeded
}

pub struct TaintFlow {
    pub source: NodeId,
    pub sink: NodeId,
    pub path: Vec<NodeId>,
    pub variable_chain: Vec<String>,
}
```

- [ ] **Step 5: Define security sources and sinks**

```rust
pub const PYTHON_SOURCES: &[&str] = &[
    "request.args", "request.form", "request.json",
    "input", "sys.argv", "os.environ",
];

pub const PYTHON_SINKS: &[&str] = &[
    "subprocess.run", "subprocess.call", "subprocess.Popen",
    "os.system", "os.popen", "eval", "exec",
    "cursor.execute", "conn.execute",
    "open", "os.remove",
];

pub const PYTHON_SANITIZERS: &[&str] = &[
    "shlex.quote", "os.path.normpath", "html.escape",
    "parameterized", "?",  // SQL parameterization
];
```

- [ ] **Step 6: Write tests**

```rust
#[test]
fn cpg_finds_command_injection_flow() {
    let src = r#"
def run_command(user_input):
    cmd = f"echo {user_input}"
    subprocess.run(cmd, shell=True)
"#;
    let cpg = build_python_cpg(src);
    let flows = reachable_by(&cpg, &sinks("subprocess.run"), &sources("user_input"), 10);
    assert!(!flows.is_empty(), "should find flow from parameter to subprocess.run");
}

#[test]
fn cpg_respects_sanitizer() {
    let src = r#"
def safe_run(user_input):
    safe = shlex.quote(user_input)
    subprocess.run(safe, shell=True)
"#;
    let cpg = build_python_cpg(src);
    let flows = reachable_by_with_sanitizers(&cpg, ...);
    assert!(flows.is_empty(), "sanitizer should break the taint flow");
}
```

- [ ] **Step 7: Run tests, verify pass**

- [ ] **Step 8: Commit**

---

## Task 4: LLM-Guided Test Refinement Strategy

**Why:** APEX's `apex-synth` writes test files from templates but never runs them, never checks coverage, and never refines. CoverUp's closed-loop `generate→run→measure→refine` with LLM improves coverage by iterating up to 3 times per gap. This is APEX's core differentiator (coverage-driven security) — the LLM loop makes it dramatically more effective.

**Mechanism from:** CoverUp's `improve_coverage()` loop + AST segment extraction + `get_info` tool

**Files:**
- Create: `crates/apex-synth/src/llm.rs` — LLM-guided synthesizer
- Modify: `crates/apex-synth/src/lib.rs` — export new synthesizer
- Modify: `crates/apex-agent/src/orchestrator.rs` — integrate as a Strategy

- [ ] **Step 1: Define the LLM synthesis loop**

```rust
pub struct LlmSynthesizer {
    pub max_attempts: usize,    // default 3 (from CoverUp)
    pub oracle: Arc<CoverageOracle>,
}

pub struct SynthAttempt {
    pub test_code: String,
    pub coverage_delta: Vec<BranchId>,  // newly covered branches
    pub error: Option<String>,          // test execution error
}

impl LlmSynthesizer {
    /// CoverUp-style refinement loop for a single coverage gap.
    pub async fn fill_gap(&self, gap: &CoverageGap) -> Result<Option<SynthAttempt>> {
        let mut messages = self.initial_prompt(gap);

        for attempt in 0..self.max_attempts {
            let test_code = self.call_llm(&messages).await?;
            let result = self.run_and_measure(&test_code).await?;

            match result {
                TestResult::Error(err) => {
                    messages.push(self.error_prompt(&err));
                    continue; // retry with error context
                }
                TestResult::NoCoverageGain => {
                    messages.push(self.missing_coverage_prompt(gap));
                    continue; // retry with "still missing" feedback
                }
                TestResult::Success(delta) => {
                    return Ok(Some(SynthAttempt { test_code, coverage_delta: delta, error: None }));
                }
            }
        }
        Ok(None) // exhausted attempts
    }
}
```

- [ ] **Step 2: Implement AST-based segment extraction (from CoverUp)**

```rust
/// Extract the smallest function/method containing the uncovered branch,
/// with line numbers tagged on lines of interest.
pub fn extract_segment(source: &str, target_line: u32) -> CodeSegment {
    // 1. Parse AST (tree-sitter)
    // 2. Find innermost function containing target_line
    // 3. Tag uncovered lines with line numbers (CoverUp format: "   42: code")
    // 4. Resolve imports used by the function
}
```

- [ ] **Step 3: Implement prompt construction**

```rust
/// Build initial prompt with coverage gap context.
/// Follows CoverUp's structure: role + file path + gap description + tagged code.
fn initial_prompt(&self, gap: &CoverageGap) -> Vec<Message> {
    // System: "You are an expert test developer. Generate a test that covers..."
    // User: source excerpt with tagged lines + gap description
}

/// Build error feedback prompt.
fn error_prompt(&self, error: &str) -> Message {
    // "Executing the test yields an error: {cleaned_error}. Modify the test to fix it."
}

/// Build missing-coverage feedback prompt.
fn missing_coverage_prompt(&self, gap: &CoverageGap) -> Message {
    // "The test runs but lines {X-Y} still don't execute. Modify it to cover them."
}
```

- [ ] **Step 4: Implement as Strategy trait**

```rust
/// Wraps LlmSynthesizer as a Strategy for the AgentCluster.
pub struct LlmSynthStrategy {
    synth: LlmSynthesizer,
}

#[async_trait]
impl Strategy for LlmSynthStrategy {
    fn name(&self) -> &str { "llm-synth" }

    async fn suggest_inputs(&self, ctx: &ExplorationContext) -> Result<Vec<InputSeed>> {
        // For each uncovered branch, extract segment, run LLM loop
        // Return InputSeeds that encode the generated test code
    }

    async fn observe(&self, result: &ExecutionResult) -> Result<()> {
        // Track which gaps were filled, update priority queue
    }
}
```

- [ ] **Step 5: Write tests**

```rust
#[test]
fn segment_extraction_finds_function() {
    let src = "def foo():\n    x = 1\n    if x > 0:\n        return x\n    return 0\n";
    let seg = extract_segment(src, 4); // line 4: "return x"
    assert!(seg.code.contains("def foo"));
    assert!(seg.tagged_lines.contains("4:"));
}

#[test]
fn error_prompt_cleans_pytest_output() {
    let raw = "===== FAILURES =====\nAssertionError: ...\n===== short test summary =====";
    let cleaned = clean_error_output(raw);
    assert!(!cleaned.contains("====="));
    assert!(cleaned.contains("AssertionError"));
}
```

- [ ] **Step 6: Run tests, verify pass**

- [ ] **Step 7: Commit**

---

## Task 5: Priority-Based Exploration Scheduler

**Why:** APEX's orchestrator round-robins through strategies with simple stall detection. Owi's composite priority scheduler (rarity + depth + loop penalty + aging) and EvoMaster's feedback-directed target sampling provide much better exploration guidance.

**Mechanism from:** Owi's `Prio` module + EvoMaster's `Archive.chooseTarget()`

**Files:**
- Modify: `crates/apex-agent/src/orchestrator.rs` — replace round-robin with priority scheduler
- Create: `crates/apex-agent/src/priority.rs` — priority computation

- [ ] **Step 1: Define priority scoring for coverage targets**

```rust
/// Priority score for an uncovered branch — higher = explore first.
pub fn target_priority(
    branch: &BranchId,
    heuristic: f64,           // from Task 1: how close we've gotten
    attempts_since_progress: u64,  // from EvoMaster: sampling staleness
    depth_in_cfg: u32,        // from Owi: penalize deep paths
    hit_count: u64,           // from Owi: rarity — prefer rarely-reached code
) -> f64 {
    let rarity = 1.0 / (hit_count as f64 + 1.0);
    let depth_penalty = 1.0 / (depth_in_cfg as f64).ln_1p().max(1.0);
    let staleness_bonus = if attempts_since_progress > 5 { 0.5 } else { 0.0 };
    let proximity = heuristic; // closer to flipping = higher priority

    rarity * depth_penalty * (1.0 + proximity) + staleness_bonus
}
```

- [ ] **Step 2: Replace round-robin with priority queue**

```rust
// In AgentCluster::run():
// 1. Compute priority for each uncovered branch
// 2. Select top-K branches to focus on this iteration
// 3. Choose strategy based on branch characteristics:
//    - High heuristic (>0.8): use GradientSolver (close, nudge it over)
//    - Medium heuristic: use FuzzStrategy (mutation might reach it)
//    - Low heuristic: use LlmSynthStrategy (need structured approach)
//    - Stalled (>10 attempts): rotate to next strategy
```

- [ ] **Step 3: Add solver caching (from Owi)**

```rust
/// Cache satisfiability results with negation inference.
/// If ¬C is cached as UNSAT, infer C is SAT without querying solver.
pub struct SolverCache {
    cache: HashMap<String, SatResult>,
}

impl SolverCache {
    pub fn check(&self, constraint: &str) -> Option<SatResult> {
        if let Some(r) = self.cache.get(constraint) {
            return Some(*r);
        }
        // Negation inference: if not(C) is UNSAT, C is SAT
        let negated = format!("(not {})", constraint);
        if self.cache.get(&negated) == Some(&SatResult::Unsat) {
            return Some(SatResult::Sat);
        }
        None
    }
}
```

- [ ] **Step 4: Write tests**

```rust
#[test]
fn priority_prefers_rare_branches() {
    let rare = target_priority(&b1, 0.5, 0, 3, 1);
    let common = target_priority(&b2, 0.5, 0, 3, 100);
    assert!(rare > common);
}

#[test]
fn priority_penalizes_deep_paths() {
    let shallow = target_priority(&b1, 0.5, 0, 2, 10);
    let deep = target_priority(&b2, 0.5, 0, 20, 10);
    assert!(shallow > deep);
}

#[test]
fn solver_cache_negation_inference() {
    let mut cache = SolverCache::new();
    cache.insert("(not (> x 5))".into(), SatResult::Unsat);
    assert_eq!(cache.check("(> x 5)"), Some(SatResult::Sat));
}
```

- [ ] **Step 5: Run tests, verify pass**

- [ ] **Step 6: Commit**

---

## Task 6: CWE ID Mapping on Findings

**Why:** This is the one "pattern matching" task retained because it's structural, not behavioral. Bearer maps every finding to CWE IDs — table-stakes for compliance reporting (SOC2, HIPAA, PCI-DSS). Adding `cwe_ids: Vec<u32>` to `Finding` and mapping from existing categories is mechanical and orthogonal to the mechanism work above.

**Files:**
- Modify: `crates/apex-detect/src/finding.rs` — add `cwe_ids` field
- Modify: `crates/apex-detect/src/detectors/security_pattern.rs` — add `cwe` field to patterns
- Modify: `crates/apex-detect/src/detectors/hardcoded_secret.rs` — CWE-798
- Modify: `crates/apex-detect/src/detectors/path_normalize.rs` — CWE-22

- [ ] **Step 1: Add `cwe_ids` field to Finding struct**

```rust
pub struct Finding {
    // ... existing fields ...
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cwe_ids: Vec<u32>,
}
```

- [ ] **Step 2: Add `cwe` to SecurityPattern and map all existing patterns**

| Category | CWE |
|----------|-----|
| eval/exec/pickle/yaml | CWE-94 (Code Injection) |
| subprocess/os.system | CWE-78 (OS Command Injection) |
| SQL .execute() | CWE-89 (SQL Injection) |
| innerHTML/document.write | CWE-79 (XSS) |
| MD5/SHA1 | CWE-328 (Weak Hash) |
| verify=False | CWE-295 (Certificate Validation) |
| gets/strcpy/sprintf | CWE-120 (Buffer Overflow) |
| Marshal.load/YAML.load | CWE-502 (Deserialization) |
| hardcoded secret | CWE-798 (Hard-coded Credentials) |
| path traversal | CWE-22 (Path Traversal) |

- [ ] **Step 3: Propagate to all detectors + write tests**

- [ ] **Step 4: Run tests, verify pass**

- [ ] **Step 5: Commit**

---

## Verification

After all tasks:
```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

Expected outcomes:
- **Task 1:** `CoverageOracle` reports continuous [0,1] heuristics alongside binary bitmap. Fuzzer uses distance to guide mutations.
- **Task 2:** `GradientSolver` solves simple numeric constraints without Z3. Falls back to Z3 for complex/non-numeric.
- **Task 3:** `apex-cpg` builds a graph from Python source and finds taint flows via backward REACHING_DEF traversal.
- **Task 4:** `LlmSynthStrategy` generates tests, runs them, checks coverage, and retries on failure — closed loop.
- **Task 5:** Orchestrator selects targets by priority (rarity × proximity) and routes to appropriate strategy.
- **Task 6:** All findings carry CWE IDs for compliance reporting.

## Key Files Reference

| File | Role |
|---|---|
| `crates/apex-coverage/src/lib.rs` | CoverageOracle + BranchHeuristic — Task 1 |
| `crates/apex-symbolic/src/gradient.rs` | NEW: Gradient descent solver — Task 2 |
| `crates/apex-cpg/src/lib.rs` | NEW: Code Property Graph types — Task 3 |
| `crates/apex-cpg/src/reaching_def.rs` | NEW: Reaching definitions — Task 3 |
| `crates/apex-cpg/src/taint.rs` | NEW: Backward taint query — Task 3 |
| `crates/apex-synth/src/llm.rs` | NEW: LLM-guided synthesizer — Task 4 |
| `crates/apex-agent/src/priority.rs` | NEW: Priority scheduler — Task 5 |
| `crates/apex-agent/src/orchestrator.rs` | Strategy routing — Tasks 1, 4, 5 |
| `crates/apex-detect/src/finding.rs` | Finding types + CWE — Task 6 |

## Dependency Graph

```
Task 1 (branch distance) ──→ Task 5 (priority scheduler uses heuristics)
Task 2 (gradient solver)  ──→ Task 5 (scheduler routes to gradient for near-miss)
Task 3 (CPG) ─────────────→ independent (new crate, no deps on other tasks)
Task 4 (LLM synth) ───────→ Task 5 (scheduler routes to LLM for hard gaps)
Task 6 (CWE mapping) ─────→ independent (finding struct extension)
```

Tasks 1, 2, 3, 6 can proceed in parallel. Task 5 depends on Tasks 1+2+4. Task 4 is independent but benefits from Task 1's heuristic data.
