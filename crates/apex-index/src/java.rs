use apex_core::types::BranchId;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::debug;

// ---------------------------------------------------------------------------
// JaCoCo XML coverage schema
// ---------------------------------------------------------------------------

/// Top-level JaCoCo XML report.
#[derive(Debug, Deserialize)]
#[serde(rename = "report")]
pub struct JacocoReport {
    #[serde(default)]
    #[serde(rename = "package")]
    pub packages: Vec<JacocoPackage>,
}

#[derive(Debug, Deserialize)]
pub struct JacocoPackage {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(default)]
    #[serde(rename = "class")]
    pub classes: Vec<JacocoClass>,
}

#[derive(Debug, Deserialize)]
pub struct JacocoClass {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(default, rename = "@sourcefilename")]
    pub source_file: String,
    #[serde(default)]
    #[serde(rename = "method")]
    pub methods: Vec<JacocoMethod>,
}

#[derive(Debug, Deserialize)]
pub struct JacocoMethod {
    #[serde(rename = "@name")]
    pub name: String,
    #[serde(default, rename = "@line")]
    pub line: u32,
    #[serde(default)]
    #[serde(rename = "counter")]
    pub counters: Vec<JacocoCounter>,
}

#[derive(Debug, Deserialize)]
pub struct JacocoCounter {
    #[serde(rename = "@type")]
    pub counter_type: String,
    #[serde(default, rename = "@missed")]
    pub missed: u32,
    #[serde(default, rename = "@covered")]
    pub covered: u32,
}

// ---------------------------------------------------------------------------
// FNV-1a hash (must match apex-instrument and python.rs)
// ---------------------------------------------------------------------------

