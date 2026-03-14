# apex-agent Research Integration Plan

Compiled: 2026-03-14
Scope: 8 techniques targeting `crates/apex-agent/`

---

## Current Architecture Summary

The `apex-agent` crate orchestrates exploration through:

- **`priority.rs`** -- `target_priority()` scores branches by rarity/depth/proximity/staleness; `recommend_strategy()` routes to Gradient/Fuzz/LlmSynth based on ad-hoc heuristic thresholds
- **`orchestrator.rs`** -- `AgentCluster` holds strategies (`Vec<Box<dyn Strategy>>`), oracle, sandbox; runs the main exploration loop with stall detection
- **`monitor.rs`** -- `CoverageMonitor` tracks coverage growth via sliding window; escalates Normal -> SwitchStrategy -> AgentCycle -> Stop
- **`driller.rs`** -- `DrillerStrategy` implements `Strategy` trait; negates frontier path constraints via SMT solver
- **`ensemble.rs`** -- `EnsembleSync` buffer for seed sharing between concurrent agents
- **`exchange.rs`** -- `SeedExchange` bidirectional fuzz<->concolic seed queue
- **`cache.rs`** -- `SolverCache` with negation inference
- **`ledger.rs`** -- `BugLedger` dedup-aware bug accumulator

Key trait from `apex-core`: `Strategy { name(), suggest_inputs(&ExplorationContext), observe(&ExecutionResult) }`

Key types: `BranchId`, `BranchCandidate`, `InputSeed`, `ExecutionResult`, `ExplorationContext`, `StrategyRecommendation`

---

## Technique 1: S2F Principled Hybrid Routing

**Paper:** arXiv:2601.10068 (Jan 2026)
**Core idea:** Replace ad-hoc proximity thresholds with formal branch classification that determines when to use fuzzing vs symbolic vs sampling.

### Design: `BranchClassifier` Trait + `S2fRouter`

```rust
/// Classification of a branch's constraint characteristics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchCategory {
    /// Numeric comparison (x > 5): gradient or Z3 solves efficiently
    NumericConstraint,
    /// String/format constraint (regex match, JSON parse): LLM handles better
    StringConstraint,
    /// Shallow branch with many paths: fuzzer mutation likely reaches it
    ShallowHighFanout,
    /// Deep path with tight constraints: symbolic execution needed
    DeepTightConstraint,
    /// Magic value (exact equality check): hybrid byte-level + symbolic
    MagicValue,
    /// Unknown/unclassifiable: use default heuristic
    Unknown,
}

/// Classifies branches by constraint type to determine optimal strategy.
pub trait BranchClassifier: Send + Sync {
    fn classify(&self, branch: &BranchId, context: &BranchClassifyContext) -> BranchCategory;
}

pub struct BranchClassifyContext {
    pub source_line: Option<String>,
    pub depth_in_cfg: u32,
    pub fanout: u32,          // number of outgoing paths from parent
    pub constraint_smtlib: Option<String>,
    pub hit_count: u64,
}
```

The `S2fRouter` replaces `recommend_strategy()`:

```rust
pub struct S2fRouter {
    classifier: Box<dyn BranchClassifier>,
}

impl S2fRouter {
    pub fn route(&self, branch: &BranchId, ctx: &BranchClassifyContext) -> StrategyRecommendation {
        match self.classifier.classify(branch, ctx) {
            BranchCategory::NumericConstraint => StrategyRecommendation::Gradient,
            BranchCategory::StringConstraint => StrategyRecommendation::LlmSynth,
            BranchCategory::ShallowHighFanout => StrategyRecommendation::Fuzz,
            BranchCategory::DeepTightConstraint => StrategyRecommendation::Gradient, // Z3
            BranchCategory::MagicValue => StrategyRecommendation::Gradient,
            BranchCategory::Unknown => fallback_heuristic(ctx),
        }
    }
}
```

### Files to Create or Modify

| File | Action | Content |
|------|--------|---------|
| `src/classifier.rs` | **Create** | `BranchCategory`, `BranchClassifier` trait, `BranchClassifyContext`, `SourcePatternClassifier` (regex-based impl) |
| `src/router.rs` | **Create** | `S2fRouter` struct, `route()` method, fallback heuristic |
| `src/priority.rs` | **Modify** | Add `StrategyRecommendation::Symbolic` variant; deprecate `recommend_strategy()` in favor of `S2fRouter::route()` |
| `src/orchestrator.rs` | **Modify** | Replace `recommend_strategy()` calls with `S2fRouter::route()` in the main loop |
| `src/lib.rs` | **Modify** | Add `pub mod classifier; pub mod router;` |

### Integration Points

- `AgentCluster::explore_iteration()` currently calls `recommend_strategy(heuristic, attempts)`. Replace with `self.router.route(branch, classify_ctx)`.
- The `BranchClassifyContext` is populated from `BranchCandidate` fields plus source line from `file_paths` map.
- `StrategyRecommendation` gains a `Symbolic` variant (distinct from `Gradient` -- Z3 path prefix solving vs gradient nudging).

### Build Sequence

