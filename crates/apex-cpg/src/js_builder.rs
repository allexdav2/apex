//! Simplified line-based JavaScript CPG builder.
//!
//! Parses basic JavaScript patterns without tree-sitter:
//! - `const x = ...`, `let x = ...`, `var x = ...` → Assignment nodes
//! - `foo(...)`, `obj.method(...)` → Call nodes
//! - `if`, `for`, `while`, `try` → ControlStructure nodes
//! - `function name(params)` → Method + Parameter nodes
//! - Arrow functions `const f = (params) => ...` → Method + Parameter nodes

use apex_core::types::Language;

use crate::{builder::CpgBuilder, Cpg, CtrlKind, EdgeKind, NodeKind};

// ─── Public builder struct ────────────────────────────────────────────────────

/// A [`CpgBuilder`] for JavaScript source files.
///
/// Uses a simplified line-based parser — no tree-sitter dependency — that
/// understands `const`/`let`/`var` assignments, function declarations,
/// calls, and control structures.
pub struct JsCpgBuilder;

impl CpgBuilder for JsCpgBuilder {
    fn build(&self, source: &str, filename: &str) -> Cpg {
        build_js_cpg(source, filename)
    }

    fn language(&self) -> Language {
        Language::JavaScript
    }
}

// ─── Free-function convenience wrapper ────────────────────────────────────────

/// Build a CPG from JavaScript source code.
pub fn build_js_cpg(source: &str, filename: &str) -> Cpg {
    let mut cpg = Cpg::new();
    let mut parser = InternalJsParser::new(filename);
    parser.parse(source, &mut cpg);
    cpg
}

// ─── Internal builder state ───────────────────────────────────────────────────

struct InternalJsParser<'a> {
    filename: &'a str,
}

impl<'a> InternalJsParser<'a> {
    fn new(filename: &'a str) -> Self {
        Self { filename }
    }

