<!-- status: ACTIVE -->
# Deep Research: Orchestrator + Sandbox + Reachability (Digs 10-12)

**Date:** 2026-03-23
**Scope:** Alternatives analysis for APEX's agent orchestration, process sandboxing, and reachability analysis

---

## Dig 10: Orchestrator / Agent Loop

### APEX Current Architecture

APEX's `AgentCluster` (in `crates/apex-agent/src/orchestrator.rs`) implements a **synchronous iteration loop**:

1. Check coverage target / deadline / stall threshold
2. Call `suggest_inputs()` on **all strategies in parallel** via `futures::join_all`
3. Run **all suggested seeds through the sandbox** in parallel via `futures::join_all`
4. Merge coverage results into the `CoverageOracle`
5. Record bugs in the `BugLedger`
6. Notify strategies via `observe()` for feedback
7. Increment stall counter if no new coverage; reset on progress

**Key components:**
- **CoverageMonitor** (`monitor.rs`): Sliding window with 4-level escalation (Normal -> SwitchStrategy -> AgentCycle -> Stop)
- **StrategyBandit** (`bandit.rs`): Thompson sampling (Beta distribution) over synthesis strategies
- **S2FRouter** (`router.rs`): Branch classifier routing to Fuzz/Gradient/LlmSynth based on heuristic, depth, stall count
- **RotationPolicy** (`rotation.rs`): Round-robin strategy rotation on stall
- **EnsembleSync** (`ensemble.rs`): GALS (Globally Asynchronous, Locally Synchronous) seed exchange buffer
- **DrillerEscalation** (`driller.rs`): Concolic escalation when fuzzer stalls

### Alternative 1: EvoMaster (MIO Algorithm)

**Architecture:** EvoMaster uses the Many Independent Objective (MIO) algorithm, where each coverage target is an independent objective. Unlike MOSA (which optimizes all objectives simultaneously), MIO treats each target separately and uses focused search on promising targets.

**Feedback loop:**
1. Instrument SUT via bytecode manipulation (JVM agent)
2. Evolutionary loop: mutate test cases, execute against SUT, measure fitness per target
3. Archives: per-target best test case; population of not-yet-covered targets
4. Phase transition: focused exploration -> exploitation as budget is consumed

**Key insight for APEX:** MIO's per-target archive is analogous to APEX's branch-level tracking but formalized. APEX could benefit from maintaining a **per-branch best seed** archive (closest heuristic value) rather than just a global corpus. This would make the driller escalation more targeted -- when switching to concolic, start from the seed that got closest to the target branch.

**Paper:** Arcuri, "Test suite generation with the Many Independent Objective (MIO) algorithm" (IST 2018)

### Alternative 2: OSS-Fuzz / ClusterFuzz

**Architecture:** ClusterFuzz runs on 25,000+ cores (100,000+ VMs for OSS-Fuzz). Key design:
- **Bot clusters**: Groups of VMs running different fuzzing engines (libFuzzer, AFL++, Honggfuzz, Centipede)
- **Corpus management**: GCS-backed shared corpus, periodically synced between engines
- **Crash pipeline**: Dedup (stack trace + crash state hash) -> minimize -> bisect -> report -> verify fix
- **Ensemble**: Each engine runs independently; corpus sync is periodic, not real-time

**Key insight for APEX:** ClusterFuzz's ensemble approach is simple -- just share the corpus directory. There is no real-time coordination between engines. This validates APEX's `EnsembleSync` approach (periodic drain of shared buffer). The 20-iteration default interval is reasonable. ClusterFuzz also shows that **crash dedup is critical at scale** -- APEX's `BugLedger` should eventually include stack-hash dedup.

### Alternative 3: LibAFL

