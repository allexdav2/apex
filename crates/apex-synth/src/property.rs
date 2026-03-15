//! Coverage-guided property-based testing — infer properties from code patterns.

use serde::{Deserialize, Serialize};

/// Inferred property categories from source code patterns.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InferredProperty {
    /// f(f(x)) == f(x) — applying function twice gives same result.
    Idempotent { function: String },
    /// f(a, b) == f(b, a) — argument order doesn't matter.
    Commutative { function: String },
    /// f(x) monotonically increases/decreases with x.
    Monotonic { function: String, increasing: bool },
    /// Function never throws/panics for any valid input.
    NoException { function: String },
    /// len(f(x)) == len(x) — output preserves input length.
    LengthPreserving { function: String },
    /// f(encode(x)) == x — round-trip property.
    RoundTrip { encode: String, decode: String },
}

/// Pattern-based property inferrer (MVP: string-matching).
pub struct PropertyInferer;

/// Idempotent indicator prefixes.
const IDEMPOTENT_PREFIXES: &[&str] = &["sort", "normalize", "canonicalize", "deduplicate", "dedup"];

/// Commutative indicator prefixes.
const COMMUTATIVE_PREFIXES: &[&str] = &["add", "merge", "combine", "union", "sum"];

/// Length-preserving indicator prefixes.
const LENGTH_PRESERVING_PREFIXES: &[&str] = &["map", "transform"];

/// Round-trip pairs: (encode_prefix, decode_prefix).
const ROUNDTRIP_PAIRS: &[(&str, &str)] = &[
    ("encode", "decode"),
    ("serialize", "deserialize"),
    ("compress", "decompress"),
    ("encrypt", "decrypt"),
    ("to_json", "from_json"),
    ("to_string", "from_string"),
    ("to_bytes", "from_bytes"),
];

impl PropertyInferer {
    /// Infer properties from source code text.
    ///
    /// Looks for common patterns: encode/decode pairs, sort functions,
    /// serialization round-trips, etc.
    pub fn infer(source: &str) -> Vec<InferredProperty> {
        let mut props = Vec::new();
        let functions = Self::extract_function_names(source);

        // Check for round-trip pairs first (uses two functions).
        for &(enc_prefix, dec_prefix) in ROUNDTRIP_PAIRS {
            let enc_match = functions.iter().find(|f| f.starts_with(enc_prefix));
            let dec_match = functions.iter().find(|f| f.starts_with(dec_prefix));
            if let (Some(enc), Some(dec)) = (enc_match, dec_match) {
                props.push(InferredProperty::RoundTrip {
                    encode: enc.clone(),
                    decode: dec.clone(),
                });
            }
        }

        for func in &functions {
            // Idempotent check.
            if IDEMPOTENT_PREFIXES.iter().any(|p| func.starts_with(p)) {
                props.push(InferredProperty::Idempotent {
                    function: func.clone(),
                });
            }

            // Commutative check.
            if COMMUTATIVE_PREFIXES.iter().any(|p| func.starts_with(p)) {
                props.push(InferredProperty::Commutative {
                    function: func.clone(),
                });
            }

            // Length-preserving check.
            if LENGTH_PRESERVING_PREFIXES
                .iter()
                .any(|p| func.starts_with(p))
            {
                props.push(InferredProperty::LengthPreserving {
                    function: func.clone(),
                });
            }

            // NoException for any public function.
            if Self::is_public_function(source, func) {
                props.push(InferredProperty::NoException {
                    function: func.clone(),
                });
            }
        }

        props
    }

    /// Generate a hypothesis test for a given property.
    pub fn generate_hypothesis_test(prop: &InferredProperty, language: &str) -> String {
        match language {
            "python" | "py" => Self::generate_python_hypothesis(prop),
            _ => Self::generate_python_hypothesis(prop), // default to Python
        }
    }

