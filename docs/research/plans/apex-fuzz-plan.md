# apex-fuzz Research Integration Plan

Comprehensive implementation plan for integrating 10 research techniques into the
`apex-fuzz` crate. Each technique is specified with new types, files, integration
points, build dependencies, complexity estimate, and test strategy.

---

## Prerequisite Fix: observe() Corpus Feedback + mutate_with_index()

Before any technique integration, two bugs in the current `apex-fuzz` must be fixed.

### P1: observe() must feed corpus

**Problem:** `FuzzStrategy::observe()` in `lib.rs:160-172` logs new coverage but
never adds the winning input to the corpus. The comment says "the orchestrator
must call seed_corpus() separately" -- but no orchestrator does this, so the
fuzzer never learns from its own discoveries.

**Fix:** Add an `input: Vec<u8>` field to `ExecutionResult` (in `apex-core/src/types.rs`)
or, less invasively, add a side-channel `last_suggested: Mutex<HashMap<SeedId, Vec<u8>>>`
on `FuzzStrategy` that maps seed IDs to their data. In `observe()`:

```rust
async fn observe(&self, result: &ExecutionResult) -> Result<()> {
    if !result.new_branches.is_empty() {
        if let Some(data) = self.last_suggested.lock()?.remove(&result.seed_id) {
            let mut corpus = self.corpus.lock()?;
            corpus.add(data, result.new_branches.len());
        }
    }
    Ok(())
}
```

**Files:** `crates/apex-fuzz/src/lib.rs`, optionally `crates/apex-core/src/types.rs`

### P2: MOptScheduler needs mutate_with_index()

**Problem:** `MOptScheduler::mutate()` returns `Vec<u8>` but does not return the
mutator index used. Callers cannot call `report_hit(idx)` / `report_miss(idx)`
because they never learn which mutator was selected. The entire MOpt feedback
loop is broken.

**Fix:** Add a method that returns both the mutated data and the index:

```rust
pub fn mutate_with_index(&mut self, input: &[u8], rng: &mut dyn RngCore) -> (Vec<u8>, usize) {
    let idx = self.select(rng);
    self.stats[idx].applications += 1;
    (self.mutators[idx].mutate(input, rng), idx)
}
```

Then update `FuzzStrategy::mutate_one()` to store the index, and in `observe()`,
call `report_hit(idx)` or `report_miss(idx)` based on whether the seed found
new coverage.

**Files:** `crates/apex-fuzz/src/scheduler.rs`, `crates/apex-fuzz/src/lib.rs`

---

## Technique 1: Thompson Sampling Seed Scheduling

**Paper:** T-Scheduler (arXiv:2312.04749, AsiaCCS 2024)

**Concept:** Model seed selection as a Beta-Bernoulli multi-armed bandit. Each
corpus entry is an arm with parameters `(alpha, beta)`. To select a seed:
sample `theta_i ~ Beta(alpha_i, beta_i)` for each seed, pick `argmax(theta_i)`.
On new coverage: `alpha += 1`. On no new coverage: `beta += 1`. Zero
hyperparameters, constant-time overhead, theoretical regret bounds.

### New Types/Traits

```rust
// crates/apex-fuzz/src/thompson.rs

/// Per-seed Beta distribution parameters for Thompson sampling.
#[derive(Debug, Clone)]
pub struct SeedArm {
    pub alpha: f64,  // success count + 1 (prior)
    pub beta: f64,   // failure count + 1 (prior)
}

/// Thompson sampling seed scheduler.
/// Replaces energy-weighted sampling in Corpus::sample().
pub struct ThompsonScheduler {
    arms: Vec<SeedArm>,
}

impl ThompsonScheduler {
    pub fn new() -> Self;
    pub fn add_arm(&mut self);
    pub fn remove_arm(&mut self, idx: usize);
    pub fn select(&self, rng: &mut impl Rng) -> usize;
    pub fn report_success(&mut self, idx: usize);
    pub fn report_failure(&mut self, idx: usize);
    pub fn len(&self) -> usize;
}
```

### Files to Create or Modify

| File | Action |
|------|--------|
| `src/thompson.rs` | **Create** -- ThompsonScheduler, SeedArm |
| `src/lib.rs` | **Modify** -- add `pub mod thompson;`, add `thompson_scheduler` field to FuzzStrategy |
| `src/corpus.rs` | **Modify** -- add `sample_by_index(&self, idx) -> Option<&CorpusEntry>` method |
| `Cargo.toml` | **Modify** -- add `rand_distr = "0.4"` for Beta distribution sampling |

### Integration Points

- `FuzzStrategy::suggest_inputs()`: replace `corpus.sample(&mut rng)` with
  `thompson_scheduler.select(&mut rng)` then `corpus.sample_by_index(idx)`
- `FuzzStrategy::observe()`: call `thompson_scheduler.report_success(seed_idx)`
  when `new_branches` is non-empty, else `report_failure(seed_idx)`
- Must track which corpus index was used for each `SeedId` emitted (add
  `last_corpus_idx: Mutex<HashMap<SeedId, usize>>`)
- Thompson scheduler arms must stay in sync with corpus size (add/remove on
  corpus.add / corpus eviction)

### Build Sequence

1. Add `rand_distr` to Cargo.toml
2. Create `thompson.rs` with unit tests
3. Add `sample_by_index` to Corpus
4. Wire into FuzzStrategy behind a `SeedSchedule` enum (`Thompson | EnergyWeighted`)
5. Update observe() per prerequisite P1

### Complexity Estimate

**Low** -- ~120 lines of new code. Beta sampling is `rand_distr::Beta::new(alpha, beta).sample(rng)`.
The core algorithm is ~30 lines. Most work is plumbing the index through suggest/observe.

### Test Strategy

- Unit test: create scheduler with 3 arms, report 100 successes on arm 0 and
  100 failures on arms 1-2, verify arm 0 is selected >90% over 1000 trials
- Unit test: new arms start with uniform prior `(1, 1)`, verify roughly uniform
  selection before any feedback
