# APEX — Autonomous Path EXploration

[![CI](https://github.com/allexdav2/apex/actions/workflows/ci.yml/badge.svg)](https://github.com/allexdav2/apex/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

APEX drives any repository toward 100% branch coverage by combining
instrumentation, fuzzing, concolic execution, symbolic solving, and
AI-guided test synthesis into a single autonomous pipeline.

## Features

- **Multi-language support** — Python, JavaScript, Java, Rust, C, WebAssembly
- **Coverage-guided fuzzing** — MOpt-mutator scheduling with corpus management
- **Concolic execution** — concrete + symbolic hybrid exploration
- **Symbolic constraint solving** — SMT-LIB2 solver with optional Z3 backend
- **AI agent orchestration** — LLM-driven test generation and refinement
- **Bug detection** — panic pattern analysis, security auditing
- **CI integration** — `apex ratchet` fails builds when coverage drops
- **Sandboxed execution** — process isolation, shared-memory bitmaps, optional Firecracker microVMs

## Quick Start

```bash
# Build
cargo build --release

# Run against a Python project
apex run --target ./my-project --lang python

# CI coverage gate (fails if coverage < 80%)
apex ratchet --target ./my-project --lang python --threshold 0.8

# Check tool dependencies
apex doctor

# Security audit
apex audit --target ./my-project --lang python
```

## Architecture

APEX is organized as a Rust workspace with 14 crates:

| Crate | Description |
|-------|-------------|
| **apex-core** | Shared types, traits, configuration, error handling |
| **apex-coverage** | Coverage oracle, bitmap management, delta tracking |
| **apex-instrument** | Multi-language instrumentation (Python, JS, Java, Rust, LLVM, WASM) |
| **apex-lang** | Language-specific test runners |
| **apex-sandbox** | Process/WASM/Firecracker sandbox execution, shared-memory bitmaps |
| **apex-agent** | AI agent orchestration, report generation, refinement loops |
| **apex-synth** | Test synthesis via Tera templates (pytest, Jest, JUnit, cargo-test) |
| **apex-symbolic** | Symbolic constraint solving (SMT-LIB2, optional Z3, Kani) |
| **apex-concolic** | Concolic execution engine (optional pyo3 tracer) |
| **apex-fuzz** | Coverage-guided fuzzing with MOpt scheduling (optional libafl) |
| **apex-detect** | Bug detection pipeline (panic patterns, security checks) |
| **apex-rpc** | gRPC distributed coordination (tonic) |
| **apex-mir** | Mid-level IR parsing and control-flow analysis |
| **apex-cli** | CLI binary — `run`, `ratchet`, `doctor`, `audit` subcommands |

```
                    ┌──────────┐
                    │ apex-cli │
                    └────┬─────┘
         ┌───────┬───────┼───────┬────────┐
         v       v       v       v        v
    apex-agent  apex-fuzz  apex-concolic  apex-detect
         │       │       │
         v       v       v
    apex-synth  apex-coverage  apex-symbolic
         │       │       │
         v       v       v
    apex-instrument  apex-sandbox  apex-mir
         │       │
         v       v
    apex-lang  apex-rpc
         │
         v
      apex-core
```

## Configuration

Copy `apex.example.toml` to your project root as `apex.toml`:

```toml
[coverage]
target = 1.0            # Coverage target (0.0–1.0)
min_ratchet = 0.8       # Minimum for CI gate

[fuzz]
corpus_max = 10000
mutations_per_input = 8
stall_iterations = 50

[agent]
max_rounds = 3

[sandbox]
process_timeout_ms = 10000
```

See [`apex.example.toml`](apex.example.toml) for all options.

## Optional Feature Flags

Heavy dependencies are behind feature flags and not compiled by default:

| Feature | Crate | Enables |
|---------|-------|---------|
| `llvm-instrument` | apex-instrument | LLVM-based instrumentation via inkwell |
| `wasm-instrument` | apex-instrument, apex-lang | WebAssembly instrumentation |
| `z3-solver` | apex-symbolic | Z3 SMT solver integration |
| `kani-prover` | apex-symbolic | Kani bounded model checking |
| `pyo3-tracer` | apex-concolic | Python concolic tracer extension |
| `libafl-backend` | apex-fuzz | LibAFL fuzzer backend |
| `firecracker` | apex-sandbox | Firecracker microVM isolation |

Enable with:

```bash
cargo build --release --features "apex-symbolic/z3-solver,apex-fuzz/libafl-backend"
```

## Development

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p apex-core

# Check formatting and lints
cargo fmt --check
cargo clippy --workspace -- -D warnings

# Build docs
cargo doc --workspace --no-deps --open
```

## License

[MIT](LICENSE)
