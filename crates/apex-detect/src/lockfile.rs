//! Shared lock-file parsers for dependency analysis.
//!
//! Provides a unified [`Dependency`] struct and parsers for Cargo.lock,
//! package-lock.json, and requirements.txt.

use apex_core::error::{ApexError, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A dependency extracted from a lock file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Dependency {
    pub name: String,
    pub version: String,
    /// Package URL (PURL) for SBOM generation.
    pub purl: String,
    /// Optional source URL / registry.
    pub source_url: Option<String>,
    /// Optional integrity hash (e.g. sha256).
    pub checksum: Option<String>,
    /// SPDX license expression, if known at parse time.
    pub license: Option<String>,
}

// ── Cargo.lock ──────────────────────────────────────────────────────────────

/// Intermediate serde model for Cargo.lock TOML.
#[derive(Deserialize)]
struct CargoLockFile {
    #[serde(default)]
    package: Vec<CargoPackage>,
}

#[derive(Deserialize)]
struct CargoPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

/// Parse a `Cargo.lock` file into a list of dependencies.
pub fn parse_cargo_lock(path: &Path) -> Result<Vec<Dependency>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ApexError::Detect(format!("read Cargo.lock: {e}")))?;
    parse_cargo_lock_str(&content)
}

pub fn parse_cargo_lock_str(content: &str) -> Result<Vec<Dependency>> {
    let lock: CargoLockFile =
        toml::from_str(content).map_err(|e| ApexError::Detect(format!("parse Cargo.lock: {e}")))?;

    Ok(lock
        .package
        .into_iter()
        .map(|p| Dependency {
            purl: format!("pkg:cargo/{}@{}", p.name, p.version),
            name: p.name,
            version: p.version,
            source_url: p.source,
            checksum: p.checksum,
            license: None,
        })
        .collect())
}

// ── package-lock.json ───────────────────────────────────────────────────────

/// Parse a `package-lock.json` (v2/v3 format) into a list of dependencies.
pub fn parse_package_lock(path: &Path) -> Result<Vec<Dependency>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ApexError::Detect(format!("read package-lock.json: {e}")))?;
    parse_package_lock_str(&content)
}

pub fn parse_package_lock_str(content: &str) -> Result<Vec<Dependency>> {
    let parsed: serde_json::Value = serde_json::from_str(content)
        .map_err(|e| ApexError::Detect(format!("parse package-lock.json: {e}")))?;

    let mut deps = Vec::new();

    // v2/v3: "packages" map (keys are node_modules paths)
    if let Some(packages) = parsed.get("packages").and_then(|v| v.as_object()) {
        for (key, pkg) in packages {
            // Skip the root package (empty key)
            if key.is_empty() {
                continue;
            }
            let name = pkg
                .get("name")
                .and_then(|v| v.as_str())
                .or_else(|| {
                    // Derive name from path: "node_modules/express" -> "express"
                    // "node_modules/@scope/pkg" -> "@scope/pkg"
                    key.rsplit_once("node_modules/").map(|(_, n)| n)
                })
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            let version = pkg
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let integrity = pkg
                .get("integrity")
                .and_then(|v| v.as_str())
                .map(String::from);
            let resolved = pkg
                .get("resolved")
                .and_then(|v| v.as_str())
                .map(String::from);
            let license = pkg
                .get("license")
                .and_then(|v| v.as_str())
                .map(String::from);

            deps.push(Dependency {
                purl: format!("pkg:npm/{name}@{version}"),
                name,
                version,
                source_url: resolved,
                checksum: integrity,
                license,
            });
        }
    }
    // v1 fallback: "dependencies" map
    else if let Some(dependencies) = parsed.get("dependencies").and_then(|v| v.as_object()) {
        for (name, info) in dependencies {
            let version = info
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let integrity = info
                .get("integrity")
                .and_then(|v| v.as_str())
                .map(String::from);
            let resolved = info
                .get("resolved")
                .and_then(|v| v.as_str())
                .map(String::from);

            deps.push(Dependency {
                purl: format!("pkg:npm/{name}@{version}"),
                name: name.clone(),
                version,
                source_url: resolved,
                checksum: integrity,
                license: None,
            });
        }
    }

    Ok(deps)
}

