# APEX Research Integration Plan

Compiled: 2026-03-14
Sources: 46 papers from arXiv (2024–2026) across three domains.

---

## Executive Summary

Three cross-cutting themes dominate the 2024–2026 literature and map directly
to APEX's architecture:

1. **CPG + LLM hybrids** are the winning pattern — pure pattern matching and
   pure LLM analysis both lose to structured-graph-guided LLM reasoning
2. **Mutation score is replacing coverage** as the primary adequacy metric —
   the "oracle gap" (coverage minus mutation score) exposes false confidence
3. **Principled strategy orchestration** is replacing ad-hoc "switch when stuck"
   — formal decision rules outperform heuristic handoffs between techniques

APEX is uniquely positioned: it already has the CPG, the per-test branch index,
the multi-strategy agent loop, and the LLM synthesis pipeline. Most of these
papers describe techniques that slot into existing crates with bounded effort.

---

## Tier 1 — High Impact, Low-Medium Effort

These techniques have strong empirical results and map to existing APEX crates
with minimal architectural change.

### 1.1 Code Elimination from Prompts
**Paper:** Xu et al. 2026 — arXiv:2602.21997
**Crate:** `apex-synth`
**Effort:** Low

Instead of highlighting uncovered lines in the prompt, *remove already-covered
code entirely*. The LLM sees only what remains uncovered, reducing prompt size
and focusing attention. Complementary to APEX's existing CoverUp-style loop.

**Implementation:** After each round in `fill_gap()`, strip covered regions from
the focal code before building the next prompt. Use `apex-instrument` coverage
data to identify covered line ranges.

---

### 1.2 Counter-Example Feedback for Hard Branches
**Paper:** TELPA — arXiv:2404.04966
**Crate:** `apex-synth`
**Effort:** Medium

When a branch remains uncovered after N rounds, include the *failed test
attempts* as counter-examples in the prompt, plus backward/forward
method-invocation context. The LLM learns what didn't work and tries
different approaches.

**Result:** +31% branch coverage over Pynguin on hard-to-cover branches.

**Implementation:** Store failed test attempts per gap. After 2 rounds without
progress, switch to TELPA-style prompts that include failed attempts and
inter-procedural dependency chains (extractable from `apex-cpg`).

---

### 1.3 Oracle Gap Metric
**Paper:** Mind the Gap — arXiv:2309.02395
**Crate:** `apex-coverage`, gap reports
**Effort:** Low

Oracle gap = coverage − mutation score. High gap means "code is executed but
tests don't verify its behavior." This is a *much* stronger signal than coverage
alone. Files with high oracle gap have tests that pass through the code without
actually checking anything.

**Implementation:** Add lightweight mutation scoring to gap reports. Use
`cargo-mutants` or a simple operator set (negate conditions, swap operators)
to compute per-region mutation scores. Report oracle gap alongside coverage.

---

### 1.4 Thompson Sampling for Seed Scheduling
**Paper:** T-Scheduler — arXiv:2312.04749 (AsiaCCS 2024)
**Crate:** `apex-fuzz`
**Effort:** Low

Model seed scheduling as a Beta-Bernoulli multi-armed bandit. Each seed is an
arm; reward = new coverage found. Thompson sampling: sample from each seed's
Beta posterior, fuzz the highest. Zero hyperparameters, constant-time overhead,
theoretical optimality guarantees. Outperforms 11 SOTA schedulers over 35 CPU-years.

**Implementation:** Replace or augment seed selection in `apex-fuzz` with
Thompson sampling. ~50 lines of code: maintain `(alpha, beta)` per seed,
sample, pick max, update on result.

---

### 1.5 LLM-Inferred Taint Specifications
**Paper:** IRIS — arXiv:2405.17238 (ICLR 2025)
**Crate:** `apex-cpg`, `apex-detect`
**Effort:** Medium