1. Add `BranchCategory` enum and `BranchClassifier` trait (no deps)
2. Implement `SourcePatternClassifier` using regex on source lines
3. Build `S2fRouter` wrapping the classifier
4. Wire into `AgentCluster`, keeping `recommend_strategy()` as fallback
5. Add feature flag `s2f-routing` (default on) to gate the new path

### Complexity Estimate

**Medium** -- ~300 LOC. The classifier is pattern-matching on source text + constraint strings; the router is a match expression. Main risk: classification accuracy depends on source-line quality.

### Test Strategy

- Unit test `SourcePatternClassifier` with known source patterns (numeric comparisons, string ops, magic values)
- Unit test `S2fRouter::route()` for each `BranchCategory` -> `StrategyRecommendation` mapping
- Integration test: given a `BranchCandidate` with source context, verify end-to-end routing
- Property test: every `BranchCategory` variant produces a valid `StrategyRecommendation`
- Regression test: old `recommend_strategy()` behavior preserved when classifier returns `Unknown`

---

## Technique 2: DeepGo Transition Table

**Paper:** NDSS 2024
**Core idea:** Learn a state transition table from execution traces. Given current coverage state and a mutation action, predict the probability of reaching each target branch. Use RL to select actions that maximize expected progress.

### Design: `TransitionTable`

```rust
/// State: set of covered branch IDs (represented as bitmap index)
/// Action: which mutator/strategy to apply
/// Transition: P(next_state | state, action)
pub struct TransitionTable {
    /// Sparse transition counts: (state_hash, action) -> HashMap<state_hash, count>
    transitions: HashMap<(u64, ActionId), HashMap<u64, u32>>,
    /// Total observations per (state, action) pair
    totals: HashMap<(u64, ActionId), u32>,
}

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct ActionId(pub u16);

impl TransitionTable {
    pub fn new() -> Self;

    /// Record an observed transition: from state_before, taking action, reached state_after.
    pub fn record(&mut self, state_before: u64, action: ActionId, state_after: u64);

    /// Predict probability of reaching target_state from current_state via action.
    pub fn predict(&self, current_state: u64, action: ActionId, target_state: u64) -> f64;

    /// Select the action with highest predicted probability of reaching target.
    pub fn best_action(&self, current_state: u64, target_state: u64, actions: &[ActionId]) -> Option<ActionId>;

    /// Compute state hash from a set of covered branch IDs.
    pub fn state_hash(covered: &[BranchId]) -> u64;
}
```

### Files to Create or Modify

| File | Action | Content |
|------|--------|---------|
| `src/transition.rs` | **Create** | `TransitionTable`, `ActionId`, state hashing, prediction logic |
| `src/orchestrator.rs` | **Modify** | After each execution, call `transition_table.record(before, action, after)`. Before selecting strategy, consult `transition_table.best_action()`. |
| `src/priority.rs` | **Modify** | `BranchCandidate` gains `transition_score: Option<f64>` field |
| `Cargo.toml` | **Modify** | No new deps (uses std HashMap + FNV hashing from existing code) |

### Integration Points

- The orchestrator's main loop already has before/after coverage snapshots (from `CoverageOracle`). Hash these to get `state_before` and `state_after`.
- `ActionId` maps to strategy indices in `AgentCluster::strategies`.
- `best_action()` is consulted alongside `S2fRouter::route()` -- the router provides the default, the transition table can override if it has high-confidence data.

### Build Sequence

1. Implement `TransitionTable` with `record()` and `predict()` (pure data structure, no deps)
2. Add `state_hash()` using FNV-1a over sorted branch IDs
3. Wire `record()` into `AgentCluster::explore_iteration()` post-execution
4. Wire `best_action()` into strategy selection, gated on minimum observation count

### Complexity Estimate

**Medium-High** -- ~400 LOC. The data structure is straightforward but state-space management (pruning, decay, bucketing) needs care to avoid memory blowup.

### Test Strategy

- Unit test `record()` + `predict()` with small deterministic traces
- Unit test `best_action()` returns the action with highest transition probability
- Unit test `state_hash()` is deterministic and order-independent
- Test memory bounds: verify table prunes when exceeding configurable limit
- Test cold-start: `best_action()` returns `None` with no observations

---

## Technique 3: Graphuzz BFS Scorer

**Paper:** TOSEM 2024
**Core idea:** Score seeds using graph neural network analysis of extended control-flow graphs (e-CFGs). Seeds that exercise rare graph neighborhoods get higher scores.

### Design: `SeedScorer` Trait + `GraphBfsScorer`

```rust
/// Assigns a priority score to a seed based on structural analysis.
pub trait SeedScorer: Send + Sync {
    fn score(&self, seed: &InputSeed, trace: &ExecutionTrace) -> f64;
    fn name(&self) -> &str;
}

/// BFS-based seed scorer on the control-flow graph.
/// Scores seeds by the rarity and graph distance of branches they reach
/// from uncovered targets.
pub struct GraphBfsScorer {
    /// Adjacency list: branch_id -> neighboring branch_ids in the CFG
    cfg_adjacency: HashMap<BranchId, Vec<BranchId>>,
    /// Per-branch visit frequency (updated after each execution)
    visit_counts: Mutex<HashMap<BranchId, u64>>,
    /// BFS distance from each uncovered branch to every other branch
    distance_cache: Mutex<HashMap<BranchId, HashMap<BranchId, u32>>>,
}

impl SeedScorer for GraphBfsScorer {
    fn score(&self, seed: &InputSeed, trace: &ExecutionTrace) -> f64 {
        // Score = sum over trace branches of:
        //   (1 / visit_count) * (1 / min_distance_to_uncovered)
        // Rare branches close to uncovered targets get highest scores.
    }

    fn name(&self) -> &str { "graphuzz-bfs" }
}
```

