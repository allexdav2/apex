# Contributing to APEX

## Development Setup

```bash
git clone https://github.com/sahajamoth/apex.git
cd apex
cargo build
cargo test --workspace
```

## Building with Optional Features

Heavy dependencies are feature-gated. To build with all features:

```bash
cargo build --features "apex-symbolic/z3-solver,apex-fuzz/libafl-backend,apex-instrument/llvm-instrument"
```

This requires Z3, LibAFL, and LLVM 17 to be installed on your system.

## Testing

```bash
# All tests
cargo test --workspace

# Single crate
cargo test -p apex-core

# With output
cargo test --workspace -- --nocapture
```

## Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy --workspace -- -D warnings` and fix all warnings
- Add tests for new functionality
- Keep crate-level `//!` doc comments up to date

## Architecture

See [README.md](README.md) for the crate dependency diagram. Key principles:

- **apex-core** is the foundation — all other crates depend on it
- Heavy external dependencies (Z3, LibAFL, pyo3, LLVM) are always feature-gated
- Each language has a corresponding instrumentor + runner pair
- The CLI orchestrates everything through trait objects

## Pull Requests

1. Fork the repository
2. Create a feature branch
3. Make your changes with tests
4. Ensure `cargo test --workspace` passes
5. Ensure `cargo clippy --workspace -- -D warnings` is clean
6. Submit a pull request