    fn generate_python_hypothesis(prop: &InferredProperty) -> String {
        match prop {
            InferredProperty::Idempotent { function } => {
                format!(
                    "from hypothesis import given, strategies as st\n\n\
                     @given(x=st.text())\n\
                     def test_{function}_idempotent(x):\n    \
                         assert {function}({function}(x)) == {function}(x)\n"
                )
            }
            InferredProperty::Commutative { function } => {
                format!(
                    "from hypothesis import given, strategies as st\n\n\
                     @given(a=st.integers(), b=st.integers())\n\
                     def test_{function}_commutative(a, b):\n    \
                         assert {function}(a, b) == {function}(b, a)\n"
                )
            }
            InferredProperty::Monotonic {
                function,
                increasing,
            } => {
                let op = if *increasing { "<=" } else { ">=" };
                format!(
                    "from hypothesis import given, assume, strategies as st\n\n\
                     @given(a=st.integers(), b=st.integers())\n\
                     def test_{function}_monotonic(a, b):\n    \
                         assume(a <= b)\n    \
                         assert {function}(a) {op} {function}(b)\n"
                )
            }
            InferredProperty::NoException { function } => {
                format!(
                    "from hypothesis import given, strategies as st\n\n\
                     @given(x=st.text())\n\
                     def test_{function}_no_exception(x):\n    \
                         {function}(x)  # should not raise\n"
                )
            }
            InferredProperty::LengthPreserving { function } => {
                format!(
                    "from hypothesis import given, strategies as st\n\n\
                     @given(xs=st.lists(st.integers()))\n\
                     def test_{function}_length_preserving(xs):\n    \
                         assert len({function}(xs)) == len(xs)\n"
                )
            }
            InferredProperty::RoundTrip { encode, decode } => {
                format!(
                    "from hypothesis import given, strategies as st\n\n\
                     @given(x=st.text())\n\
                     def test_{encode}_{decode}_roundtrip(x):\n    \
                         assert {decode}({encode}(x)) == x\n"
                )
            }
        }
    }

    /// Extract function names from source code.
    /// Supports Python `def name(`, Rust `fn name(`, JS `function name(`.
    fn extract_function_names(source: &str) -> Vec<String> {
        let mut names = Vec::new();
        for line in source.lines() {
            let trimmed = line.trim();
            // Python: def func_name(
            if let Some(rest) = trimmed.strip_prefix("def ") {
                if let Some(name) = rest.split('(').next() {
                    let name = name.trim();
                    if !name.is_empty() {
                        names.push(name.to_string());
                    }
                }
            }
            // Rust: fn func_name( or pub fn func_name(
            // Skip comment lines and string literals to avoid false positives
            else if !trimmed.starts_with("//") && !trimmed.starts_with("/*") {
                let code_part = if let Some(comment_pos) = trimmed.find("//") {
                    &trimmed[..comment_pos]
                } else {
                    trimmed
                };
                if let Some(pos) = code_part.find("fn ") {
                    let before = &code_part[..pos];
                    let dquotes = before.chars().filter(|c| *c == '"').count();
                    if dquotes % 2 != 0 {
                        continue;
                    }
                    let valid_prefix = pos == 0 || code_part.as_bytes()[pos - 1] == b' ';
                    if valid_prefix {
                        let after_fn = &code_part[pos + 3..];
                        if let Some(name) = after_fn.split('(').next() {
                            let name = name.trim();
                            if !name.is_empty()
                                && name.chars().all(|c| c.is_alphanumeric() || c == '_')
                            {
                                names.push(name.to_string());
                            }
                        }
                    }
                }
            }
            // JS: function func_name(
            else if let Some(rest) = trimmed.strip_prefix("function ") {
                if let Some(name) = rest.split('(').next() {
                    let name = name.trim();
                    if !name.is_empty() {
                        names.push(name.to_string());
                    }
                }
            }
        }
        names
    }

    /// Check if a function is public in the source.
    fn is_public_function(source: &str, func_name: &str) -> bool {
        for line in source.lines() {
            let trimmed = line.trim();
            // Python: def at module level (no leading whitespace)
            if line.starts_with(&format!("def {func_name}(")) {
                return true;
            }
            // Rust: pub fn / pub async fn / pub const fn / pub unsafe fn etc.
            if trimmed.starts_with("pub ") && trimmed.contains(&format!("fn {func_name}(")) {
                return true;
            }
            // JS: export function or function at top level
            if trimmed.starts_with(&format!("function {func_name}("))
                || trimmed.contains(&format!("export function {func_name}("))
            {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_idempotent_from_sort() {
        let source = "def sort_items(xs):\n    return sorted(xs)\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::Idempotent {
            function: "sort_items".into(),
        }));
    }