### Files to Create or Modify

| File | Action | Content |
|------|--------|---------|
| `src/scorer.rs` | **Create** | `SeedScorer` trait, `GraphBfsScorer` struct, BFS distance computation |
| `src/orchestrator.rs` | **Modify** | After execution, score the seed via `scorer.score()` and update `InputSeed::priority` |
| `src/ensemble.rs` | **Modify** | `EnsembleSync` uses seed priority for ordering during redistribution |
| `src/lib.rs` | **Modify** | Add `pub mod scorer;` |

### Integration Points

- The CFG adjacency list comes from `apex-instrument` (branch instrumentation produces a branch graph). `AgentCluster` receives this at construction time.
- Seed scoring happens after `sandbox.run()` returns an `ExecutionResult` with a trace.
- The scored priority feeds into `InputSeed::priority` which `EnsembleSync` and seed queues already respect.

### Build Sequence

1. Define `SeedScorer` trait (no deps)
2. Implement BFS distance computation over CFG adjacency (std only)
3. Implement `GraphBfsScorer::score()` combining rarity + distance
4. Wire into orchestrator loop
5. Future: replace BFS heuristic with learned GNN model (behind `gnn` feature flag)

### Complexity Estimate

**Medium** -- ~350 LOC. BFS is straightforward. The GNN version would be High effort (requires `tch` or `ort` runtime) and is deferred to a future phase.

### Test Strategy

- Unit test BFS distance computation on small graphs (3-5 nodes)
- Unit test scoring: seed hitting rare, close-to-target branches scores higher than seed hitting common, far branches
- Unit test empty trace scores 0
- Integration test: two seeds with different traces get different priority values
- Property test: score is always non-negative

---

## Technique 4: Trace-Guided DGF Filter

**Paper:** arXiv:2510.23101
**Core idea:** Use LLM-predicted call stacks to filter which seeds are relevant for reaching a specific target branch. If the seed's execution trace overlaps with the predicted call stack, it's likely useful for directed fuzzing toward that target.

### Design: `TraceFilter`

```rust
/// Predicts which call-stack patterns are likely to reach a target branch,
/// then filters seeds whose traces match.
pub struct TraceFilter {
    /// LLM-predicted call stacks for each target branch.
    /// Key: target BranchId, Value: list of predicted function/branch sequences.
    predicted_stacks: HashMap<BranchId, Vec<PredictedCallStack>>,
}

#[derive(Debug, Clone)]
pub struct PredictedCallStack {
    /// Ordered list of (file_id, line) pairs representing the predicted path.
    pub frames: Vec<(u64, u32)>,
    /// LLM confidence in this prediction [0, 1].
    pub confidence: f64,
}

impl TraceFilter {
    pub fn new() -> Self;

    /// Register LLM-predicted call stacks for a target branch.
    pub fn register_predictions(&mut self, target: BranchId, stacks: Vec<PredictedCallStack>);

    /// Filter seeds: return those whose execution trace overlaps significantly
    /// with any predicted call stack for the given target branch.
    pub fn filter_seeds(
        &self,
        seeds: &[InputSeed],
        traces: &HashMap<SeedId, ExecutionTrace>,
        target: &BranchId,
    ) -> Vec<SeedId>;

    /// Compute overlap score between a trace and a predicted stack.
    fn overlap_score(trace: &ExecutionTrace, stack: &PredictedCallStack) -> f64;
}
```

### Files to Create or Modify

| File | Action | Content |
|------|--------|---------|
| `src/trace_filter.rs` | **Create** | `TraceFilter`, `PredictedCallStack`, overlap scoring, seed filtering |
| `src/orchestrator.rs` | **Modify** | Before directed fuzzing of a target, use `TraceFilter::filter_seeds()` to narrow the seed queue |
| `src/driller.rs` | **Modify** | `DrillerStrategy` can use trace filter to prioritize which constraints to solve |
| `src/lib.rs` | **Modify** | Add `pub mod trace_filter;` |

### Integration Points

- Predicted call stacks are generated by `apex-synth` (LLM prompt: "What call stack would reach branch X?"). The orchestrator registers them with the filter.
- The filter is consulted during directed exploration: before selecting seeds for a target branch, filter to seeds whose traces overlap with the predicted path.
- Overlap score uses set intersection of `(file_id, line)` pairs between trace and predicted stack.

### Build Sequence

1. Define `PredictedCallStack` and `TraceFilter` data structures
2. Implement `overlap_score()` using Jaccard similarity of (file_id, line) sets
3. Implement `filter_seeds()` with configurable threshold
4. Wire into orchestrator's directed-exploration path
5. Add LLM prediction prompt to `apex-synth` (separate crate change, not in scope here)

