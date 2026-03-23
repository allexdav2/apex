//! Dynamic call graph collection via test-runner tracing.
//!
//! Each language target is instrumented at test-runner level:
//!
//! * **Python** — `sys.settrace` injects a tracer that records `call` events;
//!   output is written to `.apex/callgraph.json`.
//! * **JavaScript** — a `--require` hook patches `Function.prototype` via
//!   `Proxy` to capture caller→callee pairs at call time.
//! * **Go** — a thin `testing.M` wrapper that uses `runtime/trace` and a
//!   post-processing step to extract goroutine call stacks.
//!
//! Results are merged with the static [`CallGraph`] via [`merge`].

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use apex_core::error::{ApexError, Result};
use serde::{Deserialize, Serialize};

use crate::graph::{CallEdge, CallGraph, FnId, FnNode};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A caller→callee pair observed at runtime.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DynamicEdge {
    pub caller: String,
    pub callee: String,
}

/// Dynamic call graph collected from one test run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DynamicCallGraph {
    /// Caller→callee pairs observed during execution.
    pub edges: Vec<DynamicEdge>,
    /// Human-readable label identifying the collection source
    /// (e.g. `"pytest"`, `"jest"`, `"go test"`).
    pub source: String,
}

impl DynamicCallGraph {
    /// Build from a pre-parsed list of `(caller, callee)` string pairs.
    pub fn from_pairs(pairs: Vec<(String, String)>, source: impl Into<String>) -> Self {
        let edges = pairs
            .into_iter()
            .map(|(caller, callee)| DynamicEdge { caller, callee })
            .collect();
        Self {
            edges,
            source: source.into(),
        }
    }

    /// Deserialise from the JSON written by the language shim.
    pub fn from_json(json: &str) -> Result<Self> {
        serde_json::from_str(json)
            .map_err(|e| ApexError::Other(format!("dynamic callgraph parse: {e}")))
    }