// ── requirements.txt ────────────────────────────────────────────────────────

/// Parse a `requirements.txt` (pip freeze format) into a list of dependencies.
///
/// Supports `name==version`, `name>=version`, and bare `name` lines.
/// Ignores comments, blank lines, and `-r` / `--` flags.
pub fn parse_requirements(path: &Path) -> Result<Vec<Dependency>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ApexError::Detect(format!("read requirements.txt: {e}")))?;
    Ok(parse_requirements_str(&content))
}

pub fn parse_requirements_str(content: &str) -> Vec<Dependency> {
    let mut deps = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('-') {
            continue;
        }
        // Strip inline comments
        let line = line.split('#').next().unwrap_or(line).trim();
        // Strip environment markers: "requests>=2.0 ; python_version>='3'"
        let line = line.split(';').next().unwrap_or(line).trim();

        // Parse name==version, name>=version, name~=version, name!=version, name<=version
        let (name, version) = if let Some(pos) = line.find("==") {
            (&line[..pos], line[pos + 2..].trim())
        } else if let Some(pos) = line.find(">=") {
            (&line[..pos], line[pos + 2..].trim())
        } else if let Some(pos) = line.find("~=") {
            (&line[..pos], line[pos + 2..].trim())
        } else if let Some(pos) = line.find("<=") {
            (&line[..pos], line[pos + 2..].trim())
        } else if let Some(pos) = line.find("!=") {
            (&line[..pos], line[pos + 2..].trim())
        } else {
            (line, "")
        };

        let name = name.trim();
        // Strip extras: "requests[security]" -> "requests"
        let name = name.split('[').next().unwrap_or(name).trim();
        if name.is_empty() {
            continue;
        }

        deps.push(Dependency {
            purl: if version.is_empty() {
                format!("pkg:pypi/{name}")
            } else {
                format!("pkg:pypi/{name}@{version}")
            },
            name: name.to_string(),
            version: version.to_string(),
            source_url: None,
            checksum: None,
            license: None,
        });
    }

    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Cargo.lock tests ────────────────────────────────────────────────────

    #[test]
    fn parse_cargo_lock_basic() {
        let content = r#"
[[package]]
name = "serde"
version = "1.0.200"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc123"

[[package]]
name = "tokio"
version = "1.37.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
        let deps = parse_cargo_lock_str(content).unwrap();
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "serde");
        assert_eq!(deps[0].version, "1.0.200");
        assert_eq!(deps[0].purl, "pkg:cargo/serde@1.0.200");
        assert_eq!(deps[0].checksum, Some("abc123".into()));
        assert_eq!(deps[1].name, "tokio");
        assert!(deps[1].checksum.is_none());
    }

    #[test]
    fn parse_cargo_lock_empty() {
        let deps = parse_cargo_lock_str("").unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn parse_cargo_lock_invalid() {
        let result = parse_cargo_lock_str("not valid toml {{{}}}");
        assert!(result.is_err());
    }

    // ── package-lock.json tests ─────────────────────────────────────────────

    #[test]
    fn parse_package_lock_v2() {
        let content = r#"{
            "lockfileVersion": 2,
            "packages": {
                "": {"name": "my-app", "version": "1.0.0"},
                "node_modules/express": {
                    "version": "4.18.2",
                    "resolved": "https://registry.npmjs.org/express/-/express-4.18.2.tgz",
                    "integrity": "sha512-abc",
                    "license": "MIT"
                },
                "node_modules/@scope/pkg": {
                    "version": "2.0.0"
                }
            }
        }"#;
        let deps = parse_package_lock_str(content).unwrap();
        assert_eq!(deps.len(), 2);

        let express = deps.iter().find(|d| d.name == "express").unwrap();
        assert_eq!(express.version, "4.18.2");
        assert_eq!(express.purl, "pkg:npm/express@4.18.2");
        assert_eq!(express.license, Some("MIT".into()));
        assert!(express.checksum.is_some());

        let scoped = deps.iter().find(|d| d.name == "@scope/pkg").unwrap();
        assert_eq!(scoped.version, "2.0.0");
        assert_eq!(scoped.purl, "pkg:npm/@scope/pkg@2.0.0");
    }

    #[test]
    fn parse_package_lock_v1_fallback() {
        let content = r#"{
            "lockfileVersion": 1,
            "dependencies": {
                "lodash": {
                    "version": "4.17.21",
                    "resolved": "https://registry.npmjs.org/lodash/-/lodash-4.17.21.tgz",
                    "integrity": "sha512-xyz"
                }
            }
        }"#;
        let deps = parse_package_lock_str(content).unwrap();
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "lodash");
        assert_eq!(deps[0].purl, "pkg:npm/lodash@4.17.21");
    }

    #[test]
    fn parse_package_lock_empty() {
        let deps = parse_package_lock_str("{}").unwrap();
        assert!(deps.is_empty());
    }

    #[test]
    fn parse_package_lock_invalid() {
        let result = parse_package_lock_str("not json");
        assert!(result.is_err());
    }

    // ── requirements.txt tests ──────────────────────────────────────────────

    #[test]
    fn parse_requirements_basic() {
        let content = "requests==2.31.0\nflask>=2.0.0\nnumpy\n";
        let deps = parse_requirements_str(content);
        assert_eq!(deps.len(), 3);

        assert_eq!(deps[0].name, "requests");
        assert_eq!(deps[0].version, "2.31.0");
        assert_eq!(deps[0].purl, "pkg:pypi/requests@2.31.0");

        assert_eq!(deps[1].name, "flask");
        assert_eq!(deps[1].version, "2.0.0");

        assert_eq!(deps[2].name, "numpy");
        assert_eq!(deps[2].version, "");
        assert_eq!(deps[2].purl, "pkg:pypi/numpy");
    }

    #[test]
    fn parse_requirements_comments_and_blanks() {
        let content = "# comment\n\nrequests==1.0\n  # another\n";
        let deps = parse_requirements_str(content);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "requests");
    }

    #[test]
    fn parse_requirements_extras() {
        let content = "requests[security]==2.31.0\n";
        let deps = parse_requirements_str(content);
        assert_eq!(deps[0].name, "requests");
        assert_eq!(deps[0].version, "2.31.0");
    }

    #[test]
    fn parse_requirements_env_markers() {
        let content = "pywin32==306 ; sys_platform == 'win32'\n";
        let deps = parse_requirements_str(content);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].name, "pywin32");
        assert_eq!(deps[0].version, "306");
    }

    #[test]
    fn parse_requirements_flags_skipped() {
        let content = "-r base.txt\n--extra-index-url https://example.com\nrequests==1.0\n";
        let deps = parse_requirements_str(content);
        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn parse_requirements_inline_comment() {
        let content = "requests==2.31.0 # HTTP library\n";
        let deps = parse_requirements_str(content);
        assert_eq!(deps[0].name, "requests");
        assert_eq!(deps[0].version, "2.31.0");
    }

    #[test]
    fn parse_requirements_tilde_equals() {
        let content = "django~=4.2\n";
        let deps = parse_requirements_str(content);
        assert_eq!(deps[0].name, "django");
        assert_eq!(deps[0].version, "4.2");
    }

    #[test]
    fn parse_requirements_empty() {
        let deps = parse_requirements_str("");
        assert!(deps.is_empty());
    }
}