- Integration test: run FuzzStrategy with Thompson scheduling, seed corpus,
  call suggest_inputs + observe in a loop, verify corpus grows
- Property test (proptest): for any sequence of success/failure reports, selected
  index is always in bounds

---

## Technique 2: Fuzzing as Stochastic Control (FOX)

**Paper:** FOX (arXiv:2406.04517)

**Concept:** Rearchitect the fuzzer loop as an optimal stochastic control problem.
The fuzzer state is a coverage bitmap + corpus. Actions are: which seed to pick,
which mutator to apply, how many mutations (energy). The controller uses a
lightweight model-predictive approach: estimate the expected coverage gain for
each (seed, mutator) pair, allocate energy proportional to expected marginal gain.

### New Types/Traits

```rust
// crates/apex-fuzz/src/control.rs

/// Estimated reward model for (seed, mutator) pairs.
pub struct RewardModel {
    /// Rows = corpus entries, Cols = mutator operators
    estimates: Vec<Vec<f64>>,
    /// Exponential decay for stale estimates
    decay: f64,
}

/// The FOX controller: selects (seed_idx, mutator_idx, energy) triples.
pub struct FoxController {
    reward_model: RewardModel,
    /// Coverage frontier size at last observation
    frontier_size: usize,
    /// Total mutations budget per iteration
    budget: usize,
}

/// A single action prescribed by the controller.
pub struct FoxAction {
    pub seed_idx: usize,
    pub mutator_idx: usize,
    pub energy: usize,  // number of mutations to apply
}

impl FoxController {
    pub fn new(budget: usize) -> Self;
    pub fn plan(&self, corpus_size: usize, n_mutators: usize, rng: &mut impl Rng) -> Vec<FoxAction>;
    pub fn observe(&mut self, action: &FoxAction, coverage_delta: usize);
    pub fn resize_corpus(&mut self, new_size: usize);
    pub fn resize_mutators(&mut self, new_count: usize);
}
```

### Files to Create or Modify

| File | Action |
|------|--------|
| `src/control.rs` | **Create** -- FoxController, RewardModel, FoxAction |
| `src/lib.rs` | **Modify** -- add `pub mod control;`, add `FoxFuzzStrategy` as alternative to `FuzzStrategy` |
| `src/scheduler.rs` | **Modify** -- expose mutator count and per-index mutation |
| `src/corpus.rs` | **Modify** -- add `get_by_index(idx) -> Option<&CorpusEntry>` |

### Integration Points

- FOX replaces the inner loop of `suggest_inputs()`. Instead of sampling one
  seed at a time, the controller produces a plan of `(seed, mutator, energy)`
  triples for the entire iteration budget.
- `MOptScheduler` becomes a subordinate: FOX calls `scheduler.mutators[idx].mutate()`
  directly, bypassing the weighted selection.
- `observe()` updates the FOX reward model instead of (or in addition to) the
  MOpt EMA.
- The coverage frontier size comes from `oracle.coverage_pct()` or a dedicated
  edge count method.

### Build Sequence

1. Create `control.rs` with RewardModel + FoxController
2. Add `plan()` method that uses softmax over reward estimates to allocate energy
3. Add `observe()` that updates per-(seed, mutator) estimates with exponential decay
4. Create `FoxFuzzStrategy` in lib.rs that wires controller to corpus + scheduler
5. Gate behind a runtime config flag (not a compile feature -- both strategies
   should coexist)

### Complexity Estimate

**High** -- ~400 lines. The reward model needs careful tuning of decay rates.
The plan() method must solve a lightweight allocation problem (proportional to
estimated marginal gain). Risk: tuning constants may need per-target calibration.

### Test Strategy

- Unit test: RewardModel with known estimates, verify plan() allocates more
  energy to high-reward pairs
- Unit test: after observe() with positive delta, the corresponding estimate
  increases; with zero delta, it decays
- Unit test: resize_corpus/resize_mutators grow/shrink the estimate matrix
  without panicking
- Deterministic integration test: fixed RNG seed, verify the controller
  converges to the optimal (seed, mutator) pair in a synthetic scenario
- Benchmark: plan() for 10,000-entry corpus x 7 mutators must complete in <1ms

---

## Technique 3: Differential Evolution Mutation Scheduling (DEzzer)

**Paper:** DEzzer (JSS 2025)

**Concept:** Use differential evolution (DE) to optimize the weight vector for
mutation operators. Each individual in the DE population is a weight vector
`w = [w_1, ..., w_k]` for k mutators. Fitness = coverage gained per time unit
when using those weights. DE mutation: `v = w_a + F*(w_b - w_c)`. DE crossover:
binomial. Selection: keep the fitter of parent vs trial.

### New Types/Traits

```rust
// crates/apex-fuzz/src/de_scheduler.rs

/// One individual in the DE population: a mutation weight vector.
#[derive(Debug, Clone)]
pub struct WeightVector {
    pub weights: Vec<f64>,
    pub fitness: f64,
}

/// Differential evolution optimizer for mutation scheduling weights.
pub struct DEScheduler {
    population: Vec<WeightVector>,
    /// DE parameters
    scale_factor: f64,  // F, typically 0.5-0.8
    crossover_rate: f64, // CR, typically 0.7-0.9
    /// Underlying mutators
    mutators: Vec<Box<dyn Mutator>>,
    /// Current active weight vector index
    active: usize,
    /// Coverage gained during current evaluation epoch
    epoch_coverage: usize,
    epoch_mutations: usize,
}

impl DEScheduler {
    pub fn new(mutators: Vec<Box<dyn Mutator>>, pop_size: usize) -> Self;
    pub fn select(&self, rng: &mut dyn RngCore) -> usize;
    pub fn mutate(&mut self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8>;
    pub fn report_coverage(&mut self, new_edges: usize);
    pub fn end_epoch(&mut self, rng: &mut dyn RngCore);
}
```

### Files to Create or Modify

| File | Action |
|------|--------|
| `src/de_scheduler.rs` | **Create** -- DEScheduler, WeightVector |
| `src/lib.rs` | **Modify** -- add `pub mod de_scheduler;`, add scheduler enum |