    #[test]
    fn infer_roundtrip_from_encode_decode() {
        let source = "def encode(data):\n    pass\ndef decode(data):\n    pass\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::RoundTrip {
            encode: "encode".into(),
            decode: "decode".into(),
        }));
    }

    #[test]
    fn infer_commutative_from_merge() {
        let source = "def merge(a, b):\n    return a + b\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::Commutative {
            function: "merge".into(),
        }));
    }

    #[test]
    fn infer_no_exception_from_public_fn() {
        let source = "def process(data):\n    return data.strip()\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::NoException {
            function: "process".into(),
        }));
    }

    #[test]
    fn infer_length_preserving_from_map() {
        let source = "def map_items(xs):\n    return [x * 2 for x in xs]\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::LengthPreserving {
            function: "map_items".into(),
        }));
    }

    #[test]
    fn infer_empty_source_no_properties() {
        let props = PropertyInferer::infer("");
        assert!(props.is_empty());
    }

    #[test]
    fn infer_multiple_properties() {
        let source = "\
def sort_list(xs):
    return sorted(xs)
def encode(data):
    pass
def decode(data):
    pass
def merge(a, b):
    return a + b
";
        let props = PropertyInferer::infer(source);
        // Should have idempotent, roundtrip, commutative, and NoException entries
        assert!(props.len() >= 4);
        assert!(props
            .iter()
            .any(|p| matches!(p, InferredProperty::Idempotent { .. })));
        assert!(props
            .iter()
            .any(|p| matches!(p, InferredProperty::RoundTrip { .. })));
        assert!(props
            .iter()
            .any(|p| matches!(p, InferredProperty::Commutative { .. })));
    }

    #[test]
    fn generate_idempotent_test() {
        let prop = InferredProperty::Idempotent {
            function: "sort".into(),
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains("sort(sort(x)) == sort(x)"));
        assert!(test.contains("hypothesis"));
    }

    #[test]
    fn generate_roundtrip_test() {
        let prop = InferredProperty::RoundTrip {
            encode: "encode".into(),
            decode: "decode".into(),
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains("decode(encode(x)) == x"));
    }

    #[test]
    fn generate_commutative_test() {
        let prop = InferredProperty::Commutative {
            function: "add".into(),
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains("add(a, b) == add(b, a)"));
    }

    #[test]
    fn generate_no_exception_test() {
        let prop = InferredProperty::NoException {
            function: "process".into(),
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains("process(x)"));
        assert!(!test.contains("assert"));
    }

    #[test]
    fn infer_normalize_is_idempotent() {
        let source = "def normalize(text):\n    return text.lower().strip()\n";
        let props = PropertyInferer::infer(source);
        assert!(props.contains(&InferredProperty::Idempotent {
            function: "normalize".into(),
        }));
    }

    // ── Bug hunters ────────────────────────────────────────────────

    /// Verify filter is correctly NOT classified as length-preserving.
    #[test]
    fn filter_is_not_length_preserving() {
        let source = "def filter_items(xs):\n    return [x for x in xs if x > 0]\n";
        let props = PropertyInferer::infer(source);
        let has_length_preserving = props.contains(&InferredProperty::LengthPreserving {
            function: "filter_items".into(),
        });
        assert!(
            !has_length_preserving,
            "filter should not be length-preserving"
        );
    }

    /// Verify generated Python tests have correct indentation.
    /// Rust's `\` line continuation strips leading whitespace, so the
    /// output should have proper Python formatting.
    #[test]
    fn generated_idempotent_test_has_valid_indentation() {
        let prop = InferredProperty::Idempotent {
            function: "sort".into(),
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        let lines: Vec<&str> = test.lines().collect();
        // `from hypothesis` should have zero leading spaces
        assert!(
            !lines[0].starts_with(' '),
            "BUG: 'from hypothesis' line has leading whitespace: {:?}",
            lines[0]
        );
        // `@given` should have zero leading spaces
        let given_line = lines.iter().find(|l| l.contains("@given")).unwrap();
        assert!(
            !given_line.starts_with(' '),
            "BUG: '@given' decorator has leading whitespace: {:?}",
            given_line
        );
        // `def test_` should have zero leading spaces
        let def_line = lines.iter().find(|l| l.contains("def test_")).unwrap();
        assert!(
            !def_line.starts_with(' '),
            "BUG: 'def test_' has leading whitespace: {:?}",
            def_line
        );
        // `assert` should have exactly 4 leading spaces
        let assert_line = lines.iter().find(|l| l.contains("assert")).unwrap();
        assert!(
            assert_line.starts_with("    ") && !assert_line.starts_with("     "),
            "BUG: assert line should have exactly 4 spaces indent, got: {:?}",
            assert_line
        );
    }

    /// BUG: Rust `fn` extraction picks up function names from comments.
    /// `trimmed.find("fn ")` matches `// fn helper(` inside comments.
    #[test]
    fn bug_rust_fn_extracted_from_comments() {
        let source =
            "// fn ghost_function(x: i32) -> i32 { x }\npub fn real(x: i32) -> i32 { x }\n";
        let names = PropertyInferer::extract_function_names(source);
        assert!(
            !names.contains(&"ghost_function".to_string()),
            "BUG: extract_function_names picks up function names from comments. Got: {:?}",
            names
        );
    }

    /// BUG: Rust `fn` extraction picks up function names from string literals.
    #[test]
    fn bug_rust_fn_extracted_from_string_literal() {
        let source = "let s = \"fn fake_func(x)\";\nfn real_func(x: i32) {}\n";
        let names = PropertyInferer::extract_function_names(source);
        assert!(
            !names.contains(&"fake_func".to_string()),
            "BUG: extract_function_names picks up function names from string literals. Got: {:?}",
            names
        );
    }

    /// BUG: `is_public_function` treats indented Python methods as public.
    /// A method like `    def helper(self):` inside a class is trimmed to
    /// `def helper(self):` which matches as public.
    #[test]
    fn bug_indented_python_method_detected_as_public() {
        let source = "class Foo:\n    def helper(self):\n        pass\n";
        // `helper` should NOT be considered a public module-level function.
        let is_pub = PropertyInferer::is_public_function(source, "helper");
        assert!(
            !is_pub,
            "BUG: indented Python method 'helper' inside a class is detected as public. \
             is_public_function trims whitespace, erasing the indentation signal."
        );
    }

    /// Edge case: empty function body should still be extractable.
    #[test]
    fn extract_python_empty_function() {
        let source = "def noop():\n    pass\n";
        let names = PropertyInferer::extract_function_names(source);
        assert!(names.contains(&"noop".to_string()));
    }

    /// Edge case: source with no functions at all.
    #[test]
    fn extract_no_functions() {
        let source = "x = 42\ny = x + 1\n";
        let names = PropertyInferer::extract_function_names(source);
        assert!(names.is_empty());
    }

    /// Edge case: function name that is a prefix keyword match but not an
    /// actual prefix match — `sorting` starts with "sort".
    #[test]
    fn prefix_matching_is_greedy() {
        // `sorting_hat` starts_with("sort") is true, so it gets
        // classified as Idempotent. This test documents the behavior.
        let source = "def sorting_hat(x):\n    return x\n";
        let props = PropertyInferer::infer(source);
        let has_idempotent = props.contains(&InferredProperty::Idempotent {
            function: "sorting_hat".into(),
        });
        // This is arguably a false positive — "sorting_hat" is not a sort
        // function. Documenting current behavior: prefix match is greedy.
        assert!(
            has_idempotent,
            "Expected greedy prefix match: sorting_hat starts_with(\"sort\")"
        );
    }

    /// Monotonic property test generation for decreasing case.
    #[test]
    fn generate_monotonic_decreasing_test() {
        let prop = InferredProperty::Monotonic {
            function: "negate".into(),
            increasing: false,
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains(">="), "Decreasing monotonic should use >=");
        assert!(test.contains("negate(a)"));
    }

    /// Monotonic property test generation for increasing case.
    #[test]
    fn generate_monotonic_increasing_test() {
        let prop = InferredProperty::Monotonic {
            function: "double".into(),
            increasing: true,
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains("<="), "Increasing monotonic should use <=");
        assert!(test.contains("assume(a <= b)"));
    }

    /// generate_hypothesis_test with unknown language defaults to Python.
    #[test]
    fn generate_test_unknown_language_defaults_to_python() {
        let prop = InferredProperty::Idempotent {
            function: "sort".into(),
        };
        let py = PropertyInferer::generate_hypothesis_test(&prop, "python");
        let unknown = PropertyInferer::generate_hypothesis_test(&prop, "cobol");
        assert_eq!(
            py, unknown,
            "Unknown language should default to Python output"
        );
    }

    /// Rust: pub fn should be detected as public.
    #[test]
    fn rust_pub_fn_is_public() {
        let source = "pub fn compute(x: i32) -> i32 { x * 2 }\n";
        assert!(PropertyInferer::is_public_function(source, "compute"));
    }

    /// Rust: private fn should NOT be detected as public.
    #[test]
    fn rust_private_fn_is_not_public() {
        let source = "fn internal_helper(x: i32) -> i32 { x }\n";
        let is_pub = PropertyInferer::is_public_function(source, "internal_helper");
        // Rust `fn` without `pub` — not public in Rust semantics.
        // However, this also passes the Python check (starts_with("def ...")),
        // which it shouldn't since it's not Python at all.
        // Actually: "fn internal_helper(" doesn't start with "def ", so Python
        // check doesn't fire. And it doesn't contain "pub fn", so Rust check
        // doesn't fire. Good — should be false.
        assert!(!is_pub);
    }

    /// JS: export function should be detected as public.
    #[test]
    fn js_export_function_is_public() {
        let source = "export function calculate(x) { return x * 2; }\n";
        assert!(PropertyInferer::is_public_function(source, "calculate"));
    }

    /// Roundtrip detection should work for all ROUNDTRIP_PAIRS.
    #[test]
    fn all_roundtrip_pairs_detected() {
        for &(enc, dec) in ROUNDTRIP_PAIRS {
            let source = format!("def {enc}(data):\n    pass\ndef {dec}(data):\n    pass\n");
            let props = PropertyInferer::infer(&source);
            let has_rt = props
                .iter()
                .any(|p| matches!(p, InferredProperty::RoundTrip { .. }));
            assert!(has_rt, "Roundtrip not detected for pair ({enc}, {dec})");
        }
    }

    /// Only one encode function present (no matching decode) should NOT
    /// produce a RoundTrip property.
    #[test]
    fn no_roundtrip_without_matching_pair() {
        let source = "def encode(data):\n    pass\n";
        let props = PropertyInferer::infer(source);
        let has_rt = props
            .iter()
            .any(|p| matches!(p, InferredProperty::RoundTrip { .. }));
        assert!(!has_rt, "Should not infer RoundTrip with only encode");
    }

    /// LengthPreserving test generation produces valid Python.
    #[test]
    fn generate_length_preserving_test() {
        let prop = InferredProperty::LengthPreserving {
            function: "transform".into(),
        };
        let test = PropertyInferer::generate_hypothesis_test(&prop, "python");
        assert!(test.contains("len(transform(xs)) == len(xs)"));
        assert!(test.contains("st.lists"));
    }

    #[test]
    fn bug_fn_not_extracted_from_string_literal() {
        let source = "let s = \"fn fake_func(x)\";\nfn real_func() {}";
        let names = PropertyInferer::extract_function_names(source);
        assert!(
            !names.contains(&"fake_func".to_string()),
            "Should not extract fn from string literals"
        );
        assert!(names.contains(&"real_func".to_string()));
    }

    #[test]
    fn bug_fn_not_extracted_from_inline_comment() {
        let source = "let x = 1; // fn ghost_function(x) does stuff\nfn real_function() {}";
        let names = PropertyInferer::extract_function_names(source);
        assert!(
            !names.contains(&"ghost_function".to_string()),
            "Should not extract fn from inline comments"
        );
        assert!(names.contains(&"real_function".to_string()));
    }

    #[test]
    fn bug_indented_python_method_not_public() {
        let source = "class Foo:\n    def helper(self):\n        pass";
        assert!(
            !PropertyInferer::is_public_function(source, "helper"),
            "Indented Python methods should not be public"
        );
    }

    #[test]
    fn top_level_python_def_is_public() {
        let source = "def main():\n    pass";
        assert!(PropertyInferer::is_public_function(source, "main"));
    }
}
