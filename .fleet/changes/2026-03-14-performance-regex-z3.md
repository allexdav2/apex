---
date: 2026-03-14
crew: performance-officer
affected_partners: [runtime, security-detect, intelligence]
severity: major
acknowledged_by: []
---

## 15+ Regex::new() in hot loops and missing Z3 solver timeout

### Regex recompilation (15+ sites across 10 files)
- apex-concolic/python.rs: 7 regexes per branch in boundary_seeds()
- apex-instrument/mutant.rs: 7 regexes per file in generate_mutants()
- apex-detect detectors: command_injection, broken_access, path_traversal,
  hardcoded_secret, mixed_bool_ops, bandit — all compile per-file
- apex-reach extractors: python.rs, javascript.rs, rust.rs — per-file
- apex-cpg/query/executor.rs: per pattern match evaluation

Fix: convert all to static LazyLock<Regex>.

### Z3 solver has no timeout
- apex-symbolic/solver.rs:115-117 — no timeout parameter on Z3 solver
- apex-symbolic/portfolio.rs — stores timeout field but never enforces it

### Other
- MutationCache uses Vec::remove(0) — O(n) eviction, should use VecDeque
- Unbounded seed_queue in RPC coordinator
- var_names dedup uses Vec::contains instead of HashSet