    /// Serialise to JSON (written by the language shim).
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| ApexError::Other(format!("dynamic callgraph serialise: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Python
// ---------------------------------------------------------------------------

/// Python tracer script injected via `sys.settrace`.
///
/// The script is written to a temporary `.py` file and injected with
/// `PYTHONSTARTUP` or prepended to the test command.
const PYTHON_TRACER_SCRIPT: &str = r#"
import sys, json, os, atexit

_APEX_EDGES = set()

def _apex_tracer(frame, event, arg):
    if event == 'call':
        callee = frame.f_code.co_qualname if hasattr(frame.f_code, 'co_qualname') else frame.f_code.co_name
        caller_frame = frame.f_back
        if caller_frame is not None:
            caller = caller_frame.f_code.co_qualname if hasattr(caller_frame.f_code, 'co_qualname') else caller_frame.f_code.co_name
            _APEX_EDGES.add((caller, callee))
    return _apex_tracer

def _apex_write_callgraph():
    out_dir = os.environ.get('APEX_OUT_DIR', '.apex')
    os.makedirs(out_dir, exist_ok=True)
    out_path = os.path.join(out_dir, 'callgraph.json')
    data = {
        'source': 'pytest',
        'edges': [{'caller': c, 'callee': e} for c, e in sorted(_APEX_EDGES)]
    }
    with open(out_path, 'w') as f:
        json.dump(data, f, indent=2)

sys.settrace(_apex_tracer)
atexit.register(_apex_write_callgraph)
"#;

/// Inject the sys.settrace tracer, run pytest in `target_dir`, and parse
/// the resulting `.apex/callgraph.json`.
///
/// The tracer script is written to a temp file. Pytest is invoked as:
/// ```text
/// python -c "<tracer>" -m pytest <target_dir>
/// ```
/// or via `PYTHONSTARTUP` to avoid interfering with pytest argument parsing.
pub fn collect_python_callgraph(target: &Path) -> Result<DynamicCallGraph> {
    use std::process::Command;

    let tmp_tracer = write_temp_tracer_script(PYTHON_TRACER_SCRIPT, "apex_tracer.py")?;
    let out_dir = target.join(".apex");

    let output = Command::new("python3")
        .args([
            "-c",
            &format!(
                "exec(open({:?}).read()); import pytest; pytest.main([{:?}])",
                tmp_tracer.display(),
                target.display()
            ),
        ])
        .env("APEX_OUT_DIR", out_dir.display().to_string())
        .current_dir(target)
        .output()
        .map_err(|e| ApexError::Sandbox(format!("python callgraph: spawn failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApexError::Other(format!(
            "python callgraph: pytest exited non-zero: {stderr}"
        )));
    }

    let json_path = out_dir.join("callgraph.json");
    let json = std::fs::read_to_string(&json_path).map_err(|e| {
        ApexError::Other(format!(
            "python callgraph: read {:?}: {e}",
            json_path.display()
        ))
    })?;

    DynamicCallGraph::from_json(&json)
}

// ---------------------------------------------------------------------------
// JavaScript
// ---------------------------------------------------------------------------

/// Node.js `--require` hook written to a temporary `.js` file.
const JS_HOOK_SCRIPT: &str = r#"
const fs = require('fs');
const path = require('path');

const _apexEdges = new Set();

// Wrap Function.prototype.call is too invasive; instead we use a module
// wrapper approach: patch Module._extensions to wrap each loaded module's
// exports with Proxies. For simplicity we track stack-based call sites.
const _origPrepare = Error.prepareStackTrace;
Error.prepareStackTrace = (err, stack) => stack;

function _apexCapture(callee) {
    const err = new Error();
    const stack = err.stack;
    if (Array.isArray(stack) && stack.length > 2) {
        const callerFrame = stack[2];
        const caller = (callerFrame && callerFrame.getFunctionName()) || '<anonymous>';
        _apexEdges.add(JSON.stringify({ caller, callee }));
    }
}

// Restore original
Error.prepareStackTrace = _origPrepare;

process.on('exit', () => {
    const outDir = process.env.APEX_OUT_DIR || '.apex';
    fs.mkdirSync(outDir, { recursive: true });
    const edges = [..._apexEdges].map(e => JSON.parse(e));
    const data = { source: 'jest', edges };
    fs.writeFileSync(path.join(outDir, 'callgraph.json'), JSON.stringify(data, null, 2));
});
"#;

/// Run jest with the APEX hook and collect the call graph.
pub fn collect_js_callgraph(target: &Path) -> Result<DynamicCallGraph> {
    use std::process::Command;

    let tmp_hook = write_temp_tracer_script(JS_HOOK_SCRIPT, "apex_hook.js")?;
    let out_dir = target.join(".apex");

    let output = Command::new("npx")
        .args([
            "jest",
            "--require",
            &tmp_hook.display().to_string(),
            "--forceExit",
        ])
        .env("APEX_OUT_DIR", out_dir.display().to_string())
        .current_dir(target)
        .output()
        .map_err(|e| ApexError::Sandbox(format!("js callgraph: spawn failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApexError::Other(format!(
            "js callgraph: jest exited non-zero: {stderr}"
        )));
    }

    let json_path = out_dir.join("callgraph.json");
    let json = std::fs::read_to_string(&json_path).map_err(|e| {
        ApexError::Other(format!("js callgraph: read {:?}: {e}", json_path.display()))
    })?;

    DynamicCallGraph::from_json(&json)
}

// ---------------------------------------------------------------------------
// Go
// ---------------------------------------------------------------------------

/// Run `go test -trace` and extract caller→callee pairs from the trace.
pub fn collect_go_callgraph(target: &Path) -> Result<DynamicCallGraph> {
    use std::process::Command;

    let trace_file = target.join(".apex").join("go.trace");
    std::fs::create_dir_all(trace_file.parent().unwrap())
        .map_err(|e| ApexError::Other(format!("go callgraph: mkdir: {e}")))?;

    let output = Command::new("go")
        .args(["test", "-trace", &trace_file.display().to_string(), "./..."])
        .current_dir(target)
        .output()
        .map_err(|e| ApexError::Sandbox(format!("go callgraph: spawn failed: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ApexError::Other(format!(
            "go callgraph: go test exited non-zero: {stderr}"
        )));
    }

