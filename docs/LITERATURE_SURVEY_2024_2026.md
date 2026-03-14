# Literature Survey: Program Analysis, Security Detection, and Testing (2024--2026)

Compiled: 2026-03-14
Focus: Papers relevant to a tool combining per-test branch indexing, code property graphs, and security detection.

---

## 1. Code Property Graphs and Graph-Based Vulnerability Detection

### 1.1 LLMxCPG: Context-Aware Vulnerability Detection Through CPG-Guided LLMs

- **arXiv:** 2507.16585 (July 2025)
- **Key technique:** Integrates Code Property Graphs with Large Language Models. A CPG-based *slice construction* technique reduces code size by 67--91% while preserving vulnerability-relevant context. The CPG guides what the LLM examines, rather than feeding raw source.
- **Advance:** 15--40% F1 improvements over SoTA baselines. Demonstrates that CPG-guided context extraction is critical for LLM-based vuln detection -- raw code dumps are insufficient.
- **APEX relevance:** APEX already builds CPGs (inspired by Joern). The slicing technique here could feed APEX's security detector: extract thin CPG slices around taint paths, then feed to an LLM for validation. This would reduce false positives dramatically compared to pattern matching alone.

### 1.2 Detecting Code Vulnerabilities with Heterogeneous GNN Training (IPAG + HAGNN)

- **arXiv:** 2502.16835 (February 2025)
- **Authors:** Yu Luo et al.
- **Key technique:** Inter-Procedural Abstract Graphs (IPAGs) -- a language-agnostic code representation that extends CPGs with inter-procedural edges. A Heterogeneous Attention GNN (HAGNN) learns separate subgraph embeddings (AST, CFG, DFG) and fuses them via global attention.
- **Advance:** 96.6% accuracy on C (108 vuln types), 97.8% on Java (114 vuln types). Language-agnostic representation is the key differentiator from Joern's CPG.
- **APEX relevance:** IPAGs are a direct successor to Joern-style CPGs. The multi-subgraph architecture maps well to APEX's existing coverage graph (CFG layer) plus a future DFG layer. The heterogeneous attention mechanism could weight security-relevant edges higher than structural ones.

### 1.3 Vul-LMGNNs: Fusing Language Models and Graph Neural Networks

- **arXiv:** 2404.14719 (April 2024)
- **Key technique:** Combines pre-trained code language models with gated GNNs operating on CPGs. Uses online knowledge distillation so the LM and GNN teach each other during training.
- **Advance:** Outperforms 17 SoTA approaches. The online distillation is notable -- neither the LM nor GNN alone achieves the combined result.
- **APEX relevance:** If APEX adds an ML-based security detector, the dual-encoder (LM + GNN) architecture is the current best practice. The CPG that APEX already constructs feeds the GNN branch directly.

---

## 2. LLM-Augmented Static Analysis

### 2.1 IRIS: LLM-Assisted Static Analysis for Detecting Security Vulnerabilities

- **arXiv:** 2405.17238 (May 2024, updated April 2025; ICLR 2025)
- **Key technique:** Neuro-symbolic approach. Uses LLMs to *mine taint specifications* for third-party library APIs, then augments CodeQL with those specs. Whole-repository reasoning: the LLM infers source/sink/sanitizer annotations that humans would normally write.
- **Advance:** CodeQL alone finds 27/120 vulns; IRIS+GPT-4 finds 55/120 (+104% improvement) with 5pp better false discovery rate. Discovered 4 previously unknown vulnerabilities.
- **APEX relevance:** Directly applicable. APEX's taint analysis currently relies on manually defined source/sink specs. IRIS's approach of LLM-inferred taint specs could auto-populate APEX's security rules for any library, eliminating the specification bottleneck. The CWE-Bench-Java benchmark is also useful for validation. Code: https://github.com/iris-sast/iris

### 2.2 SAST-Genius: LLM-Driven Hybrid Static Analysis Framework

- **arXiv:** 2509.15433 (September 2025)
- **Authors:** Vaibhav Agrawal, Kiarash Ahi
- **Key technique:** Pairs Semgrep (rule-based SAST) with LLM post-processing for triage, validation via exploit generation, and contextual bug descriptions.
- **Advance:** Reduced false positives by ~91% (225 to 20) compared to Semgrep alone. The exploit generation step is novel -- the LLM attempts to construct a proof-of-concept exploit to validate each finding.
- **APEX relevance:** The false-positive reduction pipeline is directly relevant. APEX could run its pattern-based detectors first (cheap), then use an LLM to validate/triage the results. The exploit generation concept aligns with APEX's concolic/fuzz capabilities -- generate an actual input that triggers the vulnerability path.

