# Phase 4: Research-Grade Features

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Grammar-based mutations, custom mutator plugin API, Bolero harness emission, and MIR symbolic foundation.

**Architecture:** Four independent items. B.7 adds grammar-aware mutation via CFG parsing. B.8 adds a plugin system for external mutators. F.2 extends test synthesis for Bolero format. F.3 creates a new `apex-mir` crate for MIR-level symbolic execution foundations.

**Tech Stack:** Rust, libloading (for dlopen), Bolero (feature-gated), rustc MIR (feature-gated)

**Spec:** `docs/superpowers/specs/2026-03-11-apex-research-implementation-design.md`
**Depends on:** Phase 1 (B.1 Mutator trait), Phase 3 (F.1 Kani), Phase 3 (E Python symbolic)

> **Dependency note:** Task 2 (custom mutator plugins) imports `crate::mutators::Mutator` which is the Phase 1 B.1 deliverable. This trait does **not** exist in the current codebase — Phase 1 must be implemented first or compilation will fail.
>
> **Scope note:** B.7 (grammar mutations) is partially covered — Task 1 implements grammar definition and random generation but does not include input-to-AST parsing or a `GrammarMutator` implementing the `Mutator` trait. B.8 (custom mutator plugins) covers the in-process registry but defers `dlopen`/`libloading` support behind the `plugin-mutators` feature gate for a follow-up.

---

## Chunk 1: Grammar Mutations + Custom Mutator Plugins

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Create | `crates/apex-fuzz/src/grammar.rs` | `GrammarMutator` with CFG definition + AST-level mutation |
| Create | `crates/apex-fuzz/src/plugin.rs` | Custom mutator plugin API (`dlopen` loading) |
| Modify | `crates/apex-fuzz/src/corpus.rs` | `register_mutator()` on Corpus |
| Modify | `crates/apex-fuzz/Cargo.toml` | Add `libloading` dependency |

---

### Task 1: Grammar definition and AST parsing

**Files:**
- Create: `crates/apex-fuzz/src/grammar.rs`

- [ ] **Step 1: Write failing tests**

Create `crates/apex-fuzz/src/grammar.rs`:

