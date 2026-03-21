<!-- status: DONE -->

# Coverage Modes Strategy Research

**Goal:** Evaluate 4 coverage modes (IMPORT, WRAP, INSTRUMENT, HARDWARE TRACE) via 3 parallel research digs, then synthesize into a unified strategy.

## File Map

| Dig | Agent | Output |
|-----|-------|--------|
| Dig 1: WRAP mode design | mycelium-core:fullstack-developer | Language-specific coverage injection mechanics for 11 languages |
| Dig 2: Quality improvement | mycelium-core:security-engineer | Finding severity re-scoring based on coverage data |
| Dig 3: Frida feasibility | mycelium-core:rust-engineer | frida-gum Rust integration, CoverageOracle mapping |

## Wave 1 (parallel research digs)

### Task 1.1 -- WRAP mode deep design (fullstack-developer)
- [ ] Research coverage injection for all 11 languages
- [ ] Classify which can inject WITHOUT modifying project config
- [ ] Estimate success rates vs IMPORT and INSTRUMENT
- [ ] Design CLI UX: `apex wrap -- <command>`
- [ ] Document findings

### Task 1.2 -- Quality improvement analysis (security-engineer)
- [ ] Analyze coverage-aware severity re-scoring
- [ ] Evaluate freshness benefit of WRAP over IMPORT
- [ ] Assess INSTRUMENT's test-to-finding correlation potential
- [ ] Evaluate HARDWARE TRACE's production coverage signal
- [ ] Determine FP reduction and severity amplification from coverage data

### Task 1.3 -- Frida binary instrumentation feasibility (rust-engineer)
- [ ] Evaluate frida-gum Rust crate maturity
- [ ] Research Frida attach vs spawn for `apex wrap` integration
- [ ] Analyze DWARF/addr2line address-to-source mapping
- [ ] Estimate overhead on macOS ARM64
- [ ] Assess SIP restrictions
- [ ] Design integration with CoverageOracle and BranchId

## Wave 2 (synthesis -- captain)

### Task 2.1 -- Synthesize strategy document
- [ ] Collect all 3 dig results
- [ ] Write unified strategy at docs/research/2026-03-21-coverage-modes-strategy.md
- [ ] Priority ordering, impact analysis, effort estimates, roadmap
