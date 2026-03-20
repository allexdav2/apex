//! Runtime-extensible taint specification store.
//!
//! Replaces hardcoded PYTHON_SOURCES/SINKS/SANITIZERS with a dynamic store
//! that can be extended at runtime (e.g., by LLM-inferred specs from IRIS).

use std::collections::HashSet;

/// Runtime-extensible store for taint analysis source/sink/sanitizer specifications.
#[derive(Debug, Clone, Default)]
pub struct TaintSpecStore {
    sources: HashSet<String>,
    sinks: HashSet<String>,
    sanitizers: HashSet<String>,
}

/// JavaScript taint sources.
pub const JS_SOURCES: &[&str] = &[
    "req.query",
    "req.params",
    "req.body",
    "process.argv",
    "process.env",
    "window.location",
];

/// JavaScript taint sinks.
pub const JS_SINKS: &[&str] = &[
    "eval",
    "exec",
    "child_process",
    "fs.writeFile",
    "res.send",
    "db.query",
];

/// JavaScript sanitizers.
pub const JS_SANITIZERS: &[&str] = &[
    "escape",
    "encodeURI",
    "sanitize",
    "DOMPurify",
];

/// Go taint sources.
pub const GO_SOURCES: &[&str] = &[
    "r.URL.Query",
    "r.FormValue",
    "r.Body",
    "os.Args",
    "os.Getenv",
];

/// Go taint sinks.
pub const GO_SINKS: &[&str] = &[
    "exec.Command",
    "db.Query",
    "db.Exec",
    "http.Get",
    "fmt.Fprintf",
];

/// Go sanitizers.
pub const GO_SANITIZERS: &[&str] = &[
    "html.EscapeString",
    "url.QueryEscape",
    "strconv",
];

impl TaintSpecStore {
    /// Create an empty store.
    pub fn new() -> Self {
        Default::default()
    }

    /// Create a store pre-loaded with Python defaults from the existing hardcoded arrays.
    pub fn python_defaults() -> Self {
        use crate::taint::{PYTHON_SANITIZERS, PYTHON_SINKS, PYTHON_SOURCES};
        let mut store = Self::new();
        for s in PYTHON_SOURCES {
            store.sources.insert(s.to_string());
        }
        for s in PYTHON_SINKS {
            store.sinks.insert(s.to_string());
        }
        for s in PYTHON_SANITIZERS {
            store.sanitizers.insert(s.to_string());
        }
        store
    }

    /// Create a store pre-loaded with JavaScript taint specs.
    pub fn javascript_defaults() -> Self {
        let mut store = Self::new();
        for s in JS_SOURCES {
            store.sources.insert(s.to_string());
        }
        for s in JS_SINKS {
            store.sinks.insert(s.to_string());
        }
        for s in JS_SANITIZERS {
            store.sanitizers.insert(s.to_string());
        }
        store
    }

    /// Create a store pre-loaded with Go taint specs.
    pub fn go_defaults() -> Self {
        let mut store = Self::new();
        for s in GO_SOURCES {
            store.sources.insert(s.to_string());
        }
        for s in GO_SINKS {
            store.sinks.insert(s.to_string());
        }
        for s in GO_SANITIZERS {
            store.sanitizers.insert(s.to_string());
        }
        store
    }

    pub fn add_source(&mut self, name: String) {
        self.sources.insert(name);
    }
    pub fn add_sink(&mut self, name: String) {
        self.sinks.insert(name);
    }
    pub fn add_sanitizer(&mut self, name: String) {
        self.sanitizers.insert(name);
    }

    pub fn is_source(&self, name: &str) -> bool {
        self.sources.contains(name)
    }
    pub fn is_sink(&self, name: &str) -> bool {
        self.sinks.contains(name)
    }
    pub fn is_sanitizer(&self, name: &str) -> bool {
        self.sanitizers.contains(name)
    }

    pub fn sources(&self) -> &HashSet<String> {
        &self.sources
    }
    pub fn sinks(&self) -> &HashSet<String> {
        &self.sinks
    }
    pub fn sanitizers(&self) -> &HashSet<String> {
        &self.sanitizers
    }