Use LLMs to auto-infer source/sink/sanitizer specifications for third-party
APIs. Currently APEX's taint analysis relies on manually defined tables in
`apex-cpg/src/taint.rs`. IRIS doubles CodeQL's detection rate by auto-populating
these specs.

**Implementation:** Before taint analysis, scan imports for third-party libraries.
Prompt LLM: "For library X, which functions are sources (return user input),
sinks (execute/write), or sanitizers?" Parse response into APEX's taint table
format. Cache per-library.

---

### 1.6 Flaky Test Detection via Coverage Instability
**Paper:** FlaKat — arXiv:2403.01003
**Crate:** `apex-index`
**Effort:** Low

APEX's per-test branch index already records which tests cover which branches.
Run the index twice; tests whose branch sets differ between runs are flaky.
Classify root cause by the pattern of instability (async, order-dependent,
resource leak, etc.).

**Implementation:** Add a `--detect-flaky` flag to `apex index` that runs
two indexing passes and reports tests with divergent coverage profiles.

---

## Tier 2 — High Impact, Medium-High Effort

These require more significant implementation but unlock major new capabilities.

### 2.1 Principled Hybrid Strategy Routing (S2F)
**Paper:** S2F — arXiv:2601.10068 (Jan 2026)
**Crate:** `apex-agent/src/priority.rs`
**Effort:** Medium

Formal rules for when to invoke fuzzing vs symbolic execution vs sampling,
based on branch characteristics. Replaces APEX's current ad-hoc proximity
heuristic (high → gradient, medium → fuzzer, low → LLM).

S2F's insight: current hybrid tools (Driller, QSYM) over-prune branches
during symbolic execution and misapply sampling. Principled categorization
of branches yields 6.14% edge coverage improvement and 32.6% more crashes.

**Implementation:** Classify each uncovered branch by constraint complexity
and path depth. Route to gradient/Z3 for numeric constraints, fuzzer for
shallow branches with many paths, LLM for deep/complex branches.

---

### 2.2 LLM as Concolic Constraint Solver (Cottontail)
**Paper:** Cottontail — arXiv:2504.17542
**Crate:** `apex-concolic`
**Effort:** High

Replace Z3 with LLM reasoning for structured input constraints (JSON, XML,
protocol buffers). Z3 struggles with string operations and format constraints;
LLMs handle them naturally. 30–41% higher coverage on structured inputs.

**Implementation:** Add an `LlmSolver` backend to `apex-symbolic/src/portfolio.rs`
that routes structured/string constraints to LLM instead of Z3. Keep Z3 for
numeric/boolean constraints. The portfolio solver already supports multiple backends.

---

### 2.3 Diverse SMT Solutions (PanSampler)
**Paper:** PanSampler — arXiv:2511.10326
**Crate:** `apex-symbolic`, `apex-concolic`
**Effort:** Medium

Instead of one solution per constraint, generate multiple diverse solutions
that maximize coverage. Needs 32–76% fewer test cases for same fault detection.

**Implementation:** When Z3 finds a solution, add a diversity constraint
(solution must differ from previous ones on key variables), re-solve N times.
Use AST-guided scoring to pick the most diverse set. Augments existing
`PortfolioSolver`.

---

### 2.4 CPG-Guided LLM Slicing for Vulnerability Validation
**Paper:** LLMxCPG — arXiv:2507.16585
**Crate:** `apex-cpg`, `apex-detect`
**Effort:** Medium

Extract thin CPG slices (67–91% code reduction) around taint paths, feed to
LLM for validation. Dramatically reduces false positives compared to pattern
matching alone. 15–40% F1 improvement.

**Implementation:** After `apex-cpg` taint analysis finds a candidate flow,
extract the backward slice from sink to source (nodes + edges in the CPG).
Serialize as annotated code. Prompt LLM: "Is this a real vulnerability or
false positive? Explain." Filter findings by LLM confidence.

---

