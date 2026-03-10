//! RedQueen / CmpLog — input-to-state comparison feedback.
//!
//! Two sources of comparison data:
//! 1. SanCov CMP callbacks (via `apex_sandbox::sancov_rt::read_cmp_log()`)
//! 2. Output parsing fallback (`parse_cmp_hints_from_output()`)

use crate::traits::Mutator;
use rand::RngCore;

/// A single comparison observation: two byte sequences being compared.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CmpEntry {
    pub arg1: Vec<u8>,
    pub arg2: Vec<u8>,
}

impl CmpEntry {
    pub fn new(arg1: Vec<u8>, arg2: Vec<u8>) -> Self {
        Self { arg1, arg2 }
    }
}

/// Deduplicated collection of comparison observations from one execution.
pub struct CmpLog {
    seen: std::collections::HashSet<CmpEntry>,
    log: Vec<CmpEntry>,
}

impl CmpLog {
    pub fn new() -> Self {
        Self {
            seen: std::collections::HashSet::new(),
            log: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: CmpEntry) {
        if self.seen.insert(entry.clone()) {
            self.log.push(entry);
        }
    }

    pub fn entries(&self) -> &[CmpEntry] {
        &self.log
    }

    pub fn is_empty(&self) -> bool {
        self.log.is_empty()
    }
}

impl Default for CmpLog {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse comparison hints from test output (stderr/stdout).
///
/// Recognizes patterns like:
/// - `expected X but got Y` / `expected X, got Y`
/// - `left=`X`, right=`Y`` (Rust assert_eq)
/// - `AssertionError: X != Y`
pub fn parse_cmp_hints_from_output(output: &str) -> Vec<CmpEntry> {
    let mut hints = Vec::new();

    // Pattern: "expected <X> but got <Y>" or "expected <X>, got <Y>"
    let expected_re_str = r"expected\s+(\S+?)[\s,]+(?:but\s+)?got\s+(\S+)";
    // Pattern: "left=`<X>`, right=`<Y>`" (Rust assert_eq)
    let left_right_re_str = r"left=`([^`]+)`.*right=`([^`]+)`";

    for pattern in [expected_re_str, left_right_re_str] {
        if let Ok(re) = regex::Regex::new(pattern) {
            for caps in re.captures_iter(output) {
                if let (Some(a), Some(b)) = (caps.get(1), caps.get(2)) {
                    hints.push(CmpEntry::new(
                        a.as_str().as_bytes().to_vec(),
                        b.as_str().as_bytes().to_vec(),
                    ));
                }
            }
        }
    }

    hints
}

/// Mutator that performs input-to-state replacement using CMP log data.
///
/// For each CMP entry, scans the input for `arg1` and replaces with `arg2`
/// (or vice versa). Picks a random entry and random direction per invocation.
pub struct CmpLogMutator {
    log: CmpLog,
}

impl CmpLogMutator {
    pub fn new(log: CmpLog) -> Self {
        Self { log }
    }
}

impl Mutator for CmpLogMutator {
    fn mutate(&self, input: &[u8], rng: &mut dyn RngCore) -> Vec<u8> {
        if self.log.is_empty() {
            return input.to_vec();
        }

        let entries = self.log.entries();
        let idx = (rng.next_u32() as usize) % entries.len();
        let entry = &entries[idx];

        // Randomly pick direction: replace arg1->arg2 or arg2->arg1
        let (needle, replacement) = if rng.next_u32() % 2 == 0 {
            (&entry.arg1, &entry.arg2)
        } else {
            (&entry.arg2, &entry.arg1)
        };

        if needle.is_empty() || needle.len() > input.len() {
            return input.to_vec();
        }

        // Find all positions where needle occurs
        let mut positions = Vec::new();
        for i in 0..=input.len() - needle.len() {
            if &input[i..i + needle.len()] == needle.as_slice() {
                positions.push(i);
            }
        }

        if positions.is_empty() {
            return input.to_vec();
        }

        // Replace at a random matching position
        let pos = positions[(rng.next_u32() as usize) % positions.len()];
        let mut out = input.to_vec();
        out[pos..pos + needle.len()].copy_from_slice(replacement);
        out
    }

