<!-- status: DONE -->

# Modernize Python Toolchain: uv Integration

**Goal:** Replace brittle `pip3`/system-python dependency chain with `uv`-first approach.
Graceful fallback: `uv` -> existing behavior (pip/venv/system).

## Analysis Summary

### Current Python Touchpoints

| File | What it does | Python invocation |
|------|-------------|-------------------|
| `crates/apex-lang/src/python.rs` | `PythonRunner` — installs deps, runs tests | `pip3 install`, `python3 -m pytest` |
| `crates/apex-instrument/src/python.rs` | `PythonInstrumentor` — runs coverage.py | Hard-coded `python3 apex_instrument.py` |
| `crates/apex-instrument/src/scripts/apex_instrument.py` | Embedded script | `sys.executable -m coverage run` |
| `crates/apex-sandbox/src/python.rs` | `PythonTestSandbox` — runs candidate tests | Hard-coded `python3 -m coverage run -m pytest` |
| `crates/apex-cli/src/doctor.rs` | Prerequisite checks | Checks `python3`, `pip3`, `pytest`, `coverage.py` |

### Problems Found

1. **`PythonRunner::install_deps`** (apex-lang) — already detects `uv.lock` and calls `uv sync`, but the fallback path uses bare `pip3 install` which fails on PEP 668 systems.
2. **`PythonRunner::install_deps`** — coverage.py install fallback uses `pip3 install coverage pytest` directly, no `uv` path.
3. **`PythonInstrumentor::instrument_impl`** (apex-instrument) — hard-codes `python3` as the command, never uses `uv run`.
4. **`PythonTestSandbox::run`** (apex-sandbox) — hard-codes `python3 -m coverage run -m pytest` via `tokio::process::Command` directly (not even using `CommandRunner`). Has a TODO comment acknowledging this.
5. **`doctor.rs`** — checks `pip3` as required, does not check `uv`. No fallback chain.
6. **`apex_instrument.py`** — uses `sys.executable -m coverage` which is correct within its own process, but the parent (Rust) must ensure `coverage` is importable by that Python.

### What Already Works

- `PythonRunner::detect_package_manager` already detects `uv.lock` and returns `PackageManager::Uv`.
- `PythonRunner::install_deps` already handles `PackageManager::Uv` with `uv sync`.
- The `PackageManager` enum already has `Uv` variant.

### What Needs to Change

The key insight: when `uv` is available, we can use `uv run` as a prefix to ANY Python command. It auto-creates an ephemeral venv, installs dependencies, and runs the command. This means:

- `uv run python3 -m pytest` replaces `python3 -m pytest`
- `uv run coverage run --branch ...` replaces `python3 -m coverage run --branch ...`
- `uv pip install coverage pytest` replaces `pip3 install coverage pytest` (no PEP 668 issues)

## File Map

| Crew | Files |
|------|-------|
| foundation | `crates/apex-core/src/command.rs` (if UvContext helper needed) |
| runtime | `crates/apex-lang/src/python.rs`, `crates/apex-sandbox/src/python.rs` |
| platform | `crates/apex-cli/src/doctor.rs` |
| exploration | `crates/apex-instrument/src/python.rs`, `crates/apex-instrument/src/scripts/apex_instrument.py` |

## Wave 1 — Foundation: uv detection utility

### Task 1.1 — foundation crew
**Files:** `crates/apex-lang/src/python.rs`

Add a `resolve_uv()` static method to `PythonRunner` that checks if `uv` is on PATH (similar to `resolve_python()`). Returns `Option<&'static str>` — `Some("uv")` if found, `None` otherwise. This is the single source of truth for "is uv available?"

- [ ] Add `resolve_uv() -> Option<&'static str>` using `OnceLock`
- [ ] Add `uv_run_prefix(target: &Path) -> Option<Vec<String>>` — returns `["uv", "run", "--project", target]` when uv is available, `None` otherwise
- [ ] Write unit tests for both methods
- [ ] Run `cargo nextest run -p apex-lang`
- [ ] Commit

## Wave 2 — Runtime: uv-aware test running and dep installation

### Task 2.1 — runtime crew
**Files:** `crates/apex-lang/src/python.rs`

Upgrade `PythonRunner` methods to prefer `uv` when available:

**`install_deps`:**
- [ ] When `PackageManager::Pip` and `uv` is available: use `uv pip install` instead of bare `pip3 install` for the coverage/pytest fallback install
- [ ] When no package manager detected but `uv` is available: use `uv pip install -r requirements.txt` or `uv pip install -e .`
- [ ] Test: mock uv path, verify `uv pip install` is called
- [ ] Test: mock no uv, verify pip3 fallback still works

**`detect_test_runner` / `run_tests`:**
- [ ] When `uv` is available and no venv exists: prefix test command with `uv run --` (e.g., `uv run -- python3 -m pytest -q`)
- [ ] When venv exists: continue using venv python (venv takes priority over uv)
- [ ] Test: mock uv available, no venv -> verify uv run prefix
- [ ] Test: venv exists + uv available -> verify venv python used (not uv)
- [ ] Run `cargo nextest run -p apex-lang`
- [ ] Commit

