use super::CallGraphExtractor;
use crate::entry_points::EntryPointKind;
use crate::graph::{CallEdge, CallGraph, FnId, FnNode};
use apex_core::types::Language;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

pub struct CCppExtractor;

static RE_C_FUNC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:static\s+)?(?:inline\s+)?(?:const\s+)?\w+[\s*]+(\w+)\s*\(").unwrap()
});

static RE_CPP_METHOD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\w+[\s*]+(\w+)::(\w+)\s*\(").unwrap());

static RE_CALL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\w+)\s*\(").unwrap());

static RE_GTEST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*TEST(?:_F)?\s*\(\s*(\w+)\s*,\s*(\w+)").unwrap());

static RE_CATCH2: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"^\s*TEST_CASE\s*\(\s*"([^"]+)""#).unwrap());

static RE_BOOST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*BOOST_AUTO_TEST_CASE\s*\(\s*(\w+)").unwrap());

const KEYWORDS: &[&str] = &[
    "if",
    "else",
    "for",
    "while",
    "switch",
    "case",
    "do",
    "return",
    "break",
    "continue",
    "goto",
    "sizeof",
    "typedef",
    "struct",
    "enum",
    "union",
    "class",
    "namespace",
    "template",
    "typename",
    "static",
    "extern",
    "inline",
    "const",
    "void",
    "int",
    "char",
    "float",
    "double",
    "long",
    "short",
    "unsigned",
    "signed",
    "auto",
    "register",
    "volatile",
    "bool",
    "true",
    "false",
    "new",
    "delete",
    "throw",
    "try",
    "catch",
    "nullptr",
    "this",
    "public",
    "private",
    "protected",
    "virtual",
    "override",
    "final",
    "include",
    "define",
    "ifdef",
    "ifndef",
    "endif",
    "pragma",
];

impl CallGraphExtractor for CCppExtractor {
    fn language(&self) -> Language {
        Language::C
    }

    fn extract(&self, sources: &HashMap<PathBuf, String>) -> CallGraph {
        let mut graph = CallGraph::default();
        let mut next_id: u32 = 0;
        let mut fn_index: HashMap<String, Vec<FnId>> = HashMap::new();
        let mut pending_edges: Vec<(FnId, String, u32, Option<u32>)> = Vec::new();

        for (path, source) in sources {
            let lines: Vec<&str> = source.lines().collect();
            let mut brace_depth: i32 = 0;
            let mut current_fn: Option<(FnId, i32)> = None;
            let mut block_id: u32 = 0;

            for (i, &line) in lines.iter().enumerate() {
                let line_num = (i + 1) as u32;
                let trimmed = line.trim();

                if trimmed.starts_with("//")
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with("*")
                    || trimmed.starts_with("#")
                {
                    continue;
                }

                // GoogleTest
                if let Some(caps) = RE_GTEST.captures(trimmed) {
                    let name = format!("{}_{}", &caps[1], &caps[2]);
                    let id = FnId(next_id);
                    next_id += 1;
                    graph.nodes.push(FnNode {
                        id,
                        name: name.clone(),
                        file: path.clone(),
                        start_line: line_num,
                        end_line: line_num,
                        entry_kind: Some(EntryPointKind::Test),
                    });
                    fn_index.entry(name).or_default().push(id);
                    current_fn = Some((id, brace_depth));
                }
                // Catch2
                else if let Some(caps) = RE_CATCH2.captures(trimmed) {
                    let name = caps[1].replace(' ', "_");
                    let id = FnId(next_id);
                    next_id += 1;
                    graph.nodes.push(FnNode {
                        id,
                        name: name.clone(),
                        file: path.clone(),
                        start_line: line_num,
                        end_line: line_num,
                        entry_kind: Some(EntryPointKind::Test),
                    });
                    fn_index.entry(name).or_default().push(id);
                    current_fn = Some((id, brace_depth));
                }
                // Boost
                else if let Some(caps) = RE_BOOST.captures(trimmed) {
                    let name = caps[1].to_string();
                    let id = FnId(next_id);
                    next_id += 1;
                    graph.nodes.push(FnNode {
                        id,
                        name: name.clone(),
                        file: path.clone(),
                        start_line: line_num,
                        end_line: line_num,
                        entry_kind: Some(EntryPointKind::Test),
                    });
                    fn_index.entry(name).or_default().push(id);
                    current_fn = Some((id, brace_depth));
                }
                // C++ method
                else if let Some(caps) = RE_CPP_METHOD.captures(trimmed) {
                    if current_fn.is_some() {
                        // Close previous
                        if let Some((prev_id, _)) = current_fn.take() {
                            if let Some(n) = graph.nodes.iter_mut().find(|n| n.id == prev_id) {
                                n.end_line = line_num.saturating_sub(1);
                            }
                        }
                    }
                    let name = format!("{}::{}", &caps[1], &caps[2]);
                    let id = FnId(next_id);
                    next_id += 1;
                    graph.nodes.push(FnNode {
                        id,
                        name: name.clone(),
                        file: path.clone(),
                        start_line: line_num,
                        end_line: line_num,
                        entry_kind: None,
                    });
                    fn_index.entry(name.clone()).or_default().push(id);
                    fn_index.entry(caps[2].to_string()).or_default().push(id);
                    current_fn = Some((id, brace_depth));
                    block_id = 0;
                }
                // C function at top level
                else if brace_depth == 0 {
                    if let Some(caps) = RE_C_FUNC.captures(trimmed) {
                        let name = caps[1].to_string();
                        if !KEYWORDS.contains(&name.as_str()) && trimmed.contains('(') {
                            if let Some((prev_id, _)) = current_fn.take() {
                                if let Some(n) = graph.nodes.iter_mut().find(|n| n.id == prev_id) {
                                    n.end_line = line_num.saturating_sub(1);
                                }
                            }
                            let entry_kind = if name == "main" {
                                Some(EntryPointKind::Main)
                            } else {
                                None
                            };
                            let id = FnId(next_id);
                            next_id += 1;
                            graph.nodes.push(FnNode {
                                id,
                                name: name.clone(),
                                file: path.clone(),
                                start_line: line_num,
                                end_line: line_num,
                                entry_kind,
                            });
                            fn_index.entry(name).or_default().push(id);
                            current_fn = Some((id, brace_depth));
                            block_id = 0;
                        }
                    }
                }

                // Brace tracking
                for ch in trimmed.chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => {
                            brace_depth -= 1;
                            if let Some((fn_id, open_depth)) = &current_fn {
                                if brace_depth <= *open_depth {
                                    if let Some(n) = graph.nodes.iter_mut().find(|n| n.id == *fn_id)
                                    {
                                        n.end_line = line_num;
                                    }
                                    current_fn = None;
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // Extract calls
                if let Some((fn_id, _)) = &current_fn {
                    if trimmed.starts_with("if ")
                        || trimmed.starts_with("for ")
                        || trimmed.starts_with("while ")
                        || trimmed.starts_with("switch ")
                    {
                        block_id += 1;
                    }
                    let block = if block_id > 0 { Some(block_id) } else { None };

                    for caps in RE_CALL.captures_iter(trimmed) {
                        let name = caps[1].to_string();
                        if !KEYWORDS.contains(&name.as_str()) {
                            pending_edges.push((*fn_id, name, line_num, block));
                        }
                    }
                }
            }
        }

        // Resolve edges
        for (caller_id, callee_name, line, block) in pending_edges {
            if let Some(callee_ids) = fn_index.get(&callee_name) {
                for &callee_id in callee_ids {
                    if callee_id != caller_id {
                        graph.edges.push(CallEdge {
                            caller: caller_id,
                            callee: callee_id,
                            call_site_line: line,
                            call_site_block: block,
                        });
                    }
                }
            }
        }

        graph.build_indices();
        graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_file(name: &str, src: &str) -> HashMap<PathBuf, String> {
        let mut m = HashMap::new();
        m.insert(PathBuf::from(name), src.to_string());
        m
    }

    #[test]
    fn detects_c_functions() {
        let src = "int main(int argc, char **argv) {\n    helper();\n    return 0;\n}\n\nvoid helper() {\n}\n";
        let g = CCppExtractor.extract(&single_file("main.c", src));
        assert_eq!(g.fns_named("main").len(), 1);
        assert_eq!(g.fns_named("helper").len(), 1);
        let main_fn = g.node(g.fns_named("main")[0]).unwrap();
        assert_eq!(main_fn.entry_kind, Some(EntryPointKind::Main));
    }

    #[test]
    fn detects_call_edges() {
        let src = "void caller() {\n    callee();\n}\nvoid callee() {\n}\n";
        let g = CCppExtractor.extract(&single_file("a.c", src));
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn detects_gtest() {
        let src = "TEST(MathTest, Addition) {\n    EXPECT_EQ(1+1, 2);\n}\n\nTEST_F(FixtureTest, Works) {\n    ASSERT_TRUE(true);\n}\n";
        let g = CCppExtractor.extract(&single_file("test.cpp", src));
        let test1 = g
            .nodes
            .iter()
            .find(|n| n.name == "MathTest_Addition")
            .unwrap();
        assert_eq!(test1.entry_kind, Some(EntryPointKind::Test));
        let test2 = g
            .nodes
            .iter()
            .find(|n| n.name == "FixtureTest_Works")
            .unwrap();
        assert_eq!(test2.entry_kind, Some(EntryPointKind::Test));
    }

    #[test]
    fn detects_catch2() {
        let src = "TEST_CASE(\"vectors can be sized\") {\n    REQUIRE(v.size() == 5);\n}\n";
        let g = CCppExtractor.extract(&single_file("test.cpp", src));
        assert!(!g.nodes.is_empty());
        assert_eq!(g.nodes[0].entry_kind, Some(EntryPointKind::Test));
    }

    #[test]
    fn detects_boost() {
        let src = "BOOST_AUTO_TEST_CASE(my_test) {\n    BOOST_CHECK(true);\n}\n";
        let g = CCppExtractor.extract(&single_file("test.cpp", src));
        let test_fn = g.nodes.iter().find(|n| n.name == "my_test").unwrap();
        assert_eq!(test_fn.entry_kind, Some(EntryPointKind::Test));
    }

    #[test]
    fn detects_cpp_methods() {
        let src = "void MyClass::doWork() {\n    helper();\n}\nvoid helper() {\n}\n";
        let g = CCppExtractor.extract(&single_file("impl.cpp", src));
        assert_eq!(g.fns_named("MyClass::doWork").len(), 1);
    }

    #[test]
    fn cross_file_resolution() {
        let mut sources = HashMap::new();
        sources.insert(
            PathBuf::from("a.c"),
            "void caller() {\n    callee();\n}\n".to_string(),
        );
        sources.insert(PathBuf::from("b.c"), "void callee() {\n}\n".to_string());
        let g = CCppExtractor.extract(&sources);
        assert!(g.edge_count() >= 1);
    }

    #[test]
    fn empty_source() {
        let g = CCppExtractor.extract(&HashMap::new());
        assert_eq!(g.node_count(), 0);
    }
}
