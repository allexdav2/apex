<!-- status: DONE -->

# Unified Detector Implementation Plan

**Date:** 2026-03-20
**Sources:**
- `.fleet/plans/2026-03-19-failsafe-lockfree-detectors.md` -- 10 concurrency detectors + 12 APEX bugs
- `docs/superpowers/specs/2026-03-19-concurrency-detection-research.md` -- 21 bug classes
- `docs/superpowers/specs/2026-03-19-new-detector-research.md` -- 20 engineering detectors
- `docs/superpowers/specs/2026-03-19-dig2-detector-algorithms-design.md` -- 6 detector algorithms

**Total new detectors:** 19
**Total new tests:** ~150-190
**Existing detectors:** 36 (in `crates/apex-detect/src/detectors/`)
**Architecture:** Each detector is `pub struct XxxDetector;` implementing `Detector` trait with `async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>>`. Tests inline in `#[cfg(test)] mod tests`.

---

## File Map

| Crew | Files |
|------|-------|
| foundation | `crates/apex-detect/src/detectors/util.rs` (shared scope helpers) |
| security-detect | `crates/apex-detect/src/detectors/*.rs` (all 19 new detector files) |
| security-detect | `crates/apex-detect/src/detectors/mod.rs` (registration) |
| security-detect | `crates/apex-detect/src/pipeline.rs` (pipeline wiring) |

---

## Wave 1 -- Foundation: Shared Scope Helpers (foundation crew)

Detectors 1, 5, 6, 7, 8, 9, 10 all need scope-tracking utilities that do not exist yet.
These must land first so detector implementations can import them.

### Task 1.1 -- Add `find_scopes()` to util.rs

**Crew:** foundation
**Files:** `crates/apex-detect/src/detectors/util.rs`
**Dependencies:** none
**Tests:** 8-10

Add a generic scope-finding function:

```rust
pub struct Scope {
    pub start_line: usize,  // 0-based
    pub end_line: usize,    // 0-based, inclusive
}

/// Find scopes opened by `scope_opener` regex.
/// Brace-tracked for Rust/JS/Java/Go; indent-tracked for Python.
pub fn find_scopes(source: &str, lang: Language, scope_opener: &Regex) -> Vec<Scope>

/// Returns true if `line_idx` falls inside any of the given scopes.
pub fn in_any_scope(scopes: &[Scope], line_idx: usize) -> bool
```

This replaces duplicated scope logic across async-scope, loop-scope, and except-scope tracking.

Steps:
- [ ] Write tests for brace-tracked scopes (Rust for/while/loop, async fn)
- [ ] Write tests for indent-tracked scopes (Python for/while, async def, except)
- [ ] Write tests for nested scopes
- [ ] Implement `find_scopes()` with Language dispatch
- [ ] Implement `in_any_scope()` helper
- [ ] Run `cargo nextest run -p apex-detect` -- confirm pass
- [ ] Commit

### Task 1.2 -- Add `in_except_body()` scope helper

**Crew:** foundation
**Files:** `crates/apex-detect/src/detectors/util.rs`
**Dependencies:** Task 1.1
**Tests:** 5-6

Specialized helper for error-handling scope detection:

```rust
/// Returns true if line_idx is inside an except/catch/if-let-Err body.
pub fn in_except_body(source: &str, lang: Language, line_idx: usize) -> bool
```

Used by: swallowed-errors (Detector 2), broad-exception-catching (3), error-context-loss (4).

Steps:
- [ ] Write tests for Python except body detection
- [ ] Write tests for JS/Java catch body detection
- [ ] Write tests for Rust if-let-Err body detection
- [ ] Implement using `find_scopes()` with except-clause regex openers
- [ ] Run tests, confirm pass
- [ ] Commit

---

## Wave 2 -- High-Confidence Detectors from Dig 2 (security-detect crew)

These 6 detectors have full algorithms designed in the Dig 2 spec with validated precision.
All are independent of each other -- run in parallel within the wave.

