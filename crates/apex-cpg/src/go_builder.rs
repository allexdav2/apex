//! Simplified line-based Go CPG builder.
//!
//! Parses basic Go patterns without tree-sitter:
//! - `x := ...` and `x = ...` → Assignment nodes
//! - `foo(...)`, `http.Get(...)` → Call nodes
//! - `if err != nil`, `for`, `switch` → ControlStructure nodes
//! - `func name(params)` → Method + Parameter nodes

use apex_core::types::Language;

use crate::{builder::CpgBuilder, Cpg, CtrlKind, EdgeKind, NodeKind};

// ─── Public builder struct ────────────────────────────────────────────────────

/// A [`CpgBuilder`] for Go source files.
///
/// Uses a simplified line-based parser — no tree-sitter dependency — that
/// understands Go function declarations, short/normal assignments, calls,
/// and control structures.
pub struct GoCpgBuilder;

impl CpgBuilder for GoCpgBuilder {
    fn build(&self, source: &str, filename: &str) -> Cpg {
        build_go_cpg(source, filename)
    }

    fn language(&self) -> Language {
        Language::Go
    }
}

// ─── Free-function convenience wrapper ────────────────────────────────────────

/// Build a CPG from Go source code.
pub fn build_go_cpg(source: &str, filename: &str) -> Cpg {
    let mut cpg = Cpg::new();
    let mut parser = InternalGoParser::new(filename);
    parser.parse(source, &mut cpg);
    cpg
}

// ─── Internal builder state ───────────────────────────────────────────────────

struct InternalGoParser<'a> {
    filename: &'a str,
}

impl<'a> InternalGoParser<'a> {
    fn new(filename: &'a str) -> Self {
        Self { filename }
    }