```rust
//! Grammar-based mutations.
//!
//! Takes a context-free grammar (CFG), parses inputs into ASTs,
//! and mutates at the grammar level for structure-aware fuzzing.

use std::collections::HashMap;

/// A production rule in a context-free grammar.
#[derive(Debug, Clone)]
pub struct Production {
    /// Left-hand side non-terminal.
    pub lhs: String,
    /// Right-hand side alternatives — each is a sequence of symbols.
    pub alternatives: Vec<Vec<Symbol>>,
}

/// A symbol in a grammar production.
#[derive(Debug, Clone, PartialEq)]
pub enum Symbol {
    /// Non-terminal (references another production).
    NonTerminal(String),
    /// Terminal (literal string).
    Terminal(String),
}

/// Context-free grammar definition.
#[derive(Debug, Clone)]
pub struct Grammar {
    pub start: String,
    pub productions: HashMap<String, Production>,
}

/// A parse tree node.
#[derive(Debug, Clone)]
pub enum ParseNode {
    NonTerminal {
        name: String,
        children: Vec<ParseNode>,
        /// Which alternative was used (index into Production::alternatives).
        alt_index: usize,
    },
    Terminal(String),
}

impl Grammar {
    pub fn new(start: impl Into<String>) -> Self {
        Grammar {
            start: start.into(),
            productions: HashMap::new(),
        }
    }

    /// Add a production rule.
    pub fn add_production(&mut self, lhs: impl Into<String>, alternatives: Vec<Vec<Symbol>>) {
        let lhs = lhs.into();
        self.productions.insert(
            lhs.clone(),
            Production {
                lhs,
                alternatives,
            },
        );
    }

    /// Generate a random string from the grammar.
    pub fn generate(&self, rng: &mut dyn rand::RngCore, max_depth: usize) -> String {
        self.generate_from(&self.start, rng, max_depth)
    }

    fn generate_from(&self, symbol: &str, rng: &mut dyn rand::RngCore, depth: usize) -> String {
        if depth == 0 {
            return String::new();
        }

        let Some(prod) = self.productions.get(symbol) else {
            return symbol.to_string(); // treat as terminal
        };

        if prod.alternatives.is_empty() {
            return String::new();
        }

        use rand::Rng;
        let alt_idx = rng.gen_range(0..prod.alternatives.len());
        let alt = &prod.alternatives[alt_idx];

        alt.iter()
            .map(|sym| match sym {
                Symbol::Terminal(s) => s.clone(),
                Symbol::NonTerminal(name) => self.generate_from(name, rng, depth - 1),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn simple_grammar() -> Grammar {
        let mut g = Grammar::new("expr");
        g.add_production("expr", vec![
            vec![Symbol::NonTerminal("num".into())],
            vec![
                Symbol::NonTerminal("expr".into()),
                Symbol::Terminal("+".into()),
                Symbol::NonTerminal("num".into()),
            ],
        ]);
        g.add_production("num", vec![
            vec![Symbol::Terminal("0".into())],
            vec![Symbol::Terminal("1".into())],
            vec![Symbol::Terminal("2".into())],
        ]);
        g
    }

    #[test]
    fn grammar_creation() {
        let g = simple_grammar();
        assert_eq!(g.start, "expr");
        assert_eq!(g.productions.len(), 2);
    }

    #[test]
    fn grammar_generate_deterministic() {
        let g = simple_grammar();
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let output = g.generate(&mut rng, 5);
        assert!(!output.is_empty());
        // Should contain only valid characters from the grammar
        for c in output.chars() {
            assert!(
                c == '0' || c == '1' || c == '2' || c == '+',
                "unexpected char: {c}"
            );
        }
    }

    #[test]
    fn grammar_generate_max_depth_zero() {
        let g = simple_grammar();
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let output = g.generate(&mut rng, 0);
        assert!(output.is_empty());
    }

    #[test]
    fn grammar_generate_multiple_differ() {
        let g = simple_grammar();
        let mut outputs = std::collections::HashSet::new();
        for seed in 0..20u64 {
            let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
            outputs.insert(g.generate(&mut rng, 5));
        }
        // With different seeds, we should get at least 2 different outputs
        assert!(outputs.len() >= 2);
    }

    #[test]
    fn empty_grammar_returns_empty() {
        let g = Grammar::new("missing");
        let mut rng = rand::rngs::StdRng::seed_from_u64(0);
        let output = g.generate(&mut rng, 5);
        assert!(output.is_empty());
    }

    #[test]
    fn symbol_equality() {
        assert_eq!(
            Symbol::Terminal("a".into()),
            Symbol::Terminal("a".into())
        );
        assert_ne!(
            Symbol::Terminal("a".into()),
            Symbol::NonTerminal("a".into())
        );
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p apex-fuzz grammar`
Expected: PASS (implementation is inline)

- [ ] **Step 3: Update lib.rs and commit**

Add to `crates/apex-fuzz/src/lib.rs`:
```rust
pub mod grammar;
```

```bash
git add crates/apex-fuzz/src/grammar.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(fuzz): add grammar-based mutation framework"
```

---

### Task 2: Custom mutator plugin API

**Files:**
- Create: `crates/apex-fuzz/src/plugin.rs`
- Modify: `crates/apex-fuzz/Cargo.toml`

- [ ] **Step 1: Write failing tests**

Create `crates/apex-fuzz/src/plugin.rs`:

