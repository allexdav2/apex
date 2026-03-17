# Contributing to APEX

Thank you for your interest in contributing to APEX!

## Development Setup

```bash
# Clone and build
git clone https://github.com/sahajamoth/apex.git && cd apex
cargo build --release

# Run tests
cargo nextest run --workspace    # preferred (parallel)
cargo test --workspace           # fallback

# Lint and format
cargo clippy --workspace -- -D warnings
cargo fmt --check
```

## Building with Optional Features

Heavy dependencies are feature-gated. To build with all features:

```bash
cargo build --release --features "apex-symbolic/z3-solver,apex-fuzz/libafl-backend"
```

This requires Z3 and LibAFL to be installed on your system.

## Testing

```bash
# All tests (parallel)
cargo nextest run --workspace

# Single crate
cargo nextest run -p apex-detect

# Fallback
cargo test --workspace -- --nocapture
```

## Code Style

- Follow existing patterns in the codebase
- Run `cargo fmt` before committing
- Run `cargo clippy --workspace -- -D warnings` and fix all warnings
- Tests go in `#[cfg(test)] mod tests` inside each file
- Use `#[tokio::test]` for async tests
- Security detectors implement `async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>>`

## Architecture

See [README.md](README.md) for the crate dependency diagram. Key principles:

- **apex-core** is the foundation — all other crates depend on it
- Heavy external dependencies (Z3, LibAFL, pyo3, LLVM) are always feature-gated
- Each language has a corresponding instrumentor + runner pair
- The CLI orchestrates everything through trait objects

## Pull Requests

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/your-feature`
3. Make your changes with tests
4. Ensure all tests pass and lints are clean
5. Update `CHANGELOG.md` under `[Unreleased]`
6. Submit a pull request

## Reporting Issues

- Bug reports and feature requests: [GitHub Issues](https://github.com/sahajamoth/apex/issues)
- Security vulnerabilities: please report privately via [GitHub Security Advisories](https://github.com/sahajamoth/apex/security/advisories)

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