### Complexity Estimate

**Low-Medium** -- ~200 LOC in apex-agent. The filter itself is pure data manipulation. The LLM prediction prompt is in `apex-synth` (out of scope for this plan).

### Test Strategy

- Unit test `overlap_score()`: full overlap = 1.0, no overlap = 0.0, partial overlap proportional
- Unit test `filter_seeds()`: seeds with matching traces pass, others don't
- Unit test empty predictions: all seeds pass (no filter applied)
- Unit test confidence threshold: low-confidence predictions don't filter
- Integration test with mock traces and predictions

---

## Technique 5: Adversarial Test-Mutant Loop (AdverTest)

**Paper:** arXiv:2602.08146 (Feb 2026)
**Core idea:** Two adversarial agents iterate: Agent T writes tests to kill mutants, Agent M generates mutants that survive T's tests. Each round exposes the other's blind spots. Converges in 2-3 rounds.

### Design: `AdversarialLoop`

```rust
/// Orchestrates the adversarial test-vs-mutant loop.
pub struct AdversarialLoop {
    pub max_rounds: u32,
    pub convergence_threshold: f64, // stop when mutation kill rate exceeds this
}

#[derive(Debug, Clone)]
pub struct MutantSpec {
    pub id: Uuid,
    /// The mutation operator applied.
    pub operator: MutationOperator,
    /// File and line range where the mutation was applied.
    pub file_id: u64,
    pub line_range: (u32, u32),
    /// The mutated source code.
    pub mutated_code: String,
    /// Whether this mutant was killed by the current test suite.
    pub killed: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum MutationOperator {
    NegateCondition,
    SwapOperator,
    RemoveStatement,
    ReplaceConstant,
    BoundaryShift,
    SwapArguments,
}

#[derive(Debug, Clone)]
pub struct AdversarialRound {
    pub round: u32,
    pub tests_generated: usize,
    pub mutants_generated: usize,
    pub mutants_killed: usize,
    pub kill_rate: f64,
}

impl AdversarialLoop {
    pub fn new(max_rounds: u32, convergence_threshold: f64) -> Self;

    /// Run one complete adversarial loop for a code region.
    /// Returns the history of rounds for reporting.
    pub async fn run(
        &self,
        region: &CodeRegion,
        existing_tests: &[SynthesizedTest],
        test_agent: &dyn TestAgent,
        mutant_agent: &dyn MutantAgent,
    ) -> Result<Vec<AdversarialRound>>;
}

/// Agent T: generates tests targeting surviving mutants.
#[async_trait]
pub trait TestAgent: Send + Sync {
    async fn generate_killing_tests(
        &self,
        surviving_mutants: &[MutantSpec],
        region: &CodeRegion,
    ) -> Result<Vec<SynthesizedTest>>;
}

/// Agent M: generates mutants that survive the current test suite.
#[async_trait]
pub trait MutantAgent: Send + Sync {
    async fn generate_surviving_mutants(
        &self,
        current_tests: &[SynthesizedTest],
        region: &CodeRegion,
    ) -> Result<Vec<MutantSpec>>;
}

pub struct CodeRegion {
    pub file_id: u64,
    pub file_path: PathBuf,
    pub source: String,
    pub line_range: (u32, u32),
    pub target_branches: Vec<BranchId>,
}
```

### Files to Create or Modify

| File | Action | Content |
|------|--------|---------|
| `src/adversarial.rs` | **Create** | `AdversarialLoop`, `MutantSpec`, `MutationOperator`, `AdversarialRound`, `TestAgent`/`MutantAgent` traits, `CodeRegion` |
| `src/orchestrator.rs` | **Modify** | After standard synthesis, optionally invoke `AdversarialLoop::run()` for high-priority gaps |
| `src/ledger.rs` | **Modify** | Track mutant kill rates alongside bug counts |
| `src/lib.rs` | **Modify** | Add `pub mod adversarial;` |

### Integration Points

- The adversarial loop sits *after* initial test synthesis. When `apex-synth` generates tests for a gap, the orchestrator optionally runs the adversarial loop to strengthen them.
- `TestAgent` and `MutantAgent` are implemented in `apex-synth` (LLM-backed). The traits live in `apex-agent` so the orchestrator can drive the loop.
- Results feed into `BugLedger` (mutants that no test kills represent oracle gaps).
- `CoverageMonitor` observes kill rate as a secondary metric alongside coverage.

### Build Sequence

1. Define `MutantSpec`, `MutationOperator`, `CodeRegion` types
2. Define `TestAgent` and `MutantAgent` traits
3. Implement `AdversarialLoop::run()` loop logic (round management, convergence check)
4. Wire into orchestrator as optional post-synthesis step
5. Implement LLM-backed agents in `apex-synth` (separate crate)

### Dependencies

- `async-trait` (already a dep)
- `uuid` via `apex-core` (for `MutantSpec::id`)

### Complexity Estimate

**Medium** -- ~400 LOC in apex-agent (loop orchestration + types). The LLM-backed agent implementations in `apex-synth` are separate effort.

