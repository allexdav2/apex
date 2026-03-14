//! Syscall specification mining.
//! Based on the Caruca paper — learns normal syscall sequences from test runs
//! and flags deviations as potential security issues.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A learned specification of allowed syscalls for a function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyscallSpec {
    pub function_name: String,
    pub allowed_calls: HashSet<String>,
}

impl SyscallSpec {
    pub fn new(function_name: &str) -> Self {
        SyscallSpec {
            function_name: function_name.to_string(),
            allowed_calls: HashSet::new(),
        }
    }

    /// Learn a spec from observed syscall traces.
    pub fn learn(function_name: &str, traces: &[Vec<String>]) -> Self {
        let mut allowed = HashSet::new();
        for trace in traces {
            for call in trace {
                allowed.insert(call.clone());
            }
        }
        SyscallSpec {
            function_name: function_name.to_string(),
            allowed_calls: allowed,
        }
    }

    /// Check a trace against this spec, returning unknown calls.
    pub fn check(&self, trace: &[String]) -> Vec<String> {
        trace
            .iter()
            .filter(|call| !self.allowed_calls.contains(call.as_str()))
            .cloned()
            .collect()
    }
}

/// Collects syscall traces per function and builds specs.
pub struct SpecMiner {
    traces: HashMap<String, Vec<Vec<String>>>,
}

impl SpecMiner {
    pub fn new() -> Self {
        SpecMiner {
            traces: HashMap::new(),
        }
    }

    pub fn add_trace(&mut self, function_name: &str, trace: Vec<String>) {
        self.traces
            .entry(function_name.to_string())
            .or_default()
            .push(trace);
    }

    pub fn build_specs(&self) -> Vec<SyscallSpec> {
        self.traces
            .iter()
            .map(|(name, traces)| SyscallSpec::learn(name, traces))
            .collect()
    }
}

impl Default for SpecMiner {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syscall_spec_creation() {
        let spec = SyscallSpec::new("my_function");
        assert_eq!(spec.function_name, "my_function");
        assert!(spec.allowed_calls.is_empty());
    }

    #[test]
    fn learn_from_traces_builds_spec() {
        let traces = vec![
            vec!["open".to_string(), "read".to_string(), "close".to_string()],
            vec!["open".to_string(), "write".to_string(), "close".to_string()],
        ];
        let spec = SyscallSpec::learn("handler", &traces);
        assert!(spec.allowed_calls.contains("open"));
        assert!(spec.allowed_calls.contains("close"));
        assert!(spec.allowed_calls.contains("read"));
        assert!(spec.allowed_calls.contains("write"));
    }

    #[test]
    fn check_violation_detects_unknown_call() {
        let mut spec = SyscallSpec::new("handler");
        spec.allowed_calls.insert("open".to_string());
        spec.allowed_calls.insert("read".to_string());
        spec.allowed_calls.insert("close".to_string());

        let trace = vec![
            "open".to_string(),
            "exec".to_string(), // not in spec!
            "close".to_string(),
        ];
        let violations = spec.check(&trace);
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0], "exec");
    }

    #[test]
    fn check_no_violations() {
        let mut spec = SyscallSpec::new("handler");
        spec.allowed_calls.insert("open".to_string());
        spec.allowed_calls.insert("close".to_string());

        let trace = vec!["open".to_string(), "close".to_string()];
        let violations = spec.check(&trace);
        assert!(violations.is_empty());
    }

    #[test]
    fn learn_empty_traces() {
        let spec = SyscallSpec::learn("empty", &[]);
        assert!(spec.allowed_calls.is_empty());
    }

    #[test]
    fn spec_miner_multiple_functions() {
        let mut miner = SpecMiner::new();
        miner.add_trace("func_a", vec!["open".into(), "read".into()]);
        miner.add_trace("func_a", vec!["open".into(), "write".into()]);
        miner.add_trace("func_b", vec!["connect".into(), "send".into()]);

        let specs = miner.build_specs();
        assert_eq!(specs.len(), 2);

        let spec_a = specs.iter().find(|s| s.function_name == "func_a").unwrap();
        assert!(spec_a.allowed_calls.contains("open"));
        assert!(spec_a.allowed_calls.contains("read"));
        assert!(spec_a.allowed_calls.contains("write"));
    }
}