---

## 3. Dataflow Analysis and Taint Analysis

### 3.1 DeepDFA: Dataflow Analysis-Inspired Deep Learning for Vulnerability Detection

- **arXiv:** 2212.08108 (ICSE 2024)
- **Authors:** Benjamin Steenhoek, Hongyang Gao, Wei Le
- **Key technique:** Abstract dataflow embedding that enables graph learning to simulate classical reaching-definition analysis. Applies GNN message passing on control flow graphs where node features encode variable definition/use information.
- **Advance:** Combined with an LM, achieves 96.46 F1 on Big-Vul. Crucially, detects 8.7/17 real-world DbgBench vulnerabilities (baselines detect 0). Maintains performance with only 151 training examples (0.1% of data).
- **APEX relevance:** High relevance. DeepDFA's abstract dataflow embedding could augment APEX's CFG-based coverage analysis. Rather than tracking only which branches execute, APEX could track *dataflow facts* (which definitions reach which uses) per test. This creates a much richer per-test index for security analysis. Code: https://github.com/ISU-PAAL/DeepDFA

### 3.2 Learning to Triage Taint Flows from Dynamic Analysis

- **arXiv:** 2510.20739 (October 2025)
- **Key technique:** ML-based triage of taint flow reports from dynamic analysis in Node.js packages. Evaluates classical ML, GNNs, LLMs, and hybrid GNN+LLM models on a benchmark of 1,883 packages.
- **Advance:** Best LLM achieves F1=0.915. At <7% false-negative rate, eliminates 66.9% of benign packages from review. At precision=0.8, detects 99.2% of exploitable flows.
- **APEX relevance:** When APEX's dynamic taint tracking produces many candidate flows, this ML triage approach could prioritize which flows warrant developer attention. The hybrid GNN+LLM architecture is the top performer -- aligns with the dual-encoder theme from Vul-LMGNNs.

### 3.3 Practical Type-Based Taint Checking and Inference

- **arXiv:** 2504.18529 (April 2025)
- **Key technique:** Type-system approach to taint analysis. Sources, sinks, and sanitizers are encoded as type annotations; the type checker proves absence of taint flows.
- **APEX relevance:** A complementary approach to dataflow-based taint analysis. Type-based checking is sound (no false negatives for annotated code) but requires annotations. Could serve as the "verified core" while dynamic taint analysis handles unannotated code.

---

## 4. Mutation Testing and Test Adequacy

### 4.1 Mutation-Guided LLM-based Test Generation at Meta (MuTAP / ACH)

- **arXiv:** 2501.12862 (January 2025)
- **Key technique:** Meta's ACH system generates mutants for Android Kotlin classes, then uses LLMs to generate tests that kill those mutants. Applied to 10,795 classes across 7 platforms; generated 9,095 mutants and 571 privacy-hardening test cases.
- **Advance:** Industrial-scale mutation-guided test generation. The privacy-hardening angle is unique -- mutants model potential privacy violations, and generated tests guard against regression.
- **APEX relevance:** APEX's per-test branch index identifies undertested code regions. This paper shows how to close those gaps: generate mutants in the gap region, then use LLMs to produce killing tests. The mutation-to-test pipeline is directly implementable on APEX's gap reports.

### 4.2 AdverTest: Test vs Mutant -- Adversarial LLM Agents

- **arXiv:** 2602.08146 (February 2026)
- **Key technique:** Two adversarial LLM agents: Agent T generates tests, Agent M generates mutants that survive T's tests. They iterate in an adversarial loop, each exposing the other's blind spots.
- **Advance:** The adversarial framing yields more robust test suites than single-agent generation. Agent M specifically targets corner cases and vulnerable execution paths.
- **APEX relevance:** This adversarial loop could be layered on top of APEX's gap analysis. APEX identifies the coverage gap; Agent T tries to close it; Agent M generates mutations in those regions to stress-test T's output. The two-agent architecture naturally separates concerns.

### 4.3 Mind the Gap: Oracle Gap as Test Adequacy Metric

- **arXiv:** 2309.02395 (ISSRE 2023, still the SoTA reference)
- **Authors:** Jain et al.
- **Key technique:** Defines the *oracle gap* = coverage - mutation score. A high oracle gap means code is executed but not actually checked (weak oracles). Proposes *covered oracle gap* as a per-file diagnostic.
- **Advance:** Demonstrates that oracle gap adds information beyond either coverage or mutation score alone. Identifies files where coverage gives false confidence.
- **APEX relevance:** Directly applicable to APEX's gap reporting. Currently APEX reports uncovered lines. Adding mutation score per region would yield oracle gap -- a much more actionable metric. "You cover this code but your tests don't actually verify its behavior" is a stronger signal than "you don't cover this code."