### Test Strategy

- Unit test round logic with mock agents: Agent M generates 5 mutants, Agent T kills 3, next round generates 2 more targeting survivors
- Unit test convergence: loop stops when kill rate exceeds threshold
- Unit test max_rounds: loop stops after N rounds even without convergence
- Unit test empty mutants: loop terminates immediately
- Integration test with deterministic mock agents verifying round history

---

## Technique 6: Mutation-Guided ACH (Adequacy Criterion Hierarchy)

**Paper:** Meta's ACH -- arXiv:2501.12862
**Core idea:** Use mutation score as the primary adequacy metric instead of branch coverage. The "oracle gap" (coverage minus mutation score) reveals code that is executed but not actually tested. Drive test generation toward closing the oracle gap.

### Design: `MutationGuide`

```rust
/// Tracks mutation scores per code region and uses oracle gap
/// to guide test generation priorities.
pub struct MutationGuide {
    /// Per-region mutation results
    regions: HashMap<u64, RegionMutationState>,
    /// Minimum mutation score to consider a region "adequately tested"
    adequacy_threshold: f64,
}

#[derive(Debug, Clone)]
pub struct RegionMutationState {
    pub file_id: u64,
    pub line_range: (u32, u32),
    pub branch_coverage: f64,
    pub mutation_score: f64,
    pub total_mutants: u32,
    pub killed_mutants: u32,
    pub surviving_mutants: Vec<MutantSpec>,
}

impl RegionMutationState {
    /// Oracle gap = coverage - mutation_score.
    /// High gap = code is covered but not truly tested.
    pub fn oracle_gap(&self) -> f64 {
        (self.branch_coverage - self.mutation_score).max(0.0)
    }
}

impl MutationGuide {
    pub fn new(adequacy_threshold: f64) -> Self;

    /// Update mutation state for a region after running mutants.
    pub fn update_region(&mut self, file_id: u64, state: RegionMutationState);

    /// Get regions sorted by oracle gap (highest gap first).
    /// These are the regions where tests exist but don't actually verify behavior.
    pub fn gaps_by_oracle_gap(&self) -> Vec<&RegionMutationState>;

    /// Get regions below the adequacy threshold.
    pub fn inadequate_regions(&self) -> Vec<&RegionMutationState>;

    /// Recommend whether to generate more tests or improve existing ones.
    pub fn recommend(&self, file_id: u64) -> MutationRecommendation;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationRecommendation {
    /// Coverage is low: generate new tests to cover more branches.
    IncreaseCoverage,
    /// Coverage is high but mutation score is low: strengthen assertions/oracles.
    StrengthenOracles,
    /// Both coverage and mutation score are high: region is adequately tested.
    Adequate,
}
```

### Files to Create or Modify

| File | Action | Content |
|------|--------|---------|
| `src/mutation_guide.rs` | **Create** | `MutationGuide`, `RegionMutationState`, `MutationRecommendation`, oracle gap logic |
| `src/priority.rs` | **Modify** | `target_priority()` incorporates oracle gap as a signal; add `oracle_gap: f64` to `BranchCandidate` |
| `src/orchestrator.rs` | **Modify** | After synthesis, run lightweight mutation on the gap region; use `MutationGuide::recommend()` to decide next action |
| `src/lib.rs` | **Modify** | Add `pub mod mutation_guide;` |

### Integration Points

- `MutationGuide` is consulted in the priority scoring loop. Branches in regions with high oracle gap get boosted priority -- they need oracle strengthening, not just coverage.
- `MutationRecommendation::StrengthenOracles` triggers a different prompt strategy in `apex-synth` (focus on assertions rather than new test paths).
- `RegionMutationState` is populated after running mutants via `apex-coverage` or `cargo-mutants` output parsing.
- Feeds into `AdversarialLoop` (technique 5): regions with high oracle gap are prime candidates for adversarial rounds.

### Build Sequence

1. Define `RegionMutationState` with `oracle_gap()` method
2. Implement `MutationGuide` with `update_region()`, `gaps_by_oracle_gap()`, `recommend()`
3. Add `oracle_gap` field to `BranchCandidate` in `priority.rs`
4. Modify `target_priority()` to include oracle gap signal
5. Wire into orchestrator loop

### Complexity Estimate

**Low-Medium** -- ~250 LOC. The data structures are simple. Main complexity is in the mutation execution (handled by `apex-coverage` crate, not here).

### Test Strategy

- Unit test `oracle_gap()`: coverage=0.8, mutation=0.3 -> gap=0.5
- Unit test `gaps_by_oracle_gap()` returns regions sorted descending
- Unit test `recommend()`: low coverage -> IncreaseCoverage, high coverage + low mutation -> StrengthenOracles, both high -> Adequate
- Unit test `inadequate_regions()` filters correctly by threshold
- Unit test priority integration: branches with high oracle gap score higher

---

## Technique 7: Thompson Strategy Bandit

**Paper:** T-Scheduler (arXiv:2312.04749), extended to strategy level
**Core idea:** Model strategy selection as a multi-armed bandit. Each strategy (Gradient, Fuzz, LlmSynth, Symbolic) is an arm. Reward = new branches covered. Use Thompson sampling from Beta posteriors to select the strategy with the highest expected reward. Zero hyperparameters, theoretical optimality guarantees.