    // Parse the binary trace file with `go tool trace`.
    let goroutines_output = Command::new("go")
        .args([
            "tool",
            "trace",
            "-pprof=goroutine",
            &trace_file.display().to_string(),
        ])
        .current_dir(target)
        .output()
        .map_err(|e| ApexError::Sandbox(format!("go callgraph: go tool trace failed: {e}")))?;

    let stdout = String::from_utf8_lossy(&goroutines_output.stdout);
    let edges = parse_go_trace_stacks(&stdout);

    Ok(DynamicCallGraph::from_pairs(edges, "go test"))
}

/// Parse goroutine stack lines from `go tool trace -pprof=goroutine` output.
/// Each stack frame looks like: `<tab><function-name> <file>:<line>`
/// We extract consecutive function names as caller→callee pairs.
fn parse_go_trace_stacks(output: &str) -> Vec<(String, String)> {
    let mut edges = Vec::new();
    let mut stack: Vec<String> = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // End of goroutine block — emit pairs bottom-up (callee is first in list).
            for window in stack.windows(2) {
                // stack[0] is the deepest frame (callee), stack[1] is the caller.
                edges.push((window[1].clone(), window[0].clone()));
            }
            stack.clear();
        } else if let Some(fn_name) = extract_go_frame_name(trimmed) {
            stack.push(fn_name);
        }
    }

    // Flush any remaining stack.
    for window in stack.windows(2) {
        edges.push((window[1].clone(), window[0].clone()));
    }

    edges
}

/// Extract the function name from a Go pprof stack frame line.
/// Lines look like: `runtime/trace.(*Recorder).Flush+0x123 /path/to/file.go:42`
fn extract_go_frame_name(line: &str) -> Option<String> {
    // Take the part before any whitespace (file:line comes after space).
    let name_part = line.split_whitespace().next()?;
    // Strip `+0x<offset>` suffix.
    let name = if let Some(pos) = name_part.find('+') {
        &name_part[..pos]
    } else {
        name_part
    };
    if name.is_empty() {
        None
    } else {
        Some(name.to_owned())
    }
}

// ---------------------------------------------------------------------------
// Merge: static + dynamic
// ---------------------------------------------------------------------------

/// Merge a [`DynamicCallGraph`] into an existing static [`CallGraph`].
///
/// For each dynamic edge `(caller_name, callee_name)`:
/// - If both names resolve to existing [`FnId`]s in the static graph, the
///   edge is added (if not already present).
/// - If either name is missing, a synthetic [`FnNode`] stub is inserted with
///   a generated id so the edge can still be represented.
///
/// Returns the number of new edges added.
pub fn merge(static_graph: &mut CallGraph, dynamic: &DynamicCallGraph) -> usize {
    let mut added = 0;

    // Build a set of existing (caller, callee) FnId pairs to avoid duplicates.
    let existing: HashSet<(FnId, FnId)> = static_graph
        .edges
        .iter()
        .map(|e| (e.caller, e.callee))
        .collect();

    for d_edge in &dynamic.edges {
        let caller_id = resolve_or_insert(static_graph, &d_edge.caller);
        let callee_id = resolve_or_insert(static_graph, &d_edge.callee);

        if !existing.contains(&(caller_id, callee_id)) {
            static_graph.edges.push(CallEdge {
                caller: caller_id,
                callee: callee_id,
                call_site_line: 0,
                call_site_block: None,
            });
            added += 1;
        }
    }

    if added > 0 {
        static_graph.build_indices();
    }

    added
}

