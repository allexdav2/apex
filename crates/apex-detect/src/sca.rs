//! SCA (Software Composition Analysis) manifest parser.
//!
//! Extracts dependency names and versions from manifest files
//! (`requirements.txt`, `Cargo.toml`, `package.json`).
//! This is the foundation for vulnerability scanning against the OSV database.

use std::path::{Path, PathBuf};

/// The package registry a dependency comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSource {
    PyPI,
    CratesIo,
    Npm,
}

/// A single dependency extracted from a manifest file.
#[derive(Debug, Clone)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    pub source: DepSource,
    pub manifest_file: PathBuf,
    pub line: u32,
}

/// Parse a `requirements.txt` file content.
///
/// Handles:
/// - `name==version` (pinned)
/// - `name>=version`, `name~=version`, `name<=version`, `name!=version`
/// - `name[extra]==version` (extras)
/// - `name` (no version — recorded with empty version string)
/// - Comments (`#`) and blank lines are skipped
/// - `-r`, `-c`, and other option lines are skipped
pub fn parse_requirements_txt(content: &str, manifest: &Path) -> Vec<Dependency> {
    let mut deps = Vec::new();

    for (idx, raw_line) in content.lines().enumerate() {
        let line = raw_line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Skip pip options like -r, -c, --index-url, etc.
        if line.starts_with('-') {
            continue;
        }

        // Strip inline comments
        let line = if let Some(pos) = line.find(" #") {
            line[..pos].trim()
        } else {
            line
        };

        // Strip environment markers (e.g. `; python_version >= "3.8"`)
        let line = if let Some(pos) = line.find(';') {
            line[..pos].trim()
        } else {
            line
        };

        // Split on version specifier operators: ==, >=, <=, ~=, !=, >, <
        // Order matters: check two-char operators before single-char.
        let (name_part, version) =
            if let Some(pos) = find_version_operator(line) {
                let name_part = line[..pos].trim();
                let rest = &line[pos..];
                // Skip the operator (1 or 2 chars)
                let op_len = if rest.len() >= 2 && matches!(&rest[..2], "==" | ">=" | "<=" | "~=" | "!=") {
                    2
                } else {
                    1
                };
                let version = rest[op_len..].trim();
                // Handle multiple version specifiers (e.g. ">=1.0,<2.0") — take first
                let version = version.split(',').next().unwrap_or("").trim();
                (name_part, version)
            } else {
                (line, "")
            };

        // Strip extras from name: `name[extra1,extra2]` -> `name`
        let name = if let Some(bracket) = name_part.find('[') {
            name_part[..bracket].trim()
        } else {
            name_part.trim()
        };

        if name.is_empty() {
            continue;
        }

        deps.push(Dependency {
            name: name.to_string(),
            version: version.to_string(),
            source: DepSource::PyPI,
            manifest_file: manifest.to_path_buf(),
            line: (idx + 1) as u32,
        });
    }

    deps
}

/// Find the byte offset of the first version operator in a requirements line.
fn find_version_operator(s: &str) -> Option<usize> {
    // Two-char operators first
    for op in &["==", ">=", "<=", "~=", "!="] {
        if let Some(pos) = s.find(op) {
            return Some(pos);
        }
    }
    // Single-char: > or < (but not part of >= or <=)
    for (i, c) in s.char_indices() {
        if (c == '>' || c == '<') && s.get(i + 1..i + 2) != Some("=") {
            return Some(i);
        }
    }
    None
}

/// Parse a `Cargo.toml` file content.
///
/// Handles:
/// - `name = "version"` (string shorthand)
/// - `name = { version = "..." }` (table form)
/// - Workspace deps (`name = { workspace = true }`) are skipped (no local version)
/// - Deps with no version key are skipped
/// - Both `[dependencies]` and `[dev-dependencies]` sections
pub fn parse_cargo_toml(content: &str, manifest: &Path) -> Vec<Dependency> {
    let parsed: toml::Value = match content.parse() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut deps = Vec::new();

    // Build a line-number lookup: dep name -> line number in the file
    let line_map = build_cargo_line_map(content);

    for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(table) = parsed.get(section).and_then(|v| v.as_table()) {
            extract_cargo_deps(table, section, manifest, &line_map, &mut deps);
        }

        // Also handle [target.'cfg(...)'.dependencies] — scan top-level `target` table
        if let Some(targets) = parsed.get("target").and_then(|v| v.as_table()) {
            for (_cfg, target_val) in targets {
                if let Some(table) = target_val.get(section).and_then(|v| v.as_table()) {
                    extract_cargo_deps(table, section, manifest, &line_map, &mut deps);
                }
            }
        }
    }

    deps
}