    fn name(&self) -> &str {
        "cmplog"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmp_entry_new() {
        let e = CmpEntry::new(vec![1, 2, 3, 4], vec![5, 6, 7, 8]);
        assert_eq!(e.arg1, vec![1, 2, 3, 4]);
        assert_eq!(e.arg2, vec![5, 6, 7, 8]);
    }

    #[test]
    fn cmp_log_add_and_entries() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(vec![0xAA], vec![0xBB]));
        log.add(CmpEntry::new(vec![0xCC], vec![0xDD]));
        assert_eq!(log.entries().len(), 2);
    }

    #[test]
    fn cmp_log_dedup() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(vec![1], vec![2]));
        log.add(CmpEntry::new(vec![1], vec![2])); // duplicate
        log.add(CmpEntry::new(vec![3], vec![4]));
        assert_eq!(log.entries().len(), 2);
    }

    #[test]
    fn parse_hints_assertion_expected_got() {
        let stderr = "AssertionError: expected 42 but got 0";
        let hints = parse_cmp_hints_from_output(stderr);
        assert!(!hints.is_empty());
        // Should find the numeric pair (42, 0)
        assert!(hints.iter().any(|e| e.arg1 == b"42" && e.arg2 == b"0"));
    }

    #[test]
    fn parse_hints_not_equal() {
        let stderr = "assert_eq failed: left=`hello`, right=`world`";
        let hints = parse_cmp_hints_from_output(stderr);
        assert!(hints.iter().any(|e| {
            std::str::from_utf8(&e.arg1).ok() == Some("hello")
                && std::str::from_utf8(&e.arg2).ok() == Some("world")
        }));
    }

    #[test]
    fn parse_hints_empty_string() {
        let hints = parse_cmp_hints_from_output("");
        assert!(hints.is_empty());
    }

    #[test]
    fn parse_hints_no_comparisons() {
        let hints = parse_cmp_hints_from_output("some random output with no comparisons");
        assert!(hints.is_empty());
    }

    // CmpLogMutator tests

    #[test]
    fn cmplog_mutator_replaces_arg1_with_arg2() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"AAAA".to_vec(), b"BBBB".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"xxAAAAyy";
        let mut rng = rand::thread_rng();
        // Run multiple times -- should eventually produce the replacement
        let mut found = false;
        for _ in 0..50 {
            let out = m.mutate(input, &mut rng);
            if out == b"xxBBBByy" {
                found = true;
                break;
            }
        }
        assert!(found, "CmpLogMutator should replace AAAA with BBBB");
    }

    #[test]
    fn cmplog_mutator_replaces_arg2_with_arg1() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"XX".to_vec(), b"YY".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"__YY__";
        let mut rng = rand::thread_rng();
        let mut found = false;
        for _ in 0..50 {
            let out = m.mutate(input, &mut rng);
            if out == b"__XX__" {
                found = true;
                break;
            }
        }
        assert!(found, "CmpLogMutator should also try reverse replacement");
    }

    #[test]
    fn cmplog_mutator_no_match_returns_original() {
        let mut log = CmpLog::new();
        log.add(CmpEntry::new(b"ZZZZ".to_vec(), b"WWWW".to_vec()));
        let m = CmpLogMutator::new(log);
        let input = b"no match here";
        let mut rng = rand::thread_rng();
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn cmplog_mutator_empty_log_returns_original() {
        let m = CmpLogMutator::new(CmpLog::new());
        let input = b"test";
        let mut rng = rand::thread_rng();
        let out = m.mutate(input, &mut rng);
        assert_eq!(out, input);
    }

    #[test]
    fn cmplog_mutator_name() {
        let m = CmpLogMutator::new(CmpLog::new());
        assert_eq!(m.name(), "cmplog");
    }
}