### 4.4 Test Adequacy for Metamorphic Testing

- **arXiv:** 2412.20692 (December 2024)
- **Key technique:** Proposes adequacy criteria specifically for metamorphic testing, going beyond branch coverage to measure whether metamorphic relations sufficiently probe the input space.
- **APEX relevance:** As APEX expands beyond branch coverage, metamorphic testing adequacy criteria represent an orthogonal dimension of test quality assessment.

---

## 5. Test Prioritization and Selection

### 5.1 Slice-Based Change Impact Analysis for Test Case Prioritization

- **arXiv:** 2508.19056 (August 2025)
- **Key technique:** Computes *affected component coupling* (ACC) from program slices of changed code in OO programs. Test cases covering high-ACC nodes are prioritized. Uses static analysis (no execution needed).
- **Advance:** Mutation fault experiments show high-ACC tests find faults earlier. Static-only approach avoids the cost of running the test suite to collect coverage.
- **APEX relevance:** APEX's per-test branch index already maps tests to code regions. This paper's ACC metric could rank those regions by coupling, prioritizing tests that exercise highly-coupled (fault-prone) changed code. The slicing algorithm could reuse APEX's CPG infrastructure.

### 5.2 On Rank Aggregating Test Prioritizations

- **arXiv:** 2412.00015 (December 2024)
- **Key technique:** Combines multiple TCP heuristics (coverage-based, history-based, change-based) via rank aggregation methods to produce a single prioritized ordering.
- **APEX relevance:** APEX could expose multiple prioritization signals (branch coverage, CPG-based coupling, security risk score) and aggregate them using the rank fusion techniques from this paper.

---

## 6. Specification Mining and Invariant Discovery

### 6.1 Caruca: Specification Mining for Opaque Software Components

- **arXiv:** 2510.14279 (October 2025)
- **Authors:** Lamprou, Jung, Keoliya, Lazarek, Kallas, Greenberg, Vasilakis
- **Key technique:** Concretely executes commands, interposes at system-call and filesystem level to extract properties: parallelizability, filesystem pre/post-conditions, side effects. Generates specifications from traces via transformation rules.
- **Advance:** Correct specifications for 59/60 GNU coreutils/POSIX commands. Eliminates manual specification effort entirely.
- **APEX relevance:** The syscall-interposition approach for extracting pre/post-conditions is applicable to APEX's sandbox. When APEX instruments a target, it could simultaneously mine specifications (what files are read/written, what network calls are made) to auto-generate security policies.

### 6.2 Mining Beyond the Bools: Learning Data Transformations and Temporal Specifications

- **arXiv:** 2603.06710 (March 2026)
- **Key technique:** Extends specification mining beyond Boolean event abstractions to richer datatypes. Learns data transformations and temporal properties from execution traces.
- **APEX relevance:** Current spec mining largely works with event sequences (function called / not called). This work enables mining specifications like "output buffer length equals input length + 4" -- directly useful for detecting buffer overflows and format string vulnerabilities from dynamic traces.

### 6.3 Specification Mining for Smart Contracts with Trace Slicing

- **arXiv:** 2403.13279 (updated April 2025)
- **Key technique:** CEGAR-based specification mining powered by trace slicing and predicate abstraction. Tool: SmCon.
- **Advance:** Mined specifications enhance symbolic analysis, achieving higher coverage and up to 56% speedup.
- **APEX relevance:** The CEGAR loop (mine spec -> check -> refine) is a general pattern applicable to any domain. APEX could use it to iteratively refine security specifications: mine an initial spec from tests, check against the CPG, refine where violations are found.

---

## 7. Flaky Test Detection

### 7.1 FlaKat: ML-Based Categorization Framework for Flaky Tests

- **arXiv:** 2403.01003 (March 2024)
- **Key technique:** ML classifiers predict the *category* (root cause) of a flaky test, not just whether it is flaky. Proposes Flakiness Detection Capacity (FDC), an information-theoretic metric for classifier accuracy.
- **APEX relevance:** APEX's per-test branch index tracks which tests cover which branches across runs. Non-deterministic coverage patterns (a test covers branch B in some runs but not others) are a direct signal for flakiness. APEX could use FlaKat's category prediction to explain *why* a test is flaky (async, order-dependent, resource leak, etc.).

### 7.2 FlakyFix: LLM-Based Flaky Test Repair

- **arXiv:** 2307.00012 (updated August 2024)
- **Key technique:** Predicts fix category for flaky tests, then uses GPT-3.5 with in-context learning to generate repairs. 51--83% of generated repairs expected to pass.
- **APEX relevance:** Once APEX detects flaky tests via coverage instability, FlakyFix's approach could auto-generate fixes. The fix category prediction step maps well to APEX's root-cause classification.

