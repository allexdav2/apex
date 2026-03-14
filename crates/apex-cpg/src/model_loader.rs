//! TOML-based taint model loader.
//!
//! Parses framework model files (e.g. `flask.toml`, `django.toml`) that declare
//! taint sources, sinks, and sanitizers. This replaces hardcoded source/sink
//! tables with external, user-extensible model files.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// A complete taint model, possibly merged from multiple framework files.
#[derive(Debug, Clone, Deserialize)]
pub struct TaintModel {
    pub metadata: ModelMetadata,
    #[serde(default)]
    pub sources: Vec<TaintSource>,
    #[serde(default)]
    pub sinks: Vec<TaintSink>,
    #[serde(default)]
    pub sanitizers: Vec<TaintSanitizer>,
}

/// Metadata identifying the framework a model file covers.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelMetadata {
    pub framework: String,
    #[serde(default)]
    pub version: Option<String>,
}

/// A function or attribute that introduces tainted data.
#[derive(Debug, Clone, Deserialize)]
pub struct TaintSource {
    pub function: String,
    pub kind: String,
    #[serde(default)]
    pub returns: bool,
    #[serde(default)]
    pub attribute: bool,
}

/// A function where tainted data reaching it constitutes a vulnerability.
#[derive(Debug, Clone, Deserialize)]
pub struct TaintSink {
    pub function: String,
    pub kind: String,
    #[serde(default)]
    pub parameters: Vec<usize>,
    #[serde(default)]
    pub cwe: Option<u32>,
}

/// A function that neutralises taint for a specific vulnerability kind.
#[derive(Debug, Clone, Deserialize)]
pub struct TaintSanitizer {
    pub function: String,
    pub kind: String,
    #[serde(default)]
    pub returns: bool,
}

/// Load a single TOML model file and deserialize it into a [`TaintModel`].
pub fn load_model(path: &Path) -> Result<TaintModel> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read model file: {}", path.display()))?;
    let model: TaintModel = toml::from_str(&content)
        .with_context(|| format!("failed to parse model file: {}", path.display()))?;
    Ok(model)
}