### Integration Points

- DEScheduler is a drop-in replacement for MOptScheduler in `FuzzStrategy`.
  Same interface: `select()`, `mutate()`, plus `report_coverage()` and `end_epoch()`.
- An "epoch" is N mutations (e.g., 1000). At epoch end, compute fitness for the
  active individual, run DE mutation/crossover/selection, advance to next individual.
- `FuzzStrategy::observe()` calls `de_scheduler.report_coverage(new_branches.len())`.
- At the end of each `suggest_inputs()` batch, check if epoch boundary crossed
  and call `end_epoch()`.

### Build Sequence

1. Create `de_scheduler.rs` with WeightVector and population initialization
2. Implement `select()` using the active individual's weight vector (same
   weighted random as MOptScheduler but with DE-optimized weights)
3. Implement `end_epoch()`: DE mutation, crossover, selection
4. Wire into FuzzStrategy via a scheduler enum or trait object
5. Add epoch tracking (mutation counter) to FuzzStrategy

### Complexity Estimate

**Medium** -- ~250 lines. DE is well-understood and straightforward to implement.
Population size 10-20 is sufficient. The main subtlety is choosing epoch length
and ensuring the fitness function is not too noisy.

### Test Strategy

- Unit test: initialize population, verify all weight vectors are valid
  (non-negative, sum > 0)
- Unit test: DE mutation produces a valid trial vector (weights clamped to [0, 1])
- Unit test: selection keeps the fitter individual
- Unit test: after many epochs with one mutator consistently producing coverage,
  its weight converges to near-maximum across the population
- Property test: for any population state and random epoch, no weight goes
  negative or NaN

---

## Technique 4: LLM Seed Generators (SeedMind)

**Paper:** SeedMind (arXiv:2411.18143)

**Concept:** Instead of having the LLM generate individual seed inputs, have it
generate *programs* that generate seeds. A seed generator program can produce
thousands of diverse inputs by varying parameters. The LLM sees the target's
input format specification and coverage feedback, then writes a generator
function. Much higher seed diversity than generating inputs one at a time.

### New Types/Traits

```rust
// crates/apex-fuzz/src/seedmind.rs

/// A seed generator program produced by an LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedGenerator {
    /// Source code of the generator (Python or Rust)
    pub code: String,
    /// Language the generator is written in
    pub language: GeneratorLanguage,
    /// Number of seeds this generator has produced
    pub seeds_produced: u64,
    /// Coverage gained from seeds produced by this generator
    pub coverage_gain: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum GeneratorLanguage {
    Python,
    Rust,
}

/// Configuration for the SeedMind LLM interaction.
pub struct SeedMindConfig {
    /// Maximum number of generator programs to maintain
    pub max_generators: usize,
    /// Number of seeds to sample per generator per round
    pub seeds_per_generator: usize,
    /// Input format description (provided by user or inferred)
    pub format_spec: String,
}

/// Trait for executing seed generator programs in a sandbox.
#[async_trait]
pub trait GeneratorExecutor: Send + Sync {
    /// Execute a generator program and return the seeds it produces.
    async fn execute(&self, generator: &SeedGenerator, count: usize) -> Result<Vec<Vec<u8>>>;
}

/// The SeedMind coordinator.
pub struct SeedMind {
    config: SeedMindConfig,
    generators: Vec<SeedGenerator>,
    executor: Box<dyn GeneratorExecutor>,
}

impl SeedMind {
    pub fn new(config: SeedMindConfig, executor: Box<dyn GeneratorExecutor>) -> Self;
    /// Ask the LLM to generate a new seed generator program.
    pub async fn create_generator(&mut self, coverage_feedback: &str) -> Result<SeedGenerator>;
    /// Run all generators and collect seeds.
    pub async fn generate_seeds(&self) -> Result<Vec<Vec<u8>>>;
    /// Report coverage results to rank generators.
    pub fn report_coverage(&mut self, generator_idx: usize, new_edges: usize);
    /// Prune low-performing generators, request new ones.
    pub async fn evolve(&mut self, coverage_feedback: &str) -> Result<()>;
}
```

### Files to Create or Modify

| File | Action |
|------|--------|
| `src/seedmind.rs` | **Create** -- SeedMind, SeedGenerator, SeedMindConfig, GeneratorExecutor trait |
| `src/lib.rs` | **Modify** -- add `pub mod seedmind;` |
| `Cargo.toml` | **Modify** -- add `serde_json = "1"` (for LLM API interaction) |

### Integration Points

- SeedMind integrates at the corpus seeding layer, not the mutation layer. It
  produces initial seeds that then enter the standard mutation pipeline.
- `FuzzStrategy::suggest_inputs()` calls `seedmind.generate_seeds()` when the
  corpus is stale (no new coverage for N iterations).
- The LLM interaction uses the same `apex-agent` LLM client infrastructure
  (prompt construction, API calls). SeedMind provides the prompt templates;
  the actual LLM call is injected via a trait.
- Generator programs run in `apex-sandbox` for isolation.
- Coverage feedback is formatted as: "Generator G produced N seeds, K found
  new coverage. Uncovered branches: [list]. Generate a better generator."

### Build Sequence

1. Create `seedmind.rs` with types and GeneratorExecutor trait
2. Implement prompt templates for generator creation and evolution
3. Implement SeedMind coordinator with generator lifecycle management
4. Add sandbox-based GeneratorExecutor implementation (shells out to Python)
5. Wire into FuzzStrategy as an optional seeding source
6. Add LLM client dependency (feature-gated: `seedmind = ["apex-agent"]`)

### Complexity Estimate

**High** -- ~500 lines in apex-fuzz, plus integration with apex-agent for LLM
calls and apex-sandbox for generator execution. The core logic is ~200 lines;
the rest is plumbing. Main risk: generator program quality depends heavily on
prompt engineering.

### Test Strategy

- Unit test: SeedMind with a mock GeneratorExecutor that returns fixed seeds,
  verify generate_seeds() collects them