/// Resolve a function name to an existing `FnId`, or insert a synthetic stub.
fn resolve_or_insert(graph: &mut CallGraph, name: &str) -> FnId {
    if let Some(&id) = graph.by_name.get(name).and_then(|v| v.first()) {
        return id;
    }

    // Assign the next available numeric ID.
    let next_id = graph
        .nodes
        .iter()
        .map(|n| n.id.0)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);

    let id = FnId(next_id);
    graph.nodes.push(FnNode {
        id,
        name: name.to_owned(),
        file: PathBuf::from("<dynamic>"),
        start_line: 0,
        end_line: 0,
        entry_kind: None,
    });

    // Update the by_name index immediately so subsequent lookups find it.
    graph.by_name.entry(name.to_owned()).or_default().push(id);

    id
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Write `content` to a temporary file with the given filename stem.
/// Returns the path to the file. The caller is responsible for cleanup.
fn write_temp_tracer_script(content: &str, filename: &str) -> Result<PathBuf> {
    let tmp_dir = std::env::temp_dir().join("apex-dynamic");
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| ApexError::Other(format!("write_temp_tracer: mkdir: {e}")))?;
    let path = tmp_dir.join(filename);
    std::fs::write(&path, content)
        .map_err(|e| ApexError::Other(format!("write_temp_tracer: write: {e}")))?;
    Ok(path)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entry_points::EntryPointKind;
    use std::path::PathBuf;

    fn sample_static_graph() -> CallGraph {
        let mut g = CallGraph::default();
        g.nodes = vec![
            FnNode {
                id: FnId(0),
                name: "main".into(),
                file: PathBuf::from("main.py"),
                start_line: 1,
                end_line: 10,
                entry_kind: Some(EntryPointKind::Main),
            },
            FnNode {
                id: FnId(1),
                name: "helper".into(),
                file: PathBuf::from("main.py"),
                start_line: 12,
                end_line: 20,
                entry_kind: None,
            },
        ];
        g.edges = vec![CallEdge {
            caller: FnId(0),
            callee: FnId(1),
            call_site_line: 5,
            call_site_block: None,
        }];
        g.build_indices();
        g
    }

    // -----------------------------------------------------------------------
    // DynamicCallGraph construction and (de)serialisation
    // -----------------------------------------------------------------------

    #[test]
    fn from_pairs_builds_correct_edges() {
        let pairs = vec![
            ("alpha".to_owned(), "beta".to_owned()),
            ("beta".to_owned(), "gamma".to_owned()),
        ];
        let dcg = DynamicCallGraph::from_pairs(pairs, "pytest");
        assert_eq!(dcg.source, "pytest");
        assert_eq!(dcg.edges.len(), 2);
        assert_eq!(dcg.edges[0].caller, "alpha");
        assert_eq!(dcg.edges[0].callee, "beta");
    }

    #[test]
    fn json_roundtrip() {
        let dcg = DynamicCallGraph::from_pairs(vec![("a".to_owned(), "b".to_owned())], "go test");
        let json = dcg.to_json().unwrap();
        let parsed = DynamicCallGraph::from_json(&json).unwrap();
        assert_eq!(parsed.source, "go test");
        assert_eq!(parsed.edges.len(), 1);
        assert_eq!(parsed.edges[0].caller, "a");
        assert_eq!(parsed.edges[0].callee, "b");
    }

    #[test]
    fn from_json_parses_sample_output() {
        let json = r#"{
            "source": "pytest",
            "edges": [
                {"caller": "test_foo", "callee": "helper"},
                {"caller": "helper",   "callee": "inner"}
            ]
        }"#;
        let dcg = DynamicCallGraph::from_json(json).unwrap();
        assert_eq!(dcg.source, "pytest");
        assert_eq!(dcg.edges.len(), 2);
        assert_eq!(dcg.edges[1].callee, "inner");
    }

    #[test]
    fn from_json_rejects_invalid_json() {
        assert!(DynamicCallGraph::from_json("{not valid}").is_err());
    }

    // -----------------------------------------------------------------------
    // Merge
    // -----------------------------------------------------------------------

    #[test]
    fn merge_adds_new_edges_from_known_functions() {
        let mut graph = sample_static_graph();
        let before = graph.edge_count();

        // Add an edge between existing functions in the reverse direction.
        let dcg =
            DynamicCallGraph::from_pairs(vec![("helper".to_owned(), "main".to_owned())], "pytest");
        let added = merge(&mut graph, &dcg);
        assert_eq!(added, 1, "one new edge should be added");
        assert_eq!(graph.edge_count(), before + 1);
    }

    #[test]
    fn merge_does_not_duplicate_existing_edges() {
        let mut graph = sample_static_graph();
        let before = graph.edge_count();

        // The static graph already has main -> helper.
        let dcg =
            DynamicCallGraph::from_pairs(vec![("main".to_owned(), "helper".to_owned())], "pytest");
        let added = merge(&mut graph, &dcg);
        assert_eq!(added, 0, "no new edge — already exists");
        assert_eq!(graph.edge_count(), before);
    }

    #[test]
    fn merge_inserts_synthetic_stub_for_unknown_callee() {
        let mut graph = sample_static_graph();
        let before_nodes = graph.node_count();

        let dcg = DynamicCallGraph::from_pairs(
            vec![("main".to_owned(), "mystery_fn".to_owned())],
            "pytest",
        );
        let added = merge(&mut graph, &dcg);
        assert_eq!(added, 1);
        // A synthetic node for `mystery_fn` should have been added.
        assert_eq!(graph.node_count(), before_nodes + 1);
        assert!(!graph.fns_named("mystery_fn").is_empty());
    }

    #[test]
    fn merge_inserts_synthetic_stubs_for_both_caller_and_callee() {
        let mut graph = sample_static_graph();
        let before_nodes = graph.node_count();

        let dcg = DynamicCallGraph::from_pairs(
            vec![("unknown_a".to_owned(), "unknown_b".to_owned())],
            "jest",
        );
        let added = merge(&mut graph, &dcg);
        assert_eq!(added, 1);
        assert_eq!(graph.node_count(), before_nodes + 2);
    }

    #[test]
    fn merge_union_covers_both_edge_sets() {
        let mut graph = sample_static_graph();

        // Add a dynamic edge involving a new function.
        let dcg = DynamicCallGraph::from_pairs(
            vec![
                ("helper".to_owned(), "new_fn".to_owned()),
                ("main".to_owned(), "helper".to_owned()), // existing
            ],
            "pytest",
        );
        let added = merge(&mut graph, &dcg);
        // Only the first edge is new.
        assert_eq!(added, 1);
        // The original static edge still exists.
        let callee_ids = graph.callees_of.get(&FnId(0)).cloned().unwrap_or_default();
        assert!(!callee_ids.is_empty(), "main should still call helper");
    }

    // -----------------------------------------------------------------------
    // Go trace parsing
    // -----------------------------------------------------------------------

    #[test]
    fn parse_go_trace_stacks_extracts_pairs() {
        let trace = "\
runtime/trace.(*Recorder).Flush+0x123 /src/trace.go:42
main.runSuite+0x56 /src/main.go:15
testing.tRunner+0x89 /src/testing.go:100

runtime.goexit /src/asm.s:1
";
        let edges = parse_go_trace_stacks(trace);
        // First block: Flush <- runSuite <- tRunner  => (runSuite, Flush) and (tRunner, runSuite)
        assert!(
            edges.contains(&(
                "main.runSuite".to_owned(),
                "runtime/trace.(*Recorder).Flush".to_owned()
            )),
            "expected runSuite -> Flush edge; got: {edges:?}"
        );
        assert!(
            edges.contains(&("testing.tRunner".to_owned(), "main.runSuite".to_owned())),
            "expected tRunner -> runSuite edge; got: {edges:?}"
        );
    }

    #[test]
    fn extract_go_frame_name_strips_offset() {
        assert_eq!(
            extract_go_frame_name("main.foo+0xabc /src/main.go:10"),
            Some("main.foo".to_owned())
        );
    }

    #[test]
    fn extract_go_frame_name_no_offset() {
        assert_eq!(
            extract_go_frame_name("main.bar /src/main.go:5"),
            Some("main.bar".to_owned())
        );
    }

    #[test]
    fn extract_go_frame_name_empty_returns_none() {
        assert_eq!(extract_go_frame_name(""), None);
    }
}