```rust
//! Custom mutator plugin API.
//!
//! Supports two modes:
//! 1. Rust trait objects: `Corpus::register_mutator(Box<dyn Mutator>)`
//! 2. Dynamic loading: `dlopen` compatible with AFL++ `AFL_CUSTOM_MUTATOR_LIBRARY`
//!
//! The `dlopen` path is feature-gated behind `plugin-mutators`.

use crate::mutators::Mutator;

/// Registry of custom mutators.
pub struct MutatorRegistry {
    mutators: Vec<Box<dyn Mutator>>,
}

impl MutatorRegistry {
    pub fn new() -> Self {
        MutatorRegistry {
            mutators: Vec::new(),
        }
    }

    /// Register a custom mutator.
    pub fn register(&mut self, mutator: Box<dyn Mutator>) {
        self.mutators.push(mutator);
    }

    /// Get all registered mutators.
    pub fn mutators(&self) -> &[Box<dyn Mutator>] {
        &self.mutators
    }

    /// Number of registered mutators.
    pub fn len(&self) -> usize {
        self.mutators.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mutators.is_empty()
    }
}

impl Default for MutatorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::RngCore;

    struct TestMutator;
    impl Mutator for TestMutator {
        fn mutate(&self, input: &[u8], _rng: &mut dyn RngCore) -> Vec<u8> {
            let mut out = input.to_vec();
            if !out.is_empty() {
                out[0] ^= 0xFF;
            }
            out
        }
        fn name(&self) -> &str {
            "test-mutator"
        }
    }

    #[test]
    fn registry_new_is_empty() {
        let reg = MutatorRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn registry_register_and_retrieve() {
        let mut reg = MutatorRegistry::new();
        reg.register(Box::new(TestMutator));
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.mutators()[0].name(), "test-mutator");
    }

    #[test]
    fn registry_multiple_mutators() {
        let mut reg = MutatorRegistry::new();
        reg.register(Box::new(TestMutator));
        reg.register(Box::new(TestMutator));
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn registered_mutator_works() {
        let mut reg = MutatorRegistry::new();
        reg.register(Box::new(TestMutator));
        let mut rng = rand::rngs::OsRng;
        let input = vec![0x42];
        let output = reg.mutators()[0].mutate(&input, &mut rng);
        assert_eq!(output, vec![0x42 ^ 0xFF]);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p apex-fuzz plugin`
Expected: PASS

- [ ] **Step 3: Update lib.rs and commit**

Add to `crates/apex-fuzz/src/lib.rs`:
```rust
pub mod plugin;
```

```bash
git add crates/apex-fuzz/src/plugin.rs crates/apex-fuzz/src/lib.rs
git commit -m "feat(fuzz): add custom mutator plugin registry"
```

---

## Chunk 2: Bolero Harnesses + MIR Foundation

### File Structure

| Action | Path | Responsibility |
|--------|------|---------------|
| Modify | `crates/apex-synth/src/rust.rs` | Add `HarnessFormat::Bolero` variant |
| Create | `crates/apex-mir/Cargo.toml` | New crate (feature-gated `mir-symbolic`) |
| Create | `crates/apex-mir/src/lib.rs` | Module exports |
| Create | `crates/apex-mir/src/cfg.rs` | MIR CFG types (`MirFunction`, `BasicBlock`, `Terminator`) |
| Create | `crates/apex-mir/src/extract.rs` | MIR extraction via `rustc -Zunpretty=mir` |

---

### Task 3: Bolero harness format

**Files:**
- Modify: `crates/apex-synth/src/rust.rs`

- [ ] **Step 1: Write failing test**

Add to tests in `crates/apex-synth/src/rust.rs`:

```rust
#[test]
fn synthesize_bolero_harness() {
    let harness = bolero_harness("check_bounds", &["x: u32", "y: u32"]);
    assert!(harness.contains("bolero::check!"));
    assert!(harness.contains("check_bounds"));
}
```

- [ ] **Step 2: Implement Bolero harness generation**

Add function to `crates/apex-synth/src/rust.rs`:

```rust
/// Generate a Bolero harness that works as unit test + fuzz target + Kani proof.
pub fn bolero_harness(function_name: &str, params: &[&str]) -> String {
    let typed_params: String = params
        .iter()
        .map(|p| {
            let parts: Vec<&str> = p.split(':').collect();
            let name = parts[0].trim();
            let ty = parts.get(1).map(|t| t.trim()).unwrap_or("u32");
            format!("    let {name}: {ty} = gen.gen();")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let call_args: String = params
        .iter()
        .map(|p| p.split(':').next().unwrap().trim())
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        r#"#[test]
fn bolero_test_{function_name}() {{
    bolero::check!().with_type().for_each(|gen| {{
{typed_params}
        let _ = {function_name}({call_args});
    }});
}}"#
    )
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-synth bolero`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-synth/src/rust.rs
git commit -m "feat(synth): add Bolero harness emission format"
```

---

### Task 4: MIR CFG types (apex-mir Phase 1)

**Files:**
- Create: `crates/apex-mir/Cargo.toml`
- Create: `crates/apex-mir/src/lib.rs`
- Create: `crates/apex-mir/src/cfg.rs`
- Modify: `Cargo.toml` (workspace members)

- [ ] **Step 1: Create crate and write failing tests**

Create `crates/apex-mir/Cargo.toml`:

```toml
[package]
name = "apex-mir"
version = "0.1.0"
edition = "2021"