### Task 2.1 -- `blocking-io-in-async` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/blocking_io_in_async.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs` (add module + pub use)
- `crates/apex-detect/src/pipeline.rs` (register)
**Dependencies:** Task 1.1
**Tests:** 10-12
**CWE:** 400

Algorithm: Two-phase. Phase 1 locates async function boundaries using `find_scopes()`. Phase 2 scans each async scope for blocking calls (language-specific pattern lists). Suppression: Rust `spawn_blocking` within 3 lines above.

Languages: Rust, Python, JavaScript/TypeScript

Steps:
- [ ] Write failing tests: Rust async fn with std::fs, std::thread::sleep
- [ ] Write failing tests: Python async def with requests.get, time.sleep, open()
- [ ] Write failing tests: JS async function with fs.readFileSync, execSync
- [ ] Write negative tests: sync fn, tokio::fs, aiohttp, spawn_blocking
- [ ] Run tests, confirm failure
- [ ] Implement `BlockingIoInAsyncDetector` per Dig 2 spec
- [ ] Register in mod.rs and pipeline.rs as `"blocking-io-in-async"`
- [ ] Run tests, confirm pass
- [ ] Run `cargo clippy -p apex-detect -- -D warnings`
- [ ] Commit

### Task 2.2 -- `swallowed-errors` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/swallowed_errors.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** Task 1.2
**Tests:** 10-12
**CWE:** 390, 391

Algorithm: State machine per file (Idle -> InExceptClause -> InExceptBody). Detects empty catch/except/rescue blocks. Rust-specific: `let _ = result;` direct discard. Python: body is only `pass` or `...`. JS/Java: `catch (e) {}` with empty block.

Languages: Python, JavaScript/TypeScript, Java, Go, Rust

Steps:
- [ ] Write failing tests: Python empty except:pass, bare except
- [ ] Write failing tests: JS empty catch{}, Java catch(Exception){}
- [ ] Write failing tests: Rust `let _ = fallible();`, empty `if let Err(_) = ...{}`
- [ ] Write failing tests: Go empty `if err != nil {}`
- [ ] Write negative tests: catch with logging, catch with return, let _ = .await
- [ ] Run tests, confirm failure
- [ ] Implement `SwallowedErrorsDetector` per Dig 2 spec
- [ ] Register in mod.rs and pipeline.rs as `"swallowed-errors"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 2.3 -- `broad-exception-catching` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/broad_exception.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** Task 1.2
**Tests:** 8-10
**CWE:** 396

Algorithm: Single-pass line scan. Match broad catch patterns (`except Exception:`, `except:`, `catch(Throwable)`, `catch(Exception)`). Suppression: Python re-raise check (body contains bare `raise`).

Languages: Python, Java (JS/Go/Rust are N/A for this detector)

Steps:
- [ ] Write failing tests: Python bare except, except Exception, except BaseException
- [ ] Write failing tests: Java catch(Throwable), catch(Exception)
- [ ] Write negative tests: except ValueError, except with re-raise, Java catch(IOException)
- [ ] Run tests, confirm failure
- [ ] Implement `BroadExceptionCatchingDetector` per Dig 2 spec
- [ ] Register as `"broad-exception-catching"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 2.4 -- `error-context-loss` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/error_context_loss.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** Task 1.2
**Tests:** 8-10
**CWE:** 755

Algorithm: Multi-phase scope-aware scan. Detects raise/throw inside error-handling scope that doesn't chain the original cause. Python: `raise X(msg)` without `from e`. Rust: `.map_err(|_| ...)` discards original. JS: `throw new Error()` in catch without wrapping `e`. Go: `errors.New()` in `if err != nil` without `%w`.

Languages: Python, Rust, JavaScript/TypeScript, Go