    fn parse(&mut self, source: &str, cpg: &mut Cpg) {
        let lines: Vec<&str> = source.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let trimmed = lines[i].trim();
            if trimmed.starts_with("func ") {
                i = self.parse_function(lines.as_slice(), i, cpg);
            } else {
                i += 1;
            }
        }
    }

    /// Parse a `func name(params) returntype {` block.
    /// Returns the index of the first line after the function.
    fn parse_function(&self, lines: &[&str], def_idx: usize, cpg: &mut Cpg) -> usize {
        let def_line = lines[def_idx].trim();
        let line_no = (def_idx + 1) as u32;

        let (fn_name, params) = parse_go_func_signature(def_line);
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

        // Find body indentation from lines after the `func` line
        let body_start = def_idx + 1;
        let body_indent = body_indentation(lines, body_start);

        let mut prev_stmt: Option<u32> = None;
        let mut i = body_start;
        let mut brace_depth = 0i32;

        // Count braces on the func line itself
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

            // Track braces
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

    /// Parse a single Go statement line.
    fn parse_statement(&self, stmt: &str, line_no: u32, cpg: &mut Cpg) -> Option<u32> {
        if stmt.is_empty() || stmt.starts_with("//") || stmt == "{" || stmt == "}" {
            return None;
        }

        // return <expr>
        if stmt.starts_with("return") {
            let ret_id = cpg.add_node(NodeKind::Return { line: line_no });
            let rest = stmt.trim_start_matches("return").trim();
            if !rest.is_empty() {
                self.attach_expr(rest, line_no, ret_id, 0, cpg);
            }
            return Some(ret_id);
        }

        // Control structures
        if let Some(ctrl) = parse_ctrl(stmt, line_no) {
            return Some(cpg.add_node(ctrl));
        }

        // Short declaration: `x := ...`
        if let Some(pos) = stmt.find(":=") {
            let lhs = stmt[..pos].trim();
            let rhs = stmt[pos + 2..].trim();
            if !lhs.is_empty() && !rhs.is_empty() && lhs_is_valid_go(lhs) {
                let assign_id = cpg.add_node(NodeKind::Assignment {
                    lhs: lhs.to_string(),
                    line: line_no,
                });
                self.attach_expr(rhs, line_no, assign_id, 0, cpg);
                return Some(assign_id);
            }
        }

        // Normal assignment: `x = ...` (but not ==, !=, <=, >=, :=)
        if let Some((lhs, rhs)) = parse_go_assignment(stmt) {
            let assign_id = cpg.add_node(NodeKind::Assignment {
                lhs: lhs.to_string(),
                line: line_no,
            });
            self.attach_expr(rhs.trim(), line_no, assign_id, 0, cpg);
            return Some(assign_id);
        }

        // Bare call expression
        if let Some(call_id) = self.try_parse_call(stmt, line_no, cpg) {
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
        if expr.starts_with('"') || expr.starts_with('`') || expr.starts_with('\'') {
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

        // Plain identifier or dotted name
        let name = expr
            .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
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

    /// Try to parse `expr` as a call like `foo(...)` or `pkg.Func(...)`.
    fn try_parse_call(&self, expr: &str, line_no: u32, cpg: &mut Cpg) -> Option<u32> {
        let paren = expr.find('(')?;
        if paren == 0 {
            return None;
        }
        let callee = &expr[..paren];
        if !callee
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
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

/// Parse `func name(p1 type, p2 type) returntype {` → (name, [param_names])
/// Also handles receiver methods: `func (r *Receiver) name(params)`
fn parse_go_func_signature(line: &str) -> (String, Vec<String>) {
    let line = line.trim_start_matches("func ").trim();

    // Skip receiver: `(r *Type) name(...)`
    let line = if line.starts_with('(') {
        // Find closing `)`
        if let Some(close) = line.find(')') {
            line[close + 1..].trim()
        } else {
            line
        }
    } else {
        line
    };

    let paren = line.find('(').unwrap_or(line.len());
    let name = line[..paren].trim().to_string();

    let params = if let (Some(open), Some(close)) = (line.find('('), line.find(')')) {
        let inner = &line[open + 1..close];
        // Go params: `w http.ResponseWriter, r *http.Request`
        // We want the parameter names (first token of each param entry)
        inner
            .split(',')
            .filter_map(|p| {
                let p = p.trim();
                if p.is_empty() {
                    return None;
                }
                // The parameter name is the first word before the type
                let name = p
                    .split_whitespace()
                    .next()
                    .unwrap_or(p)
                    .trim_start_matches('*')
                    .trim()
                    .to_string();
                if name.is_empty() {
                    None
                } else {
                    Some(name)
                }
            })
            .collect()
    } else {
        vec![]
    };

    (name, params)
}

/// Parse normal Go assignment `x = rhs` (not `:=`, `==`, `!=`, `<=`, `>=`).
fn parse_go_assignment(stmt: &str) -> Option<(&str, &str)> {
    let bytes = stmt.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'=' {
            let prev = if i > 0 { bytes[i - 1] } else { 0 };
            let next = if i + 1 < bytes.len() { bytes[i + 1] } else { 0 };
            // Skip ==, !=, <=, >=, :=, +=, -=, *=, /=, %=
            if next == b'=' {
                continue;
            }
            if prev == b'!'
                || prev == b'<'
                || prev == b'>'
                || prev == b'='
                || prev == b':'
                || prev == b'+'
                || prev == b'-'
                || prev == b'*'
                || prev == b'/'
                || prev == b'%'
            {
                continue;
            }
            let lhs = stmt[..i].trim();
            let rhs = stmt[i + 1..].trim();
            if !lhs.is_empty() && !rhs.is_empty() && lhs_is_valid_go(lhs) {
                return Some((lhs, rhs));
            }
        }
    }
    None
}

/// Check that a Go LHS looks like a valid variable name (or tuple of names).
fn lhs_is_valid_go(lhs: &str) -> bool {
    lhs.split(',').all(|part| {
        let p = part.trim();
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_alphanumeric() || c == '_')
    })
}

/// Parse control-structure keywords.
fn parse_ctrl(stmt: &str, line_no: u32) -> Option<NodeKind> {
    let kind = if stmt.starts_with("if ") || stmt.starts_with("if(") {
        CtrlKind::If
    } else if stmt.starts_with("for ") || stmt.starts_with("for{") || stmt == "for {" {
        CtrlKind::For
    } else if stmt.starts_with("switch ") || stmt.starts_with("switch{") || stmt == "switch {" {
        // Represent switch as ControlStructure with CtrlKind::If (closest match)
        CtrlKind::If
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
    1 // Go uses tabs; tab counts as 1 space here
}

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

    // ── Task 3.2 tests ────────────────────────────────────────────────────────

    #[test]
    fn go_builder_parses_http_handler() {
        let source = r#"
func handler(w http.ResponseWriter, r *http.Request) {
	id := r.URL.Query().Get("id")
	db.Query(id)
}
"#;
        let cpg = build_go_cpg(source, "handler.go");
        let names = method_names(&cpg);
        assert!(names.contains(&"handler".to_string()), "should parse handler func; got {:?}", names);
        let params = param_names(&cpg);
        assert!(params.contains(&"w".to_string()), "should have param w; got {:?}", params);
        assert!(params.contains(&"r".to_string()), "should have param r; got {:?}", params);
    }

    #[test]
    fn go_builder_taint_flow_query_to_db() {
        let source = r#"
func query(w http.ResponseWriter, r *http.Request) {
	id := r.URL.Query().Get("id")
	db.Query(id)
}
"#;
        let cpg = build_go_cpg(source, "query.go");
        // Should have Assignment for id
        let assigns = assignment_lhs(&cpg);
        assert!(assigns.contains(&"id".to_string()), "should have assignment for id; got {:?}", assigns);
        // Should have Call for db.Query
        let calls = call_names(&cpg);
        assert!(calls.contains(&"db.Query".to_string()), "should have db.Query call; got {:?}", calls);
    }

    #[test]
    fn go_builder_short_declaration_assignment() {
        let source = r#"
func foo() {
	x := 42
	y := "hello"
}
"#;
        let cpg = build_go_cpg(source, "foo.go");
        let assigns = assignment_lhs(&cpg);
        assert!(assigns.contains(&"x".to_string()), "should have x assignment");
        assert!(assigns.contains(&"y".to_string()), "should have y assignment");
    }

    #[test]
    fn go_builder_normal_assignment() {
        let source = r#"
func foo() {
	var x int
	x = 5
}
"#;
        let cpg = build_go_cpg(source, "foo.go");
        let assigns = assignment_lhs(&cpg);
        assert!(assigns.contains(&"x".to_string()), "should have x assignment");
    }

    #[test]
    fn go_builder_parses_package_calls() {
        let source = r#"
func foo() {
	exec.Command("ls", "-la")
	http.Get("http://example.com")
}
"#;
        let cpg = build_go_cpg(source, "foo.go");
        let calls = call_names(&cpg);
        assert!(calls.contains(&"exec.Command".to_string()) || calls.contains(&"http.Get".to_string()),
            "should detect package calls; got {:?}", calls);
    }

    #[test]
    fn go_builder_parses_control_structures() {
        let source = r#"
func foo(err error) {
	if err != nil {
		return
	}
	for i := 0; i < 10; i++ {
	}
}
"#;
        let cpg = build_go_cpg(source, "foo.go");
        let has_if = cpg.nodes().any(|(_, k)| {
            matches!(k, NodeKind::ControlStructure { kind: CtrlKind::If, .. })
        });
        assert!(has_if, "should detect if structure");
        let has_for = cpg.nodes().any(|(_, k)| {
            matches!(k, NodeKind::ControlStructure { kind: CtrlKind::For, .. })
        });
        assert!(has_for, "should detect for structure");
    }

    #[test]
    fn go_builder_creates_cfg_edges() {
        let source = r#"
func foo() {
	a := 1
	b := 2
	c := 3
}
"#;
        let cpg = build_go_cpg(source, "foo.go");
        let cfg_count = cpg
            .edges()
            .filter(|(_, _, k)| matches!(k, EdgeKind::Cfg))
            .count();
        assert!(cfg_count >= 2, "expected >=2 CFG edges, got {cfg_count}");
    }

    #[test]
    fn go_builder_language_is_go() {
        use apex_core::types::Language;
        let builder = GoCpgBuilder;
        assert_eq!(builder.language(), Language::Go);
    }

    #[test]
    fn go_builder_build_produces_nodes() {
        let builder = GoCpgBuilder;
        let cpg = builder.build(
            "func greet(name string) {\n\tfmt.Println(name)\n}\n",
            "greet.go",
        );
        assert!(cpg.node_count() > 0);
    }

    #[test]
    fn go_builder_receiver_method() {
        let source = r#"
func (h *Handler) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	id := r.FormValue("id")
	db.Exec(id)
}
"#;
        let cpg = build_go_cpg(source, "handler.go");
        let names = method_names(&cpg);
        assert!(names.contains(&"ServeHTTP".to_string()), "should parse receiver method; got {:?}", names);
    }
}