### 2.5 Adversarial Test vs Mutant Loop (AdverTest)
**Paper:** AdverTest — arXiv:2602.08146 (Feb 2026)
**Crate:** `apex-agent`
**Effort:** Medium

Two adversarial LLM agents: Agent T writes tests, Agent M generates mutants
that survive T's tests. They iterate, each exposing the other's blind spots.
Natural fit for APEX's gap reports: identify gap → T closes it → M stress-tests.

**Implementation:** After `apex-synth` generates tests for a gap, invoke a
second LLM pass that generates mutations in the same region. If mutations
survive, feed back to the test generator. 2–3 rounds typically converge.

---

### 2.6 Method Slicing for Complex Targets (HITS)
**Paper:** HITS — arXiv:2408.11324
**Crate:** `apex-synth`, `apex-detect`
**Effort:** Medium

Decompose complex methods into logically coherent "slices," generate tests
per-slice. Each slice is simple enough for the LLM to reason about completely.
Outperforms both LLM-based and SBST methods.

**Implementation:** Use CFG analysis from `apex-mir` to identify slice boundaries
(basic block groups with single entry/exit). When a focal method has >N branches,
decompose into slices and generate separately. Union of per-slice tests covers
the whole method.

---

### 2.7 Co-Evolutionary Generation/Repair (TestART + YATE)
**Papers:** TestART (arXiv:2408.03095), YATE (arXiv:2507.18316)
**Crate:** `apex-synth`
**Effort:** Medium

