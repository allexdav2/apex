<!-- status: FUTURE -->

# xmake C/C++ Build System + Ruby mise Integration

**Priority:** P2
**Date:** 2026-03-18

## Summary

Two independent modernizations for APEX language runners:

**A) xmake for C/C++** -- xmake is a Lua-based C/C++ build system with built-in package manager and test runner. When `xmake.lua` is present, APEX should prefer it over cmake/make. xmake natively supports gcov/lcov coverage output, so the existing `CCoverageInstrumentor` gcov parser works unchanged.

**B) Ruby + mise** -- mise (formerly rtx) manages Ruby versions and replaces rbenv/rvm. When mise is available, APEX should prefix Ruby/bundle commands with `mise exec ruby --` for reliable version resolution. Doctor should also report mise-managed Ruby.

## File Map

| Crew       | Files                                      |
|------------|--------------------------------------------|
| foundation | `crates/apex-lang/src/c.rs`                |
| foundation | `crates/apex-lang/src/cpp.rs`              |
| foundation | `crates/apex-lang/src/ruby.rs`             |
| platform   | `crates/apex-cli/src/doctor.rs`            |
| platform   | `tests/fixtures/tiny-c-xmake/` (new)      |
| platform   | `tests/fixtures/tiny-ruby-mise/` (new)     |

## Wave 1 -- foundation crew (no dependencies)

### Task 1.1 -- xmake support in CRunner and CppRunner

**Files:** `crates/apex-lang/src/c.rs`, `crates/apex-lang/src/cpp.rs`

Current state: `CRunner::detect_build_system` checks CMake > Make > Autoconf. `CppRunner` checks CMake > Make > Meson. Neither knows about xmake.

Changes:
- [ ] Add `XMake` variant to `BuildSystem` enum in c.rs and `CppBuildSystem` in cpp.rs
- [ ] In `detect_build_system`, check `target.join("xmake.lua").exists()` BEFORE cmake (prefer xmake when both exist)
- [ ] In `detect` trait method, add `xmake.lua` as a project marker
- [ ] In `install_deps`: run `xmake config -m debug` then `xmake build`
- [ ] In `run_tests`: run `xmake test` for the XMake variant
- [ ] Add unit tests: detect xmake.lua, detect xmake over cmake when both present, build system priority
- [ ] Run `cargo test -p apex-lang` to verify

### Task 1.2 -- mise-aware Ruby runner

**Files:** `crates/apex-lang/src/ruby.rs`

Current state: `RubyRunner` hardcodes `bundle exec rspec` and `ruby` commands. No version manager awareness.

Changes:
- [ ] Add `fn detect_mise(target: &Path) -> bool` -- check for `.mise.toml`, `.tool-versions` with ruby entry, or `mise` on PATH
- [ ] When mise detected, wrap commands: `mise exec ruby -- bundle exec rspec` instead of `bundle exec rspec`
- [ ] In `install_deps`, use `mise exec ruby -- bundle install` and `mise exec ruby -- gem install` when mise is present
- [ ] Add unit tests with mock runner: mise detected wraps commands, no mise uses direct commands
- [ ] Run `cargo test -p apex-lang` to verify

## Wave 2 -- platform crew (depends on Wave 1)

### Task 2.1 -- doctor checks for xmake and mise

**Files:** `crates/apex-cli/src/doctor.rs`

Current state: C checks include gcc, clang, gcov, llvm-cov, llvm-profdata, make, cmake. No Ruby section exists.

Changes:
- [ ] Add optional `xmake` check to `checks_c` alongside cmake: `check_optional(runner, "xmake", "xmake build system", "xmake", &["--version"])`
- [ ] Add `checks_ruby` function: check `ruby --version`, `bundle --version`, `mise --version` (all optional/warn)
- [ ] Wire `checks_ruby` into the main doctor dispatch for Ruby language
- [ ] Add mock tests for xmake found/not-found and mise found/not-found
- [ ] Run `cargo test -p apex-cli` to verify

### Task 2.2 -- fixture projects

**Files:** `tests/fixtures/tiny-c-xmake/`, `tests/fixtures/tiny-ruby-mise/`

- [ ] Create `tests/fixtures/tiny-c-xmake/xmake.lua` with a minimal target and test
- [ ] Create `tests/fixtures/tiny-c-xmake/src/add.c` -- simple add function
- [ ] Create `tests/fixtures/tiny-c-xmake/tests/test_add.c` -- assert-based test
- [ ] Create `tests/fixtures/tiny-ruby-mise/Gemfile` with rspec dependency
- [ ] Create `tests/fixtures/tiny-ruby-mise/.mise.toml` with `[tools]\nruby = "3.3"`
- [ ] Create `tests/fixtures/tiny-ruby-mise/spec/add_spec.rb` -- simple spec
- [ ] Create `tests/fixtures/tiny-ruby-mise/lib/add.rb` -- simple module

## Coverage instrumentation note

No changes needed to `crates/apex-instrument/src/c_coverage.rs`. xmake's `xmake test --coverage` generates standard gcov `.gcov` files, which the existing `scan_gcov_files` / `parse_gcov_output` already handles.

## Verification

```bash
cargo check --workspace
cargo test -p apex-lang
cargo test -p apex-cli
cargo clippy --workspace -- -D warnings
```