    /// Total number of specs (sources + sinks + sanitizers).
    pub fn len(&self) -> usize {
        self.sources.len() + self.sinks.len() + self.sanitizers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sources.is_empty() && self.sinks.is_empty() && self.sanitizers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_store() {
        let store = TaintSpecStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
        assert!(!store.is_source("anything"));
        assert!(!store.is_sink("anything"));
        assert!(!store.is_sanitizer("anything"));
    }

    #[test]
    fn add_and_check_source() {
        let mut store = TaintSpecStore::new();
        store.add_source("os.environ.get".into());
        assert!(store.is_source("os.environ.get"));
        assert!(!store.is_sink("os.environ.get"));
    }

    #[test]
    fn add_and_check_sink() {
        let mut store = TaintSpecStore::new();
        store.add_sink("subprocess.call".into());
        assert!(store.is_sink("subprocess.call"));
    }

    #[test]
    fn add_and_check_sanitizer() {
        let mut store = TaintSpecStore::new();
        store.add_sanitizer("shlex.quote".into());
        assert!(store.is_sanitizer("shlex.quote"));
    }

    #[test]
    fn python_defaults_has_sources() {
        let store = TaintSpecStore::python_defaults();
        assert!(!store.is_empty());
        assert!(
            store.sources().len() >= 3,
            "expected at least 3 default Python sources"
        );
    }

    #[test]
    fn python_defaults_has_sinks() {
        let store = TaintSpecStore::python_defaults();
        assert!(
            store.sinks().len() >= 3,
            "expected at least 3 default Python sinks"
        );
    }

    #[test]
    fn python_defaults_has_sanitizers() {
        let store = TaintSpecStore::python_defaults();
        assert!(
            store.sanitizers().len() >= 1,
            "expected at least 1 default Python sanitizer"
        );
    }

    #[test]
    fn extend_beyond_defaults() {
        let mut store = TaintSpecStore::python_defaults();
        let before = store.len();
        store.add_source("custom.input".into());
        store.add_sink("custom.output".into());
        assert_eq!(store.len(), before + 2);
        assert!(store.is_source("custom.input"));
        assert!(store.is_sink("custom.output"));
    }

    #[test]
    fn duplicate_insert_idempotent() {
        let mut store = TaintSpecStore::new();
        store.add_source("foo".into());
        store.add_source("foo".into());
        assert_eq!(store.sources().len(), 1);
    }

    // ── JavaScript taint spec tests ──────────────────────────────────────────

    #[test]
    fn js_defaults_has_sources() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(!store.is_empty());
        assert!(
            store.sources().len() >= 3,
            "expected at least 3 JS sources, got {}",
            store.sources().len()
        );
    }

    #[test]
    fn js_defaults_req_query_is_source() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(store.is_source("req.query"), "req.query should be a JS source");
    }

    #[test]
    fn js_defaults_req_params_is_source() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(store.is_source("req.params"), "req.params should be a JS source");
    }

    #[test]
    fn js_defaults_req_body_is_source() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(store.is_source("req.body"), "req.body should be a JS source");
    }

    #[test]
    fn js_defaults_process_argv_is_source() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(store.is_source("process.argv"), "process.argv should be a JS source");
    }

    #[test]
    fn js_defaults_eval_is_sink() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(store.is_sink("eval"), "eval should be a JS sink");
    }

    #[test]
    fn js_defaults_db_query_is_sink() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(store.is_sink("db.query"), "db.query should be a JS sink");
    }

    #[test]
    fn js_defaults_res_send_is_sink() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(store.is_sink("res.send"), "res.send should be a JS sink");
    }

    #[test]
    fn js_defaults_escape_is_sanitizer() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(store.is_sanitizer("escape"), "escape should be a JS sanitizer");
    }

    #[test]
    fn js_defaults_encode_uri_is_sanitizer() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(store.is_sanitizer("encodeURI"), "encodeURI should be a JS sanitizer");
    }

    #[test]
    fn js_defaults_has_sinks() {
        let store = TaintSpecStore::javascript_defaults();
        assert!(
            store.sinks().len() >= 3,
            "expected at least 3 JS sinks"
        );
    }

    // ── Go taint spec tests ──────────────────────────────────────────────────

    #[test]
    fn go_defaults_has_sources() {
        let store = TaintSpecStore::go_defaults();
        assert!(!store.is_empty());
        assert!(
            store.sources().len() >= 3,
            "expected at least 3 Go sources, got {}",
            store.sources().len()
        );
    }

    #[test]
    fn go_defaults_url_query_is_source() {
        let store = TaintSpecStore::go_defaults();
        assert!(store.is_source("r.URL.Query"), "r.URL.Query should be a Go source");
    }

    #[test]
    fn go_defaults_form_value_is_source() {
        let store = TaintSpecStore::go_defaults();
        assert!(store.is_source("r.FormValue"), "r.FormValue should be a Go source");
    }

    #[test]
    fn go_defaults_os_getenv_is_source() {
        let store = TaintSpecStore::go_defaults();
        assert!(store.is_source("os.Getenv"), "os.Getenv should be a Go source");
    }

    #[test]
    fn go_defaults_exec_command_is_sink() {
        let store = TaintSpecStore::go_defaults();
        assert!(store.is_sink("exec.Command"), "exec.Command should be a Go sink");
    }

    #[test]
    fn go_defaults_db_query_is_sink() {
        let store = TaintSpecStore::go_defaults();
        assert!(store.is_sink("db.Query"), "db.Query should be a Go sink");
    }

    #[test]
    fn go_defaults_db_exec_is_sink() {
        let store = TaintSpecStore::go_defaults();
        assert!(store.is_sink("db.Exec"), "db.Exec should be a Go sink");
    }

    #[test]
    fn go_defaults_http_get_is_sink() {
        let store = TaintSpecStore::go_defaults();
        assert!(store.is_sink("http.Get"), "http.Get should be a Go sink");
    }

    #[test]
    fn go_defaults_html_escape_is_sanitizer() {
        let store = TaintSpecStore::go_defaults();
        assert!(store.is_sanitizer("html.EscapeString"), "html.EscapeString should be a Go sanitizer");
    }

    #[test]
    fn go_defaults_url_query_escape_is_sanitizer() {
        let store = TaintSpecStore::go_defaults();
        assert!(store.is_sanitizer("url.QueryEscape"), "url.QueryEscape should be a Go sanitizer");
    }

    #[test]
    fn go_defaults_has_sinks() {
        let store = TaintSpecStore::go_defaults();
        assert!(
            store.sinks().len() >= 3,
            "expected at least 3 Go sinks"
        );
    }
}