Interleave test generation and test repair. When a generated test fails,
analyze the failure and feed it back as repair context (not just "fix this
test" but "what does this failure reveal about the code?"). Repair contributes
+32% line coverage and +22% mutation kills over plain generation.

**Implementation:** Modify `fill_gap()` loop: on test failure, classify error
(compile error → fix syntax; runtime error → fix logic; assertion error →
strengthen oracle). Route to repair-specific prompts.

---

## Tier 3 — Exploratory / Future Architecture

These require significant new infrastructure but represent the research frontier.

| Paper | Technique | Target Crate | Notes |
|-------|-----------|-------------|-------|
| FOX (2406.04517) | Fuzzing as stochastic control | `apex-fuzz` | Rearchitects the entire fuzzer loop |
| DeepGo (NDSS 2024) | RL-based predictive directed fuzzing | `apex-agent` | Requires RL training infrastructure |
| Graphuzz (TOSEM 2024) | GNN seed scoring on e-CFGs | `apex-agent` | Requires GNN training; aligns with CPG |
| IPAG/HAGNN (2502.16835) | Heterogeneous GNN vuln detection | `apex-cpg` | 96.6% accuracy but needs ML pipeline |
| AutoBug (2505.13452) | LLM replaces symbolic execution entirely | `apex-symbolic` | Radical approach; good for languages without symbolic support |
| SeedMind (2411.18143) | LLM generates seed *generators* | `apex-fuzz` | Generators yield more diverse corpora |
| LLAMAFUZZ (2406.07714) | Fine-tuned LLM for format-aware mutation | `apex-fuzz` | +41 bugs; needs fine-tuning pipeline |
| Fuzz4All (ICSE 2024) | Autoprompting for universal fuzzing | `apex-lang` | Language-agnostic via LLM |
| Caruca (2510.14279) | Spec mining from syscall traces | `apex-sandbox` | Auto-generate security policies |
| DCE-LLM (2506.11076) | Dead code detection via CodeBERT + LLM | `apex-index` | >94% F1; complement coverage data |
| FANDANGO-RS (2511.05987) | Rust grammar-based constrained fuzzing | `apex-fuzz` | 3–4 orders of magnitude faster |
| Semantic feedback (2511.03995) | Beyond-coverage feedback signals | `apex-coverage` | Exception types, output patterns |
| SymPrompt (2402.00097) | Path-enumeration as prompting strategy | `apex-synth` | 5× improvement on CodeGen2 |
| PALM (2506.19287) | Natural-language constraint fallback | `apex-concolic` | When formal solvers fail |

---

## Cross-Cutting Architectural Implications

### 1. The Prompt Pipeline Should Be Pluggable

Five papers (CoverUp, TELPA, Xu elimination, HITS, SymPrompt) propose different
prompting strategies for test generation. APEX should make the prompt construction
in `apex-synth` pluggable:
- Default: CoverUp-style (include uncovered lines)
- Round 2+: Elimination (remove covered code)
- Hard branches: TELPA (counter-examples + dependency chains)
- Complex methods: HITS (per-slice decomposition)
- Path-aware: SymPrompt (one prompt per path)

### 2. The Solver Portfolio Should Include LLM

Three papers (Cottontail, AutoBug, HGFuzzer) use LLMs as constraint solvers.
APEX's `PortfolioSolver` already chains gradient → Z3. Add LLM as a third
backend: gradient → Z3 → LLM (for structured/string constraints that Z3 can't solve).

### 3. Per-Test Index Is Underexploited

APEX's per-test branch index is exactly the "dynamic trace data" that papers
like DeepDFA, FlaKat, DCE-LLM, and the test prioritization papers show is
underexploited. Immediate wins:
- Flaky detection (coverage instability across runs)
- Oracle gap (coverage minus mutation score per region)
- Dead code validation (never-covered + LLM-confirmed unreachable)
- Rank-aggregated test prioritization (multiple signals)

### 4. Mutation Score as First-Class Metric

Three independent papers (Mind the Gap, Meta's ACH, AdverTest) converge on
mutation score as the key metric beyond coverage. APEX should:
- Add mutation score to gap reports
- Use oracle gap (coverage − mutation) as the primary "gap" signal
- Use adversarial mutant generation to validate test quality

### 5. Strategy Router Needs Formalization

S2F and FOX provide principled frameworks for when to invoke which technique.
APEX's current `recommend_strategy()` uses ad-hoc proximity thresholds. The
upgrade path:
1. Classify branches by constraint type (numeric, string, structural, path-depth)
2. Route based on classification (not just proximity score)
3. Track per-strategy success rates and adapt (T-Scheduler's Thompson sampling
   applies here too — not just for seed selection)

---

## Implementation Sequence

**Phase 1 — Quick Wins (Tier 1, ~2 weeks)**
1. Code elimination in prompts (Xu)
2. Thompson sampling for seeds (T-Scheduler)
3. Oracle gap metric (Mind the Gap)
4. Flaky detection from index variance (FlaKat)

**Phase 2 — Synthesis Pipeline Upgrade (~3 weeks)**
5. Counter-example feedback (TELPA)
6. Method slicing (HITS)
7. Co-evolutionary generation/repair (TestART/YATE)
8. Pluggable prompt strategies

**Phase 3 — Analysis Pipeline Upgrade (~3 weeks)**
9. LLM-inferred taint specs (IRIS)
10. CPG-guided LLM validation (LLMxCPG)
11. Diverse SMT solutions (PanSampler)
12. Principled strategy routing (S2F)

**Phase 4 — Advanced Capabilities (~4 weeks)**
13. LLM as constraint solver (Cottontail)
14. Adversarial test+mutant loop (AdverTest)
15. Mutation-guided test generation (Meta ACH)

---

## Full Paper Index

### LLM + Test Generation (15 papers)
| ID | Title | Year | Key Technique |
|----|-------|------|--------------|
| 2403.16218 | CoverUp | 2024 | Coverage-feedback prompting loop |
| 2404.04966 | TELPA | 2024 | Counter-example feedback for hard branches |
| 2602.21997 | Code Elimination | 2026 | Remove covered code from prompts |
| 2503.14713 | TestForge | 2025 | Agentic file-level generation |
| 2504.17542 | Cottontail | 2025 | LLM-driven concolic execution |
| 2505.13452 | AutoBug | 2025 | LLM replaces symbolic execution |
| 2402.00097 | SymPrompt | 2024 | Path-enumeration prompting |
| 2411.18143 | SeedMind | 2024 | LLM generates seed generators |
| 2406.07714 | LLAMAFUZZ | 2024 | Fine-tuned LLM mutations |
| 2308.04748 | Fuzz4All | 2024 | Universal autoprompting fuzzer |
| 2511.03995 | Semantic Feedback | 2025 | Beyond-coverage feedback |
| 2408.03095 | TestART | 2024 | Co-evolutionary gen/repair |
| 2507.18316 | YATE | 2025 | Repair contributes +32% coverage |
| 2408.11324 | HITS | 2024 | Method slicing + CoT |
| 2506.19287 | PALM | 2025 | Path-aware + NL constraints |

### Fuzzing & Symbolic Execution (13 papers)
| ID | Title | Year | Key Technique |
|----|-------|------|--------------|
| 2601.10068 | S2F | 2026 | Principled hybrid strategy routing |
| 2406.04517 | FOX | 2024 | Fuzzing as stochastic control |
| NDSS 2024 | DeepGo | 2024 | RL-based predictive directed fuzzing |
| 2505.03425 | HGFuzzer | 2025 | LLM solves path constraints as code |
| 2510.23101 | Trace-Guided DGF | 2025 | LLM-predicted call stacks |
| 2312.04749 | T-Scheduler | 2024 | Thompson sampling seed scheduling |
| JSS 2025 | DEzzer | 2025 | Differential evolution mutation scheduling |
| TOSEM 2024 | Graphuzz | 2024 | GNN seed scoring on e-CFGs |
| 2511.05987 | FANDANGO-RS | 2025 | Rust grammar-based fuzzing |
| 2511.10326 | PanSampler | 2025 | Diverse SMT solution sampling |
| 2511.03995 | Hybrid LLM Fuzzing | 2025 | Semantic feedback loop |
| BAR 2024 | LibAFL QEMU | 2024 | Fuzzing-oriented emulation |
| 2502.00169 | Fitness Landscapes | 2025 | Landscape analysis for test gen |

### Security, Analysis & Testing (18 papers)
| ID | Title | Year | Key Technique |
|----|-------|------|--------------|
| 2507.16585 | LLMxCPG | 2025 | CPG-guided LLM slicing |
| 2405.17238 | IRIS | 2024 | LLM-inferred taint specs |
| 2212.08108 | DeepDFA | 2024 | Dataflow-inspired deep learning |
| 2502.16835 | IPAG/HAGNN | 2025 | Heterogeneous GNN on CPGs |
| 2404.14719 | Vul-LMGNNs | 2024 | LM + GNN knowledge distillation |
| 2509.15433 | SAST-Genius | 2025 | Semgrep + LLM false-positive reduction |
| 2501.12862 | Meta ACH | 2025 | Mutation-guided test gen at scale |
| 2602.08146 | AdverTest | 2026 | Adversarial test vs mutant agents |
| 2309.02395 | Mind the Gap | 2023 | Oracle gap metric |
| 2508.19056 | Slice-Based TCP | 2025 | Change impact test prioritization |
| 2412.00015 | Rank Aggregation TCP | 2024 | Multi-signal test prioritization |
| 2510.14279 | Caruca | 2025 | Spec mining from syscall traces |
| 2603.06710 | Mining Beyond Bools | 2026 | Data transformation spec mining |
| 2403.13279 | SmCon | 2025 | CEGAR-based spec mining |
| 2403.01003 | FlaKat | 2024 | ML flaky test categorization |
| 2307.00012 | FlakyFix | 2024 | LLM-based flaky test repair |
| 2502.02715 | Flaky LLM Detection | 2025 | Fine-tuned flaky detection |
| 2506.11076 | DCE-LLM | 2025 | Dead code detection via LLM |