### Design: `StrategyBandit`

```rust
/// Thompson sampling bandit for strategy selection.
/// Each strategy is an arm with Beta(alpha, beta) posterior.
pub struct StrategyBandit {
    arms: Vec<BanditArm>,
    rng: Mutex<StdRng>,
}

#[derive(Debug, Clone)]
pub struct BanditArm {
    pub strategy_name: String,
    pub alpha: f64,  // successes + prior
    pub beta: f64,   // failures + prior
    pub total_pulls: u64,
    pub total_reward: f64,
}

impl StrategyBandit {
    /// Create a bandit with uniform priors (alpha=1, beta=1) for each strategy.
    pub fn new(strategy_names: Vec<String>) -> Self;

    /// Sample from each arm's Beta posterior and return the arm with highest sample.
    pub fn select(&self) -> usize;

    /// Update the selected arm after observing reward.
    /// reward = number of new branches covered (0 = failure, >0 = success).
    pub fn update(&mut self, arm_index: usize, new_branches: usize);

    /// Get the current posterior mean for each arm (for reporting).
    pub fn posterior_means(&self) -> Vec<(String, f64)>;

    /// Reset all arms to uniform priors (e.g., when target changes).
    pub fn reset(&mut self);

    /// Decay old observations to adapt to non-stationary environments.
    /// Multiplies alpha and beta by decay_factor (0 < factor < 1).
    pub fn decay(&mut self, decay_factor: f64);
}
```

### Files to Create or Modify

| File | Action | Content |
|------|--------|---------|
| `src/bandit.rs` | **Create** | `StrategyBandit`, `BanditArm`, Thompson sampling logic |
| `src/orchestrator.rs` | **Modify** | Replace hardcoded strategy rotation with `bandit.select()` + `bandit.update()` |
| `src/monitor.rs` | **Modify** | `MonitorAction::SwitchStrategy` now triggers `bandit.select()` instead of round-robin |
| `src/lib.rs` | **Modify** | Add `pub mod bandit;` |
| `Cargo.toml` | **Modify** | Add `rand = "0.8"` and `rand_distr = "0.4"` for Beta distribution sampling |

### Integration Points

- `AgentCluster::strategies` indices map 1:1 to bandit arms.
- After each `sandbox.run()`, the orchestrator calls `bandit.update(current_arm, result.new_branches.len())`.
- The bandit is consulted in two places:
  1. Normal exploration: `bandit.select()` picks the strategy
  2. Stall recovery: `MonitorAction::SwitchStrategy` -> `bandit.select()` (Thompson sampling naturally explores underperforming arms)
- `decay()` is called periodically (every N iterations) to handle non-stationarity (as coverage saturates, strategy effectiveness changes).
- The bandit coexists with `S2fRouter`: the router provides per-branch recommendations; the bandit provides the global strategy preference. When they disagree, the per-branch recommendation wins.

### Build Sequence

1. Add `rand` and `rand_distr` deps to Cargo.toml
2. Implement `BanditArm` with Beta distribution sampling
3. Implement `StrategyBandit` with `select()`, `update()`, `decay()`
4. Wire into orchestrator loop replacing round-robin
5. Add periodic decay call

### Complexity Estimate

**Low** -- ~150 LOC. Thompson sampling is elegantly simple. The Beta distribution is a standard crate.

### Test Strategy

- Unit test `select()` with deterministic RNG: arm with highest alpha/(alpha+beta) wins most often
- Unit test `update()`: alpha increments on success, beta increments on failure
- Unit test `decay()`: alpha and beta multiplied by factor, preserving ratio
- Unit test `reset()`: all arms return to uniform priors
- Unit test `posterior_means()`: returns correct alpha/(alpha+beta) for each arm
- Property test: over many rounds, the best arm is selected proportionally more

---

## Technique 8: Fitness Landscape Analysis

**Paper:** arXiv:2502.00169
**Core idea:** Analyze the "fitness landscape" of each uncovered branch to predict which technique will be most effective. Smooth landscapes (gradual fitness changes) suit gradient methods; rugged landscapes (many local optima) suit random restarts; neutral plateaus suit symbolic execution.

### Design: `LandscapeAnalyzer`

