//! Grammar-aware mutation — swap subtrees in parse trees for structured inputs.

use crate::grammar::ParseNode;

/// Count total nodes in a parse tree.
pub fn count_nodes(node: &ParseNode) -> usize {
    match node {
        ParseNode::Leaf(_) => 1,
        ParseNode::Interior(_, children) => {
            1 + children.iter().map(count_nodes).sum::<usize>()
        }
    }
}

/// Flatten a parse tree into a string by concatenating all leaf values.
pub fn flatten_tree(node: &ParseNode) -> String {
    match node {
        ParseNode::Leaf(s) => s.clone(),
        ParseNode::Interior(_, children) => {
            children.iter().map(flatten_tree).collect::<String>()
        }
    }
}

/// Replace the subtree at the given child index with a replacement node.
/// Only replaces at the top level of an Interior node.
pub fn replace_subtree(node: &ParseNode, child_index: usize, replacement: &ParseNode) -> ParseNode {
    match node {
        ParseNode::Leaf(s) => ParseNode::Leaf(s.clone()),
        ParseNode::Interior(name, children) => {
            let new_children: Vec<ParseNode> = children
                .iter()
                .enumerate()
                .map(|(i, child)| {
                    if i == child_index {
                        replacement.clone()
                    } else {
                        child.clone()
                    }
                })
                .collect();
            ParseNode::Interior(name.clone(), new_children)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grammar::{Grammar, ParseNode, Symbol};

    fn simple_grammar() -> Grammar {
        let mut g = Grammar::new("expr");
        g.add_production("expr", vec![
            vec![Symbol::Terminal("1".into())],
            vec![Symbol::Terminal("2".into())],
            vec![
                Symbol::NonTerminal("expr".into()),
                Symbol::Terminal("+".into()),
                Symbol::NonTerminal("expr".into()),
            ],
        ]);
        g
    }

    #[test]
    fn subtree_replace_changes_output() {
        let tree = ParseNode::Interior(
            "expr".into(),
            vec![
                ParseNode::Leaf("1".into()),
                ParseNode::Leaf("+".into()),
                ParseNode::Leaf("2".into()),
            ],
        );
        let replacement = ParseNode::Leaf("3".into());
        let mutated = replace_subtree(&tree, 0, &replacement);
        let flat = flatten_tree(&mutated);
        assert!(flat.contains("3"));
    }

    #[test]
    fn flatten_tree_concatenates_leaves() {
        let tree = ParseNode::Interior(
            "expr".into(),
            vec![
                ParseNode::Leaf("a".into()),
                ParseNode::Leaf("b".into()),
            ],
        );
        assert_eq!(flatten_tree(&tree), "ab");
    }

    #[test]
    fn count_nodes_counts_all() {
        let tree = ParseNode::Interior(
            "root".into(),
            vec![
                ParseNode::Leaf("x".into()),
                ParseNode::Interior(
                    "inner".into(),
                    vec![ParseNode::Leaf("y".into())],
                ),
            ],
        );
        assert_eq!(count_nodes(&tree), 4);
    }

    // Keep simple_grammar in scope to avoid unused warning
    #[test]
    fn grammar_compiles() {
        let _g = simple_grammar();
    }
}
