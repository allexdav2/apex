# APEX Security Analysis Integration Plan

Compiled: 2026-03-14
Scope: 11 research techniques integrated into `apex-cpg` and `apex-detect`.

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Shared Types and Traits](#shared-types-and-traits)
3. [Technique 1: LLM-Inferred Taint Specifications (IRIS)](#technique-1-llm-inferred-taint-specifications)
4. [Technique 2: CPG-Guided LLM Slicing (LLMxCPG)](#technique-2-cpg-guided-llm-slicing)
5. [Technique 3: Heterogeneous GNN on CPGs (IPAG/HAGNN)](#technique-3-heterogeneous-gnn-on-cpgs)
6. [Technique 4: LM + GNN Knowledge Distillation (Vul-LMGNNs)](#technique-4-lm-gnn-knowledge-distillation)
7. [Technique 5: SAST + LLM False Positive Reduction (SAST-Genius)](#technique-5-sast-llm-false-positive-reduction)
8. [Technique 6: Dataflow-Inspired Deep Learning (DeepDFA)](#technique-6-dataflow-inspired-deep-learning)
9. [Technique 7: ML Taint Triage](#technique-7-ml-taint-triage)
10. [Technique 8: Type-Based Taint Tracking](#technique-8-type-based-taint-tracking)
11. [Technique 9: Spec Mining from Syscall Traces (Caruca)](#technique-9-spec-mining-from-syscall-traces)
12. [Technique 10: Data Transformation Spec Mining](#technique-10-data-transformation-spec-mining)
13. [Technique 11: CEGAR-Based Spec Mining (SmCon)](#technique-11-cegar-based-spec-mining)
14. [Build Sequence and Dependency Graph](#build-sequence-and-dependency-graph)
15. [Summary Table](#summary-table)

---

## Architecture Overview

### Current State

**apex-cpg** (`crates/apex-cpg/`):
- `lib.rs` — `Cpg` struct with `NodeKind`, `EdgeKind`, `NodeId` types. Nodes: Method, Parameter, Call, Identifier, Literal, Return, ControlStructure, Assignment. Edges: Ast, Cfg, ReachingDef, Argument.
- `builder.rs` — `build_python_cpg()` line-based Python parser.
- `reaching_def.rs` — Iterative MOP reaching-definitions analysis, materializes `ReachingDef` edges.
- `taint.rs` — Backward BFS taint reachability. Hardcoded `PYTHON_SOURCES`, `PYTHON_SINKS`, `PYTHON_SANITIZERS` string slices. `find_taint_flows()` and `reachable_by()` functions. Returns `Vec<TaintFlow>`.
- Dependencies: only `serde`.

**apex-detect** (`crates/apex-detect/`):
- `lib.rs` — `Detector` trait (async, `analyze() -> Result<Vec<Finding>>`).
- `finding.rs` — `Finding`, `Severity`, `FindingCategory`, `Evidence`, `Fix` types.
- `context.rs` — `AnalysisContext` with target_root, language, oracle, source_cache, config, runner.
- `config.rs` — `DetectConfig` with `enabled` list, `LlmConfig`, `StaticAnalysisConfig`, etc.
- `pipeline.rs` — `DetectorPipeline` runs detectors concurrently (pure) or sequentially (subprocess), deduplicates, sorts by severity.
- `detectors/mod.rs` — 7 existing detectors: PanicPattern, UnsafeReachability, DependencyAudit, StaticAnalysis, SecurityPattern, HardcodedSecret, PathNormalization.
- Dependencies: apex-core, apex-coverage, async-trait, serde, tokio, regex, uuid, etc.

### Target Architecture

```
apex-cpg (graph + analysis)
  lib.rs            — Cpg, NodeKind, EdgeKind (extended)
  builder.rs        — build_python_cpg (existing)
  reaching_def.rs   — reaching definitions (existing)
  taint.rs          — taint analysis (extended with TaintSpec, TaintSpecInferrer)
  slice.rs          — NEW: CpgSliceExtractor, CpgSlice
  types.rs          — NEW: TypeInfo, TypedTaintTracker
  spec.rs           — NEW: SpecMiner trait, SpecCandidate

apex-detect (detectors + pipeline)
  lib.rs            — Detector trait (existing)
  finding.rs        — Finding, Evidence (extended with new variants)
  context.rs        — AnalysisContext (extended with Cpg reference)
  config.rs         — DetectConfig (extended with ML/LLM configs)
  pipeline.rs       — DetectorPipeline (existing), VulnDetectionPipeline (NEW)
  detectors/
    mod.rs          — (extended with new detector registrations)
    taint_llm.rs    — NEW: LLM-inferred taint spec detector (IRIS)
    cpg_slice.rs    — NEW: CPG-guided LLM validation detector (LLMxCPG)
    gnn_vuln.rs     — NEW: GNN vulnerability detector (IPAG/HAGNN)
    lm_gnn.rs       — NEW: LM+GNN distillation detector (Vul-LMGNNs)
    sast_fp.rs      — NEW: SAST false-positive reducer (SAST-Genius)
    deep_dfa.rs     — NEW: Dataflow-inspired DL detector (DeepDFA)
    taint_triage.rs — NEW: ML taint triage (TaintTriager)
    type_taint.rs   — NEW: Type-based taint tracking
  llm_validator.rs  — NEW: LlmValidator shared infrastructure
  vuln_pipeline.rs  — NEW: VulnDetectionPipeline orchestrator
  spec_mining/
    mod.rs          — NEW: SpecMiner trait re-export
    syscall.rs      — NEW: Syscall trace spec miner (Caruca)
    transform.rs    — NEW: Data transformation spec miner
    cegar.rs        — NEW: CEGAR-based spec miner (SmCon)
```

---

## Shared Types and Traits

These cross-cutting types are used by multiple techniques and must be implemented first.

### `TaintSpec` (apex-cpg)

**File:** `crates/apex-cpg/src/taint.rs` (extend existing)

```rust
/// A taint specification for a single API function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaintSpec {
    /// Fully qualified function name (e.g. "flask.request.args.get").
    pub function: String,
    /// Classification of this function.
    pub kind: TaintSpecKind,
    /// Confidence score from the inferrer [0.0, 1.0].
    pub confidence: f64,
    /// Which arguments are tainted (for sources) or dangerous (for sinks).
    /// Empty means "all arguments" or "return value".
    pub tainted_args: Vec<u32>,
    /// Provenance: how this spec was determined.
    pub provenance: SpecProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaintSpecKind {
    Source,
    Sink,
    Sanitizer,
    Propagator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpecProvenance {
    /// Hardcoded in APEX (existing PYTHON_SOURCES/SINKS/SANITIZERS).
    Builtin,
    /// Inferred by LLM (IRIS technique).
    LlmInferred { model: String, prompt_hash: String },
    /// Mined from runtime traces (Caruca).
    TraceMined { trace_count: usize },
    /// Learned by ML model.
    MlLearned { model_name: String, epoch: u32 },
}

/// A collection of taint specifications, replacing the hardcoded arrays.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaintSpecDatabase {
    pub specs: Vec<TaintSpec>,
}

impl TaintSpecDatabase {
    /// Load the built-in Python specs (backward compatible).
    pub fn builtin_python() -> Self { /* ... */ }

    /// Merge additional specs, preferring higher confidence.
    pub fn merge(&mut self, other: &TaintSpecDatabase) { /* ... */ }

    /// Query: is this function name a source?
    pub fn is_source(&self, name: &str) -> bool { /* ... */ }

    /// Query: is this function name a sink?
    pub fn is_sink(&self, name: &str) -> bool { /* ... */ }

    /// Query: is this function name a sanitizer?
    pub fn is_sanitizer(&self, name: &str) -> bool { /* ... */ }
}
```

**Integration with existing code:** Refactor `find_taint_flows()` to accept a `&TaintSpecDatabase` parameter instead of using hardcoded `PYTHON_SOURCES`/`PYTHON_SINKS`/`PYTHON_SANITIZERS`. The hardcoded arrays remain as `TaintSpecDatabase::builtin_python()` for backward compatibility. The helpers `is_sink()`, `is_source_node()`, `is_sanitizer()` delegate to the database.

### `LlmValidator` (apex-detect)

**File:** `crates/apex-detect/src/llm_validator.rs` (new)

```rust
/// Shared LLM interaction layer for vulnerability validation.
///
/// Used by: IRIS (taint spec inference), LLMxCPG (slice validation),
/// SAST-Genius (false positive reduction).
#[async_trait]
pub trait LlmValidator: Send + Sync {
    /// Validate a single candidate finding. Returns confidence [0.0, 1.0]
    /// and an explanation string.
    async fn validate(
        &self,
        prompt: &str,
    ) -> Result<LlmValidationResult>;

    /// Batch-validate multiple candidates.
    async fn validate_batch(
        &self,
        prompts: &[String],
    ) -> Result<Vec<LlmValidationResult>>;
}

#[derive(Debug, Clone)]
pub struct LlmValidationResult {
    pub confidence: f64,
    pub verdict: LlmVerdict,
    pub explanation: String,
    pub model: String,
    pub tokens_used: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmVerdict {
    TruePositive,
    FalsePositive,
    Uncertain,
}

/// Default implementation that calls an LLM API via apex-rpc or similar.
pub struct DefaultLlmValidator {
    config: LlmConfig,
    // runner or HTTP client for LLM API calls
}
```

### `CpgSliceExtractor` (apex-cpg)

**File:** `crates/apex-cpg/src/slice.rs` (new)

```rust
/// A thin slice of a CPG centered on a taint flow or vulnerability candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpgSlice {
    /// Node IDs included in this slice.
    pub nodes: Vec<NodeId>,
    /// Edges between slice nodes.
    pub edges: Vec<(NodeId, NodeId, EdgeKind)>,
    /// The focal point (sink node for taint, suspicious node for patterns).
    pub focal_node: NodeId,
    /// Source code reconstruction of the slice, annotated with node IDs.
    pub annotated_source: String,
    /// Reduction ratio: slice_nodes / total_nodes.
    pub reduction_ratio: f64,
}

/// Extracts thin CPG slices for LLM analysis.
pub struct CpgSliceExtractor;

impl CpgSliceExtractor {
    /// Backward slice from a sink node: follow ReachingDef and Argument
    /// edges backward up to `max_depth`, collecting all nodes and edges
    /// on the path.
    pub fn backward_slice(
        cpg: &Cpg,
        sink: NodeId,
        max_depth: usize,
    ) -> CpgSlice { /* ... */ }

    /// Forward slice from a source node.
    pub fn forward_slice(
        cpg: &Cpg,
        source: NodeId,
        max_depth: usize,
    ) -> CpgSlice { /* ... */ }

    /// Bidirectional slice along a known taint flow path.
    pub fn taint_flow_slice(
        cpg: &Cpg,
        flow: &TaintFlow,
        context_depth: usize,
    ) -> CpgSlice { /* ... */ }

    /// Serialize a slice as annotated source code suitable for LLM prompts.
    /// Includes line numbers, taint annotations, and edge labels.
    pub fn to_annotated_source(
        cpg: &Cpg,
        slice: &CpgSlice,
        source_map: &HashMap<PathBuf, String>,
    ) -> String { /* ... */ }
}
```

### `VulnDetectionPipeline` (apex-detect)

**File:** `crates/apex-detect/src/vuln_pipeline.rs` (new)

```rust
/// Orchestrates multi-stage vulnerability detection:
/// Stage 1: Fast pattern matching (existing detectors)
/// Stage 2: CPG taint analysis (apex-cpg)
/// Stage 3: ML/GNN scoring (techniques 3, 4, 6)
/// Stage 4: LLM validation (techniques 2, 5)
/// Stage 5: Triage and ranking (technique 7)
pub struct VulnDetectionPipeline {
    /// Stage 1: existing DetectorPipeline
    pattern_pipeline: DetectorPipeline,
    /// Stage 2: taint spec database (technique 1 feeds this)
    taint_specs: TaintSpecDatabase,
    /// Stage 3: ML scorers (optional, behind feature flags)
    ml_scorers: Vec<Box<dyn MlVulnScorer>>,
    /// Stage 4: LLM validator
    llm_validator: Option<Box<dyn LlmValidator>>,
    /// Stage 5: taint triager
    triager: Option<TaintTriager>,
}

/// Trait for ML-based vulnerability scorers (GNN, DeepDFA, etc.)
#[async_trait]
pub trait MlVulnScorer: Send + Sync {
    fn name(&self) -> &str;
    async fn score(&self, cpg: &Cpg, candidates: &[Finding]) -> Result<Vec<ScoredFinding>>;
}

pub struct ScoredFinding {
    pub finding: Finding,
    pub ml_confidence: f64,
    pub model_name: String,
}
```

### `SpecMiner` Trait (apex-cpg or apex-detect)

**File:** `crates/apex-cpg/src/spec.rs` (new) — lives in apex-cpg because specs feed taint analysis.

```rust
/// Trait for specification mining algorithms.
/// Implementations: Caruca (syscall traces), SmCon (CEGAR), TransformMiner.
pub trait SpecMiner: Send + Sync {
    /// Name of the mining algorithm.
    fn name(&self) -> &str;

    /// Mine specifications from the provided evidence.
    fn mine(&self, evidence: &MiningEvidence) -> Result<Vec<SpecCandidate>>;

    /// Refine existing specs given new counterexamples (for CEGAR loop).
    fn refine(
        &self,
        current: &[SpecCandidate],
        counterexamples: &[Counterexample],
    ) -> Result<Vec<SpecCandidate>> {
        // Default: no refinement (non-CEGAR miners).
        Ok(current.to_vec())
    }
}

/// Evidence provided to a spec miner.
#[derive(Debug, Clone)]
pub struct MiningEvidence {
    /// Syscall traces (for Caruca).
    pub syscall_traces: Vec<SyscallTrace>,
    /// Input/output pairs (for transformation mining).
    pub io_pairs: Vec<(Vec<u8>, Vec<u8>)>,
    /// CPG of the program (for all miners).
    pub cpg: Option<Cpg>,
    /// Runtime execution traces.
    pub exec_traces: Vec<ExecTrace>,
}

/// A mined specification candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecCandidate {
    pub kind: SpecKind,
    pub predicate: String,
    pub confidence: f64,
    pub support: usize, // number of traces supporting this spec
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpecKind {
    /// Precondition on function input.
    Precondition,
    /// Postcondition on function output.
    Postcondition,
    /// Invariant that holds across calls.
    Invariant,
    /// Security policy (allowed/denied syscalls, file access, etc.)
    SecurityPolicy,
    /// Data transformation spec (input shape -> output shape).
    DataTransformation,
}

/// A counterexample for CEGAR refinement.
#[derive(Debug, Clone)]
pub struct Counterexample {
    pub spec: SpecCandidate,
    pub violating_trace: ExecTrace,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct SyscallTrace {
    pub pid: u32,
    pub calls: Vec<SyscallEvent>,
}

#[derive(Debug, Clone)]
pub struct SyscallEvent {
    pub name: String,
    pub args: Vec<String>,
    pub ret: i64,
    pub timestamp_ns: u64,
}

#[derive(Debug, Clone)]
pub struct ExecTrace {
    pub function: String,
    pub inputs: Vec<serde_json::Value>,
    pub output: serde_json::Value,
    pub branches_taken: Vec<u32>,
}
```

### `TaintTriager` (apex-detect)

**File:** `crates/apex-detect/src/detectors/taint_triage.rs` (new)

```rust
/// ML-based prioritization of taint analysis findings.
///
/// Features used for scoring:
/// - Path length (source to sink hop count)
/// - Sink severity class (command exec > file write > logging)
/// - Source type (user input > env var > config file)
/// - Sanitizer proximity (how close is the nearest sanitizer to the flow)
/// - Code complexity along the path (cyclomatic complexity of traversed methods)
/// - Historical true-positive rate for similar patterns
pub struct TaintTriager {
    /// Feature weights, learned or configured.
    weights: TriageWeights,
    /// Historical data for similar-pattern lookup.
    history: Vec<TriageHistoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriageWeights {
    pub path_length: f64,
    pub sink_severity: f64,
    pub source_type: f64,
    pub sanitizer_proximity: f64,
    pub code_complexity: f64,
    pub historical_tp_rate: f64,
}

impl Default for TriageWeights {
    fn default() -> Self {
        // Sensible defaults based on literature; can be learned from feedback.
        Self {
            path_length: -0.1,       // longer paths = less likely real
            sink_severity: 0.4,       // high-severity sinks = more important
            source_type: 0.3,         // user input = most important
            sanitizer_proximity: -0.2, // nearby sanitizer = less likely real
            code_complexity: 0.05,     // complex code = slightly more likely
            historical_tp_rate: 0.3,   // history is a strong signal
        }
    }
}

impl TaintTriager {
    pub fn new(weights: TriageWeights) -> Self { /* ... */ }

    /// Score a set of taint flows, returning them sorted by priority.
    pub fn prioritize(
        &self,
        flows: &[TaintFlow],
        cpg: &Cpg,
        spec_db: &TaintSpecDatabase,
    ) -> Vec<PrioritizedFlow> { /* ... */ }

    /// Update weights based on user feedback (true/false positive labels).
    pub fn update_from_feedback(
        &mut self,
        labeled: &[(TaintFlow, bool)],
    ) { /* ... */ }
}

#[derive(Debug, Clone)]
pub struct PrioritizedFlow {
    pub flow: TaintFlow,
    pub score: f64,
    pub features: TriageFeatures,
}

#[derive(Debug, Clone)]
pub struct TriageFeatures {
    pub path_length: usize,
    pub sink_severity: f64,
    pub source_type: f64,
    pub sanitizer_proximity: f64,
    pub code_complexity: f64,
    pub historical_tp_rate: f64,
}
```

---

## Technique 1: LLM-Inferred Taint Specifications

**Paper:** IRIS (arXiv:2405.17238, ICLR 2025)
**Target crates:** `apex-cpg`, `apex-detect`
**Complexity:** Medium (2-3 days)

### Concept

Scan import statements for third-party libraries. For each library, prompt an LLM to classify its API functions as sources, sinks, sanitizers, or propagators. Populate the `TaintSpecDatabase` before running taint analysis. IRIS reports 2x detection rate over CodeQL's manual specs.

### New Types

**File:** `crates/apex-cpg/src/taint.rs` (extend)

```rust
/// Infers taint specifications for third-party APIs using LLM.
pub struct TaintSpecInferrer {
    /// LLM configuration.
    llm_config: LlmConfig,
    /// Cache: library name -> previously inferred specs.
    cache: HashMap<String, Vec<TaintSpec>>,
    /// Cache file path for persistence.
    cache_path: Option<PathBuf>,
}

impl TaintSpecInferrer {
    pub fn new(llm_config: LlmConfig) -> Self;

    /// Extract import statements from source code, return library names.
    pub fn extract_imports(source: &str, lang: Language) -> Vec<String>;

    /// Infer specs for a single library. Returns cached result if available.
    pub async fn infer_library_specs(
        &mut self,
        library: &str,
        sample_usage: &[String],
    ) -> Result<Vec<TaintSpec>>;

    /// Infer specs for all libraries found in the source files.
    pub async fn infer_all(
        &mut self,
        sources: &HashMap<PathBuf, String>,
        lang: Language,
    ) -> Result<TaintSpecDatabase>;

    /// Build the LLM prompt for a library.
    fn build_prompt(library: &str, sample_usage: &[String]) -> String;

    /// Parse the LLM response into TaintSpec entries.
    fn parse_response(response: &str, library: &str) -> Vec<TaintSpec>;
}
```

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-cpg/src/taint.rs` | Modify | Add `TaintSpec`, `TaintSpecDatabase`, `TaintSpecInferrer`. Refactor `find_taint_flows` to accept `&TaintSpecDatabase`. |
| `crates/apex-cpg/Cargo.toml` | Modify | Add optional deps: `serde_json`, `tokio` (behind `llm` feature flag). |
| `crates/apex-detect/src/detectors/taint_llm.rs` | Create | `TaintLlmDetector` that wraps `TaintSpecInferrer` + `find_taint_flows`. |
| `crates/apex-detect/src/detectors/mod.rs` | Modify | Add `pub mod taint_llm;` and re-export. |
| `crates/apex-detect/src/config.rs` | Modify | Add `taint_llm` to `LlmConfig` or new `TaintInferConfig` section. |

### Integration Points

- `TaintSpecInferrer` produces a `TaintSpecDatabase`.
- `TaintSpecDatabase` is consumed by `find_taint_flows()` (refactored from hardcoded arrays).
- `TaintLlmDetector` implements `Detector` trait and runs in `DetectorPipeline`.
- The detector's `analyze()` method: (1) extracts imports from `ctx.source_cache`, (2) calls `infer_all()`, (3) builds CPG, (4) runs taint analysis with inferred specs, (5) converts `TaintFlow` to `Finding`.

### Test Strategy

- Unit test `extract_imports()` with sample Python/Rust source.
- Unit test `parse_response()` with mock LLM output strings.
- Unit test `TaintSpecDatabase::merge()` for confidence precedence.
- Integration test: hardcode a mock LLM response, verify end-to-end taint detection for a Flask app with `requests` library.
- Backward compatibility test: `TaintSpecDatabase::builtin_python()` must produce identical results to current hardcoded arrays.

---

## Technique 2: CPG-Guided LLM Slicing

**Paper:** LLMxCPG (arXiv:2507.16585)
**Target crates:** `apex-cpg`, `apex-detect`
**Complexity:** Medium (2-3 days)

### Concept

After taint analysis finds candidate flows, extract a thin backward CPG slice (67-91% code reduction) around each flow. Serialize the slice as annotated source code and prompt an LLM to validate: "Is this a real vulnerability or a false positive?" Filter findings by LLM confidence. Achieves 15-40% F1 improvement.

### New Types

**File:** `crates/apex-cpg/src/slice.rs` (new) — `CpgSliceExtractor` and `CpgSlice` as described in the shared types section above.

**File:** `crates/apex-detect/src/detectors/cpg_slice.rs` (new)

```rust
/// Detector that uses CPG slicing + LLM validation to reduce false positives.
pub struct CpgSliceDetector {
    validator: Box<dyn LlmValidator>,
    min_confidence: f64,
    max_slice_depth: usize,
}

impl CpgSliceDetector {
    pub fn new(
        validator: Box<dyn LlmValidator>,
        min_confidence: f64,
    ) -> Self;
}

#[async_trait]
impl Detector for CpgSliceDetector {
    fn name(&self) -> &str { "cpg-slice-llm" }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        // 1. Build CPG from source_cache
        // 2. Run taint analysis to get candidate TaintFlows
        // 3. For each flow, extract CpgSlice via CpgSliceExtractor
        // 4. Serialize slice to annotated source
        // 5. Prompt LLM for validation
        // 6. Filter by confidence threshold
        // 7. Convert validated flows to Findings
    }
}
```

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-cpg/src/slice.rs` | Create | `CpgSliceExtractor`, `CpgSlice` |
| `crates/apex-cpg/src/lib.rs` | Modify | Add `pub mod slice;` |
| `crates/apex-detect/src/detectors/cpg_slice.rs` | Create | `CpgSliceDetector` |
| `crates/apex-detect/src/detectors/mod.rs` | Modify | Add `pub mod cpg_slice;` |
| `crates/apex-detect/src/llm_validator.rs` | Create | `LlmValidator` trait + `DefaultLlmValidator` |
| `crates/apex-detect/Cargo.toml` | Modify | Add dep on `apex-cpg` |

### Integration Points

- `CpgSliceExtractor::backward_slice()` uses `Cpg::edges_to()` to walk backward from sink nodes, collecting all ReachingDef/Argument predecessors.
- `CpgSliceExtractor::to_annotated_source()` reconstructs a minimal code snippet from the slice by looking up line numbers in `source_cache` and annotating with taint flow direction markers.
- `CpgSliceDetector` is registered in `DetectorPipeline::from_config()` under `"cpg-slice"`.
- The LLM prompt template includes: (a) the annotated source slice, (b) the source and sink identifiers, (c) the variable chain, (d) a question asking for true/false positive classification with reasoning.

### Test Strategy

- Unit test `CpgSliceExtractor::backward_slice()` on a simple CPG: verify slice contains only flow-relevant nodes.
- Unit test `to_annotated_source()`: verify output format.
- Test reduction ratio: build CPG for a 100-line file, extract slice, verify ratio < 0.5.
- Integration test with mock `LlmValidator`: inject known TP and FP flows, verify filtering.

---

## Technique 3: Heterogeneous GNN on CPGs

**Paper:** IPAG/HAGNN (arXiv:2502.16835)
**Target crates:** `apex-cpg`, `apex-detect`
**Complexity:** High (5-7 days)

### Concept

Build a heterogeneous attention GNN that operates directly on the CPG's multiple edge types (AST, CFG, ReachingDef, Argument). Each edge type has its own attention mechanism. The GNN classifies CPG subgraphs as vulnerable/safe. Achieves 96.6% accuracy.

### New Types

**File:** `crates/apex-detect/src/detectors/gnn_vuln.rs` (new)

```rust
/// GNN-based vulnerability detector using heterogeneous attention on CPGs.
///
/// Feature-gated behind `gnn` feature to avoid heavy ML deps by default.
pub struct GnnVulnDetector {
    /// Path to the trained model weights.
    model_path: PathBuf,
    /// Confidence threshold for flagging.
    threshold: f64,
    /// Model runner (ONNX runtime or custom inference).
    runner: Box<dyn GnnRunner>,
}

/// Abstraction over the GNN inference engine.
#[async_trait]
pub trait GnnRunner: Send + Sync {
    /// Convert a CPG subgraph into a tensor representation.
    fn encode_subgraph(
        &self,
        cpg: &Cpg,
        center: NodeId,
        radius: usize,
    ) -> Result<GraphTensor>;

    /// Run inference on an encoded graph, return vulnerability probability.
    async fn predict(&self, graph: &GraphTensor) -> Result<f64>;
}

/// Tensor representation of a CPG subgraph for GNN input.
#[derive(Debug, Clone)]
pub struct GraphTensor {
    /// Node feature matrix: [num_nodes, feature_dim]
    pub node_features: Vec<Vec<f32>>,
    /// Per-edge-type adjacency lists: edge_type -> [(src, dst)]
    pub edge_indices: HashMap<String, Vec<(usize, usize)>>,
    /// Node-to-CPG-NodeId mapping.
    pub node_map: Vec<NodeId>,
}

impl GnnVulnDetector {
    /// Extract node features from a NodeKind.
    /// Features: one-hot node type, line number (normalized), name embedding hash.
    fn node_features(kind: &NodeKind) -> Vec<f32>;

    /// Extract subgraphs centered on each Call node (potential vulnerability sites).
    fn extract_candidate_subgraphs(cpg: &Cpg, radius: usize) -> Vec<(NodeId, GraphTensor)>;
}

#[async_trait]
impl Detector for GnnVulnDetector {
    fn name(&self) -> &str { "gnn-vuln" }
    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>>;
}
```

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-detect/src/detectors/gnn_vuln.rs` | Create | `GnnVulnDetector`, `GnnRunner` trait, `GraphTensor` |
| `crates/apex-detect/src/detectors/mod.rs` | Modify | Add `#[cfg(feature = "gnn")] pub mod gnn_vuln;` |
| `crates/apex-detect/Cargo.toml` | Modify | Add `gnn` feature, optional deps: `ort` (ONNX Runtime) or `tch` (libtorch) |
| `crates/apex-cpg/src/lib.rs` | Modify | Add `NodeKind::type_index()` method for one-hot encoding |

### Integration Points

- `GnnRunner::encode_subgraph()` uses `Cpg::edges_from()`, `Cpg::edges_to()`, and `Cpg::node()` to walk the neighborhood.
- Edge types map to separate adjacency matrices: `Ast` -> "ast", `Cfg` -> "cfg", `ReachingDef` -> "reaching_def", `Argument` -> "argument".
- `GnnVulnDetector` implements `MlVulnScorer` trait for use in `VulnDetectionPipeline` Stage 3.
- Model weights are loaded from a configurable path (shipped separately or downloaded).

### Build Dependencies

- Requires either ONNX Runtime (`ort` crate) or PyTorch C++ (`tch` crate).
- Both are behind `gnn` feature flag, not compiled by default.
- Training pipeline (Python, PyTorch Geometric) is out of scope for this plan; only inference is in Rust.

### Test Strategy

- Unit test `node_features()` for each `NodeKind` variant.
- Unit test `extract_candidate_subgraphs()` on a small CPG.
- Mock `GnnRunner` that returns fixed probabilities; verify detector produces correct findings.
- Integration test with a pretrained model on known vulnerable/safe code samples (requires model artifact).

---

## Technique 4: LM + GNN Knowledge Distillation

**Paper:** Vul-LMGNNs (arXiv:2404.14719)
**Target crates:** `apex-detect`
**Complexity:** High (4-5 days)

### Concept

Combine CodeBERT/UniXcoder embeddings (language model) with GNN structural reasoning. The LM provides token-level semantic features; the GNN provides graph-level structural features. Knowledge distillation: train a student model that combines both. In APEX, this means: embed code tokens with an LM, feed node embeddings + graph structure to GNN, combine scores.

### New Types

**File:** `crates/apex-detect/src/detectors/lm_gnn.rs` (new)

```rust
/// Combined LM+GNN vulnerability detector via knowledge distillation.
/// Feature-gated behind `ml` feature.
pub struct LmGnnDetector {
    /// Language model for token embeddings (ONNX).
    lm_runner: Box<dyn LmEmbedder>,
    /// GNN for structural reasoning (ONNX).
    gnn_runner: Box<dyn GnnRunner>,
    /// Combined scoring threshold.
    threshold: f64,
}

/// Language model embedder (CodeBERT, UniXcoder, etc.)
#[async_trait]
pub trait LmEmbedder: Send + Sync {
    /// Embed a code snippet, returning per-token embeddings.
    async fn embed(&self, code: &str) -> Result<Vec<Vec<f32>>>;
    /// Embedding dimension.
    fn dim(&self) -> usize;
}

impl LmGnnDetector {
    /// For each CPG node, get LM embedding of the surrounding code context,
    /// then use as node features for the GNN (replacing one-hot encoding).
    async fn enrich_node_features(
        &self,
        cpg: &Cpg,
        source: &str,
    ) -> Result<Vec<Vec<f32>>>;
}
```

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-detect/src/detectors/lm_gnn.rs` | Create | `LmGnnDetector`, `LmEmbedder` trait |
| `crates/apex-detect/src/detectors/mod.rs` | Modify | Add `#[cfg(feature = "ml")] pub mod lm_gnn;` |
| `crates/apex-detect/Cargo.toml` | Modify | Add `ml` feature (superset of `gnn`), optional `tokenizers` crate |

### Integration Points

- Reuses `GnnRunner` trait from technique 3.
- `LmEmbedder` loads a CodeBERT ONNX model and tokenizer.
- Node features are LM embeddings instead of one-hot vectors: `enrich_node_features()` maps each `NodeKind` to its source code span, embeds it, and averages sub-token embeddings to get a fixed-size node vector.
- Implements both `Detector` and `MlVulnScorer` traits.

### Test Strategy

- Mock `LmEmbedder` that returns random vectors of correct dimension.
- Mock `GnnRunner` with fixed outputs.
- Verify combined scoring produces expected results.
- Snapshot test: given a known CPG + embeddings, verify the output finding list.

---

## Technique 5: SAST + LLM False Positive Reduction

**Paper:** SAST-Genius (arXiv:2509.15433)
**Target crates:** `apex-detect`
**Complexity:** Medium (2 days)

### Concept

Run Semgrep (or another SAST tool) to get an initial finding set, then use an LLM to classify each finding as true positive or false positive. The key insight: include the SAST rule description, the matched code, and the surrounding context in the LLM prompt. Dramatically reduces false positives.

### New Types

**File:** `crates/apex-detect/src/detectors/sast_fp.rs` (new)

```rust
/// SAST + LLM false-positive reduction detector.
///
/// Wraps an existing SAST detector (e.g., StaticAnalysisDetector) and
/// filters its findings through LLM validation.
pub struct SastFpReducer {
    /// The inner SAST detector whose findings we filter.
    inner: Box<dyn Detector>,
    /// LLM validator for FP classification.
    validator: Box<dyn LlmValidator>,
    /// Minimum LLM confidence to keep a finding.
    min_confidence: f64,
    /// Context lines to include around each finding.
    context_lines: usize,
}

impl SastFpReducer {
    /// Build an LLM prompt for validating a SAST finding.
    /// Includes: rule ID, rule description, matched code with context,
    /// and a specific question about exploitability.
    fn build_validation_prompt(
        &self,
        finding: &Finding,
        source: &str,
    ) -> String;
}

#[async_trait]
impl Detector for SastFpReducer {
    fn name(&self) -> &str { "sast-fp-reducer" }

    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>> {
        // 1. Run inner detector
        let raw = self.inner.analyze(ctx).await?;
        // 2. For each finding, build validation prompt
        // 3. Batch-validate with LLM
        // 4. Filter out findings below confidence threshold
        // 5. Annotate surviving findings with LLM explanation
    }
}
```

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-detect/src/detectors/sast_fp.rs` | Create | `SastFpReducer` |
| `crates/apex-detect/src/detectors/mod.rs` | Modify | Add `pub mod sast_fp;` |
| `crates/apex-detect/src/finding.rs` | Modify | Add `Evidence::LlmValidation { confidence, explanation, model }` variant |
| `crates/apex-detect/src/pipeline.rs` | Modify | Support wrapping detectors with `SastFpReducer` in `from_config()` |
| `crates/apex-detect/src/config.rs` | Modify | Add `fp_reduction` config section with `min_confidence`, `context_lines` |

### Integration Points

- `SastFpReducer` wraps any existing `Detector` (not just Semgrep). In `from_config()`, when `llm.enabled` and `fp_reduction` are configured, wrap `StaticAnalysisDetector` and `SecurityPatternDetector` with `SastFpReducer`.
- Uses `LlmValidator` trait from the shared infrastructure.
- Adds `Evidence::LlmValidation` to each surviving finding, preserving the LLM's reasoning.
- The `Finding.explanation` field is populated with the LLM's explanation.

### Test Strategy

- Unit test `build_validation_prompt()`: verify prompt includes rule ID, code context, and structured question.
- Mock `LlmValidator` returning high confidence for known TPs and low for known FPs.
- Integration test: run `SecurityPatternDetector` on code with deliberate FP patterns, verify `SastFpReducer` filters them.
- Verify that `Evidence::LlmValidation` appears in output findings.

---

## Technique 6: Dataflow-Inspired Deep Learning

**Paper:** DeepDFA (arXiv:2212.08108)
**Target crates:** `apex-cpg`, `apex-detect`
**Complexity:** High (4-5 days)

### Concept

Learn dataflow analysis patterns from data rather than hand-coding them. The key insight: structure the neural network to mirror the iterative fixpoint computation of traditional dataflow analysis. Each "layer" of the network corresponds to one iteration of the dataflow equations. The network learns gen/kill functions from labeled data.

### New Types

**File:** `crates/apex-detect/src/detectors/deep_dfa.rs` (new)

```rust
/// Deep learning detector inspired by dataflow analysis structure.
/// Feature-gated behind `ml` feature.
pub struct DeepDfaDetector {
    /// Model runner (ONNX).
    runner: Box<dyn DfaModelRunner>,
    /// Number of "dataflow iterations" (network depth).
    num_iterations: usize,
    /// Threshold for vulnerability classification.
    threshold: f64,
}

/// Abstraction for the DeepDFA model inference.
#[async_trait]
pub trait DfaModelRunner: Send + Sync {
    /// Run the dataflow-inspired network on a CFG representation.
    /// Input: node features + CFG adjacency.
    /// Output: per-node vulnerability probability.
    async fn run(
        &self,
        node_features: &[Vec<f32>],
        cfg_edges: &[(usize, usize)],
        num_iterations: usize,
    ) -> Result<Vec<f64>>;
}

impl DeepDfaDetector {
    /// Convert a CPG into the CFG-only representation needed by DeepDFA.
    /// Extracts only CFG edges and computes node features from AST context.
    fn cpg_to_cfg_representation(
        cpg: &Cpg,
    ) -> (Vec<Vec<f32>>, Vec<(usize, usize)>, Vec<NodeId>);
}
```

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-detect/src/detectors/deep_dfa.rs` | Create | `DeepDfaDetector`, `DfaModelRunner` trait |
| `crates/apex-detect/src/detectors/mod.rs` | Modify | Add `#[cfg(feature = "ml")] pub mod deep_dfa;` |

### Integration Points

- `cpg_to_cfg_representation()` filters `Cpg::edges()` to only `EdgeKind::Cfg` edges and builds a compact adjacency list.
- Node features combine: node type (one-hot), operation type, and optionally LM embeddings (if `LmEmbedder` is available).
- Implements `MlVulnScorer` for use in `VulnDetectionPipeline` Stage 3.
- Per-node vulnerability scores are aggregated to per-function findings.

### Test Strategy

- Unit test `cpg_to_cfg_representation()`: verify correct edge filtering and node mapping.
- Mock `DfaModelRunner` with known outputs; verify finding generation.
- Verify that nodes with high vulnerability scores produce findings with correct file/line info.

---

## Technique 7: ML Taint Triage

**Target crates:** `apex-detect`
**Complexity:** Medium (2-3 days)

### Concept

Not all taint flows are equally important. ML-based triage scores flows by path length, sink severity, source type, sanitizer proximity, code complexity, and historical TP rate. The triager produces a ranked list so developers see the most likely real vulnerabilities first.

### New Types

`TaintTriager` as described in the shared types section above.

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-detect/src/detectors/taint_triage.rs` | Create | `TaintTriager`, `TriageWeights`, `PrioritizedFlow`, `TriageFeatures` |
| `crates/apex-detect/src/detectors/mod.rs` | Modify | Add `pub mod taint_triage;` |
| `crates/apex-detect/src/finding.rs` | Modify | Add `triage_score: Option<f64>` field to `Finding` |
| `crates/apex-detect/src/vuln_pipeline.rs` | Modify | Integrate `TaintTriager` as Stage 5 |

### Integration Points

- `TaintTriager::prioritize()` takes `Vec<TaintFlow>` from `find_taint_flows()`, the `Cpg` (for complexity analysis), and the `TaintSpecDatabase` (for sink/source classification).
- Feature extraction walks the CPG path of each flow to compute metrics.
- `TriageWeights` can be serialized/deserialized for persistence and tuning.
- `update_from_feedback()` implements simple online learning (gradient update on logistic regression weights) so the triager improves over time.
- Integrated into `VulnDetectionPipeline` as the final ranking stage.

### Test Strategy

- Unit test feature extraction: given a known CPG + flow, verify computed features.
- Unit test scoring: given fixed weights and features, verify score computation.
- Test ordering: create flows with different characteristics, verify they sort correctly.
- Test feedback loop: provide labeled data, verify weights shift in expected direction.
- Edge case: empty flow list, single flow, flow with no path.

---

## Technique 8: Type-Based Taint Tracking

**Target crates:** `apex-cpg`
**Complexity:** Medium (2-3 days)

### Concept

Use type system information to improve taint precision. A variable of type `int` is unlikely to carry a SQL injection payload. A variable of type `str` coming from user input is high risk. Type information allows pruning impossible taint flows and focusing on flows through string/bytes types.

### New Types

**File:** `crates/apex-cpg/src/types.rs` (new)

```rust
/// Type information associated with a CPG node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeInfo {
    /// Primitive types that cannot carry injection payloads.
    Primitive(PrimitiveType),
    /// String/bytes types that can carry payloads.
    StringLike,
    /// Collection of typed elements.
    Collection { element: Box<TypeInfo> },
    /// User-defined type with known fields.
    Struct { name: String, fields: Vec<(String, TypeInfo)> },
    /// Callable with typed parameters and return.
    Callable { params: Vec<TypeInfo>, ret: Box<TypeInfo> },
    /// Unknown type (conservative: treated as taintable).
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PrimitiveType {
    Int,
    Float,
    Bool,
    None,
}

impl TypeInfo {
    /// Can this type carry a taint payload (string content)?
    pub fn is_taintable(&self) -> bool {
        match self {
            TypeInfo::Primitive(_) => false,
            TypeInfo::StringLike => true,
            TypeInfo::Collection { element } => element.is_taintable(),
            TypeInfo::Struct { fields, .. } => fields.iter().any(|(_, t)| t.is_taintable()),
            TypeInfo::Callable { .. } => false,
            TypeInfo::Unknown => true, // conservative
        }
    }
}

/// Type-aware taint tracker that prunes flows through non-taintable types.
pub struct TypedTaintTracker;

impl TypedTaintTracker {
    /// Infer types for CPG nodes using simple heuristics:
    /// - Literal "42" -> Int, Literal "hello" -> StringLike
    /// - Known stdlib return types (len() -> Int, str() -> StringLike)
    pub fn infer_types(cpg: &Cpg) -> HashMap<NodeId, TypeInfo>;

    /// Filter taint flows: remove flows that pass through non-taintable types.
    pub fn filter_flows(
        flows: Vec<TaintFlow>,
        types: &HashMap<NodeId, TypeInfo>,
    ) -> Vec<TaintFlow>;
}
```

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-cpg/src/types.rs` | Create | `TypeInfo`, `TypedTaintTracker` |
| `crates/apex-cpg/src/lib.rs` | Modify | Add `pub mod types;` |
| `crates/apex-cpg/src/taint.rs` | Modify | Add optional type-based filtering after `find_taint_flows()` |

### Integration Points

- `TypedTaintTracker::infer_types()` runs after CPG construction, before taint analysis.
- `TypedTaintTracker::filter_flows()` is called after `find_taint_flows()` to prune impossible flows.
- Type inference is conservative: `Unknown` types are treated as taintable, so no real flows are lost.
- Heuristic-based for Python (dynamic types): uses literal analysis, known stdlib signatures, and type annotations when present.

### Test Strategy

- Unit test `TypeInfo::is_taintable()` for each variant.
- Unit test `infer_types()` on a CPG with literals of various types.
- Test `filter_flows()`: create a flow through an Int-typed intermediate, verify it is removed.
- Test conservatism: a flow through an Unknown-typed node must survive filtering.
- End-to-end: run taint analysis on code with `x = len(user_input); eval(x)` — should be filtered (len returns int, int is not taintable).

---

## Technique 9: Spec Mining from Syscall Traces

**Paper:** Caruca (arXiv:2510.14279)
**Target crates:** `apex-cpg` (spec module), `apex-detect`
**Complexity:** High (4-5 days)

### Concept

Record syscall traces during test execution. Mine temporal patterns (e.g., "open() is always followed by close()", "write() never follows read() on the same fd without seek()"). Violations of mined specs in production code indicate potential resource leaks, race conditions, or security policy violations.

### New Types

**File:** `crates/apex-cpg/src/spec.rs` — uses the `SpecMiner` trait defined in shared types.

**File:** `crates/apex-detect/src/spec_mining/syscall.rs` (new)

```rust
/// Spec miner that analyzes syscall traces to discover temporal patterns.
pub struct SyscallSpecMiner {
    /// Minimum support (fraction of traces containing the pattern).
    min_support: f64,
    /// Minimum confidence for the pattern to be considered a spec.
    min_confidence: f64,
    /// Maximum pattern length (number of syscalls in sequence).
    max_pattern_len: usize,
}

impl SpecMiner for SyscallSpecMiner {
    fn name(&self) -> &str { "caruca-syscall" }

    fn mine(&self, evidence: &MiningEvidence) -> Result<Vec<SpecCandidate>> {
        // 1. Parse syscall traces into event sequences.
        // 2. Extract frequent subsequences (n-grams up to max_pattern_len).
        // 3. Compute support and confidence for each pattern.
        // 4. Classify patterns:
        //    - Always-follows: A always precedes B -> Precondition
        //    - Never-follows: A is never followed by B -> SecurityPolicy
        //    - Always-pairs: open(fd) always matched by close(fd) -> Invariant
        // 5. Return patterns above confidence threshold as SpecCandidates.
    }
}

/// Syscall trace collector that wraps test execution with strace/dtrace.
pub struct SyscallTraceCollector {
    /// Tool to use: strace (Linux), dtrace (macOS), or sandbox log.
    backend: TraceBackend,
}

#[derive(Debug, Clone)]
pub enum TraceBackend {
    Strace,
    Dtrace,
    SandboxLog,
}

impl SyscallTraceCollector {
    /// Run a command and collect its syscall trace.
    pub async fn collect(
        &self,
        cmd: &[String],
        timeout: Duration,
    ) -> Result<SyscallTrace>;

    /// Parse raw strace/dtrace output into structured events.
    fn parse_trace(raw: &str, backend: &TraceBackend) -> Vec<SyscallEvent>;
}

/// Detector that checks code against mined syscall specs.
pub struct SyscallSpecDetector {
    /// Mined specs to check against.
    specs: Vec<SpecCandidate>,
}

#[async_trait]
impl Detector for SyscallSpecDetector {
    fn name(&self) -> &str { "syscall-spec" }
    async fn analyze(&self, ctx: &AnalysisContext) -> Result<Vec<Finding>>;
}
```

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-cpg/src/spec.rs` | Create | `SpecMiner` trait, `SpecCandidate`, `MiningEvidence`, related types |
| `crates/apex-cpg/src/lib.rs` | Modify | Add `pub mod spec;` |
| `crates/apex-detect/src/spec_mining/mod.rs` | Create | Re-export spec mining modules |
| `crates/apex-detect/src/spec_mining/syscall.rs` | Create | `SyscallSpecMiner`, `SyscallTraceCollector`, `SyscallSpecDetector` |
| `crates/apex-detect/src/detectors/mod.rs` | Modify | Add `pub mod spec_mining;` or integrate |
| `crates/apex-detect/src/lib.rs` | Modify | Add `pub mod spec_mining;` |
| `crates/apex-detect/Cargo.toml` | Modify | Add optional dep for trace parsing |

### Integration Points

- `SyscallTraceCollector` uses `CommandRunner` from `AnalysisContext` to execute traced commands.
- Mined specs feed into `TaintSpecDatabase` (security policies become taint rules) and into standalone `SyscallSpecDetector` findings.
- The detector reports violations as `Finding` with category `FindingCategory::SecuritySmell` or a new `FindingCategory::PolicyViolation`.
- Trace collection is optional: if no traces are available, the detector is a no-op.

### Test Strategy

- Unit test `SyscallSpecMiner::mine()` with hand-crafted traces containing known patterns.
- Unit test `parse_trace()` for strace output format.
- Test pattern mining: traces with consistent open/close pairing should produce an Invariant spec.
- Test violation detection: given a spec "open always followed by close" and code that doesn't close, verify finding is produced.
- Edge cases: empty traces, single-event traces, overlapping patterns.

---

## Technique 10: Data Transformation Spec Mining

**Paper:** Mining Beyond Bools (arXiv:2603.06710)
**Target crates:** `apex-cpg`, `apex-detect`
**Complexity:** Medium (3 days)

### Concept

Mine non-boolean specifications: data transformations, value relationships, and structural invariants. Instead of just "this function returns true/false", learn "this function doubles its input" or "output length equals input length" or "output is a sorted permutation of input". Violations indicate logic bugs.

### New Types

**File:** `crates/apex-detect/src/spec_mining/transform.rs` (new)

```rust
/// Mines data transformation specifications from input/output pairs.
pub struct TransformSpecMiner {
    /// Maximum number of candidate transformations to try.
    max_candidates: usize,
    /// Minimum number of supporting examples.
    min_support: usize,
}

/// A candidate data transformation.
#[derive(Debug, Clone)]
pub enum TransformCandidate {
    /// Output = f(input) for some arithmetic f.
    Arithmetic { operation: ArithOp, operand: f64 },
    /// Output length relates to input length.
    LengthRelation { relation: LengthRel },
    /// Output is a permutation of input (possibly sorted).
    Permutation { sorted: bool },
    /// Output type is always the same.
    ConstantType { type_name: String },
    /// Output contains input as a substring/subset.
    ContainsInput,
    /// Custom predicate (expressed as a small expression).
    Custom { expression: String },
}

#[derive(Debug, Clone)]
pub enum ArithOp { Add, Multiply, Negate, Abs }

#[derive(Debug, Clone)]
pub enum LengthRel { Equal, DoubleInput, HalfInput, InputPlusN(i64) }

impl SpecMiner for TransformSpecMiner {
    fn name(&self) -> &str { "transform-miner" }

    fn mine(&self, evidence: &MiningEvidence) -> Result<Vec<SpecCandidate>> {
        // 1. Group I/O pairs by function.
        // 2. For each function, try each TransformCandidate template.
        // 3. Compute support: how many I/O pairs match the candidate.
        // 4. Return candidates with support >= min_support.
    }
}
```

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-detect/src/spec_mining/transform.rs` | Create | `TransformSpecMiner`, `TransformCandidate` |
| `crates/apex-detect/src/spec_mining/mod.rs` | Modify | Add `pub mod transform;` |

### Integration Points

- `TransformSpecMiner` implements the `SpecMiner` trait from `apex-cpg/src/spec.rs`.
- I/O pairs come from `MiningEvidence::io_pairs` (collected during test execution by instrumenting function calls).
- Mined transformation specs become `SpecCandidate` with kind `SpecKind::DataTransformation`.
- A companion detector checks production code against mined specs: if a function's observed I/O violates a mined transformation, report a `FindingCategory::LogicBug`.

### Test Strategy

- Unit test each `TransformCandidate` variant against known I/O pairs.
- Test mining: provide 10 pairs of (x, 2*x), verify `Arithmetic { Multiply, 2.0 }` is mined.
- Test mining with noise: 9 correct pairs + 1 outlier, verify spec is still mined with lower confidence.
- Test `LengthRelation::Equal` on string I/O pairs.
- Edge case: no I/O pairs, single pair, conflicting pairs.

---

## Technique 11: CEGAR-Based Spec Mining

**Paper:** SmCon (arXiv:2403.13279)
**Target crates:** `apex-cpg`, `apex-detect`
**Complexity:** High (4-5 days)

### Concept

Counterexample-Guided Abstraction Refinement for spec mining. Start with an over-approximate specification. Run against test traces. When a trace violates the spec, either the spec is wrong (refine it) or the trace reveals a real bug. Iterate until the spec stabilizes. This produces high-quality specs with formal guarantees.

### New Types

**File:** `crates/apex-detect/src/spec_mining/cegar.rs` (new)

```rust
/// CEGAR-based spec miner that iteratively refines specifications.
pub struct CegarSpecMiner {
    /// Inner miner that produces initial candidates.
    inner: Box<dyn SpecMiner>,
    /// Maximum CEGAR iterations.
    max_iterations: usize,
    /// Verifier that checks specs against traces and produces counterexamples.
    verifier: Box<dyn SpecVerifier>,
}

/// Verifies spec candidates against execution traces.
#[async_trait]
pub trait SpecVerifier: Send + Sync {
    /// Check a spec against traces. Return counterexamples if any.
    async fn verify(
        &self,
        spec: &SpecCandidate,
        traces: &[ExecTrace],
    ) -> Result<VerificationResult>;
}

#[derive(Debug)]
pub enum VerificationResult {
    /// Spec holds for all traces.
    Valid,
    /// Spec is violated — counterexample provided.
    Violated(Counterexample),
    /// Verification timed out.
    Timeout,
}

impl SpecMiner for CegarSpecMiner {
    fn name(&self) -> &str { "cegar-smcon" }

    fn mine(&self, evidence: &MiningEvidence) -> Result<Vec<SpecCandidate>> {
        // 1. Use inner miner to produce initial specs.
        // 2. For each spec:
        //    a. Verify against all traces.
        //    b. If violated, call self.inner.refine() with counterexample.
        //    c. Repeat until valid or max_iterations reached.
        // 3. Return refined specs.
    }

    fn refine(
        &self,
        current: &[SpecCandidate],
        counterexamples: &[Counterexample],
    ) -> Result<Vec<SpecCandidate>> {
        // Delegate to inner miner's refine method, then re-verify.
    }
}

/// Default verifier that replays traces against spec predicates.
pub struct TraceReplayVerifier;

impl TraceReplayVerifier {
    /// Evaluate a spec predicate against a single trace.
    fn evaluate_predicate(
        spec: &SpecCandidate,
        trace: &ExecTrace,
    ) -> bool;
}
```

### Files to Create or Modify

| File | Action | Description |
|------|--------|-------------|
| `crates/apex-detect/src/spec_mining/cegar.rs` | Create | `CegarSpecMiner`, `SpecVerifier`, `VerificationResult`, `TraceReplayVerifier` |
| `crates/apex-detect/src/spec_mining/mod.rs` | Modify | Add `pub mod cegar;` |

### Integration Points

- `CegarSpecMiner` wraps any other `SpecMiner` (e.g., `SyscallSpecMiner` or `TransformSpecMiner`) and adds the CEGAR refinement loop.
- The `SpecVerifier` trait allows pluggable verification backends: trace replay (default), symbolic execution (via apex-symbolic), or LLM-based (ask LLM if spec holds).
- Refined specs are stored in the spec database and used by downstream detectors.
- The CEGAR loop's counterexamples can themselves become test cases (fed back to apex-synth).

### Test Strategy

- Unit test CEGAR loop with a mock inner miner that produces an overly broad spec and a mock verifier that produces counterexamples.
- Verify the loop terminates within `max_iterations`.
- Verify refined specs are tighter than initial specs.
- Test `TraceReplayVerifier::evaluate_predicate()` on simple predicates.
- Edge case: spec that is already valid (no refinement needed), spec that cannot be refined (stays violated).

---

## Build Sequence and Dependency Graph

### Phase 1: Foundation (Week 1)

Build order determined by dependencies:

```
1. TaintSpec + TaintSpecDatabase (apex-cpg/taint.rs)
   └── Refactor find_taint_flows() to use TaintSpecDatabase
       └── Backward compatible: builtin_python() produces same results

2. CpgSlice + CpgSliceExtractor (apex-cpg/slice.rs)
   └── Depends on: Cpg, TaintFlow (existing)

3. TypeInfo + TypedTaintTracker (apex-cpg/types.rs)
   └── Depends on: Cpg, NodeKind (existing)

4. SpecMiner trait + SpecCandidate types (apex-cpg/spec.rs)
   └── No dependencies on other new code
```

All four are in `apex-cpg` and have no cross-dependencies. They can be built in parallel.

### Phase 2: Shared Infrastructure (Week 1-2)

```
5. LlmValidator trait + DefaultLlmValidator (apex-detect/llm_validator.rs)
   └── Depends on: LlmConfig (existing in config.rs)

6. Evidence::LlmValidation variant (apex-detect/finding.rs)
   └── No new deps

7. FindingCategory extensions if needed (apex-detect/finding.rs)
   └── E.g., PolicyViolation for syscall spec violations

8. VulnDetectionPipeline skeleton (apex-detect/vuln_pipeline.rs)
   └── Depends on: DetectorPipeline (existing), TaintSpecDatabase (#1)
```

### Phase 3: Detectors — No ML (Weeks 2-3)

These require no heavy ML dependencies:

```
9.  TaintSpecInferrer + TaintLlmDetector (technique 1)
    └── Depends on: TaintSpecDatabase (#1), LlmValidator (#5)

10. CpgSliceDetector (technique 2)
    └── Depends on: CpgSliceExtractor (#2), LlmValidator (#5)

11. SastFpReducer (technique 5)
    └── Depends on: LlmValidator (#5), Evidence::LlmValidation (#6)

12. TaintTriager (technique 7)
    └── Depends on: TaintFlow (existing), TaintSpecDatabase (#1)

13. TypedTaintTracker integration (technique 8)
    └── Depends on: TypeInfo (#3)
```

### Phase 4: Spec Mining (Weeks 3-4)

```
14. SyscallSpecMiner + SyscallTraceCollector (technique 9)
    └── Depends on: SpecMiner (#4)

15. TransformSpecMiner (technique 10)
    └── Depends on: SpecMiner (#4)

16. CegarSpecMiner (technique 11)
    └── Depends on: SpecMiner (#4), wraps #14 or #15
```

### Phase 5: ML Detectors (Weeks 4-5)

Behind feature flags, require ML runtime dependencies:

```
17. GnnVulnDetector (technique 3)
    └── Depends on: Cpg (existing), feature "gnn"

18. DeepDfaDetector (technique 6)
    └── Depends on: Cpg (existing), feature "ml"

19. LmGnnDetector (technique 4)
    └── Depends on: GnnRunner (#17), LmEmbedder (new), feature "ml"
```

### Dependency Graph (ASCII)

```
                    ┌─────────────────────────────────┐
                    │     apex-cpg (foundation)        │
                    │                                  │
                    │  #1 TaintSpecDatabase            │
                    │  #2 CpgSliceExtractor            │
                    │  #3 TypeInfo                     │
                    │  #4 SpecMiner trait               │
                    └──────┬────┬────┬────┬────────────┘
                           │    │    │    │
              ┌────────────┘    │    │    └──────────────┐
              │                 │    │                    │
              v                 v    v                    v
     #5 LlmValidator    #12 Triager  #13 TypeTaint   #14-16 SpecMiners
              │                 │         │              │
    ┌─────────┼─────────┐      │         │         ┌────┼────┐
    │         │         │      │         │         │    │    │
    v         v         v      v         v         v    v    v
  #9 IRIS  #10 Slice  #11 FP  #12 Tri  #13 Typ  #14   #15  #16
                                                Sysc  Xfrm CEGAR
              │
              └─────────────────────────────────────┐
                                                    v
                                        #8 VulnDetectionPipeline
                                                    │
                              ┌─────────────────────┼─────────┐
                              │                     │         │
                              v                     v         v
                         #17 GNN              #18 DFA    #19 LM+GNN
                        (feat: gnn)          (feat: ml)  (feat: ml)
```

---

## Summary Table

| # | Technique | Paper | Target Crate(s) | New Files | Modified Files | Feature Flag | Complexity | Dependencies |
|---|-----------|-------|-----------------|-----------|----------------|-------------|------------|-------------|
| 1 | LLM Taint Specs | IRIS | apex-cpg, apex-detect | taint_llm.rs | taint.rs, mod.rs, config.rs, Cargo.toml | none (LLM optional) | Medium | #5 (LlmValidator) |
| 2 | CPG LLM Slicing | LLMxCPG | apex-cpg, apex-detect | slice.rs, cpg_slice.rs, llm_validator.rs | lib.rs, mod.rs, Cargo.toml | none (LLM optional) | Medium | #5 (LlmValidator) |
| 3 | Heterogeneous GNN | IPAG/HAGNN | apex-cpg, apex-detect | gnn_vuln.rs | mod.rs, lib.rs, Cargo.toml | `gnn` | High | ONNX Runtime |
| 4 | LM+GNN Distillation | Vul-LMGNNs | apex-detect | lm_gnn.rs | mod.rs, Cargo.toml | `ml` | High | #3 (GnnRunner), tokenizers |
| 5 | SAST FP Reduction | SAST-Genius | apex-detect | sast_fp.rs | mod.rs, finding.rs, pipeline.rs, config.rs | none (LLM optional) | Medium | #5 (LlmValidator) |
| 6 | Dataflow DL | DeepDFA | apex-cpg, apex-detect | deep_dfa.rs | mod.rs | `ml` | High | ONNX Runtime |
| 7 | ML Taint Triage | — | apex-detect | taint_triage.rs | mod.rs, finding.rs, vuln_pipeline.rs | none | Medium | TaintFlow (existing) |
| 8 | Type-Based Taint | — | apex-cpg | types.rs | lib.rs, taint.rs | none | Medium | Cpg (existing) |
| 9 | Syscall Spec Mining | Caruca | apex-cpg, apex-detect | spec.rs, syscall.rs, mod.rs | lib.rs, Cargo.toml | none | High | CommandRunner (existing) |
| 10 | Transform Spec Mining | Beyond Bools | apex-detect | transform.rs | mod.rs | none | Medium | SpecMiner (#4) |
| 11 | CEGAR Spec Mining | SmCon | apex-detect | cegar.rs | mod.rs | none | High | SpecMiner (#4) |

### Total New Files: 14
### Total Modified Files: ~12 (some modified by multiple techniques)
### Estimated Total Effort: 5-7 weeks (with parallelism in Phase 1)

### Feature Flag Strategy

- Default build: techniques 1, 2, 5, 7, 8, 9, 10, 11 (no heavy ML deps)
- `gnn` feature: adds technique 3 (ONNX Runtime dependency)
- `ml` feature: adds techniques 3, 4, 6 (ONNX Runtime + tokenizers)
- LLM-dependent techniques (1, 2, 5) degrade gracefully: if no LLM API key is configured, they skip LLM validation and fall back to rule-based analysis

### Cargo.toml Changes

**apex-cpg/Cargo.toml** (additions):
```toml
[dependencies]
serde = { workspace = true }
serde_json = { version = "1", optional = true }

[features]
default = []
llm = ["serde_json"]
```

**apex-detect/Cargo.toml** (additions):
```toml
[dependencies]
apex-cpg = { path = "../apex-cpg" }   # NEW dependency

# Optional ML dependencies
ort = { version = "2", optional = true }       # ONNX Runtime
tokenizers = { version = "0.20", optional = true }

[features]
default = []
gnn = ["ort"]
ml = ["gnn", "tokenizers"]
```