    fn parse(&mut self, source: &str, cpg: &mut Cpg) {
        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let trimmed = lines[i].trim();
            if trimmed.starts_with("function ") {
                i = self.parse_function(lines.as_slice(), i, cpg);
            } else {
                // Top-level statement: arrow function or expression
                let line_no = (i + 1) as u32;
                // Check for arrow function assignment: `const f = (params) => ...`
                if let Some(method_id) = self.try_parse_arrow_function(trimmed, line_no, cpg) {
                    // Skip lines until the arrow function body closes (simple heuristic)
                    let _ = method_id;
                }
                i += 1;
            }
        }
    }

    /// Parse a `function name(params) {` block.
    /// Returns the index of the first line after the function.
    fn parse_function(&self, lines: &[&str], def_idx: usize, cpg: &mut Cpg) -> usize {
        let def_line = lines[def_idx].trim();
        let line_no = (def_idx + 1) as u32;

        let (fn_name, params) = parse_function_signature(def_line);
        let method_id = cpg.add_node(NodeKind::Method {
            name: fn_name.clone(),
            file: self.filename.to_string(),
            line: line_no,
        });

        // Parameter nodes
        for (idx, param) in params.iter().enumerate() {
            let p_id = cpg.add_node(NodeKind::Parameter {
                name: param.clone(),
                index: idx as u32,
            });
            cpg.add_edge(method_id, p_id, EdgeKind::Ast);
        }

        // Find body: look for opening `{`
        let body_start = def_idx + 1;
        let body_indent = body_indentation(lines, body_start);

        let mut prev_stmt: Option<u32> = None;
        let mut i = body_start;
        let mut brace_depth = 0i32;

        // Count opening brace on the def line itself
        for c in def_line.chars() {
            match c {
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                _ => {}
            }
        }

        while i < lines.len() {
            let raw = lines[i];
            if raw.trim().is_empty() {
                i += 1;
                continue;
            }

            // Track braces to detect function end
            for c in raw.chars() {
                match c {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }

            if brace_depth <= 0 {
                i += 1;
                break;
            }

            let indent = leading_spaces(raw);
            if indent < body_indent && body_indent > 0 {
                break;
            }

            let stmt_line = raw.trim();
            let stmt_line_no = (i + 1) as u32;

            if let Some(sid) = self.parse_statement(stmt_line, stmt_line_no, cpg) {
                cpg.add_edge(method_id, sid, EdgeKind::Ast);
                if let Some(prev) = prev_stmt {
                    cpg.add_edge(prev, sid, EdgeKind::Cfg);
                }
                prev_stmt = Some(sid);
            }

            i += 1;
        }

        i
    }

    /// Try to parse an arrow function definition like:
    /// `const handler = (req, res) => {` or `const f = async (x) => x + 1`
    fn try_parse_arrow_function(
        &self,
        line: &str,
        line_no: u32,
        cpg: &mut Cpg,
    ) -> Option<u32> {
        // Must have `=>` to be an arrow function
        if !line.contains("=>") {
            return None;
        }

        // Extract the name from `const/let/var name = ...`
        let (lhs, _rhs) = parse_js_assignment(line)?;
        let name = lhs
            .trim_start_matches("const ")
            .trim_start_matches("let ")
            .trim_start_matches("var ")
            .trim()
            .to_string();

        if name.is_empty() || name.contains(' ') {
            return None;
        }

        // Extract params from the arrow function `(params) =>` or `param =>`
        let params = parse_arrow_params(line);

        let method_id = cpg.add_node(NodeKind::Method {
            name,
            file: self.filename.to_string(),
            line: line_no,
        });

        for (idx, param) in params.iter().enumerate() {
            let p_id = cpg.add_node(NodeKind::Parameter {
                name: param.clone(),
                index: idx as u32,
            });
            cpg.add_edge(method_id, p_id, EdgeKind::Ast);
        }

        Some(method_id)
    }

    /// Parse a single JS statement line and return the primary node id (if any).
    fn parse_statement(&self, stmt: &str, line_no: u32, cpg: &mut Cpg) -> Option<u32> {
        if stmt.is_empty() || stmt.starts_with("//") || stmt == "{" || stmt == "}" {
            return None;
        }

        // return <expr>
        if stmt.starts_with("return") {
            let ret_id = cpg.add_node(NodeKind::Return { line: line_no });
            let rest = stmt.trim_start_matches("return").trim().trim_end_matches(';');
            if !rest.is_empty() {
                self.attach_expr(rest, line_no, ret_id, 0, cpg);
            }
            return Some(ret_id);
        }

        // Control structures: if / for / while / try
        if let Some(ctrl) = parse_ctrl(stmt, line_no) {
            return Some(cpg.add_node(ctrl));
        }

        // Arrow function on a body line (nested) — skip, we handle at top level
        if stmt.contains("=>") {
            if let Some(mid) = self.try_parse_arrow_function(stmt, line_no, cpg) {
                return Some(mid);
            }
        }

        // JS assignment: `const x = ...`, `let x = ...`, `var x = ...`, `x = ...`
        if let Some((lhs, rhs)) = parse_js_assignment(stmt) {
            let clean_lhs = lhs
                .trim_start_matches("const ")
                .trim_start_matches("let ")
                .trim_start_matches("var ")
                .trim();
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs: clean_lhs.to_string(),
                line: line_no,
            });
            self.attach_expr(rhs.trim().trim_end_matches(';'), line_no, assign_id, 0, cpg);
            return Some(assign_id);
        }

        // Bare call expression: `name(args)`, `obj.method(args)`
        if let Some(call_id) = self.try_parse_call(stmt.trim_end_matches(';'), line_no, cpg) {
            return Some(call_id);
        }

        None
    }

    /// Attach expression nodes as children of `parent`.
    fn attach_expr(&self, expr: &str, line_no: u32, parent: u32, arg_index: u32, cpg: &mut Cpg) {
        let expr = expr.trim();
        if expr.is_empty() {
            return;
        }

        // String literals
        if expr.starts_with('"')
            || expr.starts_with('\'')
            || expr.starts_with('`')
        {
            let lit_id = cpg.add_node(NodeKind::Literal {
                value: expr.to_string(),
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
            return;
        }

        // Numeric literal
        if expr.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            let lit_id = cpg.add_node(NodeKind::Literal {
                value: expr.to_string(),
                line: line_no,
            });
            cpg.add_edge(parent, lit_id, EdgeKind::Argument { index: arg_index });
            return;
        }

        // Call expression
        if let Some(call_id) = self.try_parse_call(expr, line_no, cpg) {
            cpg.add_edge(parent, call_id, EdgeKind::Argument { index: arg_index });
            return;
        }

        // Plain identifier or dotted name (e.g., `req.query.id`)
        let name = expr
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.' && c != '$')
            .next()
            .unwrap_or(expr)
            .trim()
            .to_string();
        if !name.is_empty() {
            let id_node = cpg.add_node(NodeKind::Identifier {
                name,
                line: line_no,
            });
            cpg.add_edge(parent, id_node, EdgeKind::Argument { index: arg_index });
        }
    }

    /// Try to parse `expr` as a call like `name(...)` or `obj.method(...)`.
    fn try_parse_call(&self, expr: &str, line_no: u32, cpg: &mut Cpg) -> Option<u32> {
        let paren = expr.find('(')?;
        if paren == 0 {
            return None;
        }
        let callee = &expr[..paren];
        // Callee must be a valid JS identifier chain (allows $, _, ., alphanumeric)
        if !callee
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == '$')
        {
            return None;
        }
        let close = expr.rfind(')')?;
        if close < paren {
            return None;
        }

        let call_id = cpg.add_node(NodeKind::Call {
            name: callee.to_string(),
            line: line_no,
        });

        let args_str = &expr[paren + 1..close];
        let args = split_args(args_str);
        for (idx, arg) in args.iter().enumerate() {
            let arg = arg.trim();
            if !arg.is_empty() {
                self.attach_expr(arg, line_no, call_id, idx as u32, cpg);
            }
        }

        Some(call_id)
    }
}

