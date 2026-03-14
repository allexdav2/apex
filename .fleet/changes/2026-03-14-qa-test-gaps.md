---
date: 2026-03-14
crew: qa-officer
affected_partners: [runtime, platform]
severity: major
acknowledged_by: []
---

## Test coverage gaps: libafl_backend untested, apex-reach undertested

1. `crates/apex-fuzz/src/libafl_backend.rs`: 173 LOC, 5 public methods,
   0 tests. Feature-gated behind libafl-backend so never tested in CI.

2. `crates/apex-reach`: 18.2 tests/kLOC vs workspace median ~45.
   engine.rs (405 LOC, 9 tests) and graph.rs (192 LOC, 5 tests) are gaps.

3. `crates/apex-rpc/src/worker.rs`: 4 test setups use fixed 200ms sleep
   for server readiness (lines 198, 534, 809, 956). Flaky under CI load.