Steps:
- [ ] Write failing tests: Python raise without from, Rust .map_err(|_|)
- [ ] Write failing tests: JS throw new Error in catch, Go errors.New in err block
- [ ] Write negative tests: raise from e, .map_err(|e| ...), throw with cause, fmt.Errorf %w
- [ ] Run tests, confirm failure
- [ ] Implement `ErrorContextLossDetector` per Dig 2 spec
- [ ] Register as `"error-context-loss"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 2.5 -- `string-concat-in-loop` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/string_concat_in_loop.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** Task 1.1
**Tests:** 10-12
**CWE:** 400

Algorithm: Two-phase. Phase 1 locates loop boundaries using `find_scopes()`. Phase 2 scans each loop body for string concatenation patterns (`+=` on non-numeric, `x = x + ...`). Disambiguation heuristic: skip if RHS looks numeric.

Languages: Python, JavaScript, Java, Go, Rust

Steps:
- [ ] Write failing tests: Python `result += item` in for loop
- [ ] Write failing tests: Java `str += item.toString()`, JS `s += chunk`, Rust `s += &part`
- [ ] Write negative tests: numeric `count += 1`, join pattern, collect pattern
- [ ] Run tests, confirm failure
- [ ] Implement `StringConcatInLoopDetector` per Dig 2 spec
- [ ] Register as `"string-concat-in-loop"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 2.6 -- `regex-in-loop` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/regex_in_loop.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** Task 1.1
**Tests:** 8-10
**CWE:** 400

Algorithm: Same loop-scope tracking as Task 2.5. Scan each loop body for regex compilation calls. Language-specific compile patterns.

Languages: Python, JavaScript, Go, Java, Rust

Steps:
- [ ] Write failing tests: Python re.search/re.compile in for loop
- [ ] Write failing tests: Rust Regex::new in loop, JS new RegExp in loop, Go regexp.Compile
- [ ] Write negative tests: pre-compiled regex outside loop, LazyLock, JS regex literal
- [ ] Run tests, confirm failure
- [ ] Implement `RegexInLoopDetector` per Dig 2 spec
- [ ] Register as `"regex-in-loop"`
- [ ] Run tests, confirm pass
- [ ] Commit

---

## Wave 3 -- Concurrency P1 Detectors (security-detect crew)

These are the four highest-priority concurrency detectors from the failsafe/lock-free research.
No dependencies on Wave 2 -- only depend on Wave 1 scope helpers.

### Task 3.1 -- `mutex-across-await` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/mutex_across_await.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** Task 1.1 (async scope tracking)
**Tests:** 8-10
**CWE:** 833

Algorithm: Within async function scopes, track `MutexGuard` acquisitions (`.lock()`) and detect `.await` points while guard is still alive. Track guard variable name, check if it's dropped before the await.

Languages: Rust only