**Architecture:** LibAFL is a Rust framework (like APEX) with composable components:
- **Scheduler**: Pluggable (QueueScheduler, PowerScheduler, MinimizingScheduler, WeightedScheduler)
- **Observer**: Feedback collection (MapObserver for coverage, TimeObserver, etc.)
- **Feedback**: Coverage novelty detection (MapFeedbackState, CrashFeedback, TimeFeedback)
- **LLMP (Low-Level Message Passing)**: Lock-free shared-memory IPC for multi-core scaling. Near-linear scaling across cores.
- **Corpus**: In-memory or on-disk, with minimization

**Key insight for APEX:** LibAFL's LLMP is the gold standard for multi-core fuzzer coordination. APEX's `EnsembleSync` uses a `Mutex<Vec<InputSeed>>` -- this is a bottleneck for high-throughput parallel execution. For APEX's use case (test generation, not raw binary fuzzing), the current approach is adequate because the bottleneck is LLM inference, not seed synchronization. However, if APEX adds a raw fuzzing mode, it should adopt an LLMP-style lock-free ring buffer.

**Paper:** Fioraldi et al., "LibAFL: A Framework to Build Modular and Reusable Fuzzers" (CCS 2022). LibAFL-DiFuzz (2024) extends with directed fuzzing.

### Alternative 4: AFL++ Power Schedules

**Schedules:** `explore` (default), `fast`, `exploit`, `seek`, `rare`, `mmopt`, `coe`, `lin`, `quad`

**Recommended parallel config:**
- Main node: `-p exploit`
- Secondary nodes: mix of `-p coe`, `-p fast`, `-p explore`
- `rare` ignores seed runtime, favoring seeds hitting rare edges
- `mmopt` boosts newest seeds for deeper exploration

**Key insight for APEX:** APEX's `StrategyBandit` (Thompson sampling) is more adaptive than AFL++'s fixed schedules -- it learns which strategy works per branch difficulty class. However, AFL++ shows the value of **schedule diversity in parallel instances**. APEX should ensure that when running multiple strategies, they use different selection biases (some exploit, some explore), not just different synthesis methods.

### Alternative 5: Angora

**Architecture:** Taint-guided fuzzing with gradient descent:
1. **Byte-level taint tracking**: Identifies which input bytes influence each branch condition
2. **Gradient descent mutation**: Treats branch conditions as functions of input bytes, uses gradient descent to find satisfying inputs (no symbolic execution)
3. **Context-sensitive coverage**: Adds calling context to branch IDs for finer-grained exploration
4. **Input length exploration**: Only extends inputs when it might reach new branches

**Key insight for APEX:** Angora's gradient descent approach is lightweight compared to Z3 solving but handles numeric constraints well. APEX already has branch distance heuristics (`crates/apex-coverage/src/heuristic.rs`). The next step would be to add **byte-level taint tracking** to know which parts of the input to mutate. This is especially relevant for APEX's `MutationGuide` -- currently it lacks taint information to focus mutations.

**Paper:** Chen & Chen, "Angora: Efficient Fuzzing by Principled Search" (IEEE S&P 2018)

### Alternative 6: Parallel Fuzzing Research (KRAKEN, DynamiQ, TAEF, FlexFuzz)

**Key papers:**
- **DynamiQ** (2025): Call-graph-based task partitioning. Divides target into subgraphs, assigns to parallel fuzzers. Uses runtime feedback to reallocate. 25,000 CPU hours evaluation.
- **KRAKEN** (2025): Program-adaptive parallel fuzzing. Observes coverage changes during runtime to dynamically adjust strategy per fuzzer instance.
- **TAEF** (2022): Divides callgraph into subtasks, assigns subcorpora, synchronizes bitmaps.
- **FlexFuzz** (2025): Boundary-sensitive task allocation -- identifies boundary basic blocks between covered/uncovered regions and targets them.

**Key insight for APEX:** All modern parallel fuzzing research converges on **call-graph-based task partitioning**. APEX already has call graph infrastructure in `apex-cpg`. The opportunity is to use the call graph to assign different strategies to different **subgraphs** of the program rather than having all strategies compete on all branches. This is the single biggest architectural improvement APEX could make.

### Alternative 7: PASTIS (Collaborative Fuzzing)