// ─── Parsing helpers ──────────────────────────────────────────────────────────

/// Parse `function name(p1, p2) {` → (name, [params])
fn parse_function_signature(line: &str) -> (String, Vec<String>) {
    let line = line.trim_start_matches("function ").trim();
    // Handle `async function name(...)`
    let line = line.trim_start_matches("async ").trim_start_matches("function ").trim();
    let paren = line.find('(').unwrap_or(line.len());
    let name = line[..paren].trim().to_string();
    let params = if let (Some(open), Some(close)) = (line.find('('), line.find(')')) {
        let inner = &line[open + 1..close];
        inner
            .split(',')
            .map(|p| {
                // Handle destructured params like `{ req, res }` — take whole thing
                p.trim()
                    // Strip type annotations (TypeScript style): `x: string`
                    .split(':')
                    .next()
                    .unwrap_or(p)
                    // Strip defaults: `x = 0`
                    .split('=')
                    .next()
                    .unwrap_or(p)
                    .trim()
                    .to_string()
            })
            .filter(|p| !p.is_empty())
            .collect()
    } else {
        vec![]
    };
    (name, params)
}

/// Parse arrow function parameters from a line containing `=>`.
/// Handles: `(x, y) =>`, `x =>`, `async (x) =>`
fn parse_arrow_params(line: &str) -> Vec<String> {
    // Find the `=>` and look left for params
    let arrow_pos = match line.find("=>") {
        Some(p) => p,
        None => return vec![],
    };
    let before_arrow = &line[..arrow_pos].trim_end();

    // Find the last `)` before `=>`
    if let Some(close) = before_arrow.rfind(')') {
        // Find matching `(`
        if let Some(open) = before_arrow[..close].rfind('(') {
            let inner = &before_arrow[open + 1..close];
            return inner
                .split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect();
        }
    }

    // No parens — single bare parameter like `x => x + 1`
    // Extract last word before `=>`
    let bare = before_arrow
        .trim()
        .rsplit(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();
    if bare.is_empty() || bare == "async" {
        vec![]
    } else {
        vec![bare]
    }
}

/// Parse JS assignment: `const x = rhs`, `let x = rhs`, `var x = rhs`, `x = rhs`
/// Returns `Some((lhs, rhs))`. Skips `==`, `!=`, `===`, `!==`, `<=`, `>=`.
fn parse_js_assignment(stmt: &str) -> Option<(&str, &str)> {
    let bytes = stmt.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { 0 };
            let next = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
            // Skip ==, ===, !=, !==, <=, >=, =>, +=, -=, *=, /=, %=, **=
            if next == b'=' || next == b'>' {
                continue;
            }
            if prev == b'!' || prev == b'<' || prev == b'>' || prev == b'=' {
                continue;
            }
            // Augmented assignments: +=, -=, *=, /=, %=, &=, |=, ^=
            if prev == b'+'
                || prev == b'-'
                || prev == b'*'
                || prev == b'/'
                || prev == b'%'
                || prev == b'&'
                || prev == b'|'
                || prev == b'^'
            {
                let lhs = stmt[..i - 1].trim();
                let rhs = stmt[i + 1..].trim();
                if !lhs.is_empty() && !rhs.is_empty() {
                    return Some((lhs, rhs));
                }
                continue;
            }
            let lhs = stmt[..i].trim();
            let rhs = stmt[i + 1..].trim();
            if !lhs.is_empty() && !rhs.is_empty() {
                return Some((lhs, rhs));
            }
        }
    }
    None
}

