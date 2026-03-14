# Mechanism Inspiration Sources

APEX integrates fundamental analysis mechanisms from established security and testing tools.
This document records what was adopted, from where, and how it maps to APEX's architecture.

---

# Implemented Mechanisms

## Branch Distance (EvoMaster / Korel)

**Source:** EvoMaster's `HeuristicsForJumps.java` + `TruthnessUtils`
**Paper:** Korel, "Automated Software Test Data Generation" (1990)

**Mechanism:** Continuous [0,1] fitness for branch conditions instead of binary covered/not-covered.
- `x == 42` with `x = 40` scores `1 - normalize(|40-42|)` = 0.33 instead of binary 0
- `normalize(d) = d / (d + 1)` — maps any distance to [0,1)

**APEX location:** `crates/apex-coverage/src/heuristic.rs`
**Integration:** `CoverageOracle.record_heuristic()` / `best_heuristic()` in `crates/apex-coverage/src/oracle.rs`

## Gradient Descent Constraint Solving (Angora)

**Source:** Angora's `fuzzer/src/search/gd.rs` + `grad.rs`
**Paper:** Chen & Chen, "Angora: Efficient Fuzzing by Principled Search" (S&P 2018)

**Mechanism:** Treat branch conditions as distance functions, compute partial derivatives
via finite differences (perturb input byte by +/-1, measure distance change), descend toward
zero distance. Solves numeric constraints 10-100x faster than SMT solvers.
- Exponential step size search: try 1, 2, 4, 8, ... until distance stops improving
- Falls back to Z3 for non-numeric / complex constraints

**APEX location:** `crates/apex-symbolic/src/gradient.rs`
**Integration:** `PortfolioSolver::with_gradient_first()` in `crates/apex-symbolic/src/portfolio.rs`

## Code Property Graph (Joern / ShiftLeft)

**Source:** Joern CPG schema + REACHING_DEF pass + backward reachability
**Paper:** Yamaguchi et al., "Modeling and Discovering Vulnerabilities with Code Property Graphs" (S&P 2014)

**Mechanism:** Unified graph combining AST + CFG + data-dependency (REACHING_DEF) edges.
Taint analysis via backward BFS from sinks following ReachingDef edges to sources.
- MOP (Meet-Over-all-Paths) iterative dataflow for reaching definitions
- Sanitizer nodes cut taint propagation during backward traversal
- Source/sink/sanitizer tables for Python security patterns

**APEX location:** `crates/apex-cpg/` (new crate)
- `src/lib.rs` — NodeKind, EdgeKind, Cpg graph structure
- `src/builder.rs` — Python source → CPG construction
- `src/reaching_def.rs` — iterative gen/kill fixpoint
- `src/taint.rs` — backward reachability + source/sink tables

## LLM-Guided Test Refinement (CoverUp)

**Source:** CoverUp's `improve_coverage()` loop + AST segment extraction
**Paper:** Pizzorno & Berger, "CoverUp: Coverage-Guided LLM-Based Test Generation" (2024)

**Mechanism:** Closed-loop generate-run-measure-refine cycle:
1. Extract code segment around uncovered branch (with line-number tags)
2. Prompt LLM to generate a test covering it
3. Run test, measure coverage
4. If error: feed error back to LLM, retry
5. If no coverage gain: feed "still missing lines X-Y" back, retry
6. Up to 3 attempts per gap

**APEX location:**
- `crates/apex-synth/src/llm.rs` — LlmSynthesizer with `fill_gap()` loop
- `crates/apex-synth/src/segment.rs` — `extract_segment()` + `clean_error_output()`

## Priority-Based Exploration (Owi + EvoMaster)

**Source:** Owi's `Prio` module + EvoMaster's `Archive.chooseTarget()`
**Paper:** Various (Owi: WASM symbolic execution; EvoMaster: REST API testing)

**Mechanism:** Composite priority for selecting which uncovered branch to focus on:
- **Rarity** (Owi): `1 / (hit_count + 1)` — prefer code reached by fewer inputs
- **Depth penalty** (Owi): `1 / ln(1 + depth)` — penalize deeply nested paths
- **Proximity** (EvoMaster): use branch distance heuristic as priority signal
- **Staleness bonus**: boost branches stuck without progress to rotate strategies

Strategy routing: high proximity → gradient solver, medium → fuzzer, low/stalled → LLM synthesis.

**APEX location:**
- `crates/apex-agent/src/priority.rs` — `target_priority()`, `recommend_strategy()`
- `crates/apex-agent/src/cache.rs` — `SolverCache` with negation inference (from Owi)

## Solver Caching with Negation Inference (Owi)

**Source:** Owi's solver cache
**Context:** WASM parallel symbolic execution engine

**Mechanism:** Cache SAT/UNSAT results keyed by constraint string.
Negation inference: if `(not C)` is cached as UNSAT, infer `C` is SAT without querying
the solver. Reduces redundant solver calls during exploration.