fn extract_cargo_deps(
    table: &toml::map::Map<String, toml::Value>,
    _section: &str,
    manifest: &Path,
    line_map: &std::collections::HashMap<String, u32>,
    deps: &mut Vec<Dependency>,
) {
    for (name, value) in table {
        let version = match value {
            toml::Value::String(v) => v.clone(),
            toml::Value::Table(t) => {
                // Skip workspace deps
                if t.get("workspace")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    continue;
                }
                match t.get("version").and_then(|v| v.as_str()) {
                    Some(v) => v.to_string(),
                    None => continue, // path-only dep, no version
                }
            }
            _ => continue,
        };

        let line = line_map.get(name).copied().unwrap_or(0);

        deps.push(Dependency {
            name: name.clone(),
            version,
            source: DepSource::CratesIo,
            manifest_file: manifest.to_path_buf(),
            line,
        });
    }
}

/// Build a map of dependency name -> line number by scanning for `name =` patterns.
fn build_cargo_line_map(content: &str) -> std::collections::HashMap<String, u32> {
    let mut map = std::collections::HashMap::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(eq_pos) = trimmed.find('=') {
            let key = trimmed[..eq_pos].trim();
            // Only record if it looks like a dep name (no dots, no brackets)
            if !key.is_empty()
                && !key.contains('.')
                && !key.starts_with('[')
                && !key.starts_with('#')
            {
                map.entry(key.to_string())
                    .or_insert((idx + 1) as u32);
            }
        }
    }
    map
}

/// Parse a `package.json` file content.
///
/// Handles:
/// - `dependencies` and `devDependencies` sections
/// - Version strings (ranges, pinned, etc.) are stored as-is
/// - Missing sections are gracefully handled
pub fn parse_package_json(content: &str, manifest: &Path) -> Vec<Dependency> {
    let parsed: serde_json::Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let line_map = build_json_line_map(content);

    let mut deps = Vec::new();

    for section in &["dependencies", "devDependencies"] {
        if let Some(obj) = parsed.get(section).and_then(|v| v.as_object()) {
            for (name, value) in obj {
                let version = value.as_str().unwrap_or("").to_string();
                let line = line_map.get(name).copied().unwrap_or(0);

                deps.push(Dependency {
                    name: name.clone(),
                    version,
                    source: DepSource::Npm,
                    manifest_file: manifest.to_path_buf(),
                    line,
                });
            }
        }
    }

    deps
}