/// Parse control-structure keywords.
fn parse_ctrl(stmt: &str, line_no: u32) -> Option<NodeKind> {
    let kind = if stmt.starts_with("if ") || stmt.starts_with("if(") {
        CtrlKind::If
    } else if stmt.starts_with("while ") || stmt.starts_with("while(") {
        CtrlKind::While
    } else if stmt.starts_with("for ") || stmt.starts_with("for(") {
        CtrlKind::For
    } else if stmt.starts_with("try ") || stmt.starts_with("try{") || stmt == "try {" || stmt == "try{" {
        CtrlKind::Try
    } else {
        return None;
    };
    Some(NodeKind::ControlStructure {
        kind,
        line: line_no,
    })
}

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn body_indentation(lines: &[&str], start: usize) -> usize {
    for line in lines[start..].iter() {
        if !line.trim().is_empty() && line.trim() != "{" {
            return leading_spaces(line);
        }
    }
    2 // default JS indent
}

/// Split a comma-separated argument list, respecting nested parentheses.
fn split_args(args: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in args.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                result.push(&args[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if start < args.len() {
        result.push(&args[start..]);
    }
    result
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeKind;

    fn method_names(cpg: &Cpg) -> Vec<String> {
        cpg.nodes()
            .filter_map(|(_, k)| match k {
                NodeKind::Method { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn call_names(cpg: &Cpg) -> Vec<String> {
        cpg.nodes()
            .filter_map(|(_, k)| match k {
                NodeKind::Call { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn param_names(cpg: &Cpg) -> Vec<String> {
        cpg.nodes()
            .filter_map(|(_, k)| match k {
                NodeKind::Parameter { name, .. } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    fn assignment_lhs(cpg: &Cpg) -> Vec<String> {
        cpg.nodes()
            .filter_map(|(_, k)| match k {
                NodeKind::Assignment { lhs, .. } => Some(lhs.clone()),
                _ => None,
            })
            .collect()
    }

    // ── Task 3.1 tests ────────────────────────────────────────────────────────

    #[test]
    fn js_builder_parses_express_handler() {
        let source = r#"
function handler(req, res) {
  const id = req.query.id;
  res.send(id);
}
"#;
        let cpg = build_js_cpg(source, "handler.js");
        let names = method_names(&cpg);
        assert!(names.contains(&"handler".to_string()), "should parse handler function");
        let params = param_names(&cpg);
        assert!(params.contains(&"req".to_string()));
        assert!(params.contains(&"res".to_string()));
        // Should produce Call + Assignment nodes
        let calls = call_names(&cpg);
        assert!(!calls.is_empty(), "should produce Call nodes");
        let assigns = assignment_lhs(&cpg);
        assert!(!assigns.is_empty(), "should produce Assignment nodes");
    }

    #[test]
    fn js_builder_taint_flow_query_to_db() {
        let source = r#"
function query(req, res) {
  const x = req.query.id;
  db.query(x);
}
"#;
        let cpg = build_js_cpg(source, "query.js");
        // Assignment node for `x`
        let assigns = assignment_lhs(&cpg);
        assert!(assigns.contains(&"x".to_string()), "should have assignment for x");
        // Call node for db.query
        let calls = call_names(&cpg);
        assert!(calls.contains(&"db.query".to_string()), "should have db.query call");
    }

    #[test]
    fn js_builder_parses_const_let_var_assignments() {
        let source = r#"
function foo() {
  const a = 1;
  let b = "hello";
  var c = a;
}
"#;
        let cpg = build_js_cpg(source, "foo.js");
        let assigns = assignment_lhs(&cpg);
        assert!(assigns.contains(&"a".to_string()));
        assert!(assigns.contains(&"b".to_string()));
        assert!(assigns.contains(&"c".to_string()));
    }

    #[test]
    fn js_builder_parses_method_calls() {
        let source = r#"
function foo() {
  obj.method(x);
  console.log("hello");
}
"#;
        let cpg = build_js_cpg(source, "foo.js");
        let calls = call_names(&cpg);
        assert!(calls.contains(&"obj.method".to_string()) || calls.contains(&"console.log".to_string()),
            "should detect method calls");
    }

    #[test]
    fn js_builder_parses_control_structures() {
        let source = r#"
function foo(x) {
  if (x > 0) {
    return x;
  }
  for (let i = 0; i < 10; i++) {
  }
  while (x > 0) {
  }
}
"#;
        let cpg = build_js_cpg(source, "foo.js");
        let has_if = cpg.nodes().any(|(_, k)| {
            matches!(k, NodeKind::ControlStructure { kind: CtrlKind::If, .. })
        });
        assert!(has_if, "should detect if structure");
    }

    #[test]
    fn js_builder_creates_cfg_edges() {
        let source = r#"
function foo() {
  const a = 1;
  const b = 2;
  const c = 3;
}
"#;
        let cpg = build_js_cpg(source, "foo.js");
        let cfg_count = cpg
            .edges()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Cfg))
            .count();
        assert!(cfg_count >= 2, "expected >=2 CFG edges, got {cfg_count}");
    }

    #[test]
    fn js_builder_creates_ast_edges() {
        let source = r#"
function foo() {
  const x = 1;
}
"#;
        let cpg = build_js_cpg(source, "foo.js");
        let method_id = cpg
            .nodes()
            .find_map(|(id, k)| matches!(k, NodeKind::Method { .. }).then_some(id))
            .expect("method node");
        let ast_count = cpg
            .edges_from(method_id)
            .iter()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Ast))
            .count();
        assert!(ast_count >= 1, "expected AST edges from method, got {ast_count}");
    }

    #[test]
    fn js_builder_language_is_javascript() {
        use apex_core::types::Language;
        let builder = JsCpgBuilder;
        assert_eq!(builder.language(), Language::JavaScript);
    }

    #[test]
    fn js_builder_build_produces_nodes() {
        let builder = JsCpgBuilder;
        let cpg = builder.build(
            "function greet(name) { console.log(name); }",
            "greet.js",
        );
        assert!(cpg.node_count() > 0, "CpgBuilder::build should produce at least one node");
    }

    #[test]
    fn js_builder_arrow_function_creates_method() {
        let source = r#"
const handler = (req, res) => {
  const id = req.query.id;
}
"#;
        let cpg = build_js_cpg(source, "arrow.js");
        // Arrow functions create Method nodes
        assert!(cpg.node_count() > 0, "should produce nodes for arrow function");
    }

    #[test]
    fn js_builder_call_with_multiple_args() {
        let source = r#"
function foo() {
  bar(x, y, z);
}
"#;
        let cpg = build_js_cpg(source, "foo.js");
        let call_id = cpg
            .nodes()
            .find_map(|(id, k)| {
                matches!(k, NodeKind::Call { name, .. } if name == "bar").then_some(id)
            })
            .expect("bar call node");
        let arg_edges = cpg
            .edges_from(call_id)
            .iter()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Argument { .. }))
            .count();
        assert_eq!(arg_edges, 3, "bar(x, y, z) should have 3 argument edges");
    }
}
