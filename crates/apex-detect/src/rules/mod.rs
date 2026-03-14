//! External rule definition and pattern matching for APEX detectors.
//!
//! Supports loading security rules from YAML files with pattern expressions
//! for matching source code.

pub mod loader;
pub mod matcher;

pub use loader::{load_rules_from_yaml, RuleDefinition, RuleSeverity};
pub use matcher::{match_pattern, parse_pattern, PatternExpr};
