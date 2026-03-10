use serde::{Deserialize, Serialize};

/// A MIR function consisting of a name and a sequence of basic blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirFunction {
    pub name: String,
    pub blocks: Vec<BasicBlock>,
}

/// A basic block in MIR: an id, a list of statements, and a terminator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicBlock {
    pub id: usize,
    pub statements: Vec<Statement>,
    pub terminator: Terminator,
}

/// MIR statement variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    Assign { place: String, rvalue: String },
    StorageLive(String),
    StorageDead(String),
    Nop,
}

/// MIR terminator variants — each basic block ends with exactly one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Terminator {
    Goto {
        target: usize,
    },
    SwitchInt {
        discriminant: String,
        targets: Vec<(i128, usize)>,
        otherwise: usize,
    },
    Return,
    Unreachable,
    Call {
        func: String,
        destination: Option<usize>,
        cleanup: Option<usize>,
    },
    Drop {
        target: usize,
        unwind: Option<usize>,
    },
    Abort,
}

impl MirFunction {
    /// Create a new `MirFunction` with no blocks.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            blocks: Vec::new(),
        }
    }

    /// Append a basic block and return its id.
    pub fn add_block(&mut self, statements: Vec<Statement>, terminator: Terminator) -> usize {
        let id = self.blocks.len();
        self.blocks.push(BasicBlock {
            id,
            statements,
            terminator,
        });
        id
    }

    /// Return successor block ids for the given block.
    pub fn successors(&self, block_id: usize) -> Vec<usize> {
        self.blocks
            .get(block_id)
            .map(|b| b.terminator.successors())
            .unwrap_or_default()
    }

    /// Number of basic blocks.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Total number of branch edges (sum of successor counts across all blocks).
    pub fn branch_count(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| b.terminator.successors().len())
            .sum()
    }
}

impl Terminator {
    /// Return the set of successor block ids.
    pub fn successors(&self) -> Vec<usize> {
        match self {
            Terminator::Goto { target } => vec![*target],
            Terminator::SwitchInt {
                targets, otherwise, ..
            } => {
                let mut succs: Vec<usize> = targets.iter().map(|(_, t)| *t).collect();
                succs.push(*otherwise);
                succs
            }
            Terminator::Return | Terminator::Unreachable | Terminator::Abort => vec![],
            Terminator::Call {
                destination,
                cleanup,
                ..
            } => {
                let mut succs = Vec::new();
                if let Some(d) = destination {
                    succs.push(*d);
                }
                if let Some(c) = cleanup {
                    succs.push(*c);
                }
                succs
            }
            Terminator::Drop { target, unwind } => {
                let mut succs = vec![*target];
                if let Some(u) = unwind {
                    succs.push(*u);
                }
                succs
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_function_has_no_blocks() {
        let f = MirFunction::new("foo");
        assert_eq!(f.name, "foo");
        assert_eq!(f.block_count(), 0);
    }

    #[test]
    fn add_block_returns_sequential_ids() {
        let mut f = MirFunction::new("bar");
        let id0 = f.add_block(vec![], Terminator::Return);
        let id1 = f.add_block(vec![], Terminator::Return);
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(f.block_count(), 2);
    }

    #[test]
    fn goto_has_one_successor() {
        let t = Terminator::Goto { target: 3 };
        assert_eq!(t.successors(), vec![3]);
    }

    #[test]
    fn switch_int_successors_include_otherwise() {
        let t = Terminator::SwitchInt {
            discriminant: "_1".into(),
            targets: vec![(0, 1), (1, 2)],
            otherwise: 3,
        };
        assert_eq!(t.successors(), vec![1, 2, 3]);
    }

    #[test]
    fn return_has_no_successors() {
        assert!(Terminator::Return.successors().is_empty());
    }

    #[test]
    fn unreachable_has_no_successors() {
        assert!(Terminator::Unreachable.successors().is_empty());
    }

    #[test]
    fn abort_has_no_successors() {
        assert!(Terminator::Abort.successors().is_empty());
    }

    #[test]
    fn call_successors() {
        let t = Terminator::Call {
            func: "foo".into(),
            destination: Some(1),
            cleanup: Some(2),
        };
        assert_eq!(t.successors(), vec![1, 2]);

        let t2 = Terminator::Call {
            func: "bar".into(),
            destination: None,
            cleanup: None,
        };
        assert!(t2.successors().is_empty());
    }

    #[test]
    fn drop_successors() {
        let t = Terminator::Drop {
            target: 5,
            unwind: Some(6),
        };
        assert_eq!(t.successors(), vec![5, 6]);

        let t2 = Terminator::Drop {
            target: 5,
            unwind: None,
        };
        assert_eq!(t2.successors(), vec![5]);
    }

    #[test]
    fn branch_count_sums_all_edges() {
        let mut f = MirFunction::new("test");
        f.add_block(vec![], Terminator::Goto { target: 1 }); // 1 edge
        f.add_block(
            vec![],
            Terminator::SwitchInt {
                discriminant: "_1".into(),
                targets: vec![(0, 2)],
                otherwise: 3,
            },
        ); // 2 edges
        f.add_block(vec![], Terminator::Return); // 0 edges
        f.add_block(vec![], Terminator::Return); // 0 edges
        assert_eq!(f.branch_count(), 3);
    }
}