/// Build a map of dependency name -> line number by scanning for `"name":` patterns in JSON.
fn build_json_line_map(content: &str) -> std::collections::HashMap<String, u32> {
    let mut map = std::collections::HashMap::new();
    for (idx, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        // Match patterns like `"package-name": "version"`
        if trimmed.starts_with('"') {
            if let Some(end_quote) = trimmed[1..].find('"') {
                let key = &trimmed[1..1 + end_quote];
                if trimmed[1 + end_quote + 1..].trim_start().starts_with(':') {
                    map.entry(key.to_string())
                        .or_insert((idx + 1) as u32);
                }
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn manifest(name: &str) -> PathBuf {
        PathBuf::from(format!("/project/{name}"))
    }

    // ── requirements.txt ────────────────────────────────────────────────────

    #[test]
    fn req_pinned_version() {
        let content = "requests==2.28.0\nflask==2.3.1\n";
        let deps = parse_requirements_txt(content, &manifest("requirements.txt"));
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "requests");
        assert_eq!(deps[0].version, "2.28.0");
        assert_eq!(deps[0].source, DepSource::PyPI);
        assert_eq!(deps[1].name, "flask");
        assert_eq!(deps[1].version, "2.3.1");
    }

    #[test]
    fn req_gte_version() {
        let content = "numpy>=1.21.0\n";
        let deps = parse_requirements_txt(content, &manifest("requirements.txt"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "numpy");
        assert_eq!(deps[0].version, "1.21.0");
    }

    #[test]
    fn req_compatible_release() {
        let content = "django~=4.2\n";
        let deps = parse_requirements_txt(content, &manifest("requirements.txt"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "django");
        assert_eq!(deps[0].version, "4.2");
    }

    #[test]
    fn req_no_version() {
        let content = "pytest\n";
        let deps = parse_requirements_txt(content, &manifest("requirements.txt"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "pytest");
        assert_eq!(deps[0].version, "");
    }

    #[test]
    fn req_comments_and_blanks() {
        let content = "# This is a comment\n\nrequests==2.28.0\n\n# Another comment\n";
        let deps = parse_requirements_txt(content, &manifest("requirements.txt"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "requests");
    }

    #[test]
    fn req_extras() {
        let content = "uvicorn[standard]==0.20.0\n";
        let deps = parse_requirements_txt(content, &manifest("requirements.txt"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "uvicorn");
        assert_eq!(deps[0].version, "0.20.0");
    }

    #[test]
    fn req_option_lines_skipped() {
        let content = "-r base.txt\n--index-url https://pypi.org/simple\nflask==2.0\n";
        let deps = parse_requirements_txt(content, &manifest("requirements.txt"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "flask");
    }

    #[test]
    fn req_inline_comment() {
        let content = "requests==2.28.0  # HTTP library\n";
        let deps = parse_requirements_txt(content, &manifest("requirements.txt"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "requests");
        assert_eq!(deps[0].version, "2.28.0");
    }

    #[test]
    fn req_line_numbers() {
        let content = "# comment\nrequests==2.28.0\n\nflask==2.3.1\n";
        let deps = parse_requirements_txt(content, &manifest("requirements.txt"));
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].line, 2); // 1-indexed, skipping comment
        assert_eq!(deps[1].line, 4); // 1-indexed, skipping blank
    }

    #[test]
    fn req_empty_content() {
        let deps = parse_requirements_txt("", &manifest("requirements.txt"));
        assert!(deps.is_empty());
    }

    #[test]
    fn req_env_markers() {
        let content = "pywin32>=300; sys_platform == \"win32\"\n";
        let deps = parse_requirements_txt(content, &manifest("requirements.txt"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "pywin32");
        assert_eq!(deps[0].version, "300");
    }

    // ── Cargo.toml ──────────────────────────────────────────────────────────

    #[test]
    fn cargo_string_version() {
        let content = r#"
[dependencies]
serde = "1.0"
"#;
        let deps = parse_cargo_toml(content, &manifest("Cargo.toml"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "serde");
        assert_eq!(deps[0].version, "1.0");
        assert_eq!(deps[0].source, DepSource::CratesIo);
    }

    #[test]
    fn cargo_table_version() {
        let content = r#"
[dependencies]
tokio = { version = "1.28", features = ["full"] }
"#;
        let deps = parse_cargo_toml(content, &manifest("Cargo.toml"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "tokio");
        assert_eq!(deps[0].version, "1.28");
    }

    #[test]
    fn cargo_workspace_dep_skipped() {
        let content = r#"
[dependencies]
serde = { workspace = true }
"#;
        let deps = parse_cargo_toml(content, &manifest("Cargo.toml"));
        assert!(deps.is_empty());
    }

    #[test]
    fn cargo_path_only_dep_skipped() {
        let content = r#"
[dependencies]
my-crate = { path = "../my-crate" }
"#;
        let deps = parse_cargo_toml(content, &manifest("Cargo.toml"));
        assert!(deps.is_empty());
    }

    #[test]
    fn cargo_dev_dependencies() {
        let content = r#"
[dev-dependencies]
tempfile = "3"
"#;
        let deps = parse_cargo_toml(content, &manifest("Cargo.toml"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "tempfile");
        assert_eq!(deps[0].version, "3");
    }

    #[test]
    fn cargo_mixed_sections() {
        let content = r#"
[dependencies]
serde = "1.0"

[dev-dependencies]
tempfile = "3"
"#;
        let deps = parse_cargo_toml(content, &manifest("Cargo.toml"));
        assert_eq!(deps.len(), 2);
        let names: Vec<&str> = deps.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"serde"));
        assert!(names.contains(&"tempfile"));
    }

    #[test]
    fn cargo_line_numbers() {
        let content = r#"[package]
name = "my-crate"

[dependencies]
serde = "1.0"
tokio = { version = "1.28" }
"#;
        let deps = parse_cargo_toml(content, &manifest("Cargo.toml"));
        assert_eq!(deps.len(), 2);
        // serde is on line 5, tokio on line 6
        let serde_dep = deps.iter().find(|d| d.name == "serde").unwrap();
        assert_eq!(serde_dep.line, 5);
        let tokio_dep = deps.iter().find(|d| d.name == "tokio").unwrap();
        assert_eq!(tokio_dep.line, 6);
    }

    #[test]
    fn cargo_empty_content() {
        let deps = parse_cargo_toml("", &manifest("Cargo.toml"));
        assert!(deps.is_empty());
    }

    #[test]
    fn cargo_invalid_toml() {
        let deps = parse_cargo_toml("not [valid toml", &manifest("Cargo.toml"));
        assert!(deps.is_empty());
    }

    #[test]
    fn cargo_workspace_with_version_still_extracted() {
        // workspace = true with version override — version should be extracted
        let content = r#"
[dependencies]
serde = { workspace = true, version = "1.0" }
"#;
        // workspace = true means skip regardless of version override
        let deps = parse_cargo_toml(content, &manifest("Cargo.toml"));
        assert!(deps.is_empty());
    }

    // ── package.json ────────────────────────────────────────────────────────

    #[test]
    fn pkg_dependencies() {
        let content = r#"{
  "dependencies": {
    "express": "^4.18.0",
    "lodash": "4.17.21"
  }
}"#;
        let deps = parse_package_json(content, &manifest("package.json"));
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].source, DepSource::Npm);

        let express = deps.iter().find(|d| d.name == "express").unwrap();
        assert_eq!(express.version, "^4.18.0");

        let lodash = deps.iter().find(|d| d.name == "lodash").unwrap();
        assert_eq!(lodash.version, "4.17.21");
    }

    #[test]
    fn pkg_dev_dependencies() {
        let content = r#"{
  "devDependencies": {
    "jest": "^29.0.0"
  }
}"#;
        let deps = parse_package_json(content, &manifest("package.json"));
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "jest");
        assert_eq!(deps[0].version, "^29.0.0");
    }

    #[test]
    fn pkg_both_sections() {
        let content = r#"{
  "dependencies": {
    "express": "^4.18.0"
  },
  "devDependencies": {
    "jest": "^29.0.0"
  }
}"#;
        let deps = parse_package_json(content, &manifest("package.json"));
        assert_eq!(deps.len(), 2);
        let names: Vec<&str> = deps.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"express"));
        assert!(names.contains(&"jest"));
    }

    #[test]
    fn pkg_no_deps_section() {
        let content = r#"{"name": "my-app", "version": "1.0.0"}"#;
        let deps = parse_package_json(content, &manifest("package.json"));
        assert!(deps.is_empty());
    }

    #[test]
    fn pkg_version_ranges() {
        let content = r#"{
  "dependencies": {
    "a": "^1.0.0",
    "b": "~2.0.0",
    "c": ">=3.0.0 <4.0.0",
    "d": "*"
  }
}"#;
        let deps = parse_package_json(content, &manifest("package.json"));
        assert_eq!(deps.len(), 4);
        let d_dep = deps.iter().find(|d| d.name == "d").unwrap();
        assert_eq!(d_dep.version, "*");
    }

    #[test]
    fn pkg_line_numbers() {
        let content = r#"{
  "dependencies": {
    "express": "^4.18.0",
    "lodash": "4.17.21"
  }
}"#;
        let deps = parse_package_json(content, &manifest("package.json"));
        let express = deps.iter().find(|d| d.name == "express").unwrap();
        assert_eq!(express.line, 3);
        let lodash = deps.iter().find(|d| d.name == "lodash").unwrap();
        assert_eq!(lodash.line, 4);
    }

    #[test]
    fn pkg_empty_content() {
        let deps = parse_package_json("", &manifest("package.json"));
        assert!(deps.is_empty());
    }

    #[test]
    fn pkg_invalid_json() {
        let deps = parse_package_json("not json", &manifest("package.json"));
        assert!(deps.is_empty());
    }

    #[test]
    fn pkg_manifest_path_preserved() {
        let content = r#"{"dependencies": {"a": "1.0"}}"#;
        let path = Path::new("/some/project/package.json");
        let deps = parse_package_json(content, path);
        assert_eq!(deps[0].manifest_file, path);
    }
}
