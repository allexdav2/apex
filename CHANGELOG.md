# Changelog

All notable changes to APEX will be documented in this file.

## [0.1.0] — 2026-03-12

Initial release.

### Core Infrastructure
- Workspace with 14 crates and shared dependency management
- `ApexConfig` with TOML configuration loading and defaults
- Unified error handling with `ApexError` and `thiserror`
- Async trait-based `Instrumentor` and `LanguageRunner` abstractions

### Coverage
- `CoverageOracle` with bitmap-based edge coverage tracking
- Delta coverage computation for incremental analysis
- Shared-memory bitmap support for cross-process coverage

### Instrumentation
- Python: AST-based branch probe injection
- JavaScript: Istanbul-compatible instrumentation
- Java: bytecode instrumentation
- Rust: `cargo-llvm-cov` integration + `sancov` runtime
- Optional LLVM IR instrumentation (feature: `llvm-instrument`)
- Optional WebAssembly instrumentation (feature: `wasm-instrument`)

### Language Runners
- Test runners for Python (pytest), JavaScript (Jest/Node), Java (JUnit), Rust (cargo test), C (gcc), WebAssembly

### Sandbox
- Process-based sandbox with timeout and resource limits
- Shared-memory bitmap for coverage collection
- Optional Firecracker microVM isolation (feature: `firecracker`)

### Fuzzing
- Coverage-guided fuzzer with MOpt mutator scheduling
- Corpus management with LRU eviction
- Grammar-aware mutation, CmpLog feedback, directed fuzzing
- Optional LibAFL backend (feature: `libafl-backend`)

### Symbolic & Concolic
- SMT-LIB2 constraint solver with caching
- Portfolio solver strategy with bounded model checking
- Optional Z3 integration (feature: `z3-solver`)
- Optional Kani prover (feature: `kani-prover`)
- Python concolic execution engine with taint tracking
- Optional pyo3 tracer extension (feature: `pyo3-tracer`)

### AI Agent
- Multi-agent orchestration with ensemble strategies
- Source-context-aware test generation and refinement
- Bug ledger for tracking discovered issues
- Driller integration for coverage-guided exploration

### Test Synthesis
- Tera template-based test generation
- Synthesizers for pytest, Jest, JUnit, cargo-test

### Bug Detection
- Panic pattern detector for Rust code
- Security audit pipeline with configurable detectors
- Finding categorization and severity classification

### CLI
- `apex run` — full autonomous coverage pipeline
- `apex ratchet` — CI coverage gate with configurable threshold
- `apex doctor` — external tool dependency checker
- `apex audit` — security and bug detection analysis

### Infrastructure
- gRPC distributed coordination service (tonic/prost)
- MIR parsing and control-flow graph analysis