- Unit test: report_coverage updates generator stats, evolve() prunes generators
  with zero coverage gain
- Unit test: prompt template includes coverage feedback and format spec
- Integration test: end-to-end with a simple target (e.g., "accepts JSON with
  field 'x' > 100"), verify generators produce valid JSON
- Mock test: verify LLM prompt format matches expected structure

---

## Technique 5: Format-Aware LLM Mutations (LLAMAFUZZ)

**Paper:** LLAMAFUZZ (arXiv:2406.07714)

**Concept:** Use a fine-tuned LLM to mutate structured inputs (JSON, XML, SQL,
protocol buffers) while preserving format validity. The LLM understands the
grammar and can make semantically meaningful mutations that random byte-level
mutators cannot. +41 bugs over AFL++ on structured targets.

### New Types/Traits

```rust
// crates/apex-fuzz/src/llm_mutator.rs

/// Configuration for LLM-based mutation.
#[derive(Debug, Clone)]
pub struct LlmMutatorConfig {
    /// Input format (json, xml, sql, protobuf, custom)
    pub format: InputFormat,
    /// Whether to validate output format before returning
    pub validate_output: bool,
    /// Maximum retries on format validation failure
    pub max_retries: usize,
    /// Temperature for LLM sampling (higher = more diverse)
    pub temperature: f64,
}

#[derive(Debug, Clone)]
pub enum InputFormat {
    Json,
    Xml,
    Sql,
    Protobuf,
    Custom(String),  // grammar spec or example
}

/// LLM-based mutator implementing the Mutator trait.
/// Uses an LLM to produce format-aware mutations.
pub struct LlmMutator {
    config: LlmMutatorConfig,
    /// Handle to the LLM client (injected)
    llm: Arc<dyn LlmClient>,
    /// Cache of recent mutations to avoid duplicates
    cache: Mutex<lru::LruCache<u64, Vec<u8>>>,
}

/// Trait for LLM interaction (mockable for testing).
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn mutate_input(&self, input: &str, format: &InputFormat, temperature: f64)
        -> Result<String>;
}

impl Mutator for LlmMutator { ... }
```

### Files to Create or Modify

| File | Action |
|------|--------|
| `src/llm_mutator.rs` | **Create** -- LlmMutator, LlmMutatorConfig, InputFormat, LlmClient trait |
| `src/lib.rs` | **Modify** -- add `pub mod llm_mutator;` |
| `src/plugin.rs` | **Modify** -- allow registering LlmMutator in MutatorRegistry |
| `Cargo.toml` | **Modify** -- add `lru = "0.12"` for mutation cache |

### Integration Points

- LlmMutator implements `Mutator` trait, so it plugs directly into MOptScheduler
  or DEScheduler as an additional operator alongside the 7 byte-level mutators.
- The `mutate()` method is synchronous (Mutator trait requirement), so it must
  use `tokio::runtime::Handle::current().block_on()` for the async LLM call,
  or pre-compute a batch of mutations asynchronously and serve from a buffer.
  **Preferred approach:** batch pre-computation. Add `prefetch()` async method
  that fills an internal buffer, then `mutate()` pops from the buffer.
- Format detection: if the corpus contains valid JSON/XML, auto-detect and
  enable LlmMutator for that format.
- Fallback: if the LLM is unavailable or returns invalid format, fall through
  to byte-level mutation.

### Build Sequence

1. Create `llm_mutator.rs` with LlmClient trait and LlmMutatorConfig
2. Implement the batch prefetch pattern (async fill, sync drain)
3. Implement format validation per InputFormat
4. Register LlmMutator as optional mutator in MOptScheduler
5. Add auto-detection of input format from corpus samples
6. Feature-gate LLM dependency: `llm-mutator = ["apex-agent"]`

### Complexity Estimate

**High** -- ~350 lines. The async-to-sync bridge is the hardest part. The
prefetch buffer adds complexity but is essential for performance. Format
validation is straightforward for JSON/XML but needs work for custom formats.

### Test Strategy

- Unit test: LlmMutator with mock LlmClient, verify it returns the mock's output
- Unit test: when LLM returns invalid format and validate_output=true, the
  mutator retries up to max_retries then falls back to input passthrough
- Unit test: prefetch buffer fills asynchronously, mutate() drains synchronously
- Unit test: LRU cache prevents duplicate mutations
- Property test: for any valid JSON input, the mutator always returns valid JSON
  (with mock LLM that returns valid JSON)

---

## Technique 6: Rust Grammar-Based Fuzzing (FANDANGO-RS)

**Paper:** FANDANGO-RS (arXiv:2511.05987)

**Concept:** Grammar-constrained fuzzing that generates inputs from a CFG with
coverage guidance. The key insight: use the grammar to *constrain* mutations so
they always produce syntactically valid inputs. 3-4 orders of magnitude faster
than unconstrained fuzzing for grammar-heavy targets. Uses the existing
`grammar.rs` module as foundation.

### New Types/Traits

```rust
// crates/apex-fuzz/src/grammar_mutator.rs

/// A grammar-aware mutator that performs tree-level mutations on parse trees.
pub struct GrammarMutator {
    grammar: Grammar,
    /// Parse tree of the current best input (for subtree replacement)
    current_tree: Option<ParseNode>,
}

/// Grammar mutation operations (tree-level, not byte-level).
pub enum GrammarMutation {
    /// Replace a random subtree with a freshly generated one.
    SubtreeRegeneration,
    /// Swap two subtrees rooted at the same non-terminal.
    SubtreeSwap,
    /// Splice subtrees from two different inputs.
    SubtreeSplice,
    /// Minimize a subtree (pick shortest alternative).
    SubtreeMinimize,
}

impl GrammarMutator {
    pub fn new(grammar: Grammar) -> Self;
    /// Parse input bytes into a parse tree (best-effort).
    pub fn parse(&self, input: &[u8]) -> Option<ParseNode>;
    /// Apply a grammar-aware mutation to a parse tree.
    pub fn mutate_tree(&self, tree: &ParseNode, op: GrammarMutation, rng: &mut impl Rng) -> ParseNode;
    /// Flatten a parse tree back to bytes.
    pub fn flatten(tree: &ParseNode) -> Vec<u8>;
}

impl Mutator for GrammarMutator { ... }

/// Coverage-guided grammar fuzzer that combines grammar generation with
/// feedback-directed tree mutations.
pub struct GrammarFuzzer {
    grammar: Grammar,
    mutator: GrammarMutator,
    /// Parse tree corpus (trees, not just byte vectors)
    tree_corpus: Vec<ParseNode>,
    max_corpus: usize,
}
```

### Files to Create or Modify

| File | Action |
|------|--------|
| `src/grammar_mutator.rs` | **Create** -- GrammarMutator, GrammarMutation, GrammarFuzzer |
| `src/grammar.rs` | **Modify** -- add `flatten()` method to ParseNode, add parser (input -> ParseNode) |
| `src/lib.rs` | **Modify** -- add `pub mod grammar_mutator;` |

### Integration Points

- GrammarMutator implements `Mutator` trait, pluggable into any scheduler.
- GrammarFuzzer is a standalone strategy (implements `Strategy` trait) for
  targets where a grammar is known.
- Grammar definitions come from:
  1. User-provided grammar files (BNF/EBNF format)
  2. Grammar inference from corpus samples (future work)
  3. Built-in grammars for common formats (JSON, XML, HTML, SQL)
- Coverage feedback directs which subtrees to regenerate: subtrees covering
  uncovered non-terminals get higher regeneration priority.

### Build Sequence

1. Add `flatten()` to ParseNode in `grammar.rs`
2. Implement best-effort parser (input -> ParseNode) in `grammar.rs`
3. Create `grammar_mutator.rs` with tree-level mutation operations
4. Implement GrammarMutator as Mutator trait
5. Create GrammarFuzzer as a Strategy with tree-level corpus
6. Add built-in JSON grammar as first concrete grammar

### Complexity Estimate

**Medium-High** -- ~450 lines. The parser (input -> parse tree) is the hardest
part. For ambiguous grammars, a best-effort approach (greedy top-down) is
sufficient since the primary use case is *mutation*, not exact parsing. Tree
mutation operations themselves are straightforward.

### Test Strategy

- Unit test: simple arithmetic grammar, generate -> flatten -> parse roundtrip
  produces equivalent tree
- Unit test: each GrammarMutation variant produces valid output (re-parseable
  by the same grammar)
- Unit test: GrammarFuzzer with JSON grammar produces valid JSON
- Unit test: SubtreeSwap only swaps nodes with matching non-terminal types
- Property test: for any generated tree, flatten produces a string accepted by
  the grammar
- Benchmark: grammar-constrained generation vs byte-level mutation, measure
  parse success rate

---

## Technique 7: Universal Autoprompting (Fuzz4All)

**Paper:** Fuzz4All (ICSE 2024, arXiv:2308.04748)

**Concept:** Language-agnostic fuzzing via LLM autoprompting. Given a target
system (compiler, interpreter, library), Fuzz4All uses an "autoprompt" phase
where the LLM generates diverse prompts for itself, then uses those prompts to
generate test inputs. The autoprompt is iteratively refined based on coverage
feedback. Works for any language with zero manual grammar writing.

### New Types/Traits

```rust
// crates/apex-fuzz/src/autoprompt.rs

/// An autoprompt: a meta-prompt that instructs the LLM how to generate inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoPrompt {
    pub text: String,
    /// Coverage achieved by inputs generated from this prompt
    pub coverage_score: f64,
    /// Number of times this prompt has been used
    pub usage_count: u64,
}

/// Autoprompt evolution strategy.
pub struct AutoPromptEvolver {
    /// Current pool of autoprompts
    prompts: Vec<AutoPrompt>,
    /// Maximum pool size
    max_prompts: usize,
    /// Generation counter
    generation: u64,
}

impl AutoPromptEvolver {
    pub fn new(max_prompts: usize) -> Self;
    /// Generate initial autoprompts from a target description.
    pub async fn initialize(&mut self, target_description: &str, llm: &dyn LlmClient) -> Result<()>;
    /// Select the best autoprompt for the next generation round.
    pub fn select(&self, rng: &mut impl Rng) -> &AutoPrompt;
    /// Evolve: mutate high-performing prompts, prune low-performing ones.
    pub async fn evolve(&mut self, llm: &dyn LlmClient, rng: &mut impl Rng) -> Result<()>;
    /// Report coverage feedback for a prompt.
    pub fn report_coverage(&mut self, prompt_idx: usize, coverage_delta: f64);
}

/// Fuzz4All strategy: autoprompt-driven input generation.
pub struct Fuzz4AllStrategy {
    evolver: AutoPromptEvolver,
    llm: Arc<dyn LlmClient>,
    /// Target description (language, API surface, input format)
    target_desc: String,
}

#[async_trait]
impl Strategy for Fuzz4AllStrategy { ... }
```

### Files to Create or Modify

| File | Action |
|------|--------|
| `src/autoprompt.rs` | **Create** -- AutoPrompt, AutoPromptEvolver, Fuzz4AllStrategy |
| `src/llm_mutator.rs` | **Modify** -- reuse LlmClient trait (or extract to shared module) |
| `src/lib.rs` | **Modify** -- add `pub mod autoprompt;` |

### Integration Points

- Fuzz4AllStrategy implements `Strategy` trait, coexists with FuzzStrategy as
  an alternative.
- The autoprompt evolver uses Thompson sampling (technique 1) to select among
  prompts -- natural reuse.
- Target description comes from `ExplorationContext.target` (language, root
  path) plus user-provided documentation.
- Generated inputs feed into the standard coverage oracle for feedback.
- Can run in parallel with byte-level fuzzing: Fuzz4All generates high-level
  valid inputs, byte-level fuzzer mutates them for edge exploration.

### Build Sequence

1. Extract LlmClient trait to a shared location (or keep in llm_mutator.rs)
2. Create `autoprompt.rs` with AutoPrompt and AutoPromptEvolver
3. Implement prompt mutation strategies (append constraint, change style, etc.)
4. Implement Fuzz4AllStrategy with suggest_inputs/observe
5. Feature-gate: `autoprompt = ["apex-agent"]`

### Complexity Estimate

**Medium** -- ~300 lines. The core autoprompt evolution is simple (tournament
selection + LLM-based mutation of prompt text). The main challenge is prompt
engineering for the "generate an autoprompt" meta-prompt.

### Test Strategy

- Unit test: AutoPromptEvolver with mock LLM, verify initialization creates
  `max_prompts` prompts
- Unit test: select() favors prompts with higher coverage_score
- Unit test: evolve() prunes the lowest-scoring prompt and adds a new one
- Unit test: Fuzz4AllStrategy produces InputSeeds from LLM output
- Integration test: with a toy target (Python function that checks string
  format), verify Fuzz4All achieves higher coverage than random generation

---

## Technique 8: Directed Greybox Fuzzing (HGFuzzer)

**Paper:** HGFuzzer (arXiv:2505.03425)

**Concept:** Use an LLM to solve path constraints as code. Given an uncovered
branch and the path condition to reach it, the LLM generates a program fragment
that constructs an input satisfying the constraint. Unlike symbolic execution
(which struggles with string operations), LLMs handle complex constraints
naturally. Combines with directed fuzzing (`directed.rs`) for target-aware
energy allocation.

### New Types/Traits

```rust
// crates/apex-fuzz/src/hg_directed.rs

/// A path constraint extracted from the target program.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathConstraint {
    /// Source location of the branch
    pub branch_id: BranchId,
    /// Human-readable constraint description
    pub constraint: String,
    /// Code context around the branch
    pub context: String,
    /// Call stack to reach this branch
    pub call_stack: Vec<String>,
}

/// An LLM-generated input constructor for a specific path constraint.
#[derive(Debug, Clone)]
pub struct ConstraintSolution {
    /// The generated input bytes
    pub input: Vec<u8>,
    /// Confidence score from the LLM
    pub confidence: f64,
    /// The constraint this solves
    pub target_branch: BranchId,
}

/// HGFuzzer: directed fuzzing with LLM-based constraint solving.
pub struct HGFuzzer {
    /// Pending constraints to solve
    constraints: Vec<PathConstraint>,
    /// Solutions generated by LLM
    solutions: Vec<ConstraintSolution>,
    /// Directed energy parameters (reuses directed.rs)
    temperature: f64,
    total_iterations: u64,
}

impl HGFuzzer {
    pub fn new(total_iterations: u64) -> Self;
    /// Extract path constraints from uncovered branches.
    pub fn extract_constraints(&mut self, uncovered: &[BranchId], oracle: &CoverageOracle);
    /// Ask LLM to solve constraints and generate inputs.
    pub async fn solve_constraints(&mut self, llm: &dyn LlmClient) -> Result<Vec<ConstraintSolution>>;
    /// Get directed energy for a corpus entry.
    pub fn energy(&self, entry: &CorpusEntry, iteration: u64) -> f64;
}
```

### Files to Create or Modify

| File | Action |
|------|--------|
| `src/hg_directed.rs` | **Create** -- HGFuzzer, PathConstraint, ConstraintSolution |
| `src/directed.rs` | **Modify** -- extract shared energy/temperature types for reuse |
| `src/lib.rs` | **Modify** -- add `pub mod hg_directed;` |
| `src/corpus.rs` | **Modify** -- add `target_branches` field to CorpusEntry |

### Integration Points

- HGFuzzer wraps the existing `directed_energy()` and `temperature()` functions
  from `directed.rs`.
- PathConstraint extraction depends on `apex-coverage` (branch proximity data)
  and `apex-cpg` (code context around branches). Initially, constraints can be
  extracted from source code via pattern matching; full CPG integration comes later.
- LLM constraint solving reuses the LlmClient trait from technique 5.
- Solutions are injected into the corpus as high-priority seeds with
  `target_branches` set, so the directed energy function gives them a boost.
- `FuzzStrategy::suggest_inputs()` mixes HGFuzzer solutions with regular
  mutations when in directed mode.

### Build Sequence

1. Add `target_branches` field to CorpusEntry
2. Extract shared types from `directed.rs` if needed
3. Create `hg_directed.rs` with PathConstraint and extraction logic
4. Implement LLM constraint solving with prompt template
5. Wire solutions into corpus with appropriate directed energy
6. Feature-gate LLM parts: `hg-directed = ["apex-agent"]`

### Complexity Estimate

**Medium-High** -- ~350 lines. Constraint extraction from source is the hardest
part without full CPG support. LLM prompt for constraint solving is relatively
straightforward ("Given this branch condition and context, generate an input
that takes the true branch").

### Test Strategy

- Unit test: extract_constraints from synthetic branch conditions
- Unit test: energy function gives higher energy to solutions targeting nearby
  branches
- Unit test: solve_constraints with mock LLM returns valid ConstraintSolutions
- Unit test: solutions added to corpus have correct target_branches
- Integration test: directed fuzzing with HGFuzzer finds a branch guarded by
  a specific string comparison faster than undirected fuzzing

---

## Technique 9: Semantic Feedback Signals

**Paper:** arXiv:2511.03995

**Concept:** Go beyond edge coverage as the sole feedback signal. Track
additional semantic signals: exception/error types, output patterns (stdout/stderr
hashes), memory allocation patterns, execution timing. Use these as additional
dimensions in the coverage bitmap, so inputs that trigger new error types or
output patterns are kept even if they do not increase edge coverage.

### New Types/Traits

```rust
// crates/apex-fuzz/src/semantic_feedback.rs

/// A semantic feedback signal extracted from an execution.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum SemanticSignal {
    /// Exception/error type observed (e.g., "ValueError", "SIGSEGV")
    ExceptionType(String),
    /// Hash of stdout pattern (first 256 bytes)
    OutputPattern(u64),
    /// Hash of stderr pattern (first 256 bytes)
    ErrorPattern(u64),
    /// Execution time bucket (log2 of milliseconds)
    TimingBucket(u8),
    /// Return code
    ExitCode(i32),
}

/// Tracks which semantic signals have been seen.
pub struct SemanticOracle {
    seen: HashSet<SemanticSignal>,
}

impl SemanticOracle {
    pub fn new() -> Self;
    /// Extract semantic signals from an execution result.
    pub fn extract(&self, result: &ExecutionResult) -> Vec<SemanticSignal>;
    /// Report signals; returns the subset that are novel.
    pub fn report(&mut self, signals: Vec<SemanticSignal>) -> Vec<SemanticSignal>;
    /// Number of unique signals seen.
    pub fn signal_count(&self) -> usize;
}

/// Extended interestingness check: edge coverage OR novel semantic signals.
pub fn is_interesting(
    edge_coverage_new: bool,
    semantic_signals_new: &[SemanticSignal],
) -> bool {
    edge_coverage_new || !semantic_signals_new.is_empty()
}
```

### Files to Create or Modify

| File | Action |
|------|--------|
| `src/semantic_feedback.rs` | **Create** -- SemanticSignal, SemanticOracle, is_interesting() |
| `src/lib.rs` | **Modify** -- add `pub mod semantic_feedback;`, update observe() to check semantic signals |
| `src/corpus.rs` | **Modify** -- add `semantic_signals: Vec<SemanticSignal>` to CorpusEntry |

### Integration Points

- SemanticOracle sits alongside CoverageOracle. `FuzzStrategy::observe()` calls
  both: `oracle.check_coverage()` for edge coverage and
  `semantic_oracle.report(extract(result))` for semantic signals.
- A seed is "interesting" (added to corpus) if it produces new edge coverage
  **OR** new semantic signals.
- `cmplog.rs` already parses stdout/stderr for comparison hints; semantic
  feedback extraction can reuse the same parsing infrastructure.
- Semantic signals are extracted from `ExecutionResult.stdout`, `.stderr`,
  `.status`, and `.duration_ms`.
- Signal hashing uses `std::hash::DefaultHasher` for fast, non-cryptographic
  output fingerprinting.

### Build Sequence

1. Create `semantic_feedback.rs` with SemanticSignal enum and SemanticOracle
2. Implement signal extraction from ExecutionResult
3. Add `semantic_signals` field to CorpusEntry
4. Update `FuzzStrategy::observe()` to use is_interesting()
5. Update `FuzzStrategy::new()` to create SemanticOracle
6. Add semantic signal count to stats/logging

### Complexity Estimate

**Low-Medium** -- ~200 lines. Signal extraction is straightforward pattern
matching. The main design decision is which signals to track and how to hash
them. No external dependencies needed.

### Test Strategy

- Unit test: extract signals from a synthetic ExecutionResult with known
  stdout/stderr/exit code, verify expected signals
- Unit test: SemanticOracle.report() returns novel signals on first call,
  empty on duplicate
- Unit test: is_interesting() returns true when only semantic signals are new
- Unit test: timing bucket calculation (log2 of duration_ms)
- Unit test: ExceptionType extraction from stderr patterns ("TypeError:",
  "panic:", "SIGSEGV")
- Integration test: fuzzer keeps a seed that triggers a new error type even
  when edge coverage is unchanged

---

## Technique 10: LibAFL QEMU Binary Fuzzing

**Paper:** BAR 2024 — LibAFL QEMU

**Concept:** Use LibAFL's QEMU-based emulation backend for fuzzing binary-only
targets (no source, no instrumentation). LibAFL QEMU provides:
- Full-system or usermode emulation
- Edge coverage via basic block translation hooks
- CmpLog via comparison instruction hooks
- Snapshot-based fork server for fast reset

### New Types/Traits

```rust
// crates/apex-fuzz/src/qemu_backend.rs (feature-gated: libafl-qemu)

/// Configuration for QEMU-based binary fuzzing.
#[derive(Debug, Clone)]
pub struct QemuFuzzerConfig {
    /// Path to the target binary
    pub binary: PathBuf,
    /// Command-line arguments (use @@ for input file placeholder)
    pub args: Vec<String>,
    /// QEMU mode: usermode or full-system
    pub mode: QemuMode,
    /// Coverage map size
    pub map_size: usize,
    /// Timeout per execution (milliseconds)
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub enum QemuMode {
    Usermode,
    FullSystem { kernel: PathBuf, rootfs: PathBuf },
}

/// QEMU-backed fuzzer using LibAFL's emulation layer.
pub struct QemuFuzzer {
    config: QemuFuzzerConfig,
    // Internal libafl-qemu state (opaque, feature-gated)
}

impl QemuFuzzer {
    pub fn new(config: QemuFuzzerConfig) -> Result<Self>;
    /// Run a single input through the emulator, return coverage.
    pub fn execute(&mut self, input: &[u8]) -> Result<QemuExecResult>;
    /// Get the current coverage bitmap.
    pub fn coverage_map(&self) -> &[u8];
    /// Get CmpLog entries from the last execution.
    pub fn cmp_log(&self) -> Vec<CmpEntry>;
}

pub struct QemuExecResult {
    pub exit_status: i32,
    pub new_edges: usize,
    pub timeout: bool,
    pub crash: bool,
    pub signal: Option<i32>,
}

/// Strategy wrapper for QEMU-based fuzzing.
pub struct QemuFuzzStrategy {
    fuzzer: QemuFuzzer,
    corpus: Mutex<Corpus>,
    scheduler: Mutex<MOptScheduler>,
    rng: Mutex<StdRng>,
}

#[async_trait]
impl Strategy for QemuFuzzStrategy { ... }
```

### Files to Create or Modify

| File | Action |
|------|--------|
| `src/qemu_backend.rs` | **Create** -- QemuFuzzer, QemuFuzzerConfig, QemuMode, QemuFuzzStrategy |
| `src/lib.rs` | **Modify** -- add `pub mod qemu_backend;` (feature-gated) |
| `src/cmplog.rs` | **Modify** -- reuse CmpEntry type (may need to make it pub(crate) or re-export) |
| `Cargo.toml` | **Modify** -- add libafl_qemu dependency (feature-gated) |

### Integration Points

- QemuFuzzStrategy implements `Strategy` trait, so it integrates with the
  existing agent loop in `apex-agent`.
- Coverage feedback from QEMU's edge bitmap feeds into `CoverageOracle` (map
  edge IDs from QEMU block addresses to APEX BranchIds via a translation table).
- CmpLog data from QEMU comparison hooks feeds into `CmpLogMutator` from
  `cmplog.rs` for input-to-state mutation.
- The QEMU executor replaces the sandbox execution path for binary targets.
- Configuration flows from CLI: `apex run --target ./binary --lang binary`
  selects QemuFuzzStrategy.

### Build Sequence

1. Add `libafl_qemu` feature to Cargo.toml with appropriate dependency
2. Create `qemu_backend.rs` with config types
3. Implement QemuFuzzer::new() that initializes QEMU emulator
4. Implement execute() with coverage map and CmpLog extraction
5. Implement QemuFuzzStrategy with suggest_inputs/observe
6. Add `Language::Binary` variant to apex-core if not present
7. Wire into CLI argument parsing

### Complexity Estimate

**Very High** -- ~600 lines, plus significant integration work. LibAFL QEMU has
a complex setup (emulator initialization, hook registration, snapshot management).
Platform-specific: QEMU usermode works on Linux only (not macOS). Requires
`libafl_qemu` crate which pulls in QEMU as a build dependency (~10 min compile).

### Test Strategy

- Unit test: QemuFuzzerConfig serialization/deserialization
- Unit test: QemuExecResult fields correctly populated from mock execution
- Integration test (Linux CI only): fuzz a simple binary (e.g., compiled from
  `void target(char* s) { if(s[0]=='F' && s[1]=='U' && s[2]=='Z' && s[3]=='Z') abort(); }`)
  and verify crash discovery
- Test: coverage bitmap is non-zero after executing a reachable path
- Test: CmpLog entries are populated after executing code with comparisons
- Feature gate test: crate compiles cleanly without `libafl-qemu` feature

---

## Dependency Graph

```
Prerequisites (P1, P2) ──> Everything
                  │
                  ├──> T1 (Thompson)  ──> T7 (Fuzz4All, reuses Thompson for prompt selection)
                  │
                  ├──> T9 (Semantic Feedback) [no external deps]
                  │
                  ├──> T3 (DEzzer) [no external deps]
                  │
                  ├──> T2 (FOX) [depends on T1 or T3 for comparison]
                  │
                  ├──> T6 (FANDANGO-RS) [extends grammar.rs]
                  │
                  ├──> T5 (LLAMAFUZZ) ──┐
                  │                     ├──> shared LlmClient trait
                  ├──> T4 (SeedMind)  ──┘
                  │
                  ├──> T8 (HGFuzzer) [needs LlmClient + directed.rs]
                  │
                  └──> T10 (LibAFL QEMU) [independent, heavy build dep]
```

## Recommended Build Order

| Phase | Techniques | Rationale |
|-------|-----------|-----------|
| **0 (Fix)** | P1 + P2 | Unblock all feedback-dependent techniques |
| **1 (Quick Wins)** | T1 (Thompson) + T9 (Semantic) + T3 (DEzzer) | No external deps, immediate fuzzer improvement |
| **2 (Grammar)** | T6 (FANDANGO-RS) | Extends existing grammar.rs, no LLM needed |
| **3 (LLM Foundation)** | T5 (LLAMAFUZZ) + T7 (Fuzz4All) | Establish LlmClient trait, then both use it |
| **4 (LLM Advanced)** | T4 (SeedMind) + T8 (HGFuzzer) | Build on LlmClient + coverage feedback |
| **5 (Control Theory)** | T2 (FOX) | Requires mature feedback pipeline from phases 0-1 |
| **6 (Binary)** | T10 (LibAFL QEMU) | Independent track, heavy compile cost, Linux-only |

## Summary Table

| # | Technique | New Files | Lines (est) | External Deps | Complexity |
|---|-----------|-----------|-------------|---------------|------------|
| P | Prerequisites | 0 | ~50 | none | Low |
| 1 | Thompson Sampling | 1 | ~120 | rand_distr | Low |
| 2 | FOX Control | 1 | ~400 | none | High |
| 3 | DEzzer | 1 | ~250 | none | Medium |
| 4 | SeedMind | 1 | ~500 | serde_json, apex-agent | High |
| 5 | LLAMAFUZZ | 1 | ~350 | lru, apex-agent | High |
| 6 | FANDANGO-RS | 1 | ~450 | none | Medium-High |
| 7 | Fuzz4All | 1 | ~300 | apex-agent | Medium |
| 8 | HGFuzzer | 1 | ~350 | apex-agent | Medium-High |
| 9 | Semantic Feedback | 1 | ~200 | none | Low-Medium |
| 10 | LibAFL QEMU | 1 | ~600 | libafl_qemu | Very High |
| **Total** | | **10 new files** | **~3,570** | | |

## Feature Gate Strategy

All LLM-dependent techniques (4, 5, 7, 8) should be feature-gated to avoid
pulling in `apex-agent` (and transitively, HTTP client, tokio, etc.) for users
who only want byte-level fuzzing:

```toml
[features]
default = []
libafl-backend = ["libafl", "libafl_bolts"]
libafl-qemu = ["libafl", "libafl_bolts", "libafl_qemu"]
llm-fuzz = []  # enables LLM mutator, SeedMind, Fuzz4All, HGFuzzer
thompson = ["rand_distr"]
```

Techniques 1 (Thompson), 2 (FOX), 3 (DEzzer), 6 (FANDANGO-RS), and 9 (Semantic)
compile unconditionally with zero new heavy dependencies.