fn fnv1a_hash(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Result from parsing JaCoCo XML coverage.
#[derive(Debug)]
pub struct JavaCoverageResult {
    /// All branch IDs discovered (both covered and uncovered).
    pub branches: Vec<BranchId>,
    /// Mapping from file_id -> relative file path.
    pub file_paths: HashMap<u64, PathBuf>,
    /// Total branch count (covered + missed).
    pub total_branches: usize,
    /// Covered branch count.
    pub covered_branches: usize,
}

/// Parse JaCoCo XML coverage report into branch IDs.
pub fn parse_jacoco_xml(xml: &str) -> Result<JavaCoverageResult, String> {
    let report: JacocoReport =
        quick_xml::de::from_str(xml).map_err(|e| format!("invalid JaCoCo XML: {e}"))?;

    let mut branches = Vec::new();
    let mut file_paths = HashMap::new();
    let mut total_branches: usize = 0;
    let mut covered_branches: usize = 0;

    for package in &report.packages {
        for class in &package.classes {
            // Build file path from package + source file
            let file_path = if class.source_file.is_empty() {
                // Fall back to class name with slashes -> path
                format!("{}.java", class.name.replace('.', "/"))
            } else {
                let pkg_path = package.name.replace('.', "/");
                format!("{}/{}", pkg_path, class.source_file)
            };

            let file_id = fnv1a_hash(&file_path);
            file_paths.insert(file_id, PathBuf::from(&file_path));

            for method in &class.methods {
                let line = method.line;

                for counter in &method.counters {
                    if counter.counter_type != "BRANCH" {
                        continue;
                    }

                    let covered = counter.covered;
                    let missed = counter.missed;
                    let method_total = (covered + missed) as usize;
                    total_branches += method_total;
                    covered_branches += covered as usize;

                    debug!(
                        file = %file_path,
                        method = %method.name,
                        line,
                        covered,
                        missed,
                        "JaCoCo branch counter"
                    );

                    // Emit covered branches
                    for i in 0..covered {
                        branches.push(BranchId::new(file_id, line, 0, i as u8));
                    }

                    // Emit uncovered branches with discriminator = 1
                    for i in 0..missed {
                        let mut bid = BranchId::new(file_id, line, 0, (covered + i) as u8);
                        bid.discriminator = 1; // mark as uncovered
                        branches.push(bid);
                    }
                }
            }
        }
    }

    Ok(JavaCoverageResult {
        branches,
        file_paths,
        total_branches,
        covered_branches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const JACOCO_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<report name="test-project">
  <package name="com/example">
    <class name="com/example/Calculator" sourcefilename="Calculator.java">
      <method name="add" line="10">
        <counter type="INSTRUCTION" missed="0" covered="5"/>
        <counter type="BRANCH" missed="1" covered="3"/>
        <counter type="LINE" missed="0" covered="3"/>
      </method>
      <method name="divide" line="20">
        <counter type="INSTRUCTION" missed="2" covered="3"/>
        <counter type="BRANCH" missed="2" covered="2"/>
        <counter type="LINE" missed="1" covered="2"/>
      </method>
    </class>
    <class name="com/example/Utils" sourcefilename="Utils.java">
      <method name="validate" line="5">
        <counter type="BRANCH" missed="0" covered="2"/>
      </method>
    </class>
  </package>
</report>"#;

    #[test]
    fn parse_jacoco_basic() {
        let result = parse_jacoco_xml(JACOCO_XML).unwrap();

        // Total branches: add(3+1) + divide(2+2) + validate(2+0) = 10
        assert_eq!(result.total_branches, 10);
        // Covered: add(3) + divide(2) + validate(2) = 7
        assert_eq!(result.covered_branches, 7);
    }

    #[test]
    fn parse_jacoco_file_paths() {
        let result = parse_jacoco_xml(JACOCO_XML).unwrap();

        // Should have 2 file paths: Calculator.java and Utils.java
        assert_eq!(result.file_paths.len(), 2);

        let calc_id = fnv1a_hash("com/example/Calculator.java");
        assert_eq!(
            result.file_paths.get(&calc_id),
            Some(&PathBuf::from("com/example/Calculator.java"))
        );

        let utils_id = fnv1a_hash("com/example/Utils.java");
        assert_eq!(
            result.file_paths.get(&utils_id),
            Some(&PathBuf::from("com/example/Utils.java"))
        );
    }

    #[test]
    fn parse_jacoco_branch_ids() {
        let result = parse_jacoco_xml(JACOCO_XML).unwrap();

        let calc_id = fnv1a_hash("com/example/Calculator.java");

        // add method at line 10: 3 covered + 1 uncovered = 4 branches
        let add_branches: Vec<_> = result
            .branches
            .iter()
            .filter(|b| b.file_id == calc_id && b.line == 10)
            .collect();
        assert_eq!(add_branches.len(), 4);

        // 3 covered (discriminator 0)
        let covered_count = add_branches.iter().filter(|b| b.discriminator == 0).count();
        assert_eq!(covered_count, 3);

        // 1 uncovered (discriminator 1)
        let uncovered_count = add_branches.iter().filter(|b| b.discriminator == 1).count();
        assert_eq!(uncovered_count, 1);
    }

    #[test]
    fn parse_jacoco_empty_report() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<report name="empty">
</report>"#;
        let result = parse_jacoco_xml(xml).unwrap();
        assert_eq!(result.total_branches, 0);
        assert_eq!(result.covered_branches, 0);
        assert!(result.branches.is_empty());
        assert!(result.file_paths.is_empty());
    }

    #[test]
    fn parse_jacoco_no_branch_counters() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<report name="no-branches">
  <package name="com/example">
    <class name="com/example/Simple" sourcefilename="Simple.java">
      <method name="run" line="1">
        <counter type="INSTRUCTION" missed="0" covered="3"/>
        <counter type="LINE" missed="0" covered="1"/>
      </method>
    </class>
  </package>
</report>"#;
        let result = parse_jacoco_xml(xml).unwrap();
        assert_eq!(result.total_branches, 0);
        assert!(result.branches.is_empty());
    }

    #[test]
    fn parse_jacoco_invalid_xml() {
        let result = parse_jacoco_xml("not xml at all");
        assert!(result.is_err());
    }

    #[test]
    fn parse_jacoco_missing_source_file() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<report name="no-source">
  <package name="com/example">
    <class name="com/example/NoSource">
      <method name="run" line="1">
        <counter type="BRANCH" missed="1" covered="1"/>
      </method>
    </class>
  </package>
</report>"#;
        let result = parse_jacoco_xml(xml).unwrap();
        assert_eq!(result.total_branches, 2);

        // Falls back to class name
        let file_id = fnv1a_hash("com/example/NoSource.java");
        assert!(result.file_paths.contains_key(&file_id));
    }

    #[test]
    fn parse_jacoco_all_covered() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<report name="all-covered">
  <package name="pkg">
    <class name="pkg/Foo" sourcefilename="Foo.java">
      <method name="bar" line="5">
        <counter type="BRANCH" missed="0" covered="4"/>
      </method>
    </class>
  </package>
</report>"#;
        let result = parse_jacoco_xml(xml).unwrap();
        assert_eq!(result.total_branches, 4);
        assert_eq!(result.covered_branches, 4);
        // All branches should have discriminator 0 (covered)
        assert!(result.branches.iter().all(|b| b.discriminator == 0));
    }

    #[test]
    fn parse_jacoco_all_missed() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<report name="all-missed">
  <package name="pkg">
    <class name="pkg/Foo" sourcefilename="Foo.java">
      <method name="bar" line="5">
        <counter type="BRANCH" missed="3" covered="0"/>
      </method>
    </class>
  </package>
</report>"#;
        let result = parse_jacoco_xml(xml).unwrap();
        assert_eq!(result.total_branches, 3);
        assert_eq!(result.covered_branches, 0);
        // All branches should have discriminator 1 (uncovered)
        assert!(result.branches.iter().all(|b| b.discriminator == 1));
    }

    #[test]
    fn fnv1a_hash_deterministic() {
        let h1 = fnv1a_hash("com/example/Foo.java");
        let h2 = fnv1a_hash("com/example/Foo.java");
        assert_eq!(h1, h2);

        let h3 = fnv1a_hash("com/example/Bar.java");
        assert_ne!(h1, h3);
    }

    #[test]
    fn parse_jacoco_multiple_packages() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<report name="multi-pkg">
  <package name="com/alpha">
    <class name="com/alpha/A" sourcefilename="A.java">
      <method name="run" line="1">
        <counter type="BRANCH" missed="1" covered="1"/>
      </method>
    </class>
  </package>
  <package name="com/beta">
    <class name="com/beta/B" sourcefilename="B.java">
      <method name="exec" line="10">
        <counter type="BRANCH" missed="0" covered="2"/>
      </method>
    </class>
  </package>
</report>"#;
        let result = parse_jacoco_xml(xml).unwrap();
        assert_eq!(result.total_branches, 4);
        assert_eq!(result.covered_branches, 3);
        assert_eq!(result.file_paths.len(), 2);
    }
}