[dependencies]
apex-core = { path = "../apex-core" }
serde = { workspace = true }

[dev-dependencies]
proptest = { workspace = true }
```

Create `crates/apex-mir/src/lib.rs`:
```rust
pub mod cfg;
```

Create `crates/apex-mir/src/cfg.rs`:

```rust
//! MIR Control Flow Graph types.
//!
//! Phase 1: typed CFG representation extracted from `rustc -Zunpretty=mir`.

use serde::{Deserialize, Serialize};

/// A function's MIR representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirFunction {
    pub name: String,
    pub blocks: Vec<BasicBlock>,
}

/// A basic block in the MIR CFG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicBlock {
    pub id: usize,
    pub statements: Vec<Statement>,
    pub terminator: Terminator,
}

/// A MIR statement (simplified).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    Assign { place: String, rvalue: String },
    StorageLive(String),
    StorageDead(String),
    Nop,
}

/// A basic block terminator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Terminator {
    /// Unconditional jump.
    Goto { target: usize },
    /// Conditional branch.
    SwitchInt {
        discriminant: String,
        targets: Vec<(i128, usize)>,
        otherwise: usize,
    },
    /// Function return.
    Return,
    /// Unreachable code.
    Unreachable,
    /// Function call.
    Call {
        func: String,
        destination: Option<usize>,
        cleanup: Option<usize>,
    },
    /// Drop (destructor).
    Drop {
        target: usize,
        unwind: Option<usize>,
    },
    /// Panic / abort.
    Abort,
}

impl MirFunction {
    pub fn new(name: impl Into<String>) -> Self {
        MirFunction {
            name: name.into(),
            blocks: Vec::new(),
        }
    }

    pub fn add_block(&mut self, block: BasicBlock) {
        self.blocks.push(block);
    }

    /// Get all successor block IDs from a given block.
    pub fn successors(&self, block_id: usize) -> Vec<usize> {
        if let Some(block) = self.blocks.get(block_id) {
            block.terminator.successors()
        } else {
            Vec::new()
        }
    }

    /// Number of basic blocks.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Count of branch points (SwitchInt terminators).
    pub fn branch_count(&self) -> usize {
        self.blocks
            .iter()
            .filter(|b| matches!(b.terminator, Terminator::SwitchInt { .. }))
            .count()
    }
}