**Architecture:** Broker-based ensemble fuzzing:
- **Broker**: Central coordinator that receives seeds from all engines and redistributes
- **Agents**: Heterogeneous engines (AFL++, Honggfuzz, TritonDSE) connected to broker
- **Protocol**: libpastis defines client/broker communication
- **Seed sharing**: Each engine decides independently whether to keep a shared seed

**Key insight for APEX:** PASTIS won the SBFT 2023 fuzzing competition by combining grey-box and white-box fuzzers. APEX's architecture (fuzzer + concolic + LLM synth) is similar. The broker pattern could replace APEX's direct `EnsembleSync`. However, for single-machine deployment, the current direct-sharing approach is simpler and sufficient.

### Answer to Key Question

> APEX's orchestrator is sequential (one strategy at a time per iteration). Should it switch to parallel execution with shared corpus?

**Assessment: APEX already runs strategies in parallel within each iteration** (`futures::join_all` on all strategies' `suggest_inputs`). However, the loop is synchronous -- iteration N+1 waits for all of iteration N to complete.

**Recommendation: Hybrid approach.**

1. **Keep the synchronous loop for LLM-based strategies** (inherently slow, ~seconds per suggestion). The current architecture is appropriate here.

2. **Add an async background fuzzer** that runs continuously between LLM iterations, feeding into the shared corpus. This is the PASTIS pattern -- the fuzzer and LLM operate at different timescales but share seeds.

3. **Adopt call-graph-based task partitioning** (from DynamiQ/TAEF) to assign branches to strategies based on subgraph topology, not just branch-level heuristics.

4. **Maintain per-branch best seed archive** (from MIO) for targeted escalation.

**Priority: Medium.** The current architecture handles APEX's primary use case (LLM-guided test generation) well. The background fuzzer would mainly help the optional fuzzing/concolic modes.

---

## Dig 11: Sandbox / Process Isolation

### APEX Current Architecture

**ProcessSandbox** (`crates/apex-sandbox/src/process.rs`):
- Spawns target as subprocess via `CommandRunner`
- AFL++-compatible SHM coverage bitmap (65KB, POSIX `shm_open`)
- `__APEX_SHM_NAME` env var for coverage shim
- Timeout-based hung process detection
- Mock-able via `CommandRunner` trait for testing

**FirecrackerSandbox** (`crates/apex-sandbox/src/firecracker.rs`):
- Drives Firecracker via REST API on Unix domain socket
- Pre-built rootfs per language in `~/.apex/rootfs/`
- Snapshot/restore: boot once, snapshot after target load, restore for each seed
- Virtio-vsock frame protocol for seed injection and bitmap retrieval
- Feature-gated (`--features firecracker`)

**Language-specific sandboxes:**
- `PythonTestSandbox`: pytest + coverage.py
- `JavaScriptTestSandbox`: Node.js/vitest
- `RubyTestSandbox`: Ruby test runner
- `RustTestSandbox`: cargo test runner

### Alternative 1: gVisor

**Architecture:** User-space kernel (Sentry) that intercepts and re-implements syscalls. Runs as an OCI runtime (`runsc`).

| Metric | Value |
|--------|-------|
| Startup | 20-50% overhead vs native container |
| CPU | ~identical to native |
| Syscall overhead | 2-10x per syscall (user-space trap) |
| Memory | Minimal per-instance overhead |
| Isolation | Process-level (weaker than VM) |
| Kubernetes | Native integration (RuntimeClass) |

**Relevance to APEX:** Good for containerized deployment of APEX analyzing untrusted code. Not suitable as the per-seed execution sandbox (syscall overhead would tank fuzzing throughput).

### Alternative 2: Firecracker (Deeper Analysis)

**Who uses it:**
- AWS Lambda: Primary execution environment
- Fly.io: Edge compute platform
- Kata Containers: Alternative microVM runtime
- Tonic.ai: Data generation in isolation

| Metric | Value |
|--------|-------|
| Startup | ~125ms cold boot, <5ms snapshot restore |
| Memory | ~5MB overhead per instance |
| Isolation | Hardware virtualization (KVM) |
| Attack surface | Minimal device model (no USB, GPU, PCI) |

**Key insight for APEX:** APEX's snapshot/restore approach is correct -- it amortizes boot cost. The <5ms restore time is competitive with process spawn. For analyzing malicious code, Firecracker provides the strongest isolation with acceptable overhead.

### Alternative 3: WASM Sandboxing (Wasmtime, Wasmer)

| Metric | Value |
|--------|-------|
| Startup | Microseconds (pre-compiled modules) |
| Isolation | Memory-safe by construction |
| Syscall | Capability-based (WASI) |
| Overhead | ~10-30% CPU for compute |
| Limitation | No filesystem, no threads (WASI preview 2 adds some) |

**Relevance to APEX:** WASM is attractive for sandboxing pure computation but APEX targets need filesystem access (reading source files, running interpreters). WASM would require porting entire language runtimes to WASI, which is not practical for Python/Node.js. **Not recommended for APEX's current use case.**

However, WASM could be valuable for a future "APEX-as-a-service" mode where users submit analysis jobs -- the APEX analysis engine itself could run in a WASM sandbox.

### Alternative 4: seccomp-bpf

**Architecture:** Linux kernel syscall filter. One-way restriction -- once applied, cannot be relaxed.

| Metric | Value |
|--------|-------|
| Overhead | Near-zero (BPF filter in kernel) |
| Startup | Microseconds (just set filter) |
| Granularity | Syscall number + arguments |
| Limitation | Cannot make path-based decisions |

**Key insight for APEX:** seccomp-bpf is the **minimum viable sandbox layer** for Linux. APEX should apply a seccomp filter to subprocess execution that blocks dangerous syscalls:
- Block: `execve` (prevent spawning shells), `ptrace`, `mount`, `reboot`, `kexec_load`
- Allow: `read`, `write`, `open`, `close`, `mmap`, `brk`, `exit_group`, `clock_gettime`
- Allow: `shm_open`, `mmap` (needed for coverage SHM)

This is ~50 lines of code and provides defense-in-depth at near-zero cost.

### Alternative 5: Landlock

**Architecture:** Linux security module for unprivileged filesystem sandboxing. Available since kernel 5.13.

| Metric | Value |
|--------|-------|
| Overhead | Near-zero |
| Granularity | Filesystem paths + operations (read/write/execute) |
| Limitation | Filesystem only (no network, no syscall filtering) |

**Key insight for APEX:** Landlock complements seccomp-bpf. Together they provide:
- seccomp-bpf: syscall-level restriction
- Landlock: filesystem-level restriction (read-only access to source tree, no access to `~/.ssh`, `/etc/shadow`)

This is the **recommended minimum viable sandbox for Linux** and requires no root privileges.

### Alternative 6: Bubblewrap (bwrap)

**Architecture:** Single static binary (~50KB). Uses Linux namespaces (PID, UTS, IPC, net, mount).

| Metric | Value |
|--------|-------|
| Overhead | ~10-100ms startup |
| Binary size | ~50KB |
| Features | Namespace isolation, bind mounts, tmpfs, --die-with-parent |
| Root required | No (user namespaces) |

**Key insight for APEX:** Bubblewrap is ideal as a **lightweight wrapper** for ProcessSandbox on Linux. It provides network isolation (private network namespace), filesystem isolation (bind-mount only the target directory), and process isolation (separate PID namespace). The `--die-with-parent` flag is equivalent to APEX's `kill_on_drop`.

### Alternative 7: nsjail

**Architecture:** Google's lightweight process isolation tool. Combines namespaces + cgroups + rlimits + seccomp-bpf + Kafel BPF language.

| Metric | Value |
|--------|-------|
| Startup | ~10-100ms |
| Features | All of bubblewrap + cgroups + seccomp (Kafel DSL) |
| Used by | Google for internal fuzzing |

**Key insight for APEX:** nsjail is the most feature-complete lightweight sandbox and is specifically designed for fuzzing. It is **the recommended sandbox for APEX's Linux fuzzing mode** where Firecracker is too heavy but process isolation is needed.

### Alternative 8: macOS (sandbox-exec)

**Status:** Deprecated since macOS 10.15+ but still functional. Used by OpenAI Codex, Chrome, and Apple's own tools internally.

**Key insight for APEX:** On macOS, sandbox-exec is the only viable lightweight sandbox. APEX should use it with a restrictive profile (deny network, deny file-write-* except temp dirs). The deprecation is concerning but Apple has no announced replacement, and internal dependencies ensure continued support.

A newer alternative is **Alcoholless** (2025), which runs commands as a separate user with filesystem copy-on-write semantics.

### Answer to Key Question

> For a security tool analyzing malicious code, what's the minimum viable sandbox?

**Recommended layered approach by platform:**

| Layer | Linux | macOS |
|-------|-------|-------|
| L0 (always) | seccomp-bpf + Landlock | sandbox-exec profile |
| L1 (default) | bubblewrap/nsjail wrapper | sandbox-exec + separate user |
| L2 (untrusted) | Firecracker microVM | Firecracker (Linux VM on macOS) |

**Implementation priority:**
1. **L0: seccomp-bpf filter** -- ~50 lines of Rust using the `seccompiler` crate. Apply to all ProcessSandbox spawns on Linux. Near-zero overhead, blocks the most dangerous syscalls. **Do this first.**
2. **L0: sandbox-exec profile** -- ~30 lines of SBPL. Apply to ProcessSandbox on macOS. **Do this second.**
3. **L1: nsjail wrapper** -- Shell out to nsjail before spawning target on Linux. ~20 lines of integration code. **Do this third.**
4. **L2: Firecracker** -- Already implemented. Use when `--untrusted` flag is passed. No additional work needed.

---

## Dig 12: Reachability Analysis

### APEX Current Architecture

**CPG-based analysis** (`crates/apex-cpg/src/`):
- **Cpg struct**: HashMap-based graph with AST, CFG, ReachingDef, and Argument edges
- **Taint analysis** (`taint.rs`): Backward BFS from sinks over ReachingDef edges, with sanitizer break, inter-procedural via TaintSummary cache
- **Reaching definitions** (`reaching_def.rs`): Computes def-use chains within methods
- **Language-specific builders**: Python, JS, Go CPG builders (regex/AST-based)

**Call graph construction** (in language extractors):
- Regex-based function/method call extraction
- No pointer analysis, no type resolution
- Static, single-pass construction
- Max traversal depth: 20

### Alternative 1: Andersen's Points-To Analysis

**How it works:** Subset-based inclusion constraints. For each assignment `x = &y`, add constraint `y in pts(x)`. For `x = y`, add `pts(y) subset pts(x)`. Fixed-point iteration until stable.

| Metric | Value |
|--------|-------|
| Precision | High (flow-insensitive, context-insensitive baseline) |
| Scalability | O(n^3) worst case, practical with cycle detection |
| Languages | Best for C/C++, Java (static types) |
| Limitation | Impractical for Python/JS (dynamic types) |

**Key insight for APEX:** Andersen's analysis is irrelevant for APEX's primary targets (Python, JavaScript, Go) because these languages lack the static type information needed for precise pointer analysis. For Java and C, it would help, but APEX's regex-based approach is a reasonable trade-off given the multi-language requirement.

### Alternative 2: RTA (Rapid Type Analysis)

**How it works:** Tracks which classes are instantiated at runtime (via `new` expressions). When resolving a virtual call `x.foo()`, only considers implementations in classes that are actually instantiated somewhere in the program.

| Metric | Value |
|--------|-------|
| Precision | Better than CHA (excludes uninstantiated classes) |
| Performance | O(n) -- single pass over program |
| Languages | OOP languages (Java, Python, Ruby) |

**Key insight for APEX:** RTA is cheap and would improve Python call graph precision significantly. Currently, when APEX sees `obj.method()`, it cannot resolve which class `method` belongs to. With RTA, it could narrow down based on which classes are instantiated. **Recommended for Python and Ruby support.**

### Alternative 3: CHA (Class Hierarchy Analysis)

**How it works:** For virtual call `x.foo()` where `x` has declared type `T`, include all implementations of `foo` in `T` and its subclasses.

| Metric | Value |
|--------|-------|
| Precision | Lowest of the three (over-approximates) |
| Performance | O(1) per call (pre-compute hierarchy) |
| Soundness | Sound (includes all possible targets) |

**Key insight for APEX:** CHA is simpler than RTA but produces many spurious edges. APEX's current approach is actually less precise than CHA (it does not even resolve the class hierarchy). Adding a class hierarchy extraction pass would be a low-cost improvement.

### Alternative 4: IFDS/IDE Framework

**How it works:** Solves interprocedural, finite, distributive, subset (IFDS) problems by reducing them to graph reachability on an exploded supergraph. Handles context sensitivity through procedure summaries.

**Implementations:**
- **Heros** (Java): Generic IFDS/IDE solver, pluggable into Soot/SootUp
- **WALA** (Java): IFDS solver, highly scalable, memory-efficient
- **SVF** (LLVM): Interprocedural static value-flow analysis for C/C++

**Applicable problems:** Typestate checking, taint analysis, uninitialized variables, security information flow.

**Key insight for APEX:** APEX's taint analysis (`taint.rs`) is essentially a simplified IFDS problem -- it computes backward reachability over reaching definitions. The current BFS-based approach works but lacks context sensitivity (it can report false positives where taint flows through a function that actually sanitizes in a different calling context). Implementing a proper IFDS solver would improve precision but is a significant engineering effort. **Recommended as a Tier 2 improvement.**

**Paper:** Reps, Horwitz, Sagiv, "Precise Interprocedural Dataflow Analysis via Graph Reachability" (POPL 1995); Bodden, "Inter-procedural data-flow analysis with IFDS/IDE and Soot" (SOAP 2012)

### Alternative 5: Dynamic Call Graphs

**How it works:** Instrument the program at runtime (e.g., Python's `sys.settrace`, Node.js `--trace-*`, Go `runtime.Callers`) and record actual call edges during execution.

**Precision study (ICSE 2020):**
- Static analysis median recall: 0.884
- With dynamic language support: 0.935
- Hybrid (static + dynamic): >0.95
- Static precision on programs without inputs: ~97% (TAJS for JS)

**Key insight for APEX:** APEX already runs the target program (for coverage). It should **piggyback call graph collection onto execution**. For Python, `sys.settrace` can record caller/callee pairs with negligible overhead. This gives APEX a dynamic call graph for free, which can be merged with the static graph:
- Static graph: sound (includes edges that aren't exercised)
- Dynamic graph: precise (only real edges, but incomplete)
- Union: best of both worlds

**Recommended implementation:** Add a `--collect-callgraph` flag to APEX's coverage probes that records call edges alongside branch coverage. For Python, this is ~20 lines added to the coverage shim.

**Papers:**
- Reif et al., "On the Recall of Static Call Graph Construction in Practice" (ICSE 2020)
- Antal et al., "Total Recall? How Good Are Static Call Graphs Really?" (ISSTA 2024)

### Alternative 6: ML-Predicted Call Edges

**Recent work:** "Call Me Maybe: Enhancing JavaScript Call Graph Construction using Graph Neural Networks" (2025) -- models call graph augmentation as link prediction on program graphs using GNNs.

**Key insight for APEX:** This is an emerging approach that could help with JavaScript's extreme dynamism (callbacks, closures, prototype chain). However, it requires training data and model inference. **Not recommended for APEX currently** -- the dynamic call graph approach above gives better results with less complexity.

### Alternative 7: PyCG (Python-Specific)

**Architecture:** Python-specific call graph generator using assignment-based flow analysis.

| Metric | Value |
|--------|-------|
| Precision | ~99.2% |
| Recall | ~69.9% |
| Performance | 0.38s per 1K LoC |

**Successor:** JARVIS (2023) improves PyCG by 67% speed, 84% precision, 20% recall.

**Key insight for APEX:** PyCG's precision is excellent but recall is only ~70%. This means 30% of real call edges are missed. For a security tool, false negatives are dangerous (missing attack paths). APEX should combine static (PyCG-like) with dynamic analysis to get both high precision and high recall.

### Answer to Key Question

> APEX's call graph is static and regex-based. How much precision is lost vs a proper points-to analysis?

**Assessment: Significant precision loss, but recall may actually be acceptable.**

APEX's regex-based extraction is similar to a name-matching approach -- if it sees `foo()`, it connects to all functions named `foo`. This has:
- **High recall** (~90%+): Most calls in Python/JS use simple names
- **Low precision** (~60-70%): Overconnects due to name ambiguity (multiple `__init__`, `setup`, `get` across classes)

A proper points-to analysis would improve precision to ~95%+ but is impractical for dynamic languages. The practical path is:

**Recommended improvements (in priority order):**

1. **Dynamic call graph collection** -- piggyback on execution, ~20 lines per language runtime. Gives ground truth for exercised paths. **Highest ROI.**

2. **RTA for Python/Ruby** -- track class instantiations (`MyClass()`) and use them to resolve method calls. Eliminates most false edges from name ambiguity. **Medium effort, high impact.**

3. **Scope-aware resolution** -- when seeing `x.foo()`, resolve `x` to its most recent assignment in the same scope. This is a lightweight form of flow-sensitive analysis. **Low effort, medium impact.**

4. **IFDS-based taint analysis** -- replace BFS backward reachability with IFDS for context-sensitive taint tracking. **High effort, high impact for security detectors.**

---

## Cross-Cutting Recommendations

### Immediate Actions (Tier 0)
1. Add seccomp-bpf filter to ProcessSandbox on Linux
2. Add sandbox-exec profile for macOS ProcessSandbox
3. Add dynamic call graph collection to Python/JS coverage probes
4. Maintain per-branch best-seed archive for targeted escalation

### Short-Term (Tier 1)
1. Implement RTA-based call graph refinement for Python
2. Add nsjail wrapper as default Linux sandbox layer
3. Add scope-aware name resolution to call graph extractors
4. Add background async fuzzer that runs between LLM iterations

### Medium-Term (Tier 2)
1. Call-graph-based task partitioning (DynamiQ-style) for parallel strategy assignment
2. IFDS-based interprocedural taint analysis
3. Byte-level taint tracking for mutation guidance (Angora-style)
4. Crash dedup via stack hash in BugLedger

### Long-Term (Tier 3)
1. LLMP-style lock-free IPC for high-throughput fuzzing mode
2. ML-predicted call edges for JavaScript
3. WASM sandbox for APEX-as-a-service mode

---

## Sources

### Orchestrator / Agent Loop
- [EvoMaster: Tool Report (ASE 2024)](https://link.springer.com/article/10.1007/s10515-024-00478-1)
- [EvoMaster GitHub](https://github.com/WebFuzzing/EvoMaster)
- [MIO Algorithm Paper](https://www.sciencedirect.com/science/article/abs/pii/S0950584917304822)
- [LibAFL: CCS 2022 Paper](https://www.s3.eurecom.fr/docs/ccs22_fioraldi.pdf)
- [LibAFL Architecture](https://aflplus.plus/libafl-book/design/architecture.html)
- [LibAFL-DiFuzz (2024)](https://arxiv.org/abs/2412.19143)
- [LibAFL GitHub](https://github.com/AFLplusplus/LibAFL)
- [ClusterFuzz Architecture](https://google.github.io/clusterfuzz/architecture/)
- [ClusterFuzz GitHub](https://github.com/google/clusterfuzz)
- [AFL++ Power Schedules](https://aflplus.plus/docs/power_schedules/)
- [AFL++ Paper (WOOT 2020)](https://www.usenix.org/system/files/woot20-paper-fioraldi.pdf)
- [Angora: Efficient Fuzzing by Principled Search (S&P 2018)](https://web.cs.ucdavis.edu/~hchen/paper/chen2018angora.pdf)
- [Angora GitHub](https://github.com/AngoraFuzzer/Angora)
- [DynamiQ: Dynamic Task Allocation in Parallel Fuzzing (2025)](https://arxiv.org/html/2510.04469v1)
- [KRAKEN: Program-Adaptive Parallel Fuzzing (2025)](https://dl.acm.org/doi/10.1145/3728882)
- [PASTIS: Collaborative Fuzzing Framework](https://github.com/quarkslab/pastis)
- [PASTIS Blog Post](https://blog.quarkslab.com/pastis-for-the-win.html)
- [Hierarchical Seed Scheduling (AFL-Hier)](https://www.cs.ucr.edu/~heng/pubs/afl-hier.pdf)

### Sandbox / Process Isolation
- [gVisor vs Firecracker Comparison (Northflank)](https://northflank.com/blog/firecracker-vs-gvisor)
- [Sandbox Isolation Comparison for AI Agents](https://dev.to/agentsphere/choosing-a-workspace-for-ai-agents-the-ultimate-showdown-between-gvisor-kata-and-firecracker-b10)
- [Isolation Technologies Comparison (SoftwareSeni)](https://www.softwareseni.com/firecracker-gvisor-containers-and-webassembly-comparing-isolation-technologies-for-ai-agents/)
- [Blending Containers and VMs (VEE 2020)](https://pages.cs.wisc.edu/~swift/papers/vee20-isolation.pdf)
- [nsjail GitHub](https://github.com/google/nsjail)
- [Figma: Server-side Sandboxing](https://www.figma.com/blog/server-side-sandboxing-containers-and-seccomp/)
- [awesome-sandbox GitHub](https://github.com/restyler/awesome-sandbox)
- [sandbox-exec on macOS](https://igorstechnoclub.com/sandbox-exec/)
- [Alcoholless: macOS Sandbox (2025)](https://medium.com/nttlabs/alcoholless-a-lightweight-security-sandbox-for-macos-programs-homebrew-ai-agents-etc-ccf0d1927301)
- [Sandbox Isolation Discussion](https://www.shayon.dev/post/2026/52/lets-discuss-sandbox-isolation/)

### Reachability Analysis
- [Pointer Analysis Tutorial (Smaragdakis)](https://yanniss.github.io/points-to-tutorial15.pdf)
- [Call Graph Construction Algorithms Explained](https://ben-holland.com/call-graph-construction-algorithms-explained/)
- [IFDS/IDE and Soot (SOAP 2012)](http://www.bodden.de/pubs/bodden12inter-procedural.pdf)
- [Heros: IFDS/IDE Solver GitHub](https://github.com/soot-oss/heros)
- [SVF: Interprocedural Value-Flow Analysis](https://yuleisui.github.io/publications/cc16.pdf)
- [PyCG: Call Graph Generation in Python (ICSE 2021)](https://arxiv.org/abs/2103.00587)
- [JARVIS: Scalable Python Call Graphs](https://arxiv.org/abs/2305.05949)
- [Static Call Graph Recall (ICSE 2020)](https://dl.acm.org/doi/10.1145/3377811.3380441)
- [Total Recall? Static Call Graphs (ISSTA 2024)](https://dl.acm.org/doi/10.1145/3650212.3652114)
- [Call Me Maybe: GNN for JS Call Graphs (2025)](https://arxiv.org/html/2506.18191)
- [Joern CPG Documentation](https://docs.joern.io/code-property-graph/)
- [Code Property Graph Specification](https://cpg.joern.io/)
