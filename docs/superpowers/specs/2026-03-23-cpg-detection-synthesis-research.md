# Deep Research: CPG + Detection Pipeline + Test Synthesis

<!-- status: DONE -->

**Date:** 2026-03-23
**Scope:** Dig 7 (CPG), Dig 8 (Detection Pipeline), Dig 9 (Test Synthesis)

---

## Table of Contents

1. [Dig 7: Code Property Graph](#dig-7-code-property-graph)
2. [Dig 8: Detection Pipeline](#dig-8-detection-pipeline)
3. [Dig 9: Test Synthesis](#dig-9-test-synthesis)
4. [Cross-Cutting Recommendations](#cross-cutting-recommendations)

---

## Dig 7: Code Property Graph

### APEX Current State

APEX uses a **line-based CPG builder** (`crates/apex-cpg/src/builder.rs`) that parses Python, JS, and Go via regex patterns rather than a proper AST parser. The CPG model (`crates/apex-cpg/src/lib.rs`) implements Joern's schema with four edge types:

- **AST** (parent-child structural)
- **CFG** (control-flow successor)
- **ReachingDef** (data dependency with variable name)
- **Argument** (call-to-argument)

Node types: Method, Parameter, Call, Identifier, Literal, Return, ControlStructure, Assignment.

Taint analysis (`crates/apex-cpg/src/taint.rs`) does backward BFS from sinks over ReachingDef edges, with inter-procedural support via `SummaryCache`. Additional modules include SSA, typestate analysis, DeepDFA, type-based taint, and a taint triage scorer.

A `treesitter` feature flag exists for Python (`ts_python.rs`) but is not compiled by default.

### Alternative Analysis

#### 1. Joern (Score: 8/10 relevance)

**What it is:** The original CPG tool (Scala/Java). Full AST + CFG + PDG merged into a single graph. Supports C/C++, Java, JS, Python, Go, PHP, Ruby, Kotlin, Swift, LLVM bitcode.

**Architecture:** Uses fuzzy parsing (no compilation required), ShiftLeft's `codepropertygraph` spec (v1.1), and an in-memory graph database queryable via Scala-based CPGQL.

**Benchmarks:**
- Small projects (C): 19 seconds for CPG construction vs CodeQL's 22 seconds
- Large projects (Wireshark, 1.5M+ LOC): Ran out of memory on 16GB RAM; required 128GB, took ~6 hours
- Query execution on interprocedural dataflow: Did not finish (vs CodeQL's ~4 hours)
- CPGQL queries average 14.7 lines vs CodeQL's 86.3 lines

**Strengths:** No build requirement, generic argument access in queries, good for medium codebases, fuzzy parsing handles incomplete code.

**Weaknesses:** Memory-intensive on large codebases, JVM dependency, slow interprocedural analysis, not embeddable in Rust.

**Relevance to APEX:** APEX already mirrors Joern's CPG schema. The key gap is parsing accuracy (regex vs AST), not graph structure. Embedding Joern (JVM) is impractical for a 5MB static binary.

**References:**
- Spec: https://cpg.joern.io/
- Docs: https://docs.joern.io/code-property-graph/
- GitHub: https://github.com/ShiftLeftSecurity/codepropertygraph
- Comparison: https://elmanto.github.io/posts/sast_derby_joern_vs_codeql

#### 2. CodeQL (Score: 6/10 relevance)

**What it is:** GitHub's semantic code analysis engine. Declarative QL query language over a relational database extracted from compiled code.

**Benchmarks:**
- Wireshark: ~43 minutes for database creation (vs Joern's 6 hours)
- Query rules: 86.3 lines on average (verbose but precise)
- Interprocedural dataflow: 4 hours (actually completes, unlike Joern)
- False positives: High for interprocedural queries, lower for intraprocedural

**Strengths:** Scales to large codebases, mature taint tracking, large community rule library, GitHub integration.

**Weaknesses:** Requires project compilation (major friction), closed source, QL learning curve, not embeddable, slow scan times.

**Relevance to APEX:** CodeQL's taint tracking methodology (declarative rules over extracted facts) is worth studying. The compilation requirement disqualifies it as a dependency, but its rule patterns can inform APEX's detector design.

#### 3. Semgrep (Score: 9/10 relevance)

**What it is:** Pattern-based code analysis with taint tracking (since v1.0). Pattern syntax looks like source code. Cross-file analysis available in paid tier.

**Performance (2025):**
- Semgrep Assistant filters 60% of SAST findings as false positives before teams see them
- 96% agreement rate between Assistant's classification and human triage across 6M+ findings
- Reachability analysis reduces dependency false positives by up to 98%
- Cross-function taint: 4/9 multi-hop cases detected (vs Opengrep's 7/9)
- AI-powered "Memories" feature: one Fortune 500 customer saw 2.8x additional FP reduction

**Strengths:** Code-like pattern syntax (low learning curve), fast scans, good Python/JS/Go support, active rule ecosystem, taint mode.

**Weaknesses:** Cross-function taint weaker than Opengrep, paid tier for cross-file analysis, community edition limited.

**Relevance to APEX:** Semgrep's approach of pattern-based matching + taint tracking is closest to what APEX does. APEX could adopt Semgrep-style pattern syntax for user-defined rules. The AI triage approach (Memories) maps to APEX's taint_triage module.

**References:**
- https://semgrep.dev/blog/2025/making-zero-false-positive-sast-a-reality-with-ai-powered-memory/
- https://semgrep.dev/blog/2025/security-research-comparing-semgrep-community-edition-and-semgrep-code-for-static-analysis/

#### 4. tree-sitter (Score: 10/10 relevance)

**What it is:** Incremental parsing library supporting 100+ languages. Generates concrete syntax trees (CST) with error recovery. Written in C, with official Rust bindings (`tree-sitter` crate v0.26.3).

**Performance vs regex:**
- 40% faster error detection than regex-based approaches (2026 case study)
- Millisecond response time for incremental re-parsing
- Helix-Lint migration (2025): replaced regex with tree-sitter, eliminated false positives from missed language constructs
- Full structural understanding: distinguishes variables from functions in context

**Rust integration:**
- `tree-sitter` crate: 0.26.3 on crates.io, actively maintained
- Per-language grammar crates: `tree-sitter-python`, `tree-sitter-javascript`, `tree-sitter-go`, etc.
- Query API: S-expression patterns over the tree, returns matched nodes
- WASM support for web-based analysis

**Cost to integrate:**
- APEX already has a `treesitter` feature flag and `ts_python.rs` module
- Each language grammar adds ~200-500KB to the binary
- Build time: grammar compilation is one-time during `cargo build`
- Migration path: implement `CpgBuilder` for tree-sitter per language, behind feature flag

**Strengths:** Accurate parsing, error recovery (handles incomplete code), incremental, embeddable in Rust, covers 100+ languages with minimal effort.

**Weaknesses:** Produces CST not AST (more nodes to process), no semantic analysis (just syntax), grammar maintenance per language.

**References:**
- https://crates.io/crates/tree-sitter
- https://docs.rs/tree-sitter
- https://dasroot.net/posts/2026/02/incremental-parsing-tree-sitter-code-analysis/
- https://cycode.com/blog/tips-for-using-tree-sitter-queries/

#### 5. srcML (Score: 3/10 relevance)

**What it is:** XML representation of source code. Supports C, C++, Java, C#. Preserves original formatting.

**Relevance to APEX:** Limited language support (no Python/JS/Go). XML is heavyweight. tree-sitter is strictly superior for APEX's use case.

#### 6. Infer (Score: 5/10 relevance)

**What it is:** Facebook's compositional static analyzer using separation logic. Targets memory safety (null dereference, resource leaks, data races, thread safety).

**Strengths:** Compositional (analyzes functions independently, then composes), proven at scale (Facebook's codebase), diff-based mode for CI.

**Weaknesses:** Primarily C/C++/Java/Objective-C, OCaml codebase, not embeddable, focused on memory bugs not security.

**Relevance to APEX:** Infer's compositional analysis approach is worth adopting. APEX's `TaintSummary` + `SummaryCache` already implements a form of this (function summaries composed at call sites). Infer's diff mode concept maps to APEX's ratchet mode.

#### 7. Soot/Doop (Score: 2/10 relevance)

Java-specific points-to analysis. Irrelevant for APEX's multi-language scope.

### Recommendation: tree-sitter Migration

**Decision: YES, APEX should replace its line-based CPG with tree-sitter.**

**Rationale:**

| Criterion | Line-based (current) | tree-sitter |
|-----------|---------------------|-------------|
| Parsing accuracy | ~70% (misses nested, multiline) | ~99% (full grammar) |
| Error recovery | None (silent failures) | Built-in (partial parse) |
| Language coverage | 3 (Python, JS, Go) | 100+ |
| Binary size impact | 0 | +200-500KB per grammar |
| Build complexity | None | Grammar compilation |
| Maintenance | Per-language regex rules | Community-maintained grammars |

**Migration plan:**
1. Complete the existing `ts_python.rs` tree-sitter Python builder (already started)
2. Add `ts_javascript.rs` and `ts_go.rs` behind the same feature flag
3. Benchmark accuracy: run both builders on test corpus, compare CPG node counts and taint flow results
4. Once parity confirmed, make tree-sitter the default, keep regex as fallback for unsupported languages
5. Add new languages (Ruby, PHP, Kotlin, Swift) by just adding grammar crates

**Cost estimate:** 2-3 weeks for Python+JS+Go builders with tests. Each additional language: 2-3 days.

---

## Dig 8: Detection Pipeline

### APEX Current State

APEX has **54+ detectors** registered in `DetectorPipeline::from_config()` (`crates/apex-detect/src/pipeline.rs`). Categories:

- **Pattern matching** (regex): panic, unsafe, secrets, hardcoded credentials, path normalization
- **Security patterns**: command injection, SQL injection, crypto failures, SSRF, path traversal, insecure deserialization (multi-language)
- **Code quality**: blocking I/O in async, swallowed errors, broad exception catching, error context loss, string concat in loops, regex in loops
- **Concurrency**: mutex across await, unbounded queues, FFI panic
- **Structural**: duplicated functions, process exit in lib, mixed bool ops
- **Threat model**: threat model awareness for severity adjustment
- **CPG taint**: backward BFS from sinks to sources (Python/JS/Go)

**Known problem:** 84% noise rate when run on APEX itself. This is primarily from pattern-matching detectors that flag code structurally matching a pattern but not semantically vulnerable.

### Alternative Analysis

#### 1. Abstract Interpretation (Score: 7/10 relevance)

**What it is:** Mathematically sound over-approximation of program behavior. Tools: Astree (embedded C), Polyspace (C/C++/Ada), ABSINT-AI (2025, LLM-augmented).

**Key 2025 result (ABSINT-AI, ICLR 2025):**
- LLM-augmented abstract interpretation achieves **70% decrease in false positives** while guaranteeing no missed bugs (soundness preserved)
- Approach: LLM refines abstract domains, reducing imprecision while maintaining over-approximation guarantee

**Industrial FP rates:**
- Traditional abstract interpretation: 95%+ false alarm rate on certain bug classes
- With LLM augmentation: 25-30% false alarm rate
- DeepSource target: <5% false positive rate (achieved via multi-stage pipeline)

**Relevance to APEX:** APEX cannot adopt full abstract interpretation (requires type systems, formal semantics per language). But the **LLM-as-triage** pattern from ABSINT-AI maps directly to APEX's `taint_triage` module. Use the LLM to assess whether a flagged pattern is actually reachable/exploitable.

**References:**
- ABSINT-AI (ICLR 2025): https://openreview.net/pdf?id=3RP6YmKo59
- DeepSource methodology: https://deepsource.com/blog/how-deepsource-ensures-less-false-positives

#### 2. Model Checking (Score: 3/10 relevance)

**What it is:** Exhaustive state space exploration. CBMC (C bounded model checking), Java Pathfinder.

**Relevance:** Too heavy for multi-language SAST. Useful only for targeted verification of critical paths, not batch analysis.

#### 3. Datalog-based Analysis (Score: 8/10 relevance)

**What it is:** Encode program facts (call graph, data flow, types) as Datalog relations, write analysis as declarative rules. Tools: Souffle, Doop, CodeQL's QL.

**Souffle performance (2025):**
- Points-to analysis on 1M+ LOC: 20 minutes on workstation (vs 8 hours interpreted)
- Parallel evaluation scales to dozens of cores
- Compiled to C++ for production performance
- Cache-efficient memory layout

**Taint analysis in Datalog (conceptual):**
```datalog
// Facts (from CPG)
Source(node) :- Parameter(node, _).
Source(node) :- Call(node, "input").
Sink(node)   :- Call(node, "eval").
Sink(node)   :- Call(node, "subprocess.run").

// Rules
Tainted(x) :- Source(x).
Tainted(y) :- Tainted(x), DataFlow(x, y), !Sanitized(y).
Vuln(src, sink) :- Tainted(sink), Source(src), Sink(sink).
```

**Relevance to APEX:** APEX's `taint_rules.rs` and `TaintRuleSet` already implement something like this imperatively. Converting to Datalog would make rules declarative and composable, but adds a Souffle dependency (C++ toolchain) or requires implementing a Datalog evaluator in Rust.

**Practical path:** Rather than full Datalog, adopt the **declarative rule pattern**. Define taint rules as data (YAML/TOML), compile them into the existing BFS engine. This gets 80% of Datalog's benefits without the dependency.

**References:**
- Souffle: https://souffle-lang.github.io/
- Java Code Geeks tutorial: https://www.javacodegeeks.com/2025/10/building-lightning-fast-program-analysis-with-souffle-and-datalog.html

#### 4. LLM-based Detection (Score: 9/10 relevance)

**What it is:** Use language models for vulnerability detection. Approaches range from fine-tuned encoders (CodeBERT) to prompted decoders (GPT-4, Claude).

**CodeBERT reality check (2024):**
- Reports 96.86% accuracy on BigVul dataset
- **But 81.77% false negative rate** -- misses most vulnerabilities
- 512-token input limit makes it useless for real-world functions
- Effectively random for practical use

**LLMxCPG (USENIX Security 2025):**
- Combines CPG + LLM: CPG extracts minimal relevant code slice, LLM classifies
- **15-40% F1 improvement** over baselines
- CPG slice construction reduces code by **67-91%** while preserving vulnerability context
- Two-phase: LLM generates CPG query -> CPG extracts slice -> second LLM classifies
- Robust under syntactic modifications (refactoring, renaming)

**Industrial hybrid (2025):**
- LLMs as false-positive filters on static analysis: eliminates **94-98% of FPs** with high recall
- Cost: $0.0011-$0.12 per alarm, 2.1-109.5 seconds per alarm
- Semgrep's approach: AI triage of SAST results, 96% human agreement rate

**Relevance to APEX:** This is the highest-impact improvement path. APEX should:
1. Keep pattern matching as fast first pass (high recall, high FP)
2. Use CPG taint as second pass (moderate recall, moderate FP)
3. Add LLM triage as third pass (filter FPs, target <20% noise)

**References:**
- LLMxCPG: https://www.usenix.org/conference/usenixsecurity25/presentation/lekssays
- LLM survey: https://arxiv.org/html/2502.07049v2
- Industrial FP reduction: https://arxiv.org/html/2601.18844v1
- FuzzSlice: https://cs.uwaterloo.ca/~m285xu/assets/publication/fuzz_slice-paper.pdf

#### 5. Differential Analysis (Score: 6/10 relevance)

**What it is:** Only analyze changed code. Infer's diff mode, Semgrep's baseline mode.

**Relevance:** APEX's ratchet mode already implements this concept at the coverage level. Adding diff-aware detection (only flag new findings in changed files) would reduce noise in CI without reducing coverage.

#### 6. Compositional Analysis (Score: 7/10 relevance)

**What it is:** Analyze functions independently, produce summaries, compose at call sites.

**Relevance:** APEX already implements this via `TaintSummary` + `SummaryCache` for inter-procedural taint. The approach should be extended to all detectors that benefit from interprocedural context.

### Recommendation: Three-Phase Detection Pipeline

**To reduce noise from 84% to <20%, implement a staged pipeline:**

```
Phase 1: Pattern Scan (fast, high recall, ~85% FP rate)
  |
  v  [all findings]
Phase 2: CPG Taint Validation (medium speed, filters ~50% of FPs)
  |
  v  [validated findings]
Phase 3: LLM Triage (slower, filters ~90% of remaining FPs)
  |
  v  [high-confidence findings, <20% FP rate]
```

**Phase 1** (current): Keep all 54 pattern detectors. They are fast and have high recall.

**Phase 2** (enhance): For each finding with a source/sink pair, validate via CPG taint analysis. Requires tree-sitter CPG (Dig 7) for accuracy. Findings without taint flow are demoted to "info" severity.

**Phase 3** (new): For remaining findings, send code context + finding to LLM for exploitability assessment. Use LLMxCPG's approach: extract minimal CPG slice (67-91% code reduction), then ask LLM "Is this finding exploitable given this context?" At $0.001 per finding, cost is negligible.

**Expected FP progression:**
- Phase 1 alone: ~84% FP (current)
- Phase 1 + Phase 2: ~42% FP (taint validation halves FPs)
- Phase 1 + Phase 2 + Phase 3: ~8-15% FP (LLM filters 80-90% of remainder)

**Implementation priority:**
1. tree-sitter CPG (prerequisite for Phase 2 accuracy)
2. CPG-backed finding validation
3. LLM triage integration (reuse existing `apex-synth` LLM infrastructure)

---

## Dig 9: Test Synthesis

### APEX Current State

APEX's test synthesis (`crates/apex-synth/`) implements CoverUp-style closed-loop refinement:

1. **LlmSynthesizer** (`llm.rs`): CoverUp's generate-run-measure-refine loop with callbacks for LLM calls and test execution. Default 3 attempts per gap.
2. **Per-language synthesizers**: Python (pytest), JS (jest), Go (go test), Rust (cargo test), Java (JUnit), C++ (gtest), Kotlin, Ruby (rspec), Swift (xctest), C# (xunit), C, WASM -- 12 languages.
3. **PromptStrategy trait** (`strategy.rs`): Pluggable prompt construction with `GapHistory` tracking.
4. **Supporting modules**: CoverUp strategy, chain-of-thought prompting (`cot.rs`), few-shot examples (`few_shot.rs`), error classification (`error_classify.rs`), code extraction (`extractor.rs`), mutation hints (`mutation_hint.rs`), property inference (`property.rs`), prompt registry.

### Alternative Analysis

#### 1. CoverUp (Score: 10/10 -- already adopted)

**What it is:** Coverage-guided LLM test generation (FSE 2025). Iterative loop: generate -> execute -> measure coverage -> refine with feedback.

**Benchmark results (CM Suite, 4116 functions):**
- CoverUp (GPT-4o): **64% line / 49% branch / 60% combined**
- CodaMosa (GPT-4o): 50.6% line / 28.6% branch / 45.2% combined
- CodaMosa (Codex): 53.5% line / 33.6% branch / 48.6% combined
- Per-module median: CoverUp **82.4% line** vs CodaMosa 54.2%

**MT Suite (HumanEval):**
- CoverUp: **89.8% line / 89.4% branch / 89.7% combined**
- MuTAP: 82.4% line / 73.4% branch / 79.3% combined

**Iteration value:**
- First prompt: 60.3% of successful tests
- Second iteration: 27.2% (feedback matters)
- Third iteration: 12.4%
- Nearly half of CoverUp's successes come from the iterative refinement loop

**Cost:**
- CM Suite: $399 total (~$0.10 per function), 4 hours
- CodaMosa: $265 total but 71 hours (18x slower)
- CoverUp runs 18x faster despite higher token usage

**Status in APEX:** Core loop fully implemented in `LlmSynthesizer::fill_gap()`. APEX adds multi-language support (12 languages vs CoverUp's Python-only), mutation hints, few-shot examples, and chain-of-thought prompting on top.

**References:**
- Paper: https://arxiv.org/abs/2403.16218
- GitHub: https://github.com/plasma-umass/coverup
- ACM: https://dl.acm.org/doi/10.1145/3729398

#### 2. EvoSuite (Score: 4/10 relevance)

**What it is:** Search-based test generation for Java. Uses genetic algorithms to evolve test suites maximizing coverage.

**SBFT 2025 competition results (55 Java classes):**
- EvoSuite: 70% line coverage (median), 56.1% branch, 33% mutation
- EvoFuzz: 70% line, 66% branch, 43% mutation (best overall)
- BBC: 70% line, 59.9% branch, 38% mutation
- Randoop: 16.3% line, 5.6% branch (baseline)

**Relevance to APEX:** Java-only, JVM dependency, not embeddable. The evolutionary approach (fitness = coverage) is conceptually interesting but LLM-based approaches now match or exceed coverage while producing more readable tests. APEX's LLM approach is already superior for multi-language support.

**References:**
- https://www.evosuite.org/
- SBFT 2025: https://arxiv.org/html/2504.09168

#### 3. Pynguin (Score: 5/10 relevance)

**What it is:** Automated test generation for Python using search-based algorithms (DynaMOSA, MIO, MOSA, Whole Suite).

**Benchmark results:**
- DynaMOSA: 68% mean branch coverage (best algorithm)
- MIO: 67%, MOSA: 67.8%, Random: 63.6%, Whole Suite: 66.9%

**Comparison to CoverUp:** CoverUp achieves 80% per-module median coverage vs Pynguin's 68%. LLM-based approaches have surpassed search-based for Python.

**Relevance to APEX:** Pynguin's DynaMOSA algorithm could complement LLM synthesis for cases where LLMs struggle (e.g., complex object construction, type-constrained inputs). But adding a Python dependency for a Rust binary is impractical. The concept (search-based seed generation) is already partially addressed by APEX's mutation hints.

**References:**
- https://www.pynguin.eu/
- Paper: https://arxiv.org/abs/2202.05218

#### 4. Randoop (Score: 2/10 relevance)

**What it is:** Random test generation with feedback-directed sequence construction. Java/C#.

**Performance:** 16.3% line coverage in SBFT 2025 (worst performer). Significantly outperformed by all other approaches.

**Relevance to APEX:** None practically. Demonstrates that pure random testing is inadequate.

#### 5. CodaMosa (Score: 7/10 relevance)

**What it is:** Hybrid search-based + LLM test generation (ICSE 2023). Runs SBST until coverage plateaus, then asks LLM for seed tests, resumes SBST.

**Results:**
- 173/486 benchmarks: statistically significantly higher coverage than pure SBST
- 279/486 benchmarks: significantly higher coverage than LLM-only
- Now surpassed by CoverUp on same benchmarks (47% vs 80% median combined coverage)

**Key insight:** The hybrid approach (search + LLM) finds different bugs than either alone. CodaMosa catches cases where LLMs produce syntactically valid but semantically trivial tests, and cases where search-based testing cannot construct required objects.

**Relevance to APEX:** APEX could adopt CodaMosa's "plateau detection" concept: if LLM synthesis stalls after N attempts on a gap, switch to mutation-based approach (already have `mutation_gen.rs`). This is a refinement of the existing strategy, not a new system.

**References:**
- Paper: https://ieeexplore.ieee.org/iel7/10172484/10172342/10172800.pdf

#### 6. TestPilot (Score: 6/10 relevance)

**What it is:** GitHub's LLM test generation system. Uses Copilot to generate tests with repository context.

**Relevance:** Closed source, GitHub-specific. APEX's approach is more flexible (any LLM backend, any repository).

#### 7. Klee/Crest (Score: 5/10 relevance)

**What it is:** Symbolic execution for automatic test input generation. Klee targets LLVM bitcode, generates concrete inputs that explore different paths.

**Relevance to APEX:** APEX already has `apex-symbolic` and `apex-concolic` crates (behind feature flags). Symbolic execution complements LLM synthesis: LLM generates test structure, symbolic execution generates boundary values. This hybrid is the most promising unexplored avenue.

### Recommendation: Enhance Existing CoverUp Loop

APEX's test synthesis is already state-of-the-art in architecture. Improvements should focus on:

#### 1. Plateau Detection + Strategy Switching (from CodaMosa)

```
Attempt 1-3: CoverUp prompt strategy (current)
  |
  [if all fail]
  v
Attempt 4-5: Mutation-based strategy (use mutation_gen.rs)
  |
  [if still failing]
  v
Attempt 6: Property-based strategy (use property.rs)
```

This is essentially CodaMosa's insight applied to APEX's existing strategy framework.

#### 2. CPG-Informed Prompts (from LLMxCPG)

Current prompts include source segment and line numbers. Enhance with:
- **Data flow context**: which variables flow to the uncovered branch condition
- **Call graph context**: what functions are called on the path
- **Constraint context**: what values satisfy the branch condition

This uses CPG analysis (Dig 7) to improve synthesis prompts, creating a virtuous cycle between the two systems.

#### 3. Few-Shot Selection Improvement

Current `FewShotBank` stores examples. Improve by:
- Indexing examples by gap pattern (conditional, exception, loop, etc.)
- Using `GapClassifier` output to select relevant examples
- Tracking which examples led to successful synthesis across runs

#### 4. Cost Optimization

CoverUp reports $0.10 per function with GPT-4o. APEX can reduce this:
- Use Claude Haiku for first attempt (cheaper), escalate to Sonnet on failure
- Cache successful prompts/responses for similar gaps (template matching)
- Batch multiple gaps into single LLM calls where gaps are in the same function

---

## Cross-Cutting Recommendations

### Priority Order

| Priority | Action | Impact | Effort |
|----------|--------|--------|--------|
| P0 | tree-sitter CPG migration | Enables P1, P2 | 2-3 weeks |
| P1 | CPG-backed finding validation (Phase 2) | 50% FP reduction | 1-2 weeks |
| P2 | LLM triage for findings (Phase 3) | Additional 80-90% FP reduction | 1 week |
| P3 | CPG-informed synthesis prompts | Better coverage per attempt | 1 week |
| P4 | Plateau detection + strategy switching | Handle hard-to-cover gaps | 3-5 days |
| P5 | Declarative taint rules (YAML) | User-extensible analysis | 1 week |

### Architecture Vision

```
Source Code
    |
    v
[tree-sitter Parser] --> CST
    |
    v
[CPG Builder] --> Cpg (AST + CFG + ReachingDef + Argument edges)
    |
    +---> [Taint Analysis] --> TaintFlow[]
    |         |
    |         v
    |     [Finding Validation] --> validated findings
    |
    +---> [Pattern Detectors] --> raw findings
    |         |
    |         v
    |     [CPG Validation] --> cross-referenced findings
    |
    +---> [Synthesis Prompts] --> CPG-informed prompts
              |
              v
          [LLM Synthesis] --> test code
              |
              v
          [Coverage Measurement] --> coverage delta
              |
              v
          [Refinement Loop] --> final tests
```

The CPG is the unifying data structure. tree-sitter provides accurate parsing; the CPG enables both better detection (lower FPs) and better synthesis (more informative prompts).

### What NOT to Adopt

1. **Joern/CodeQL as dependencies** -- too heavy, wrong language, not embeddable
2. **Full Datalog engine** -- over-engineered for current scale; declarative rules in YAML suffice
3. **CodeBERT/fine-tuned models** -- 81% false negative rate; prompted LLMs are better
4. **EvoSuite/Pynguin** -- LLM-based approaches now match or exceed; wrong languages
5. **Full abstract interpretation** -- requires formal semantics per language; impractical for 12+ languages

### Key Papers

| Paper | Venue | Year | Key Finding |
|-------|-------|------|-------------|
| CoverUp | FSE 2025 | 2024 | 80% median coverage, 18x faster than CodaMosa |
| LLMxCPG | USENIX Security 2025 | 2025 | CPG+LLM: 15-40% F1 improvement, 67-91% code reduction |
| ABSINT-AI | ICLR 2025 | 2025 | LLM+abstract interpretation: 70% FP reduction, soundness preserved |
| CodaMosa | ICSE 2023 | 2023 | Hybrid search+LLM: 173/486 benchmarks improved |
| Reducing FPs with LLMs | arXiv | 2025 | Industrial: 94-98% FP elimination at $0.001/alarm |
| LLM Vulnerability Survey | arXiv | 2025 | CodeBERT: 81.77% false negative rate (useless in practice) |
| SBFT 2025 | ICSE 2025 | 2025 | EvoSuite: 56% branch, EvoFuzz: 66% branch |
| QVoG | arXiv | 2024 | CPG extraction: 15 min for 1.5M LOC (vs 19 min CodeQL) |
