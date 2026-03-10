//! Grammar-based input generation for structured fuzzing.

use rand::Rng;
use std::collections::HashMap;

/// A symbol in a grammar production rule.
#[derive(Debug, Clone, PartialEq)]
pub enum Symbol {
    /// A non-terminal symbol referencing another production.
    NonTerminal(String),
    /// A terminal symbol representing literal text.
    Terminal(String),
}

/// A production rule: `lhs -> alt1 | alt2 | ...`
#[derive(Debug, Clone)]
pub struct Production {
    /// Left-hand side non-terminal name.
    pub lhs: String,
    /// Each alternative is a sequence of symbols.
    pub alternatives: Vec<Vec<Symbol>>,
}

/// A node in the parse tree produced by generation.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseNode {
    /// A leaf node containing terminal text.
    Leaf(String),
    /// An interior node with its non-terminal name and children.
    Interior(String, Vec<ParseNode>),
}

/// A context-free grammar for structured input generation.
#[derive(Debug, Clone)]
pub struct Grammar {
    /// The start symbol of the grammar.
    pub start: String,
    /// Map from non-terminal name to its production rule.
    pub productions: HashMap<String, Production>,
}

impl Grammar {
    /// Create a new grammar with the given start symbol.
    pub fn new(start: impl Into<String>) -> Self {
        Grammar {
            start: start.into(),
            productions: HashMap::new(),
        }
    }

    /// Add a production rule. If a production for `lhs` already exists, the new
    /// alternatives are appended.
    pub fn add_production(&mut self, lhs: impl Into<String>, alternatives: Vec<Vec<Symbol>>) {
        let lhs = lhs.into();
        self.productions
            .entry(lhs.clone())
            .and_modify(|p| p.alternatives.extend(alternatives.clone()))
            .or_insert_with(|| Production {
                lhs: lhs.clone(),
                alternatives,
            });
    }

    /// Generate a random string from the grammar up to `max_depth` levels of
    /// non-terminal expansion. Returns `None` if the start symbol has no
    /// production.
    pub fn generate(&self, rng: &mut impl Rng, max_depth: usize) -> Option<String> {
        if !self.productions.contains_key(&self.start) {
            return None;
        }
        let mut output = String::new();
        self.generate_from(&self.start, rng, max_depth, &mut output);
        Some(output)
    }

    /// Recursive helper that expands `symbol` into `output`.
    fn generate_from(&self, symbol: &str, rng: &mut impl Rng, depth: usize, output: &mut String) {
        let prod = match self.productions.get(symbol) {
            Some(p) => p,
            None => {
                // Unknown non-terminal — emit its name as-is.
                output.push_str(symbol);
                return;
            }
        };

        if depth == 0 || prod.alternatives.is_empty() {
            // At max depth, pick the shortest alternative to terminate quickly.
            if let Some(alt) = prod.alternatives.iter().min_by_key(|a| a.len()) {
                for sym in alt {
                    match sym {
                        Symbol::Terminal(t) => output.push_str(t),
                        Symbol::NonTerminal(_) => {
                            // Cannot recurse further; skip non-terminals.
                        }
                    }
                }
            }
            return;
        }

        let idx = rng.gen_range(0..prod.alternatives.len());
        let alt = &prod.alternatives[idx];

        for sym in alt {
            match sym {
                Symbol::Terminal(t) => output.push_str(t),
                Symbol::NonTerminal(nt) => {
                    self.generate_from(nt, rng, depth - 1, output);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn simple_grammar() -> Grammar {
        let mut g = Grammar::new("expr");
        g.add_production(
            "expr",
            vec![
                vec![Symbol::Terminal("1".into())],
                vec![
                    Symbol::NonTerminal("expr".into()),
                    Symbol::Terminal("+".into()),
                    Symbol::NonTerminal("expr".into()),
                ],
            ],
        );
        g
    }

    #[test]
    fn grammar_creation() {
        let g = simple_grammar();
        assert_eq!(g.start, "expr");
        assert!(g.productions.contains_key("expr"));
        assert_eq!(g.productions["expr"].alternatives.len(), 2);
    }

    #[test]
    fn deterministic_generation() {
        let g = simple_grammar();
        let mut rng1 = StdRng::seed_from_u64(42);
        let mut rng2 = StdRng::seed_from_u64(42);
        let a = g.generate(&mut rng1, 5).unwrap();
        let b = g.generate(&mut rng2, 5).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn max_depth_zero() {
        let g = simple_grammar();
        let mut rng = StdRng::seed_from_u64(99);
        let result = g.generate(&mut rng, 0).unwrap();
        // At depth 0, the shortest alternative is picked and non-terminals are
        // skipped, so we expect just "1".
        assert_eq!(result, "1");
    }

    #[test]
    fn multiple_differ() {
        let g = simple_grammar();
        let mut results = std::collections::HashSet::new();
        for seed in 0..50 {
            let mut rng = StdRng::seed_from_u64(seed);
            if let Some(s) = g.generate(&mut rng, 5) {
                results.insert(s);
            }
        }
        // With 50 different seeds we should get more than one unique output.
        assert!(results.len() > 1, "expected variation, got {:?}", results);
    }

    #[test]
    fn empty_grammar() {
        let g = Grammar::new("missing");
        let mut rng = StdRng::seed_from_u64(0);
        assert!(g.generate(&mut rng, 5).is_none());
    }

    #[test]
    fn symbol_equality() {
        assert_eq!(Symbol::Terminal("a".into()), Symbol::Terminal("a".into()));
        assert_ne!(
            Symbol::Terminal("a".into()),
            Symbol::NonTerminal("a".into())
        );
        assert_eq!(
            Symbol::NonTerminal("x".into()),
            Symbol::NonTerminal("x".into())
        );
    }

    #[test]
    fn add_production_merges_alternatives() {
        let mut g = Grammar::new("expr");
        g.add_production("expr", vec![vec![Symbol::Terminal("1".into())]]);
        g.add_production("expr", vec![vec![Symbol::Terminal("2".into())]]);
        assert_eq!(g.productions["expr"].alternatives.len(), 2);
    }

    #[test]
    fn generate_unknown_nonterminal_emits_name() {
        let mut g = Grammar::new("start");
        g.add_production("start", vec![vec![Symbol::NonTerminal("unknown".into())]]);
        let mut rng = StdRng::seed_from_u64(0);
        let result = g.generate(&mut rng, 5).unwrap();
        assert_eq!(result, "unknown");
    }

    #[test]
    fn depth_zero_skips_nonterminals_in_alt() {
        let mut g = Grammar::new("start");
        g.add_production(
            "start",
            vec![vec![
                Symbol::Terminal("hello".into()),
                Symbol::NonTerminal("other".into()),
            ]],
        );
        g.add_production("other", vec![vec![Symbol::Terminal("world".into())]]);
        let mut rng = StdRng::seed_from_u64(0);
        let result = g.generate(&mut rng, 0).unwrap();
        // At depth 0, non-terminals are skipped
        assert_eq!(result, "hello");
    }

    #[test]
    fn parse_node_equality() {
        assert_eq!(ParseNode::Leaf("a".into()), ParseNode::Leaf("a".into()));
        assert_ne!(ParseNode::Leaf("a".into()), ParseNode::Leaf("b".into()));
        let interior = ParseNode::Interior("x".into(), vec![ParseNode::Leaf("y".into())]);
        let interior2 = ParseNode::Interior("x".into(), vec![ParseNode::Leaf("y".into())]);
        assert_eq!(interior, interior2);
    }
}
