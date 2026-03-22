<!-- status: DONE -->

# Research Round 3 — Detector Quality, Coverage, and Novel Approaches

## Goal

Deep research into WHAT APEX should detect and HOW to detect it better.
APEX has 64 detector files covering ~16 CWEs. Round 3 evaluates gaps, taint analysis maturity, and novel detection approaches.

## Wave 1 (all parallel — independent research)

### Task 1.1 — Dig 7: Detector Gap Analysis (security-engineer)
**Output:** `docs/research/2026-03-21-detector-gap-analysis.md`
- Compare APEX CWE coverage against OWASP Top 10, CWE Top 25, SANS Top 25
- Catalog Semgrep, CodeQL, SonarQube, Bearer rule counts and CWE coverage
- For each missing CWE: static-only vs taint-required vs CPG-required
- Rate by impact x detectability matrix

### Task 1.2 — Dig 8: Taint Analysis State of the Art (research-analyst)
**Output:** `docs/research/2026-03-21-taint-analysis-survey.md`
- Compare apex-cpg to Joern, CodeQL dataflow, Semgrep taint, Bearer flow
- Survey academic: FlowDroid, IFDS/IDE, Doop
- Evaluate inter-procedural taint for APEX
- Assess tree-sitter CPG vs line-based builder

### Task 1.3 — Dig 9: Novel Detector Approaches (ai-engineer)
**Output:** `docs/research/2026-03-21-novel-detector-approaches.md`
- LLM vulnerability detection accuracy vs regex
- GNN for vuln detection (APEX has hagnn.rs)
- Differential analysis, spec inference, type-state, abstract interpretation
- Symbolic execution for vuln classes, contract mining
- FP reduction comparison, implementation cost, Rust crates