Steps:
- [ ] Write failing tests: MutexGuard alive across .await
- [ ] Write failing tests: RwLock guard across .await
- [ ] Write negative tests: guard dropped before await, tokio::sync::Mutex (async-aware)
- [ ] Run tests, confirm failure
- [ ] Implement `MutexAcrossAwaitDetector`
- [ ] Register as `"mutex-across-await"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 3.2 -- `open-without-with` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/open_without_with.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** none (pure pattern matching)
**Tests:** 6-8
**CWE:** 775

Algorithm: Detect `f = open(...)` not inside a `with` statement. Pattern: line has `= open(` but no `with` keyword on same or preceding line within 2 lines.

Languages: Python only

Steps:
- [ ] Write failing tests: `f = open("file")` standalone, open() in assignment
- [ ] Write negative tests: `with open("file") as f`, `open(` inside string literal
- [ ] Run tests, confirm failure
- [ ] Implement `OpenWithoutWithDetector`
- [ ] Register as `"open-without-with"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 3.3 -- `unbounded-queue` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/unbounded_queue.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** none (pure pattern matching)
**Tests:** 8-10
**CWE:** 400, 770

Algorithm: Pattern match for unbounded channel/queue creation. Rust: `mpsc::channel()`, `unbounded_channel()`. Python: `queue.Queue()` without maxsize. Go: `make(chan ...)` with very large buffers. Java: `LinkedBlockingQueue<>()` without capacity.

Languages: Rust, Python, Go, Java

Steps:
- [ ] Write failing tests: Rust std mpsc::channel(), tokio unbounded_channel
- [ ] Write failing tests: Python Queue() no maxsize, Go large chan
- [ ] Write negative tests: bounded channels, Queue(maxsize=100)
- [ ] Run tests, confirm failure
- [ ] Implement `UnboundedQueueDetector`
- [ ] Register as `"unbounded-queue"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 3.4 -- `ffi-panic` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/ffi_panic.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** Task 1.1 (scope tracking for extern fn bodies)
**Tests:** 6-8
**CWE:** 248

Algorithm: Find `extern "C" fn` function bodies. Scan for panic-inducing patterns: `panic!`, `.unwrap()`, `.expect(`, `todo!()`, `unimplemented!()`, `assert!`. These cause UB when unwinding across FFI boundary.

Languages: Rust only

Steps:
- [ ] Write failing tests: unwrap() inside extern "C" fn, panic! in extern fn
- [ ] Write failing tests: .expect() in #[no_mangle] extern "C" fn
- [ ] Write negative tests: catch_unwind wrapping, safe Rust fn with unwrap
- [ ] Run tests, confirm failure
- [ ] Implement `FfiPanicDetector`
- [ ] Register as `"ffi-panic"`
- [ ] Run tests, confirm pass
- [ ] Commit

---

## Wave 4 -- Concurrency P2 + Engineering Detectors (security-detect crew)

Medium-confidence detectors that need tuning or filtering.
Depends on Waves 1-3 being complete and stable.

### Task 4.1 -- `missing-async-timeout` detector (extend existing)

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/timeout.rs` (modify existing)
- `crates/apex-detect/src/detectors/mod.rs` (no change needed)
**Dependencies:** Task 1.1
**Tests:** 6-8 (added to existing test module)
**CWE:** 400

Algorithm: Extend existing `MissingTimeoutDetector` to cover Rust async I/O without `tokio::time::timeout` wrapper, and Go HTTP calls without context deadline.

Languages: Rust, Go (extending existing Python/JS coverage)

Steps:
- [ ] Write failing tests: Rust `reqwest::get()` without timeout, `TcpStream::connect` without timeout
- [ ] Write failing tests: Go `http.Get()` without context deadline
- [ ] Write negative tests: reqwest with .timeout(), Go with context.WithTimeout
- [ ] Run tests, confirm failure
- [ ] Extend existing detector with Rust/Go patterns
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 4.2 -- `zombie-subprocess` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/zombie_subprocess.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** none
**Tests:** 6-8
**CWE:** 772

Algorithm: Detect `Command::output()` or `subprocess.run()` in a timeout context without a corresponding `kill()` call. Pattern: look for subprocess creation followed by timeout, check if kill/terminate is called on failure path.

Languages: Rust, Python

Steps:
- [ ] Write failing tests: Rust Command::output() in timeout without kill
- [ ] Write failing tests: Python subprocess.run(timeout=...) without except handling kill
- [ ] Write negative tests: properly killed subprocesses
- [ ] Run tests, confirm failure
- [ ] Implement `ZombieSubprocessDetector`
- [ ] Register as `"zombie-subprocess"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 4.3 -- `relaxed-atomics` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/relaxed_atomics.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** none
**Tests:** 6-8
**CWE:** 362

Algorithm: Flag `Ordering::Relaxed` on stores to variables with semantic publish names (flag, ready, init, done, published). Flag `Relaxed` loads immediately followed by array/pointer dereference.

Languages: Rust only

Steps:
- [ ] Write failing tests: Relaxed store on flag/ready/done variable
- [ ] Write failing tests: Relaxed load followed by data[idx] access
- [ ] Write negative tests: Relaxed on counter, Acquire/Release pairs
- [ ] Run tests, confirm failure
- [ ] Implement `RelaxedAtomicsDetector`
- [ ] Register as `"relaxed-atomics"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 4.4 -- `hardcoded-env-values` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/hardcoded_env.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** none
**Tests:** 6-8
**CWE:** 547

Algorithm: Pattern match for hardcoded localhost, 127.0.0.1, 0.0.0.0, staging URLs, specific port numbers in non-test production code. Use existing `is_test_file()` to exclude tests. Exclude config/defaults files.

Languages: all

Steps:
- [ ] Write failing tests: hardcoded localhost in handler code, dev URLs
- [ ] Write negative tests: test files, config defaults, env var references
- [ ] Run tests, confirm failure
- [ ] Implement `HardcodedEnvDetector`
- [ ] Register as `"hardcoded-env"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 4.5 -- `wall-clock-misuse` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/wall_clock_misuse.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** none
**Tests:** 6-8
**CWE:** 682

Algorithm: Detect wall-clock calls (`time.time()`, `Date.now()`, `SystemTime::now()`, `System.currentTimeMillis()`) used for elapsed duration measurement (subtraction pattern). Flag when two wall-clock calls are subtracted.

Languages: Python, JavaScript, Rust, Java, Go

Steps:
- [ ] Write failing tests: Python `time.time() - start`, Rust `SystemTime::now() - start`
- [ ] Write failing tests: JS `Date.now() - start`, Java `currentTimeMillis` diff
- [ ] Write negative tests: monotonic clocks, Instant::now, time.monotonic, performance.now
- [ ] Run tests, confirm failure
- [ ] Implement `WallClockMisuseDetector`
- [ ] Register as `"wall-clock-misuse"`
- [ ] Run tests, confirm pass
- [ ] Commit

---

## Wave 5 -- Concurrency P3 Detectors (security-detect crew)

Lower-impact concurrency detectors.

### Task 5.1 -- `missing-shutdown-handler` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/missing_shutdown.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** none
**Tests:** 6-8
**CWE:** 772

Algorithm: Identify server/daemon binaries (presence of `listen()`, `bind()`, `serve()`, `#[tokio::main]`). Search for signal handler registration. If server code has no signal handler, flag.

Languages: Rust, Go, Python, JS

Steps:
- [ ] Write failing tests: Rust tokio::main with no signal handler, Go http.ListenAndServe without signal.Notify
- [ ] Write negative tests: ctrlc::set_handler present, tokio::signal present
- [ ] Run tests, confirm failure
- [ ] Implement `MissingShutdownDetector`
- [ ] Register as `"missing-shutdown"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 5.2 -- `connection-in-loop` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/connection_in_loop.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** Task 1.1
**Tests:** 6-8
**CWE:** 400

Algorithm: Detect database connection creation (`connect()`, `create_engine()`, `getConnection()`) inside loop bodies using loop scope tracking.

Languages: Python, JavaScript, Java

Steps:
- [ ] Write failing tests: Python sqlite3.connect() in loop, JS pg.connect in loop
- [ ] Write negative tests: connection pooling outside loop
- [ ] Implement `ConnectionInLoopDetector`
- [ ] Register as `"connection-in-loop"`
- [ ] Run tests, confirm pass
- [ ] Commit

### Task 5.3 -- `poisoned-mutex-recovery` detector

**Crew:** security-detect
**Files:**
- `crates/apex-detect/src/detectors/poisoned_mutex.rs` (create)
- `crates/apex-detect/src/detectors/mod.rs`
- `crates/apex-detect/src/pipeline.rs`
**Dependencies:** none
**Tests:** 5-6
**CWE:** 362

Algorithm: Pattern match for `unwrap_or_else(|e| e.into_inner())` or `unwrap_or_else(PoisonError::into_inner)` on mutex lock results -- silently recovering from poisoned mutex.

Languages: Rust only

Steps:
- [ ] Write failing tests: `.lock().unwrap_or_else(|e| e.into_inner())`
- [ ] Write negative tests: `.lock().unwrap()`, proper poison handling with logging
- [ ] Implement `PoisonedMutexDetector`
- [ ] Register as `"poisoned-mutex"`
- [ ] Run tests, confirm pass
- [ ] Commit

---

## Wave 6 -- Dogfood: Fix APEX's Own Bugs (multiple crews)

Apply the new detectors to APEX itself. Fix the 12 bugs from the failsafe research
plus the blocking-io and string-concat instances.

### Task 6.1 -- Fix APEX concurrency/resource bugs (12 bugs)

**Crew:** security-detect (detect), then foundation/exploration/runtime (fix, per crate ownership)
**Files:** Various -- see bug list below
**Dependencies:** Waves 2-5 complete
**Tests:** Run new detectors on APEX source as validation

Bug list from failsafe research:
1. `oracle.rs:106` -- Auto-covered branches invisible to merge_bitmap (WRONG 92%)
2. `main.rs` -- No SIGTERM/SIGINT handler -> SHM + zombie leaks (LEAK 97%)
3. `python.py:217` -- Subprocess not killed on timeout -> zombies (LEAK 95%)
4. `coordinator.rs:33` -- Unbounded seed queue -> OOM (LEAK 95%)
5. `sancov_rt.rs:54` -- Null deref in unsafe extern C (CRASH 88%)
6. `orchestrator.rs:210` -- Poisoned mutex silently recovered (WRONG 85%)
7. `driller.rs:75` -- std::sync::Mutex held across blocking solver (WRONG 85%)
8. `python.py:244` -- Coverage json step has no timeout (WRONG 85%)
9. `sancov_rt.rs:77` -- Relaxed atomics on ARM data race (WRONG 85%)
10. `oracle.rs:116` -- Relaxed ordering -> stale coverage on ARM (WRONG 82%)
11. `coordinator.rs:108` -- SeedId per batch not per seed (WRONG 80%)
12. `coordinator.rs:184` -- gRPC handle no cancellation (LEAK 88%)

Steps:
- [ ] Run all new detectors on APEX workspace to confirm they find these bugs
- [ ] Fix bugs 2, 4, 5, 6, 9, 10 (highest confidence, most severe)
- [ ] Fix bugs 1, 3, 7, 8, 11, 12
- [ ] Run full test suite: `cargo nextest run --workspace`
- [ ] Commit fixes

### Task 6.2 -- Fix blocking-io-in-async instances in apex-cli

**Crew:** platform (owns apex-cli)
**Files:** `crates/apex-cli/src/**` (13 instances per Dig 3)
**Dependencies:** Task 2.1 (detector implemented and validated)
**Tests:** Existing tests should still pass

Steps:
- [ ] Run blocking-io-in-async detector on apex-cli
- [ ] Replace std::fs calls in async contexts with tokio::fs
- [ ] Replace std::thread::sleep with tokio::time::sleep
- [ ] Run `cargo nextest run -p apex-cli`
- [ ] Commit

### Task 6.3 -- Fix string-concat-in-loop instances in apex-synth

**Crew:** intelligence (owns apex-synth)
**Files:** `crates/apex-synth/src/**` (9 instances per Dig 3)
**Dependencies:** Task 2.5 (detector implemented and validated)
**Tests:** Existing tests should still pass

Steps:
- [ ] Run string-concat-in-loop detector on apex-synth
- [ ] Replace `s += &chunk` with collect/join patterns
- [ ] Run `cargo nextest run -p apex-synth`
- [ ] Commit

---

## Dependency Graph

```
Wave 1: [foundation]
  Task 1.1  find_scopes() ----+
  Task 1.2  in_except_body()  |  (depends on 1.1)
                               |
Wave 2: [security-detect]     |  (depends on Wave 1)
  Task 2.1  blocking-io ------+-- needs 1.1
  Task 2.2  swallowed-errors -+-- needs 1.2
  Task 2.3  broad-exception --+-- needs 1.2
  Task 2.4  error-context ----+-- needs 1.2
  Task 2.5  string-concat ----+-- needs 1.1
  Task 2.6  regex-in-loop ----+-- needs 1.1
                               |
Wave 3: [security-detect]     |  (depends on Wave 1, parallel with Wave 2)
  Task 3.1  mutex-across-await +-- needs 1.1
  Task 3.2  open-without-with  |  (no deps beyond Wave 1)
  Task 3.3  unbounded-queue    |  (no deps)
  Task 3.4  ffi-panic ---------+-- needs 1.1
                               |
Wave 4: [security-detect]     |  (depends on Waves 1-3 stable)
  Task 4.1  missing-async-timeout
  Task 4.2  zombie-subprocess
  Task 4.3  relaxed-atomics
  Task 4.4  hardcoded-env
  Task 4.5  wall-clock-misuse
                               |
Wave 5: [security-detect]     |  (depends on Wave 4)
  Task 5.1  missing-shutdown
  Task 5.2  connection-in-loop
  Task 5.3  poisoned-mutex
                               |
Wave 6: [foundation, platform, intelligence]  (depends on all above)
  Task 6.1  Fix APEX concurrency bugs
  Task 6.2  Fix apex-cli blocking-io
  Task 6.3  Fix apex-synth string-concat
```

## Execution Note: Waves 2 and 3 Can Run in Parallel

Waves 2 and 3 both depend only on Wave 1 completion. They are independent of each other
and can be dispatched simultaneously to different security-detect crew agents.

- Wave 2 tasks 2.1-2.6 all touch different files (separate detector .rs files)
- Wave 3 tasks 3.1-3.4 all touch different files
- Both waves modify mod.rs and pipeline.rs, so merge conflicts are expected
  at those two files -- captain merges them manually after both waves complete

## Summary Statistics

| Wave | Tasks | New Files | New Tests | Crew |
|------|-------|-----------|-----------|------|
| 1 | 2 | 0 (modify util.rs) | ~16 | foundation |
| 2 | 6 | 6 detector files | ~60 | security-detect |
| 3 | 4 | 4 detector files | ~32 | security-detect |
| 4 | 5 | 4 new + 1 modify | ~32 | security-detect |
| 5 | 3 | 3 detector files | ~18 | security-detect |
| 6 | 3 | 0 (fix existing) | 0 (existing pass) | foundation, platform, intelligence |
| **Total** | **23** | **17 new files** | **~158** | **4 crews** |

## CWE Coverage Added

| CWE | Name | Detectors |
|-----|------|-----------|
| 248 | Execution with Unnecessary Privileges | ffi-panic |
| 362 | Race Condition | relaxed-atomics, poisoned-mutex |
| 390 | Detection of Error Condition Without Action | swallowed-errors |
| 391 | Unchecked Error Condition | swallowed-errors |
| 396 | Declaration of Catch for Generic Exception | broad-exception-catching |
| 400 | Uncontrolled Resource Consumption | blocking-io-in-async, string-concat-in-loop, regex-in-loop, unbounded-queue, missing-async-timeout, connection-in-loop |
| 547 | Use of Hard-coded Security-relevant Constants | hardcoded-env |
| 682 | Incorrect Calculation | wall-clock-misuse |
| 755 | Improper Handling of Exceptional Conditions | error-context-loss |
| 770 | Allocation of Resources Without Limits | unbounded-queue |
| 772 | Missing Release of Resource | zombie-subprocess, missing-shutdown |
| 775 | Missing Release of File Descriptor | open-without-with |
| 833 | Deadlock | mutex-across-await |

**Before:** 19 CWEs covered
**After:** 32 CWEs covered (+13 new)
