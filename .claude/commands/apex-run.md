# APEX Run — Agent Loop

Multi-round coverage improvement loop. Measures gaps, writes tests, invokes strategies, re-measures.

## Usage
```
/apex-run [target] [lang] [rounds] [coverage-target]
```
Examples:
- `/apex-run` — run APEX agent loop on itself (self-hosted)
- `/apex-run /tmp/my-project python 5 0.95`
- `/apex-run /tmp/my-c-project c 3 1.0`

## Instructions

Parse `$ARGUMENTS`: target path, language, rounds, coverage target.
Defaults: target=`/Users/ad/prj/bcov`, lang=`rust`, rounds=`5`, coverage_target=`1.0`.

### Environment

```bash
export LLVM_COV=/opt/homebrew/opt/llvm/bin/llvm-cov
export LLVM_PROFDATA=/opt/homebrew/opt/llvm/bin/llvm-profdata
```

### Agent Loop

For each round (1 to max_rounds):

**Step 1 — Measure.** Run APEX with `--strategy agent --output-format json`:
```bash
cargo run --bin apex --manifest-path /Users/ad/prj/bcov/Cargo.toml -- \
  run --target <TARGET> --lang <LANG> --strategy agent \
  --output-format json 2>/dev/null
```
Capture JSON output. Parse `summary`, `gaps`, and `blocked` arrays.

**Step 2 — Analyze.** Sort gaps by `bang_for_buck` descending. For each gap, select strategy:
- `difficulty: easy/medium` → write a targeted test file (source-level)
- `difficulty: hard` + binary/fuzz hint → run `--strategy fuzz`
- `difficulty: hard` + constraint/SMT hint → run `--strategy driller`
- Python target + constraint path → run `--strategy concolic`
- `difficulty: blocked` → skip, include in report

**Step 3 — Act.**
- For source-level tests: read the `source_context` and `branch_condition` from JSON, write test files to `tests/` directory. Use `closest_existing_test` as a reference for test structure. Run `cargo test` (or equivalent) to verify tests compile.
- For fuzz/driller/concolic: run the appropriate APEX command:
  ```bash
  cargo run --bin apex --manifest-path /Users/ad/prj/bcov/Cargo.toml -- \
    run --target <TARGET> --lang <LANG> --strategy <fuzz|driller|concolic> \
    --rounds 1 --output /tmp/apex-output 2>&1
  ```

**Step 4 — Re-measure.** Run Step 1 again. Compare `summary.covered_branches` with previous round.

**Step 5 — Report.** Print the round report:

```
## Round N/M — Coverage: X% → Y% (+Z%)

█████████████████████████████████████████░░░░░ Y%
+NNN branches covered  |  NNN remaining  |  N tests written  |  N strategy runs

### This round
+NN file.rs (test)  +NN file.rs (fuzz)  +NN file.py (concolic)

### File coverage
  ██████████ 100%  file1.rs, file2.rs (N files)
  █████████░  95%  file3.rs (N files)
  ████████░░  85%  file4.rs ↑N%
  ...
  █░░░░░░░░░  12%  file5.rs (reason)

### Blocked files (need integration harness)
  file.rs (NNN) — reason
```

**Step 6 — Breakpoints.** Check:
- **Stall** (0% improvement): Pause. Show which gaps were attempted and strategies used. Ask user whether to continue, switch strategies, or stop.
- **Regression** (coverage dropped): Pause. Identify which new tests caused regression. Ask user.
- **Compile failure**: Auto-retry once with the compiler error message. If still failing, pause.
- **Strategy failure** (fuzz/driller/concolic crash/timeout): Log the error, skip that gap, continue.

If no breakpoint → continue to next round.

**Step 7 — Terminate** when:
- Coverage target reached → report success
- Max rounds hit → report final state
- User stops → report final state

### Strategy Selection Guide

| Target type | Primary strategy | Fallback |
|-------------|-----------------|----------|
| Rust workspace | Source-level tests | fuzz (if binary harness exists) |
| Python project | Source-level tests | concolic (for constraint paths) |
| C/Rust binary | fuzz | driller (when fuzz stalls) |
| JavaScript | Source-level tests | — |

If the run fails, diagnose the error and suggest a fix.