### Task 2.2 — runtime crew
**Files:** `crates/apex-sandbox/src/python.rs`

Refactor `PythonTestSandbox` to use uv when available:

- [ ] Add `runner: Arc<dyn CommandRunner>` field (addresses existing TODO)
- [ ] Add `has_uv: bool` field, detected at construction time
- [ ] When `has_uv`: use `uv run coverage run --branch ...` instead of `python3 -m coverage run`
- [ ] When `has_uv`: use `uv run coverage json ...` instead of `python3 -m coverage json`
- [ ] Fallback: keep existing `python3 -m coverage` path
- [ ] Write tests with mock CommandRunner for both paths
- [ ] Run `cargo nextest run -p apex-sandbox`
- [ ] Commit

### Task 2.3 — exploration crew
**Files:** `crates/apex-instrument/src/python.rs`

Upgrade `PythonInstrumentor` to use uv when available:

- [ ] In `instrument_impl`: when uv is detected, use `uv run python3 apex_instrument.py <cmd>` instead of `python3 apex_instrument.py <cmd>`
- [ ] Alternative (better): use `uv run --with coverage --with pytest -- python3 apex_instrument.py <cmd>` to ensure deps are available without pre-install
- [ ] Test: mock uv available, verify command includes `uv run`
- [ ] Test: mock no uv, verify `python3` used directly
- [ ] Run `cargo nextest run -p apex-instrument`
- [ ] Commit

## Wave 3 — Platform: doctor checks and user-facing messaging

### Task 3.1 — platform crew
**Files:** `crates/apex-cli/src/doctor.rs`

Upgrade `checks_python` to reflect the new fallback chain:

- [ ] Add `uv` as an optional check (with a note: "recommended — fixes PEP 668 issues")
- [ ] Downgrade `pip3` from required to optional when `uv` is found
- [ ] Keep `python3` as required (uv still needs a Python interpreter to exist somewhere, though it can manage versions)
- [ ] Add a diagnostic note when neither uv nor pip3 is found: "Install uv (curl -LsSf https://astral.sh/uv/install.sh | sh) or pip3"
- [ ] Test: mock uv found + pip3 missing -> no failures
- [ ] Test: mock uv missing + pip3 found -> no failures (pip3 still works)
- [ ] Test: mock both missing -> failure
- [ ] Run `cargo nextest run -p apex-cli`
- [ ] Commit

## Wave 4 — Integration: end-to-end validation

### Task 4.1 — platform crew
**Files:** (no new files, test-only)

Integration test that validates the full uv path works end-to-end:

- [ ] Add an integration test (gated behind `#[cfg(feature = "integration")]` or `#[ignore]`) that:
  1. Creates a temp Python project with a `pyproject.toml`
  2. Runs `PythonRunner::install_deps` with real `uv` (if available on CI)
  3. Runs `PythonRunner::run_tests`
  4. Verifies tests executed successfully
- [ ] Skip test gracefully if `uv` not in PATH
- [ ] Run test locally to verify
- [ ] Commit

---

## Other Language Modernization Opportunities

| Language | Current | Modern Alternative | Priority | Notes |
|----------|---------|-------------------|----------|-------|
| Python | pip3/system | **uv** | **HIGH** | PEP 668 breaks pip on macOS/Linux. uv is a drop-in fix. |
| JavaScript | npm/yarn/pnpm/bun | Already handled | LOW | `js_env.rs` already detects bun, deno, pnpm, yarn. Well-covered. |
| Go | go test | Already modern | NONE | Go toolchain is self-contained. |
| Rust | cargo/rustc | Already modern | NONE | Native toolchain. |
| Java | javac/gradle/maven | Already handled | LOW | `java.rs` already detects gradle vs maven. |
| Ruby | bundler/rake | **mise/rbenv** | LOW | Less urgent; Ruby users already manage envs. |
| C/C++ | gcc/clang/gcov | Already handled | NONE | System compilers are stable. |
| Swift | swift test | Already handled | NONE | Xcode toolchain. |
| Kotlin | gradle/kotlinc | Already handled | NONE | Follows Java patterns. |
| C# | dotnet test | Already handled | NONE | .NET SDK is self-managing. |

**Verdict:** Python is the only language where the current approach is actively broken on modern systems. JavaScript is already well-handled. Everything else is fine.

## Risk Assessment

- **Low risk:** All changes are additive. The fallback chain means existing behavior is preserved when uv is not installed.
- **No breaking changes:** `uv` is detected at runtime, not compile time. No new dependencies.
- **Testing:** All changes are mockable via the existing `CommandRunner` trait.
- **CI:** GitHub Actions can install uv with `curl -LsSf https://astral.sh/uv/install.sh | sh` (one line in workflow).