```rust
/// Characterizes the fitness landscape around an uncovered branch.
#[derive(Debug, Clone)]
pub struct LandscapeProfile {
    pub branch: BranchId,
    /// Ruggedness: ratio of fitness changes that are sign-reversals.
    /// High ruggedness (>0.5) -> many local optima -> random restart / LLM.
    pub ruggedness: f64,
    /// Neutrality: fraction of neighboring inputs with identical fitness.
    /// High neutrality (>0.3) -> plateau -> symbolic execution.
    pub neutrality: f64,
    /// Gradient magnitude: average fitness improvement per step.
    /// High gradient (>0.01) -> gradient solver is effective.
    pub gradient_magnitude: f64,
    /// Deceptiveness: correlation between fitness and actual distance to target.
    /// Low correlation (<0.3) -> fitness is misleading -> try symbolic or LLM.
    pub deceptiveness: f64,
    /// Number of samples used to compute this profile.
    pub sample_count: u32,
}

impl LandscapeProfile {
    /// Recommend strategy based on landscape characteristics.
    pub fn recommended_strategy(&self) -> StrategyRecommendation {
        if self.gradient_magnitude > 0.01 && self.deceptiveness < 0.3 {
            StrategyRecommendation::Gradient
        } else if self.ruggedness < 0.3 && self.neutrality < 0.3 {
            StrategyRecommendation::Fuzz
        } else if self.neutrality > 0.5 {
            // Plateau: need symbolic to jump off
            StrategyRecommendation::Symbolic
        } else {
            StrategyRecommendation::LlmSynth
        }
    }
}

pub struct LandscapeAnalyzer {
    /// Cached profiles per branch
    profiles: HashMap<BranchId, LandscapeProfile>,
    /// Minimum samples before a profile is considered reliable
    min_samples: u32,
}

impl LandscapeAnalyzer {
    pub fn new(min_samples: u32) -> Self;

    /// Record a fitness observation for a branch.
    /// fitness = heuristic distance to flipping the branch [0, 1].
    pub fn record_observation(
        &mut self,
        branch: &BranchId,
        input_hash: u64,
        fitness: f64,
        neighbor_fitnesses: &[f64],
    );

    /// Get the landscape profile for a branch (if enough samples exist).
    pub fn profile(&self, branch: &BranchId) -> Option<&LandscapeProfile>;

    /// Get strategy recommendation for a branch based on landscape analysis.
    /// Returns None if insufficient samples.
    pub fn recommend(&self, branch: &BranchId) -> Option<StrategyRecommendation>;

    /// Compute ruggedness from a sequence of fitness values.
    fn compute_ruggedness(fitnesses: &[f64]) -> f64;

    /// Compute neutrality from a sequence of fitness values.
    fn compute_neutrality(fitnesses: &[f64], epsilon: f64) -> f64;
}
```

### Files to Create or Modify

| File | Action | Content |
|------|--------|---------|
| `src/landscape.rs` | **Create** | `LandscapeProfile`, `LandscapeAnalyzer`, ruggedness/neutrality/gradient computations |
| `src/priority.rs` | **Modify** | `BranchCandidate` gains optional `landscape: Option<LandscapeProfile>` |
| `src/router.rs` | **Modify** | `S2fRouter` consults landscape analyzer as additional signal alongside source-pattern classification |
| `src/orchestrator.rs` | **Modify** | After each execution, feed heuristic values into `LandscapeAnalyzer::record_observation()` |
| `src/lib.rs` | **Modify** | Add `pub mod landscape;` |

### Integration Points

- The orchestrator already computes per-branch heuristic values (from `CoverageOracle`). After mutation, the orchestrator records `(branch, input_hash, fitness, neighbor_fitnesses)` into the analyzer.
- Neighbor fitnesses come from the seed's mutation neighborhood -- the fuzzer mutates an input slightly and records the resulting fitness for each branch.
- The landscape recommendation integrates with `S2fRouter`: source-pattern classification provides static analysis, landscape analysis provides dynamic feedback. When both are available, landscape wins (empirical > static).
- Profiles are computed lazily (only after `min_samples` observations).

### Build Sequence

1. Implement `compute_ruggedness()` and `compute_neutrality()` as pure functions
2. Implement `LandscapeProfile` with `recommended_strategy()`
3. Implement `LandscapeAnalyzer` with `record_observation()` and `profile()`
4. Wire into orchestrator's post-execution path
5. Integrate with `S2fRouter` as a dynamic override

### Complexity Estimate

**Medium** -- ~300 LOC. The statistical computations (ruggedness, neutrality, gradient) are straightforward. The challenge is collecting enough samples without excessive overhead.

### Test Strategy

- Unit test `compute_ruggedness()`: monotonic sequence -> 0.0, alternating sequence -> 1.0
- Unit test `compute_neutrality()`: constant sequence -> 1.0, all-different sequence -> 0.0
- Unit test `recommended_strategy()`: high gradient -> Gradient, low ruggedness -> Fuzz, high neutrality -> Symbolic, else -> LlmSynth
- Unit test `min_samples` threshold: `profile()` returns None before threshold
- Unit test `record_observation()` accumulates samples correctly

---

## Cross-Cutting: Extended Types in `apex-core`

Several techniques require extending `ExplorationContext` and `BranchCandidate`. These changes go in `apex-core/src/types.rs` but are driven by `apex-agent` needs.

### `ExplorationContext` Extensions

```rust
pub struct ExplorationContext {
    pub target: Target,
    pub uncovered_branches: Vec<BranchId>,
    pub iteration: u64,
    // --- New fields ---
    pub covered_branches: Vec<BranchId>,        // needed for TransitionTable state hashing
    pub branch_heuristics: HashMap<BranchId, f64>, // needed for LandscapeAnalyzer
}
```

### `BranchCandidate` Extensions