**APEX location:** `crates/apex-agent/src/cache.rs`

## CWE ID Mapping (Bearer / Industry Standard)

**Source:** Bearer's finding-to-CWE mapping pattern
**Standard:** MITRE CWE (Common Weakness Enumeration)

**Mechanism:** Every security finding carries `cwe_ids: Vec<u32>` for compliance reporting
(SOC2, HIPAA, PCI-DSS). Mapping table from detection category to CWE:

| Category | CWE |
|----------|-----|
| OS command injection | CWE-78 |
| XSS | CWE-79 |
| SQL injection | CWE-89 |
| Code injection (eval/exec) | CWE-94 |
| Buffer overflow | CWE-120 |
| Certificate validation | CWE-295 |
| Weak hash | CWE-328 |
| Deserialization | CWE-502 |
| Hardcoded credentials | CWE-798 |
| Path traversal | CWE-22 |

**APEX location:**
- `crates/apex-detect/src/finding.rs` — `cwe_ids` field on `Finding`
- `crates/apex-detect/src/detectors/security_pattern.rs` — `cwe` field on `SecurityPattern`

---

# Candidate Mechanisms — Security Frameworks

## OWASP Top 10 Detection Patterns

**Source:** [OWASP Top 10:2021](https://owasp.org/Top10/2021/)
**Standard:** Industry-standard web application security risk classification

**Mechanism:** Each OWASP category maps to specific CWEs detectable via static/dataflow analysis:

| Category | Key CWEs | Detection Pattern | Feasibility |
|----------|----------|-------------------|-------------|
| A01: Broken Access Control | CWE-200, 352, 862, 863 | Missing auth decorators on endpoints; IDOR without ownership checks; missing CSRF tokens | Medium |
| A02: Cryptographic Failures | CWE-259, 327, 331 | Weak algorithms (MD5, SHA1, DES, RC4); hardcoded keys; missing TLS enforcement | High |
| A03: Injection | CWE-79, 89, 78, 77 | Taint: user input → SQL/shell/HTML sinks without sanitization | High |
| A04: Insecure Design | 40 CWEs | Missing rate limiting; absent input validation on business logic | Low |
| A05: Security Misconfiguration | CWE-16, 611 | `DEBUG=True`; XML external entity processing enabled; default credentials | Medium |
| A06: Vulnerable Components | N/A | SCA: match dependency versions against NVD/OSV/Safety DB | High |
| A07: Auth Failures | CWE-287, 306, 798 | Hardcoded credentials; weak password policies; session fixation | Medium |
| A08: Integrity Failures | CWE-502, 829 | `pickle.loads()`, `yaml.load()` without safe Loader; missing integrity checks | High |
| A09: Logging Failures | CWE-778 | Missing audit logging after auth events; sensitive data in logs | Low-Medium |
| A10: SSRF | CWE-918 | Taint: user-controlled URLs → HTTP client without allowlist | High |

**Potential APEX location:** `crates/apex-detect/src/detectors/` — one detector per OWASP category
**Priority:** High — A03 (Injection), A02 (Crypto), A08 (Integrity), A10 (SSRF) are directly implementable via existing CPG taint + pattern matching

## CWE/SANS Top 25 Coverage

**Source:** [CWE Top 25 (2023)](https://cwe.mitre.org/top25/archive/2023/2023_top25_list.html)

**Mechanism:** 60% of Top 25 are detectable through taint analysis (source-sink tracking). The remainder require AST pattern matching or semantic analysis.

| Rank | CWE | Name | Detection |
|------|-----|------|-----------|
| 1 | 787 | Out-of-bounds Write | Buffer size tracking |
| 2 | 79 | XSS | Taint: input → HTML output |
| 3 | 89 | SQL Injection | Taint: input → SQL query |
| 5 | 78 | OS Command Injection | Taint: input → shell exec |
| 8 | 22 | Path Traversal | Taint: input → file path |
| 9 | 352 | CSRF | Missing CSRF token checks |
| 15 | 502 | Deserialization | Pattern: dangerous deserializers |
| 18 | 798 | Hardcoded Credentials | Pattern: string literals matching secrets |
| 19 | 918 | SSRF | Taint: input → HTTP request URL |
| 23 | 94 | Code Injection | Taint: input → eval/exec |

**APEX already detects:** CWE-78, 79, 89, 94, 22, 502, 798 via `apex-detect`
**Gaps to fill:** CWE-787, 352, 918, 416, 20, 862, 362

## OWASP ASVS Compliance Verification

**Source:** [OWASP ASVS v4](https://owasp.org/www-project-application-security-verification-standard/)

**Mechanism:** 286 requirements across 14 chapters, 3 levels (L1/L2/L3). Map each automatable requirement to a detection rule, then output a compliance matrix.

Automatable chapters for static analysis:
- V2 Authentication — hardcoded credentials, weak hashing (bcrypt vs MD5)
- V5 Validation/Sanitization — input validation on API boundaries
- V6 Cryptography — algorithm allowlist, key length, RNG usage
- V8 Data Protection — sensitive data in logs, PII in error messages
- V14 Configuration — debug mode, default credentials, unnecessary endpoints

**Potential APEX location:** `apex-detect` rules tagged with ASVS requirement IDs; `apex-cli` ASVS compliance report output
**Priority:** Medium — primarily metadata/output formatting on top of existing detectors

## STRIDE Automated Threat Matrix

**Source:** Microsoft STRIDE threat modeling framework
**Reference:** [STRIDE model](https://en.wikipedia.org/wiki/STRIDE_model)

**Mechanism:** Map each STRIDE category to detectable code patterns:

| Category | Detection |
|----------|-----------|
| **Spoofing** | Missing authentication checks on endpoints |
| **Tampering** | Missing integrity checks; unvalidated input modifying state |
| **Repudiation** | Missing audit logging; no transaction logging |
| **Information Disclosure** | Sensitive data in logs; verbose error messages; PII exposure |
| **Denial of Service** | Missing rate limiting; unbounded allocation; ReDoS patterns |
| **Elevation of Privilege** | Missing authorization; IDOR; privilege escalation patterns |

APEX could auto-generate a STRIDE matrix: "Based on analysis, these threats have no detected mitigation."

**Potential APEX location:** `apex-detect` + report formatter in `apex-cli`
**Priority:** Medium — leverages existing detectors, adds threat-model framing

## CVSS Auto-Scoring for Findings

**Source:** [CVSS v4.0](https://www.first.org/cvss/)

**Mechanism:** Derive CVSS base metrics from code context:

| Metric | Derivation |
|--------|-----------|
| Attack Vector (AV) | Network if finding is in web handler; Local if in CLI |
| Attack Complexity (AC) | Low if direct taint path; High if multiple conditions needed |
| Privileges Required (PR) | None if pre-auth; Low/High based on auth context |
| User Interaction (UI) | None if server-side; Required if client-side (XSS) |
| Scope (S) | Changed if finding crosses trust boundary |
| Confidentiality (C) | High if data exposure; Low if metadata only |
| Integrity (I) | High if data modification; Low if read-only |
| Availability (A) | High if DoS; Low if degradation |

Map CWE → default CVSS base metrics, refine by reachability from network handlers.

**Potential APEX location:** `crates/apex-detect/src/finding.rs` — `cvss_score` field; scoring logic in new `cvss.rs`
**Priority:** High — immediate value for finding prioritization

## DREAD Risk Scoring

**Source:** Microsoft DREAD model (deprecated but useful for auto-scoring)
**Reference:** [DREAD model](https://en.wikipedia.org/wiki/DREAD_(risk_assessment_model))

**Mechanism:** 5 dimensions (0-10 each), partially auto-derivable:
- **Damage** — map CWE to typical severity
- **Reproducibility** — deterministic findings = 10; race conditions = lower
- **Exploitability** — taint path length as proxy (shorter = higher)
- **Affected Users** — auth/session vuln = all users; admin-only = fewer
- **Discoverability** — publicly documented CWE = high

**Potential APEX location:** `crates/apex-detect/src/scoring.rs`
**Priority:** Low — CVSS is the industry standard; DREAD adds minimal value on top

---

# Candidate Mechanisms — Static Analysis Engines

## Pattern DSL (Semgrep)

**Source:** [Semgrep](https://semgrep.dev/docs/writing-rules/pattern-syntax)

**Mechanism:** YAML-based code pattern matching with metavariables (`$X`), ellipsis (`...`),
boolean composition (`pattern-and`, `pattern-or`, `pattern-not`). Deterministic — same code +
same rules = same findings. Taint mode defines `pattern-sources`, `pattern-sinks`,
`pattern-sanitizers` with iterative intra-file dataflow.

**Why it matters:** Users could write custom detection rules in target-language syntax without
modifying APEX source code. The source/sink/sanitizer model is the standard for taint analysis.

**Potential APEX location:** `crates/apex-detect/src/rules/` — YAML rule loader + pattern matcher
**Priority:** High — extensible rule system is a force multiplier

## Code-as-Database Querying (CodeQL)

**Source:** [CodeQL](https://codeql.github.com/docs/codeql-overview/about-codeql/)

**Mechanism:** Converts source into a relational database. Queries written in QL (object-oriented
Datalog) traverse the database. Variant analysis: use a known vulnerability as a "seed query" to
find similar patterns across codebases.

**Why it matters:** Extremely expressive — "find all functions where user input reaches SQL
execution without parameterization." The variant analysis concept (find-similar-bugs) is powerful.

**Potential APEX location:** Would require a query language layer on top of `apex-cpg`
**Priority:** Low (high value but very large effort) — consider adopting query concepts on CPG

## Bi-Abduction / Compositional Analysis (Facebook Infer)

**Source:** [Infer](https://fbinfer.com/docs/separation-logic-and-bi-abduction/)

**Mechanism:** Uses separation logic to reason about independent memory regions. Bi-abduction
automatically infers pre/post conditions per procedure, enabling bottom-up compositional analysis.
Each function analyzed independently; results composed incrementally. Only re-analyzes changed
procedures on incremental runs.

**Why it matters:** The scaling secret — breaks large-program analysis into small independent
per-function analyses. Essential for CI/CD where only changed files should be re-analyzed.

**Potential APEX location:** `crates/apex-cpg/src/summary.rs` — per-function taint summaries
**Priority:** Medium — the incremental analysis concept is high value; full separation logic is complex

## Python Taint Analysis (Pysa / Meta)

**Source:** [Pysa](https://pyre-check.org/docs/pysa-basics/)

**Mechanism:** Built on Pyre type checker. Iterative per-function taint summaries. Uses `.pysa`
model files to annotate framework APIs as sources/sinks/sanitizers. `taint.config` connects
source types to sink types. Favors completeness over soundness (catch everything, accept FPs).

**Why it matters:** The model file concept is essential — framework APIs (Flask, Django, FastAPI)
need source/sink annotations. Iterative summary-building scales to large codebases.

**Potential APEX location:** `crates/apex-cpg/src/taint.rs` — extend with Pysa-style model files
**Priority:** High — APEX's taint analysis needs framework-aware source/sink annotations

## SSA-Based IR with Def-Use Chains (Slither)

**Source:** [Slither](https://blog.trailofbits.com/2018/10/19/slither-a-solidity-static-analysis-framework/)

**Mechanism:** Converts source to intermediate representation in Static Single Assignment form.
Enables standard dataflow: explicit def-use chains, taint fixpoint across functions, read/write
set analysis. Though built for Solidity, the IR pattern is universal.

**Why it matters:** SSA form simplifies dataflow analysis significantly. Explicit def-use chains
enable precise taint tracking without re-computing reaching definitions.

**Potential APEX location:** `crates/apex-cpg/src/ssa.rs` — SSA conversion pass on CPG
**Priority:** Medium — architectural improvement to existing CPG

## AST Pattern Checks — 47 Python Rules (Bandit)

**Source:** [Bandit](https://github.com/PyCQA/bandit)

**Mechanism:** Plugin-based NodeVisitor on Python AST. 47 built-in checks across 7 categories
(injection, crypto, XSS, hardcoded credentials, etc.). Single-function scope — no inter-procedural.

**Why it matters:** Ready-made Python security ruleset. APEX could implement the same checks
with inter-procedural taint tracking, catching what Bandit misses.

**Potential APEX location:** `crates/apex-detect/src/detectors/` — adopt Bandit's check catalog
**Priority:** High — quick wins, well-documented rules

---

# Candidate Mechanisms — Fuzzing Innovations

## MOpt Adaptive Mutation Scheduling (AFL++)

**Source:** [AFL++](https://aflplus.plus/docs/fuzzing_in_depth/)
**Paper:** Fioraldi et al., "AFL++: Combining Incremental Steps of Fuzzing Research" (USENIX WOOT 2020)

**Mechanism:** Particle swarm optimization to dynamically assign probabilities to mutation
operators. Adapts strategy distribution based on which operators find new paths. Enabled with `-L`.

**Why it matters:** Transferable to any mutation-based search (fuzzing, test generation).
Operators that produce coverage gains get more chances.

**Potential APEX location:** `crates/apex-fuzz/src/scheduler.rs`
**Priority:** High — directly improves fuzzer effectiveness

## CmpLog / RedQueen Input-to-State (AFL++)

**Source:** AFL++'s CmpLog + RedQueen (from original RedQueen paper, NDSS 2019)

**Mechanism:** Instruments comparison operations, logs operands of last 256 executions per
comparison in a 256 MB shared table. Fuzzer extracts values and places them at various input
positions, solving "magic byte" constraints without SMT solver.

**Why it matters:** Solves the magic-number problem cheaply. When a branch compares input to
a constant (e.g., `if header == "MAGIC"`), CmpLog captures the expected value and injects it.

**Potential APEX location:** `crates/apex-fuzz/src/cmplog.rs`
**Priority:** High — cheap alternative to symbolic execution for many constraint types

## Hybrid Fuzzing — Fuzz-then-Solve (Driller)

**Source:** [Driller](https://github.com/shellphish/driller)
**Paper:** Stephens et al., "Driller: Augmenting Fuzzing Through Selective Symbolic Execution" (NDSS 2016)

**Mechanism:** Alternates between AFL (cheap) and concolic execution (expensive). AFL explores
within program "compartments." When AFL gets stuck (no new coverage for N iterations), Driller
invokes symbolic execution to solve the specific blocking constraint, generating an input that
crosses into the next compartment.

**Why it matters:** APEX already has both fuzzing and concolic — Driller provides the orchestration
pattern. "Stuck detection" triggers concolic only when needed, avoiding path explosion.

**Potential APEX location:** `crates/apex-agent/src/strategy.rs` — stuck detection + escalation logic
**Priority:** High — directly maps to APEX's existing `recommend_strategy()` architecture

## Coverage-Guided Property-Based Testing (HypoFuzz)

**Source:** [HypoFuzz](https://hypofuzz.com/)
**Paper:** FuzzChick (ICFP 2019) — coverage-guided PBT theory

**Mechanism:** Fuses Hypothesis (structured PBT) with coverage-guided fuzzing. Mutations are
semantically meaningful (understand input structure via Hypothesis strategies). Uses multiple
feedback signals — branch coverage + `hypothesis.target()` scores.

**Why it matters:** Bridges "find crashes" (fuzzing) and "find logic bugs" (PBT). APEX could
generate Hypothesis-decorated tests via LLM, then use coverage feedback to guide refinement.

**Potential APEX location:** `crates/apex-synth/src/property.rs` — Hypothesis test generation
**Priority:** High — single highest-leverage integration per methodology research

## Power Schedules / Seed Energy (AFLfast)

**Source:** AFLfast (Böhme et al., CCS 2016)

**Mechanism:** Assign energy (number of mutations) to seeds based on rarity — seeds exercising
rare paths get exponentially more mutations. Low-frequency paths are under-explored by default;
power schedules correct this bias.

**Why it matters:** Informs seed prioritization in coverage-guided testing. APEX's priority
system could weight corpus entries similarly.

**Potential APEX location:** `crates/apex-fuzz/src/corpus.rs` — seed energy assignment
**Priority:** Medium — refines existing fuzzing, not a new capability

## Grammar-Based Fuzzing (Nautilus)

**Source:** Nautilus — grammar-based fuzzing with coverage guidance
**Paper:** Aschermann et al., "NAUTILUS: Fishing for Deep Bugs with Grammars" (NDSS 2019)

**Mechanism:** Uses context-free grammars to generate syntactically valid inputs. Combines
grammar-aware mutations (subtree replacement, random recursion) with coverage feedback.
Essential for structured inputs (JSON, XML, SQL, Python source).

**Why it matters:** When fuzzing parsers or structured-input handlers, random byte mutation
is wasteful. Grammar-aware generation explores meaningful input space.

**Potential APEX location:** `crates/apex-fuzz/src/grammar.rs`
**Priority:** Medium — valuable for specific target types

---

# Candidate Mechanisms — Symbolic Execution

## Search Strategy Framework (KLEE)

**Source:** [KLEE](https://llvm.org/pubs/2008-12-OSDI-KLEE.pdf)
**Paper:** Cadar et al., "KLEE: Unassisted and Automatic Generation of High-Coverage Tests" (OSDI 2008)

**Mechanism:** Maintains a set of symbolic execution states. Core search strategies:
- **Coverage-Optimized:** Weight states by likelihood of covering new code
- **Random Path Selection:** Shorter paths get higher probability, preventing deep-path starvation
- **Round-Robin:** Combine multiple heuristics to avoid local maxima

**Why it matters:** APEX's concolic engine needs principled path selection. Round-robin
multi-heuristic is a robust default.

**Potential APEX location:** `crates/apex-concolic/src/search.rs`
**Priority:** Medium — improves exploration efficiency

## Selective Symbolic Execution (S2E)

**Source:** [S2E](https://github.com/S2E/s2e)
**Paper:** Chipounov et al., "S2E: A Platform for In-Vivo Multi-Path Analysis" (ASPLOS 2011)

**Mechanism:** Runs full system concretely (QEMU). Only switches to symbolic execution when
a translation block references registers with symbolic content. Key insight: only go symbolic
where needed — everything else runs at native speed.

**Why it matters:** The scope-of-interest model applies to APEX: only analyze functions
touching user input or security-critical operations symbolically. Everything else stays concrete.

**Potential APEX location:** `crates/apex-concolic/src/selective.rs`
**Priority:** Low — requires significant architectural work

## Vulnerability Condition Solving (Mythril)

**Source:** [Mythril](https://github.com/ConsenSysDiligence/mythril)

**Mechanism:** Encode vulnerability condition as Z3 SMT constraint, solve for triggering input.
Pattern: "can user input reach this dangerous state?" becomes a satisfiability query.

**Why it matters:** Universal pattern applicable to any language. Given a taint path from
source to sink, encode the path constraint and solve for a concrete exploit input.

**Potential APEX location:** `crates/apex-symbolic/src/exploit.rs` — exploit input generation
**Priority:** Medium — powerful for security-focused analysis

---

# Candidate Mechanisms — Testing Methodologies

## Mutation Testing as Coverage Metric (mutmut / cosmic-ray)

**Source:** [mutmut](https://github.com/boxed/mutmut) | [cosmic-ray](https://github.com/sixty-north/cosmic-ray)
**Paper:** DeMillo et al., "Hints on Test Data Selection" (1978)

**Mechanism:** Introduce small code changes (mutants): flip operators, change constants, alter
return values, remove statements. Run test suite against each mutant. Surviving mutants =
weak test assertions.

- mutmut: In-memory AST mutation via Parso (round-trip-safe); no disk writes
- cosmic-ray: Distributed execution across workers; configurable mutation operators

**Why it matters:** Mutation score answers "are my tests actually testing anything?" vs coverage
which only asks "did tests run this line?" If APEX generates tests, mutation testing validates
their quality.

**Integration flow:** `apex-instrument` (inject mutants) → `apex-sandbox` (run tests) →
`apex-coverage` (record mutation score) → `apex-agent` (surviving mutant → `apex-synth` → new killing test)

**Potential APEX location:** `crates/apex-coverage/src/mutation.rs` — mutation score metric
**Priority:** High — most valuable metric APEX can add beyond line/branch coverage

## Property-Based Test Generation (Hypothesis / QuickCheck)

**Source:** QuickCheck (1999), Hypothesis (2013), HypoFuzz
**Paper:** "Property-Based Testing Is Fuzzing" — Nelson Elhage (2019)

**Mechanism:** Generate random structured inputs to verify program properties:
1. Infer properties from function signatures + docstrings (idempotency, commutativity, roundtrip)
2. Generate `@given(...)` decorators with appropriate Hypothesis strategies from type annotations
3. Use coverage feedback to guide which inputs to mutate (HypoFuzz approach)
4. Shrink failing inputs to minimal reproduction case

**Why it matters:** Bridges APEX's fuzzing and test synthesis. LLM infers properties,
Hypothesis generates inputs, coverage feedback guides exploration.

**Potential APEX location:**
- `crates/apex-synth/src/property.rs` — Hypothesis test generation
- `crates/apex-fuzz/src/shrinker.rs` — input minimization trait
**Priority:** High — bridges fuzzing and logic-bug detection

## Clean Architecture Conformance (import-linter / CALint)

**Source:** [import-linter](https://github.com/seddonym/import-linter) | CALint (ICCSA 2022)
**Paper:** "CALint: Clean Architecture Linter for Python" (ICCSA 2022)

**Mechanism:** Enforce dependency rule — source code dependencies must point inward only
(Entities ← Use Cases ← Adapters ← Frameworks). Graph problem on module-level import graph:
1. Classify modules into architectural layers via config or directory convention
2. Build import graph from CPG
3. Any edge from inner layer to outer layer = violation

**Why it matters:** Architectural drift is a major source of tech debt. APEX's CPG already
has import edges — adding layer classification enables automated architecture enforcement.

**Potential APEX location:** `crates/apex-cpg/src/architecture.rs` — layer classification + violation detection
**Priority:** Medium — reuses existing CPG infrastructure

## 12-Factor App Verification

**Source:** [The Twelve-Factor App](https://12factor.net/) (Adam Wiggins, 2011)

**Mechanism:** Static detection of 12-factor violations:

| Factor | Detection |
|--------|-----------|
| III. Config | Hardcoded DB URLs, API keys, `configparser` without env fallback |
| IV. Backing Services | Hardcoded connection strings (`localhost:5432`) vs env-driven |
| VI. Processes | File-system writes for session state in request handlers; global mutable state |
| VII. Port Binding | Non-configurable port numbers; `0.0.0.0` vs `127.0.0.1` |
| X. Dev/Prod Parity | `if DEBUG` branches diverging behavior; mock-only deps |
| XI. Logs | `open("app.log", "w")` file logging vs stdout; `FileHandler` without stream |

Factors I, II, V, VIII, IX, XII are primarily infrastructure — not amenable to source analysis.

**Potential APEX location:** `crates/apex-detect/src/detectors/twelve_factor.rs`
**Priority:** Medium — Factor III (Config) overlaps with existing hardcoded secret detection

## Missing Resilience Patterns (Reactive Manifesto)

**Source:** Reactive Manifesto + resilience patterns (circuit breakers, bulkheads, timeouts)

**Mechanism:** Detect absence of resilience patterns:
- `requests.get(url)` without `timeout=` → hangs indefinitely
- Bare `except` + retry without exponential backoff
- HTTP calls without circuit breaker wrapping
- Shared connection pools across unrelated services

**Potential APEX location:** `crates/apex-detect/src/detectors/resilience.rs`
**Priority:** High for timeout detection (real production bugs), Low for others

---

# Candidate Mechanisms — Supply Chain & Compliance

## SCA via Dependency Manifest Parsing (OWASP Dependency-Check / pip-audit)

**Source:** [OWASP Dependency-Check](https://owasp.org/www-project-dependency-check/) | [pip-audit](https://github.com/pypa/pip-audit)

**Mechanism:**
1. Parse manifests (requirements.txt, Cargo.toml, package-lock.json)
2. Evidence-based CPE matching with confidence scoring (not just exact version lookup)
3. Query NVD/OSV/PyPI Advisory DB for known CVEs
4. Report with CVSS scores

**Differentiator:** Most SCA tools stop at "this dep has a CVE." APEX could combine SCA with
call-graph analysis to report whether the vulnerable function is actually reachable.

**Potential APEX location:** `crates/apex-detect/src/sca/` — new SCA module
**Priority:** High — table-stakes capability for security tooling

## Reachability-Annotated SBOM (CycloneDX + Call Graph)

**Source:** [CycloneDX](https://cyclonedx.org/) | [SPDX](https://spdx.dev/)

**Mechanism:** Generate CycloneDX JSON SBOM from dependency manifests. Then annotate with
reachability data from APEX's call graph — mark which vulnerable dependency functions are
actually called. Attach VEX (Vulnerability Exploitability eXchange) data.

**Why it matters:** Most SBOMs just list deps. A reachability-annotated SBOM answers "is this
CVE actually exploitable in my code?" — dramatically reducing false-positive noise.

**Potential APEX location:** `crates/apex-detect/src/sbom.rs`
**Priority:** Medium — high value but depends on SCA + call graph infrastructure

## SARIF Output Format

**Source:** [SARIF (Static Analysis Results Interchange Format)](https://sarifweb.azurewebsites.net/)

**Mechanism:** Standardized JSON format for static analysis results. Native integration with
GitHub Security tab, VS Code Problems panel, GitLab SAST dashboards.

**Why it matters:** Zero-effort integration with existing developer workflows. APEX findings
appear natively in GitHub PRs without custom tooling.

**Potential APEX location:** `crates/apex-detect/src/report/sarif.rs`
**Priority:** High — small effort, huge integration payoff

## SLSA Provenance & Attestation (Sigstore / in-toto)

**Source:** [SLSA](https://slsa.dev/) | [Sigstore](https://docs.sigstore.dev/) | [in-toto](https://in-toto.io/)

**Mechanism:** APEX generates signed attestations for its analysis:
- Sigstore/Fulcio: keyless signing via OIDC identity
- Rekor: immutable transparency log (append-only Merkle tree)
- in-toto: layout (expected steps) + links (evidence of performed steps)

Attestation: "this code was scanned by APEX vX, these findings were produced."

**Potential APEX location:** `crates/apex-cli/src/attest.rs`
**Priority:** Low — strategic for enterprise adoption

## NIST SSDF Compliance Tags (SP 800-218)

**Source:** [NIST SP 800-218](https://csrc.nist.gov/pubs/sp/800/218/final)

**Mechanism:** Tag APEX reports with SSDF task IDs they satisfy:
- PW.7.1 — static analysis performed
- RV.1.1 — vulnerability findings gathered
- PO.3.1 — security-focused tools in toolchain

**Potential APEX location:** Report metadata in `apex-cli`
**Priority:** Low — documentation/metadata effort

---

# Candidate Mechanisms — DevSecOps Integration

## Extended Ratchet Gates

**Source:** DevSecOps pipeline best practices

**Mechanism:** Extend `apex ratchet` beyond coverage:

| Gate | Condition |
|------|-----------|
| Coverage ratchet | Coverage % ≥ previous commit (implemented) |
| Security ratchet | No new HIGH/CRITICAL findings |
| Architecture ratchet | No new dependency rule violations |
| Mutation score ratchet | Mutation score ≥ threshold on changed files |
| Complexity ratchet | No function exceeding cyclomatic complexity threshold |

**Potential APEX location:** `apex-cli ratchet --security --architecture --mutation`
**Priority:** High — security ratchet is a quick win

## Pre-Commit Fast Mode

**Source:** Shift-left security practice

**Mechanism:** `apex detect --fast` for pre-commit hooks — pattern matching only, no CPG
construction, completes in <2s. Full analysis (`--full` with CPG/taint) runs in CI.

**Potential APEX location:** `apex-cli detect --fast`
**Priority:** High — instant developer feedback loop

## Pipeline Integration Flow

```
Developer Commit
  → [Pre-commit] apex detect --fast (pattern matching, <2s)
  → [CI: Build]  apex detect --full (CPG analysis + taint, 30-60s)
  → [CI: Test]   apex run --target . --lang python (coverage + gap report)
  → [CI: Gate]   apex ratchet --security --coverage --architecture
  → [CI: Report] apex report --format sarif --output findings.sarif
  → Merge / Deploy
```

---

# Reference Papers & Implementations

## Must-Read Papers

| Paper | Year | Relevance |
|-------|------|-----------|
| Korel, "Automated Software Test Data Generation" | 1990 | Branch distance (implemented) |
| Cadar et al., "KLEE: Unassisted Test Generation" (OSDI) | 2008 | Symbolic execution search strategies |
| Yamaguchi et al., "Code Property Graphs" (S&P) | 2014 | CPG analysis (implemented) |
| Stephens et al., "Driller" (NDSS) | 2016 | Hybrid fuzzing orchestration |
| Böhme et al., "AFLfast" (CCS) | 2016 | Power schedules for seed energy |
| Chen & Chen, "Angora" (S&P) | 2018 | Gradient descent solving (implemented) |
| Aschermann et al., "NAUTILUS" (NDSS) | 2019 | Grammar-based fuzzing |
| Aschermann et al., "RedQueen" (NDSS) | 2019 | Input-to-state correspondence |
| Fioraldi et al., "AFL++" (USENIX WOOT) | 2020 | MOpt, CmpLog, collision-free coverage |
| Pizzorno & Berger, "CoverUp" | 2024 | LLM-guided test refinement (implemented) |
| FuzzChick (ICFP 2019) | 2019 | Coverage-guided property-based testing |
| CALint (ICCSA 2022) | 2022 | Clean Architecture linting for Python |
| LLMxCPG (USENIX Security 2025) | 2025 | Combining CPG with LLM for vulnerability detection |

## Key Reference Implementations

| Tool | Mechanism | URL |
|------|-----------|-----|
| HypoFuzz | Coverage-guided PBT | https://github.com/Zac-HD/hypofuzz |
| mutmut | Python mutation testing | https://github.com/boxed/mutmut |
| cosmic-ray | Distributed mutation testing | https://github.com/sixty-north/cosmic-ray |
| import-linter | Architecture rule enforcement | https://github.com/seddonym/import-linter |
| Semgrep | Pattern DSL + taint | https://github.com/semgrep/semgrep |
| Bandit | Python AST security checks | https://github.com/PyCQA/bandit |
| Atheris | Python coverage-guided fuzzing | https://github.com/google/atheris |
| Driller | Hybrid fuzz + concolic | https://github.com/shellphish/driller |
| Infer | Compositional analysis | https://github.com/facebook/infer |
| Pysa | Python taint analysis | https://github.com/facebook/pyre-check |
| Slither | SSA-based IR analysis | https://github.com/crytic/slither |
| Triton | Dynamic symbolic execution | https://github.com/JonathanSalwan/Triton |

---

# Priority Matrix

## Tier 1 — High Value, Directly Implementable

| Mechanism | Source | APEX Crate | Effort |
|-----------|--------|------------|--------|
| SARIF output | SARIF standard | `apex-detect` | Small |
| Security ratchet gate | DevSecOps | `apex-cli` | Small |
| Missing timeout detection | Reactive Manifesto | `apex-detect` | Small |
| Pre-commit fast mode | Shift-left | `apex-cli` | Small |
| CVSS auto-scoring | CVSS v4.0 | `apex-detect` | Small |
| CmpLog / RedQueen | AFL++ | `apex-fuzz` | Medium |
| MOpt mutation scheduling | AFL++ | `apex-fuzz` | Medium |
| Bandit rule catalog (47 checks) | Bandit | `apex-detect` | Medium |
| Pysa-style model files | Pysa | `apex-cpg` | Medium |
| SCA dependency scanning | Dependency-Check / pip-audit | `apex-detect` | Medium |

## Tier 2 — High Value, Moderate Effort

| Mechanism | Source | APEX Crate |
|-----------|--------|------------|
| Hybrid fuzzing orchestration | Driller | `apex-agent` |
| Coverage-guided PBT | HypoFuzz | `apex-synth` + `apex-fuzz` |
| Mutation testing as metric | mutmut | `apex-coverage` + `apex-instrument` |
| Pattern DSL for rules | Semgrep | `apex-detect` |
| Clean Architecture conformance | import-linter / CALint | `apex-cpg` |
| Compositional per-function analysis | Infer | `apex-cpg` |
| Reachability-annotated SBOM | CycloneDX + call graph | `apex-detect` |
| OWASP Top 10 full detector set | OWASP | `apex-detect` |

## Tier 3 — Strategic / Long-Term

| Mechanism | Source | APEX Crate |
|-----------|--------|------------|
| Code-as-database querying | CodeQL | `apex-cpg` |
| ASVS compliance reporting | OWASP ASVS | `apex-detect` + `apex-cli` |
| STRIDE automated threat matrix | STRIDE | `apex-detect` + `apex-cli` |
| SLSA provenance / attestation | Sigstore / in-toto | `apex-cli` |
| Selective symbolic execution | S2E | `apex-concolic` |
| SSA-based IR conversion | Slither | `apex-cpg` |
| KLEE search strategies | KLEE | `apex-concolic` |
| NIST SSDF compliance tags | NIST SP 800-218 | `apex-cli` |
