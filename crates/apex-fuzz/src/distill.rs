//! Seed distillation — minimize the corpus to a covering set.
//!
//! Uses a greedy set-cover algorithm: repeatedly pick the seed covering the
//! most uncovered branches, until all branches are covered.

use std::collections::HashSet;
use apex_core::types::BranchId;

/// A corpus entry with its input data and the branches it covers.
#[derive(Debug, Clone)]
pub struct CorpusEntry {
    pub data: Vec<u8>,
    pub branches: Vec<BranchId>,
}

/// Distill a corpus to a minimal covering set using greedy set cover.
pub fn distill_corpus(entries: &[CorpusEntry]) -> Vec<CorpusEntry> {
    if entries.is_empty() {
        return Vec::new();
    }

    let mut uncovered: HashSet<&BranchId> = entries.iter().flat_map(|e| &e.branches).collect();
    let mut remaining: Vec<&CorpusEntry> = entries.iter().collect();
    let mut result = Vec::new();

    while !uncovered.is_empty() && !remaining.is_empty() {
        // Pick the entry covering the most uncovered branches.
        let best_idx = remaining
            .iter()
            .enumerate()
            .max_by_key(|(_, e)| e.branches.iter().filter(|b| uncovered.contains(b)).count())
            .map(|(i, _)| i);

        let Some(idx) = best_idx else { break };
        let best = remaining.remove(idx);

        // If it covers zero new branches, stop.
        let new_coverage: Vec<_> = best.branches.iter().filter(|b| uncovered.contains(b)).collect();
        if new_coverage.is_empty() {
            break;
        }

        for b in &best.branches {
            uncovered.remove(b);
        }
        result.push(best.clone());
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use apex_core::types::BranchId;

    #[test]
    fn distill_removes_redundant_seeds() {
        // Seed A covers {1,2}, seed B covers {2,3}, seed C covers {1,2,3}.
        // C alone covers everything — A and B are redundant.
        let entries = vec![
            CorpusEntry {
                data: b"A".to_vec(),
                branches: vec![BranchId::new(1, 1, 0, 0), BranchId::new(1, 2, 0, 0)],
            },
            CorpusEntry {
                data: b"B".to_vec(),
                branches: vec![BranchId::new(1, 2, 0, 0), BranchId::new(1, 3, 0, 0)],
            },
            CorpusEntry {
                data: b"C".to_vec(),
                branches: vec![
                    BranchId::new(1, 1, 0, 0),
                    BranchId::new(1, 2, 0, 0),
                    BranchId::new(1, 3, 0, 0),
                ],
            },
        ];
        let distilled = distill_corpus(&entries);
        // C covers all 3 branches, so at most 1 seed needed.
        assert!(distilled.len() <= 2);
        // All branches still covered.
        let all_branches: std::collections::HashSet<_> =
            distilled.iter().flat_map(|e| &e.branches).collect();
        assert!(all_branches.contains(&BranchId::new(1, 1, 0, 0)));
        assert!(all_branches.contains(&BranchId::new(1, 2, 0, 0)));
        assert!(all_branches.contains(&BranchId::new(1, 3, 0, 0)));
    }

    #[test]
    fn distill_empty_corpus() {
        let distilled = distill_corpus(&[]);
        assert!(distilled.is_empty());
    }

    #[test]
    fn distill_single_seed() {
        let entries = vec![CorpusEntry {
            data: b"only".to_vec(),
            branches: vec![BranchId::new(1, 1, 0, 0)],
        }];
        let distilled = distill_corpus(&entries);
        assert_eq!(distilled.len(), 1);
    }

    #[test]
    fn distill_disjoint_seeds_all_kept() {
        let entries = vec![
            CorpusEntry {
                data: b"A".to_vec(),
                branches: vec![BranchId::new(1, 1, 0, 0)],
            },
            CorpusEntry {
                data: b"B".to_vec(),
                branches: vec![BranchId::new(1, 2, 0, 0)],
            },
        ];
        let distilled = distill_corpus(&entries);
        assert_eq!(distilled.len(), 2);
    }
}