impl Terminator {
    pub fn successors(&self) -> Vec<usize> {
        match self {
            Terminator::Goto { target } => vec![*target],
            Terminator::SwitchInt { targets, otherwise, .. } => {
                let mut succs: Vec<usize> = targets.iter().map(|(_, t)| *t).collect();
                succs.push(*otherwise);
                succs
            }
            Terminator::Return | Terminator::Unreachable | Terminator::Abort => vec![],
            Terminator::Call { destination, cleanup, .. } => {
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
    fn mir_function_creation() {
        let f = MirFunction::new("test_func");
        assert_eq!(f.name, "test_func");
        assert_eq!(f.block_count(), 0);
    }

    #[test]
    fn add_blocks_and_count() {
        let mut f = MirFunction::new("f");
        f.add_block(BasicBlock {
            id: 0,
            statements: vec![],
            terminator: Terminator::Return,
        });
        f.add_block(BasicBlock {
            id: 1,
            statements: vec![Statement::Nop],
            terminator: Terminator::Goto { target: 0 },
        });
        assert_eq!(f.block_count(), 2);
    }

    #[test]
    fn branch_count() {
        let mut f = MirFunction::new("f");
        f.add_block(BasicBlock {
            id: 0,
            statements: vec![],
            terminator: Terminator::SwitchInt {
                discriminant: "x".into(),
                targets: vec![(0, 1), (1, 2)],
                otherwise: 3,
            },
        });
        f.add_block(BasicBlock {
            id: 1,
            statements: vec![],
            terminator: Terminator::Return,
        });
        assert_eq!(f.branch_count(), 1);
    }

    #[test]
    fn successors_goto() {
        let f = MirFunction::new("f");
        let t = Terminator::Goto { target: 5 };
        assert_eq!(t.successors(), vec![5]);
    }

    #[test]
    fn successors_switch_int() {
        let t = Terminator::SwitchInt {
            discriminant: "x".into(),
            targets: vec![(0, 1), (1, 2)],
            otherwise: 3,
        };
        let succs = t.successors();
        assert_eq!(succs, vec![1, 2, 3]);
    }

    #[test]
    fn successors_return_empty() {
        assert!(Terminator::Return.successors().is_empty());
        assert!(Terminator::Unreachable.successors().is_empty());
        assert!(Terminator::Abort.successors().is_empty());
    }

    #[test]
    fn successors_call() {
        let t = Terminator::Call {
            func: "foo".into(),
            destination: Some(1),
            cleanup: Some(2),
        };
        assert_eq!(t.successors(), vec![1, 2]);
    }

    #[test]
    fn successors_drop() {
        let t = Terminator::Drop {
            target: 1,
            unwind: Some(2),
        };
        assert_eq!(t.successors(), vec![1, 2]);
    }

    #[test]
    fn statement_variants() {
        let assign = Statement::Assign {
            place: "_1".into(),
            rvalue: "42".into(),
        };
        let live = Statement::StorageLive("_2".into());
        let dead = Statement::StorageDead("_2".into());
        let nop = Statement::Nop;

        // Just verify they construct without panic
        let _ = format!("{:?}", assign);
        let _ = format!("{:?}", live);
        let _ = format!("{:?}", dead);
        let _ = format!("{:?}", nop);
    }

    #[test]
    fn function_successors_out_of_bounds() {
        let f = MirFunction::new("f");
        assert!(f.successors(999).is_empty());
    }
}
```

- [ ] **Step 2: Add to workspace**

Add `"crates/apex-mir"` to the workspace members in the root `Cargo.toml`.

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-mir`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-mir/ Cargo.toml
git commit -m "feat: add apex-mir crate with MIR CFG types (Phase 1 of MIR symbolic)"
```

---

### Task 5: MIR extraction from rustc output

**Files:**
- Create: `crates/apex-mir/src/extract.rs`
- Modify: `crates/apex-mir/src/lib.rs`

- [ ] **Step 1: Write failing tests**

Create `crates/apex-mir/src/extract.rs`:

```rust
//! MIR extraction from `rustc -Zunpretty=mir` output.
//!
//! Parses the text MIR format into `MirFunction` structs.

use crate::cfg::{BasicBlock, MirFunction, Statement, Terminator};

/// Parse `rustc -Zunpretty=mir` output into MIR functions.
pub fn parse_mir_output(mir_text: &str) -> Vec<MirFunction> {
    let mut functions = Vec::new();
    let mut current_fn: Option<MirFunction> = None;
    let mut current_block: Option<BasicBlock> = None;
    let mut block_id = 0;

    for line in mir_text.lines() {
        let trimmed = line.trim();

        // Function header: `fn function_name(...) -> ... {`
        if trimmed.starts_with("fn ") && trimmed.ends_with('{') {
            if let Some(f) = current_fn.take() {
                functions.push(f);
            }
            let name = extract_fn_name(trimmed);
            current_fn = Some(MirFunction::new(name));
            block_id = 0;
        }

        // Basic block header: `bb0: {` or `bb12: {`
        if trimmed.starts_with("bb") && trimmed.contains(": {") {
            if let Some(ref mut f) = current_fn {
                if let Some(block) = current_block.take() {
                    f.add_block(block);
                }
            }
            current_block = Some(BasicBlock {
                id: block_id,
                statements: Vec::new(),
                terminator: Terminator::Return, // placeholder
            });
            block_id += 1;
        }

        // Terminator lines (simplified detection)
        if let Some(ref mut block) = current_block {
            if trimmed.starts_with("return;") {
                block.terminator = Terminator::Return;
            } else if trimmed.starts_with("goto ->") {
                if let Some(target) = parse_bb_ref(trimmed.trim_start_matches("goto -> ")) {
                    block.terminator = Terminator::Goto { target };
                }
            } else if trimmed.starts_with("unreachable;") {
                block.terminator = Terminator::Unreachable;
            } else if trimmed.contains("_") && trimmed.contains(" = ") && !trimmed.starts_with("//") {
                // Simple assignment detection
                let parts: Vec<&str> = trimmed.splitn(2, " = ").collect();
                if parts.len() == 2 {
                    block.statements.push(Statement::Assign {
                        place: parts[0].trim().trim_end_matches(';').to_string(),
                        rvalue: parts[1].trim().trim_end_matches(';').to_string(),
                    });
                }
            }
        }

        // End of function: closing `}`
        if trimmed == "}" {
            if let Some(ref mut f) = current_fn {
                if let Some(block) = current_block.take() {
                    f.add_block(block);
                }
            }
        }
    }

    if let Some(f) = current_fn {
        functions.push(f);
    }

    functions
}

/// Extract function name from `fn name(...) -> Type {`.
fn extract_fn_name(line: &str) -> String {
    let after_fn = line.trim_start_matches("fn ").trim();
    if let Some(paren_idx) = after_fn.find('(') {
        after_fn[..paren_idx].trim().to_string()
    } else {
        after_fn.trim_end_matches('{').trim().to_string()
    }
}

/// Parse `bb0` → `0`.
fn parse_bb_ref(s: &str) -> Option<usize> {
    let s = s.trim().trim_end_matches(';');
    if s.starts_with("bb") {
        s[2..].parse().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_fn_name_simple() {
        assert_eq!(extract_fn_name("fn foo() -> i32 {"), "foo");
    }

    #[test]
    fn extract_fn_name_with_args() {
        assert_eq!(extract_fn_name("fn bar(x: i32, y: bool) -> () {"), "bar");
    }

    #[test]
    fn parse_bb_ref_valid() {
        assert_eq!(parse_bb_ref("bb0;"), Some(0));
        assert_eq!(parse_bb_ref("bb12"), Some(12));
    }

    #[test]
    fn parse_bb_ref_invalid() {
        assert_eq!(parse_bb_ref("xyz"), None);
    }

    #[test]
    fn parse_simple_mir() {
        let mir = r#"
fn simple() -> i32 {
    bb0: {
        _0 = const 42_i32;
        return;
    }
}
"#;
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].name, "simple");
        assert_eq!(funcs[0].block_count(), 1);
    }

    #[test]
    fn parse_multi_block_mir() {
        let mir = r#"
fn branching(_1: bool) -> i32 {
    bb0: {
        _2 = _1;
        goto -> bb1;
    }

    bb1: {
        return;
    }
}
"#;
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 1);
        assert_eq!(funcs[0].block_count(), 2);
    }

    #[test]
    fn parse_empty_input() {
        let funcs = parse_mir_output("");
        assert!(funcs.is_empty());
    }

    #[test]
    fn parse_multiple_functions() {
        let mir = r#"
fn alpha() -> () {
    bb0: {
        return;
    }
}

fn beta() -> () {
    bb0: {
        return;
    }
}
"#;
        let funcs = parse_mir_output(mir);
        assert_eq!(funcs.len(), 2);
        assert_eq!(funcs[0].name, "alpha");
        assert_eq!(funcs[1].name, "beta");
    }
}
```

- [ ] **Step 2: Update lib.rs**

Add to `crates/apex-mir/src/lib.rs`:
```rust
pub mod extract;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p apex-mir`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/apex-mir/src/extract.rs crates/apex-mir/src/lib.rs
git commit -m "feat(mir): add MIR text parser for rustc -Zunpretty=mir output"
```

---

### Task 6: Final integration verification

- [ ] **Step 1: Run full workspace tests**

```bash
cargo test --workspace
```

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace -- -D warnings
```

- [ ] **Step 3: Run Python tests**

```bash
cd /Users/ad/prj/bcov && python3 -m pytest crates/apex-concolic/python/tests/ -v
```

- [ ] **Step 4: Commit any fixes**

```bash
git add -u crates/
git commit -m "fix: address Phase 4 integration issues"
```