### 7.3 LLM Fine-Tuning and Few-Shot Learning for Flaky Test Detection

- **arXiv:** 2502.02715 (February 2025)
- **Key technique:** Evaluates fine-tuned LLMs and few-shot approaches (FlakyXbert with Siamese network architecture) for flaky test detection and classification.
- **APEX relevance:** Provides benchmarks for ML-based flaky detection that APEX could compare against its coverage-instability-based approach.

---

## 8. Dead Code Detection

### 8.1 DCE-LLM: Dead Code Elimination with Large Language Models

- **arXiv:** 2506.11076 (NAACL 2025)
- **Key technique:** Small CodeBERT model with attribution-based line selector identifies suspect dead code; LLM then generates judgments, explanations, and patches. Fine-tuned on a large-scale dead code dataset.
- **Advance:** >94% F1 for unused and unreachable code, surpassing GPT-4o by 30%. Supports multiple languages.
- **APEX relevance:** APEX's per-test coverage data is a natural complement. Code that is never covered by any test is a dead-code candidate. DCE-LLM's approach could validate those candidates (distinguishing truly dead code from code that just lacks tests). The two signals together -- "never executed dynamically" + "confirmed unreachable statically by LLM" -- would have very high precision.

---

## 9. Hybrid Static + Dynamic Analysis

### 9.1 Combining Static Analysis and Dynamic Symbolic Execution for Fault Injection Vulnerabilities

- **arXiv:** 2303.03999 (2023, still relevant methodology)
- **Key technique:** Fast static analysis identifies injection points and assesses impactfulness; precise dynamic symbolic execution validates attack paths.
- **APEX relevance:** This is essentially APEX's architecture for security: static CPG analysis identifies candidate vulnerabilities, then concolic/fuzz execution validates them. The paper provides a formal framework for this two-phase approach.

---

## Summary: Integration Priorities for APEX

| Priority | Technique | Source Paper(s) | Implementation Effort |
|----------|-----------|----------------|----------------------|
| **HIGH** | LLM-inferred taint specifications | IRIS (2405.17238) | Medium -- augment existing taint rules |
| **HIGH** | Oracle gap metric (coverage - mutation score) | Mind the Gap (2309.02395) | Low -- add mutation score to gap report |
| **HIGH** | CPG-guided LLM slicing for vuln validation | LLMxCPG (2507.16585) | Medium -- slice CPG, feed to LLM |
| **HIGH** | Flaky test detection via coverage instability | FlaKat (2403.01003) | Low -- analyze per-test index variance |
| **MEDIUM** | IPAG / heterogeneous GNN for vuln detection | HAGNN (2502.16835) | High -- requires ML training pipeline |
| **MEDIUM** | Dataflow embedding for richer per-test index | DeepDFA (2212.08108) | Medium -- extend coverage model |
| **MEDIUM** | Adversarial test+mutant generation | AdverTest (2602.08146) | Medium -- requires LLM agent loop |
| **MEDIUM** | Spec mining from execution traces | Caruca (2510.14279) | Medium -- syscall interposition |
| **LOW** | Dead code detection via coverage + LLM | DCE-LLM (2506.11076) | Low -- post-process coverage data |
| **LOW** | Test prioritization via rank aggregation | Rank Aggregation (2412.00015) | Low -- combine existing signals |
| **LOW** | ML triage of taint flow reports | Taint Triage (2510.20739) | Medium -- requires training data |

---

## Key Themes Across the Literature

1. **CPG + LLM is the dominant pattern for 2024--2026.** Nearly every top paper combines structured program representations (CPGs, CFGs, DFGs) with LLMs. Pure pattern matching is dead; pure LLM analysis hallucinates. The hybrid wins.

2. **Taint specification is the bottleneck.** Multiple papers (IRIS, SAST-Genius, Taint Triage) identify manual taint specs as the main limitation of static security tools. Auto-inferring specs via LLMs is the breakthrough.

3. **Mutation score is eating coverage.** The oracle gap paper and the mutation-guided test generation papers all argue that coverage without mutation analysis gives false confidence. The field is moving toward mutation score as the primary adequacy metric.

4. **Adversarial/multi-agent architectures are emerging.** AdverTest and related work show that pitting test generators against mutant generators produces better results than either alone.

5. **Dynamic analysis data is underused for static analysis.** Papers like Caruca and DCE-LLM show that execution traces can bootstrap specifications and validate static findings. APEX's per-test branch index is exactly this kind of dynamic data.