/// Load all `.toml` model files from a directory and merge them into a single
/// [`TaintModel`].
///
/// The merged model uses `"merged"` as its framework name. Sources, sinks, and
/// sanitizers from every file are concatenated.
pub fn load_models(model_dir: &Path) -> Result<TaintModel> {
    let mut entries: Vec<_> = std::fs::read_dir(model_dir)
        .with_context(|| format!("failed to read model directory: {}", model_dir.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml"))
        .collect();

    // Sort for deterministic merge order.
    entries.sort_by_key(|e| e.file_name());

    let mut merged = TaintModel {
        metadata: ModelMetadata {
            framework: "merged".into(),
            version: None,
        },
        sources: Vec::new(),
        sinks: Vec::new(),
        sanitizers: Vec::new(),
    };

    for entry in entries {
        let model = load_model(&entry.path())?;
        merged.sources.extend(model.sources);
        merged.sinks.extend(model.sinks);
        merged.sanitizers.extend(model.sanitizers);
    }

    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Helper: create a temp dir and write model files into it.
    fn write_model(dir: &Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn loads_single_model_file() {
        let dir = tempfile::tempdir().unwrap();
        write_model(
            dir.path(),
            "test.toml",
            r#"
[metadata]
framework = "test"

[[sources]]
function = "get_input"
kind = "UserInput"
returns = true

[[sinks]]
function = "run_cmd"
kind = "CommandInjection"
parameters = [0]
cwe = 78
"#,
        );

        let model = load_model(&dir.path().join("test.toml")).unwrap();
        assert_eq!(model.metadata.framework, "test");
        assert_eq!(model.sources.len(), 1);
        assert_eq!(model.sinks.len(), 1);
        assert!(model.sources[0].returns);
        assert_eq!(model.sinks[0].cwe, Some(78));
    }

    #[test]
    fn loads_multiple_models_from_directory() {
        let dir = tempfile::tempdir().unwrap();
        write_model(
            dir.path(),
            "a.toml",
            r#"
[metadata]
framework = "a"

[[sources]]
function = "a.source"
kind = "UserInput"
"#,
        );
        write_model(
            dir.path(),
            "b.toml",
            r#"
[metadata]
framework = "b"

[[sources]]
function = "b.source"
kind = "UserInput"
"#,
        );

        let merged = load_models(dir.path()).unwrap();
        assert_eq!(merged.metadata.framework, "merged");
        assert_eq!(merged.sources.len(), 2);
    }

    #[test]
    fn flask_model_has_expected_sources_and_sinks() {
        let flask_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("models/flask.toml");
        let model = load_model(&flask_path).unwrap();
        assert_eq!(model.metadata.framework, "flask");
        assert_eq!(model.sources.len(), 5);
        assert_eq!(model.sinks.len(), 3);
        assert_eq!(model.sanitizers.len(), 2);

        // Check a specific source
        assert!(model
            .sources
            .iter()
            .any(|s| s.function == "flask.request.args.get"));

        // Check a specific sink
        assert!(model
            .sinks
            .iter()
            .any(|s| s.function == "os.system" && s.kind == "CommandInjection"));
    }

    #[test]
    fn django_model_has_expected_cwe_ids() {
        let django_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("models/django.toml");
        let model = load_model(&django_path).unwrap();
        assert_eq!(model.metadata.framework, "django");

        let cwes: Vec<u32> = model.sinks.iter().filter_map(|s| s.cwe).collect();
        assert!(cwes.contains(&78), "expected CWE-78 (command injection)");
        assert!(cwes.contains(&89), "expected CWE-89 (SQL injection)");
    }

    #[test]
    fn empty_model_file_deserializes_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        write_model(
            dir.path(),
            "empty.toml",
            r#"
[metadata]
framework = "empty"
"#,
        );

        let model = load_model(&dir.path().join("empty.toml")).unwrap();
        assert_eq!(model.metadata.framework, "empty");
        assert!(model.sources.is_empty());
        assert!(model.sinks.is_empty());
        assert!(model.sanitizers.is_empty());
    }

    #[test]
    fn invalid_toml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        write_model(dir.path(), "bad.toml", "this is not valid toml {{{{");

        let result = load_model(&dir.path().join("bad.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn model_merge_combines_sources_from_multiple_files() {
        let dir = tempfile::tempdir().unwrap();
        write_model(
            dir.path(),
            "fw1.toml",
            r#"
[metadata]
framework = "fw1"

[[sources]]
function = "fw1.input"
kind = "UserInput"

[[sinks]]
function = "fw1.exec"
kind = "CommandInjection"
parameters = [0]
"#,
        );
        write_model(
            dir.path(),
            "fw2.toml",
            r#"
[metadata]
framework = "fw2"

[[sources]]
function = "fw2.input"
kind = "UserInput"

[[sanitizers]]
function = "fw2.clean"
kind = "XSS"
returns = true
"#,
        );

        let merged = load_models(dir.path()).unwrap();
        assert_eq!(merged.sources.len(), 2);
        assert_eq!(merged.sinks.len(), 1);
        assert_eq!(merged.sanitizers.len(), 1);

        let source_fns: Vec<&str> = merged.sources.iter().map(|s| s.function.as_str()).collect();
        assert!(source_fns.contains(&"fw1.input"));
        assert!(source_fns.contains(&"fw2.input"));
    }

    #[test]
    fn load_models_ignores_non_toml_files() {
        let dir = tempfile::tempdir().unwrap();
        write_model(
            dir.path(),
            "model.toml",
            r#"
[metadata]
framework = "real"

[[sources]]
function = "real.input"
kind = "UserInput"
"#,
        );
        write_model(dir.path(), "readme.txt", "not a model file");

        let merged = load_models(dir.path()).unwrap();
        assert_eq!(merged.sources.len(), 1);
    }

    #[test]
    fn load_models_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let merged = load_models(dir.path()).unwrap();
        assert!(merged.sources.is_empty());
        assert!(merged.sinks.is_empty());
        assert!(merged.sanitizers.is_empty());
    }

    #[test]
    fn source_defaults() {
        let dir = tempfile::tempdir().unwrap();
        write_model(
            dir.path(),
            "defaults.toml",
            r#"
[metadata]
framework = "defaults"

[[sources]]
function = "src"
kind = "UserInput"
"#,
        );
        let model = load_model(&dir.path().join("defaults.toml")).unwrap();
        assert!(!model.sources[0].returns);
        assert!(!model.sources[0].attribute);
    }

    #[test]
    fn sink_defaults() {
        let dir = tempfile::tempdir().unwrap();
        write_model(
            dir.path(),
            "defaults.toml",
            r#"
[metadata]
framework = "defaults"

[[sinks]]
function = "snk"
kind = "Injection"
"#,
        );
        let model = load_model(&dir.path().join("defaults.toml")).unwrap();
        assert!(model.sinks[0].parameters.is_empty());
        assert_eq!(model.sinks[0].cwe, None);
    }
}