```rust
pub struct BranchCandidate {
    pub id: BranchId,
    pub heuristic: f64,
    pub attempts_since_progress: u64,
    pub depth_in_cfg: u32,
    pub hit_count: u64,
    // --- New fields ---
    pub category: Option<BranchCategory>,        // from S2f classifier
    pub oracle_gap: Option<f64>,                 // from MutationGuide
    pub landscape: Option<LandscapeProfile>,     // from LandscapeAnalyzer
    pub transition_score: Option<f64>,           // from TransitionTable
}
```

### `StrategyRecommendation` Extensions

```rust
pub enum StrategyRecommendation {
    Gradient,
    Fuzz,
    LlmSynth,
    Symbolic,           // NEW: distinct from Gradient (Z3 path solving vs gradient nudging)
    AdversarialLoop,    // NEW: triggers adversarial test-mutant loop
}
```

---

## Module Dependency Graph

```
                    orchestrator.rs
                   /    |    \     \
                  /     |     \     \
           router.rs  bandit.rs  adversarial.rs  monitor.rs
              |          |            |
        classifier.rs    |       mutation_guide.rs
              |          |
        landscape.rs     |
                         |
                   transition.rs
                         |
                    scorer.rs
                         |
                   trace_filter.rs
```

All new modules depend on `apex-core::types` for `BranchId`, `InputSeed`, etc.
No circular dependencies are introduced.

---

## Build Sequence (Ordered by Dependencies)

### Phase A: Foundation Types (no inter-technique deps)

| Order | Module | Technique | Est. LOC | Deps |
|-------|--------|-----------|----------|------|
| A1 | `classifier.rs` | S2F | 150 | apex-core types |
| A2 | `bandit.rs` | Thompson | 150 | rand, rand_distr |
| A3 | `mutation_guide.rs` | ACH | 200 | apex-core types |
| A4 | `trace_filter.rs` | Trace-Guided DGF | 200 | apex-core types |
| A5 | `transition.rs` | DeepGo | 300 | apex-core types |
| A6 | `scorer.rs` | Graphuzz | 250 | apex-core types |

All Phase A modules can be built in parallel.

### Phase B: Composite Modules (depend on Phase A)

| Order | Module | Technique | Est. LOC | Deps |
|-------|--------|-----------|----------|------|
| B1 | `landscape.rs` | Fitness Landscape | 300 | priority.rs types |
| B2 | `router.rs` | S2F Router | 150 | classifier.rs, landscape.rs |
| B3 | `adversarial.rs` | AdverTest | 350 | mutation_guide.rs |

### Phase C: Orchestrator Integration (depends on all above)

| Order | Module | Change | Est. LOC |
|-------|--------|--------|----------|
| C1 | `priority.rs` | Extended BranchCandidate, new StrategyRecommendation variants | 50 |
| C2 | `orchestrator.rs` | Wire all modules into exploration loop | 200 |
| C3 | `monitor.rs` | Track mutation kill rate alongside coverage | 50 |

### Total Estimated LOC: ~2,350

---

## Cargo.toml Changes

```toml
[dependencies]
apex-core = { path = "../apex-core" }
apex-coverage = { path = "../apex-coverage" }
apex-symbolic = { path = "../apex-symbolic" }
tracing = "0.1"
futures = "0.3"
async-trait = "0.1"
rand = "0.8"           # NEW: Thompson sampling
rand_distr = "0.4"     # NEW: Beta distribution
uuid = { version = "1", features = ["v4"] }  # NEW: MutantSpec IDs
```

No heavy ML dependencies (tch, ort) are introduced. The GNN scorer (Graphuzz) uses a BFS heuristic approximation; a future `gnn` feature flag can add learned scoring.

---

## lib.rs After Integration

```rust
pub mod adversarial;
pub mod bandit;
pub mod cache;
pub mod classifier;
pub mod driller;
pub mod ensemble;
pub mod exchange;
pub mod landscape;
pub mod ledger;
pub mod monitor;
pub mod mutation_guide;
pub mod orchestrator;
pub mod priority;
pub mod router;
pub mod scorer;
pub mod source;
pub mod trace_filter;
pub mod transition;

pub use bandit::StrategyBandit;
pub use classifier::{BranchCategory, BranchClassifier};
pub use ledger::BugLedger;
pub use orchestrator::{AgentCluster, OrchestratorConfig};
pub use router::S2fRouter;
pub use source::{build_uncovered_with_lines, extract_source_contexts};
```

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| TransitionTable memory blowup | Configurable max entries + LRU eviction |
| Landscape analysis too slow (many samples needed) | Minimum sample threshold; lazy computation; sample from mutation neighborhood only |
| Bandit non-stationarity | Periodic decay of alpha/beta; reset on major coverage jumps |
| BranchClassifier accuracy | Start with regex patterns; upgrade to AST-level classification later |
| Adversarial loop cost (2-3 LLM rounds per region) | Budget-gated: only run on top-K oracle-gap regions per iteration |
| Trace filter false negatives (filtering out useful seeds) | Configurable overlap threshold; fallback to unfiltered when predictions are low-confidence |
| Integration complexity (8 modules in one orchestrator) | Feature flags per technique; each module has a clear enable/disable switch |
| Circular dependency between router and landscape | Landscape feeds into router, not vice versa; router is the final aggregation point |
