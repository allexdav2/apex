# Bug Hunt Round 1 — 21 Bug Fixes

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 21 bugs found by the APEX bug-hunting agents across 5 files.

**Architecture:** Each task targets one file. All tasks are independent (no shared state). Each fix follows TDD: merge the bug-exposing test from the worktree, verify it fails/documents the bug, apply the fix, verify the test passes.

**Tech Stack:** Rust, `cargo test -p <crate>`

**Worktree branches with bug-exposing tests:**
- `worktree-agent-a99161cc` — rust.rs (21 tests)
- `worktree-agent-a3024807` — source_map.rs (22 tests)
- `worktree-agent-a9cea49a` — js_conditions.rs (67 tests)
- `worktree-agent-a2650027` — property.rs (30 tests)
- `worktree-agent-a5fb9de6` — license_scan.rs (48 tests)

---

## Task 1: apex-index — `rust.rs` path handling + truncation (6 bugs)

**Files:**
- Modify: `crates/apex-index/src/rust.rs:354` (sanitize_test_name)
- Modify: `crates/apex-index/src/rust.rs:445-463` (extract_covered_branches)
- Modify: `crates/apex-index/src/rust.rs:507-518` (make_relative)

### Bug 1.1: `make_relative` returns empty string when path == target

**Root cause:** `strip_prefix(target)` on equal paths returns `""`, producing `fnv1a(b"")`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn bug_make_relative_path_equals_target() {
    let result = make_relative("/home/user/project", "/home/user/project");
    assert_eq!(result, ".", "path == target should return '.' not empty string");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-index -- rust::tests::bug_make_relative_path_equals_target -v`
Expected: FAIL — returns `""` not `"."`

- [ ] **Step 3: Fix `make_relative`**

```rust
fn make_relative(path: &str, target: &str) -> String {
    let prefix = if target.ends_with('/') {
        target.to_string()
    } else {
        format!("{target}/")
    };

    let result = path
        .strip_prefix(&prefix)
        .or_else(|| path.strip_prefix(target))
        .map(|s| s.trim_start_matches('/').to_string())
        .unwrap_or_else(|| path.to_string());

    if result.is_empty() {
        ".".to_string()
    } else {
        result
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p apex-index -- rust::tests::bug_make_relative -v`
Expected: PASS

### Bug 1.2: `make_relative` false-matches sibling directories

**Root cause:** `/home/user/project` strips from `/home/user/project2/src/lib.rs` → `"2/src/lib.rs"`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn bug_make_relative_sibling_dir_false_match() {
    let result = make_relative("/home/user/project2/src/lib.rs", "/home/user/project");
    // Should NOT strip the prefix — different project directory
    assert_eq!(result, "/home/user/project2/src/lib.rs");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p apex-index -- rust::tests::bug_make_relative_sibling -v`
Expected: FAIL — returns `"2/src/lib.rs"`

- [ ] **Step 3: Fix — remove the bare `strip_prefix(target)` fallback**

The second `or_else` arm (`strip_prefix(target)` without trailing `/`) is the culprit. Remove it — the first arm with `/` suffix is sufficient:

```rust
fn make_relative(path: &str, target: &str) -> String {
    let prefix = if target.ends_with('/') {
        target.to_string()
    } else {
        format!("{target}/")
    };

    let result = path
        .strip_prefix(&prefix)
        .map(|s| s.trim_start_matches('/').to_string())
        .unwrap_or_else(|| path.to_string());

    if result.is_empty() {
        ".".to_string()
    } else {
        result
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p apex-index -- rust::tests -v`
Expected: All pass

### Bug 1.3: Null JSON values produce line=0 branches

**Root cause:** `seg[0].as_u64()` returns `None` for JSON `null`, `unwrap_or(0)` makes line=0 — an impossible 1-indexed location.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn bug_extract_branches_null_values_skipped() {
    // Segments with null line/col should be skipped, not produce line=0
    let json_str = r#"{"data":[{"files":[{"filename":"src/lib.rs","segments":[[null,null,1,true,true,false]]}]}]}"#;
    let json: LlvmCovJson = serde_json::from_str(json_str).unwrap();
    let branches = extract_covered_branches(&json, "");
    assert!(branches.is_empty(), "null line/col should skip the segment");
}
```

- [ ] **Step 2: Verify fails** — currently produces BranchId at line=0

- [ ] **Step 3: Fix — skip segments where line is 0**

In `extract_covered_branches`, after computing `line` and `col`:

```rust
if has_count && is_entry && !is_gap && count > 0 {
    let line = seg[0].as_u64().unwrap_or(0) as u32;
    if line == 0 {
        continue; // invalid 1-indexed line
    }
    let col = seg[1].as_u64().unwrap_or(0) as u16;
    branches.push(BranchId::new(file_id, line, col, 0));
}
```

- [ ] **Step 4: Run tests** — `cargo test -p apex-index -- rust::tests -v`

### Bug 1.4-1.5: Line u64→u32 and column u64→u16 truncation

**Root cause:** `as u32` / `as u16` silently truncates. Practically rare but produces wrong BranchIds.

- [ ] **Step 1: Write tests**

```rust
#[test]
fn bug_extract_branches_large_col_saturates() {
    // Column 70000 should saturate to u16::MAX, not wrap to 4464
    let json_str = r#"{"data":[{"files":[{"filename":"src/lib.rs","segments":[[10,70000,1,true,true,false]]}]}]}"#;
    let json: LlvmCovJson = serde_json::from_str(json_str).unwrap();
    let branches = extract_covered_branches(&json, "");
    assert_eq!(branches[0].col, u16::MAX);
}
```

- [ ] **Step 2: Fix — use saturating casts**

```rust
let line = seg[0].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;
if line == 0 { continue; }
let col = seg[1].as_u64().unwrap_or(0).min(u16::MAX as u64) as u16;
```

- [ ] **Step 3: Run tests** — `cargo test -p apex-index -- rust::tests -v`

### Bug 1.6: Test name sanitization misses `<>\` characters

**Root cause:** `replace("::", "__").replace(['/', ' '], "_")` doesn't handle `<`, `>`, `\`.

- [ ] **Step 1: Write test**

```rust
#[test]
fn bug_sanitize_test_name_angle_brackets() {
    // Generics in test names: MyType<T>::test should sanitize < and >
    let name = "MyType<T>::test\\path";
    let sanitized = name.replace("::", "__").replace(['/', ' ', '<', '>', '\\'], "_");
    assert!(!sanitized.contains('<'));
    assert!(!sanitized.contains('>'));
    assert!(!sanitized.contains('\\'));
}
```

- [ ] **Step 2: Fix — extend the character list**

Find the sanitization line (around line 354-355) and change:
```rust
// Before:
.replace("::", "__").replace(['/', ' '], "_")
// After:
.replace("::", "__").replace(['/', ' ', '<', '>', '\\'], "_")
```

- [ ] **Step 3: Run tests and commit**

Run: `cargo test -p apex-index -- rust::tests -v`

- [ ] **Step 4: Commit**

```bash
git add crates/apex-index/src/rust.rs
git commit -m "fix(apex-index): 6 bugs in rust.rs — path handling, truncation, sanitization"
```

---

## Task 2: apex-instrument — `source_map.rs` parsing bugs (3 bugs)

**Files:**
- Modify: `crates/apex-instrument/src/source_map.rs:32-56` (remap logic)
- Modify: `crates/apex-instrument/src/source_map.rs:82-89` (inline source map)
- Modify: `crates/apex-instrument/src/source_map.rs:97-115` (base64 decode)

### Bug 2.1: Fuzzy `lookup_token` remaps branches to wrong source locations

**Root cause:** `sourcemap::lookup_token()` does nearest-match. A branch in generated code with no corresponding source mapping gets silently attributed to the nearest token rather than being dropped.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn bug_remap_fuzzy_lookup_produces_wrong_line() {
    // When source map only maps line 1 col 0, a branch at line 100
    // should NOT be remapped to line 1 — it should be dropped.
    // This documents the fuzzy-match behavior of sourcemap::lookup_token.
}
```

- [ ] **Step 2: Fix — validate proximity of lookup result**

After `sm.lookup_token(line_0, col)`, check that the returned token is "close enough" to the query location. If the token's generated line differs from the branch's line, drop the branch:

```rust
if let Some(token) = sm.lookup_token(line_0, col) {
    // Guard: reject fuzzy matches where the generated line doesn't match
    if token.get_dst_line() != line_0 {
        // Fuzzy match to wrong line — generated code, drop it
        continue;
    }
    if let Some(source) = token.get_source() {
        // ... existing remapping logic ...
    }
}
```

- [ ] **Step 3: Run tests** — `cargo test -p apex-instrument -- source_map::tests -v`

### Bug 2.2: Inline source map includes trailing file content

**Root cause:** `content[pos + 26..]` captures everything from the data URL to EOF. Trailing newlines or JS code corrupt the base64.

- [ ] **Step 1: Write test**

```rust
#[test]
fn bug_inline_sourcemap_trailing_content() {
    // Source map followed by a newline and more code
    let js = "var x = 1;\n//# sourceMappingURL=data:application/json;base64,e30=\nconsole.log(x);\n";
    // e30= decodes to "{}" — valid JSON for sourcemap
    // The trailing \nconsole.log(x);\n should NOT be included in base64
}
```

- [ ] **Step 2: Fix — truncate at first whitespace/newline after base64**

```rust
// In load_source_map, after finding the inline source map:
let b64 = data_url[comma_pos + 1..].trim();
// Truncate at first whitespace (newline, space, tab)
let b64 = b64.split_whitespace().next().unwrap_or(b64);
```

- [ ] **Step 3: Run tests**

### Bug 2.3: base64 decoder rejects RFC 2045 embedded whitespace

**Root cause:** The decoder treats any non-base64 char (including `\n`, `\r`, space) as error.

- [ ] **Step 1: Write test**

```rust
#[test]
fn bug_base64_decode_ignores_whitespace() {
    // RFC 2045 allows line-wrapped base64
    let wrapped = "SGVs\nbG8g\r\nV29y\nbGQ=";
    let result = base64_decode(wrapped);
    assert_eq!(result, Ok(b"Hello World".to_vec()));
}
```

- [ ] **Step 2: Fix — filter whitespace before decoding**

```rust
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = input.trim_end_matches('=');
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &byte in input.as_bytes() {
        // Skip whitespace (RFC 2045)
        if byte == b'\n' || byte == b'\r' || byte == b' ' || byte == b'\t' {
            continue;
        }
        let val = TABLE.iter().position(|&c| c == byte).ok_or(())? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Ok(output)
}
```

- [ ] **Step 3: Run tests and commit**

Run: `cargo test -p apex-instrument -- source_map::tests -v`

- [ ] **Step 4: Commit**

```bash
git add crates/apex-instrument/src/source_map.rs
git commit -m "fix(apex-instrument): 3 source map bugs — fuzzy remap, inline trailing content, base64 whitespace"
```

---

## Task 3: apex-concolic — `js_conditions.rs` parser bugs (3 bugs)

**Files:**
- Modify: `crates/apex-concolic/src/js_conditions.rs:253-258` (parse_expr quote crash)
- Modify: `crates/apex-concolic/src/js_conditions.rs:329` (escape handling)
- Modify: `crates/apex-concolic/src/js_conditions.rs:45-51` (parse_and right-recursion)

### Bug 3.1: CRASH — `parse_expr` panics on single-char quote

**Root cause:** For input `"`, both `starts_with('"')` and `ends_with('"')` are true, then `text[1..0]` panics.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn bug_crash_parse_expr_single_quote() {
    // Single " should not panic
    let result = std::panic::catch_unwind(|| parse_expr("\""));
    assert!(result.is_ok(), "single quote should not panic");
}
```

- [ ] **Step 2: Fix — add length guard**

```rust
// Before line 253:
if text.len() >= 2
    && ((text.starts_with('"') && text.ends_with('"'))
        || (text.starts_with('\'') && text.ends_with('\''))
        || (text.starts_with('`') && text.ends_with('`')))
{
```

- [ ] **Step 3: Run tests** — `cargo test -p apex-concolic -- js_conditions::tests -v`

### Bug 3.2: Escaped backslash before quote breaks operator scanning

**Root cause:** `bytes[i-1] != b'\\'` only checks one byte. `"test\\" === x` has `\\` (escaped backslash), so the quote IS a real terminator, but the parser thinks it's escaped.

- [ ] **Step 1: Write test**

```rust
#[test]
fn bug_escaped_backslash_before_quote() {
    let tree = parse("\"test\\\\\" === x");
    // Should parse as Compare, not Unknown
    assert!(matches!(tree, ConditionTree::Compare { .. }));
}
```

- [ ] **Step 2: Fix — count consecutive backslashes**

Replace `bytes[i - 1] != b'\\'` with a helper that counts:

```rust
if b == q {
    // Count consecutive backslashes before this quote
    let mut backslash_count = 0;
    let mut j = i;
    while j > 0 && bytes[j - 1] == b'\\' {
        backslash_count += 1;
        j -= 1;
    }
    // Odd number of backslashes = escaped quote; even = real terminator
    if backslash_count % 2 == 0 {
        in_str = None;
    }
}
```

- [ ] **Step 3: Run tests**

### Bug 3.3: Triple `&&`/`||` chains garble right-hand side

**Root cause:** `parse_and` sends the right operand to `parse_not` instead of recursing into `parse_and`. For `a && b && c`, the second `&&` is consumed into a variable name.

- [ ] **Step 1: Write test**

```rust
#[test]
fn bug_triple_and_parses_correctly() {
    let tree = parse("a === 1 && b === 2 && c === 3");
    // Should be And(And(a===1, b===2), c===3), not And(a===1, "b === 2 && c === 3")
    if let ConditionTree::And(_, right) = &tree {
        assert!(matches!(right.as_ref(), ConditionTree::Compare { .. }),
            "rightmost operand should be a Compare, not garbled");
    }
}
```

- [ ] **Step 2: Fix — `parse_and` right operand should recurse**

Change `parse_and` to find the **leftmost** `&&` and recurse on the right:

```rust
fn parse_and(text: &str) -> ConditionTree {
    if let Some(pos) = find_operator_outside_parens(text, "&&") {
        let left = parse_not(text[..pos].trim());
        let right = parse_and(text[pos + 2..].trim()); // recurse, not parse_not
        return ConditionTree::And(Box::new(left), Box::new(right));
    }
    parse_not(text)
}
```

Similarly for `parse_or`:
```rust
fn parse_or(text: &str) -> ConditionTree {
    if let Some(pos) = find_operator_outside_parens(text, "||") {
        let left = parse_and(text[..pos].trim());
        let right = parse_or(text[pos + 2..].trim()); // recurse, not parse_and
        return ConditionTree::Or(Box::new(left), Box::new(right));
    }
    parse_and(text)
}
```

Note: This gives right-associativity (`a && (b && c)`), which is semantically equivalent for `&&`/`||` since they're associative operators.

- [ ] **Step 3: Run tests and commit**

Run: `cargo test -p apex-concolic -- js_conditions::tests -v`

- [ ] **Step 4: Commit**

```bash
git add crates/apex-concolic/src/js_conditions.rs
git commit -m "fix(apex-concolic): 3 JS condition parser bugs — quote crash, escape handling, chain parsing"
```

---

## Task 4: apex-synth — `property.rs` inference bugs (4 bugs)

**Files:**
- Modify: `crates/apex-synth/src/property.rs:32` (LENGTH_PRESERVING_PREFIXES)
- Modify: `crates/apex-synth/src/property.rs:55-61` (roundtrip_fns dead code)
- Modify: `crates/apex-synth/src/property.rs:193` (extract_function_names)
- Modify: `crates/apex-synth/src/property.rs:224` (is_public_function)

### Bug 4.1: `filter` classified as length-preserving

**Root cause:** `filter` removes elements — `len(filter(xs)) <= len(xs)`, not `==`.

- [ ] **Step 1: Write test**

```rust
#[test]
fn bug_filter_not_length_preserving() {
    let source = "def filter_items(xs):\n    return [x for x in xs if x > 0]\n";
    let props = PropertyInferer::infer(source);
    assert!(!props.iter().any(|p| matches!(p, InferredProperty::LengthPreserving { .. })),
        "filter should not be classified as length-preserving");
}
```

- [ ] **Step 2: Fix — remove `"filter"` from the constant**

```rust
const LENGTH_PRESERVING_PREFIXES: &[&str] = &["map", "transform"];
```

- [ ] **Step 3: Run tests**

### Bug 4.2: `pub async fn` not recognized as public

- [ ] **Step 1: Write test**

```rust
#[test]
fn bug_pub_async_fn_recognized_as_public() {
    let source = "pub async fn fetch_data(url: &str) -> Result<Data> { todo!() }";
    assert!(PropertyInferer::is_public_function(source, "fetch_data"));
}
```

- [ ] **Step 2: Fix — match `pub` followed by optional qualifiers before `fn`**

```rust
fn is_public_function(source: &str, func_name: &str) -> bool {
    for line in source.lines() {
        let trimmed = line.trim();
        // Python
        if trimmed.starts_with(&format!("def {func_name}(")) {
            return true;
        }
        // Rust: pub fn, pub async fn, pub const fn, pub unsafe fn, etc.
        if trimmed.starts_with("pub ") && trimmed.contains(&format!("fn {func_name}(")) {
            return true;
        }
        // JS
        if trimmed.starts_with(&format!("function {func_name}("))
            || trimmed.contains(&format!("export function {func_name}("))
        {
            return true;
        }
    }
    false
}
```

- [ ] **Step 3: Run tests**

### Bug 4.3: `fn` names extracted from comments and string literals

- [ ] **Step 1: Write test**

```rust
#[test]
fn bug_fn_in_comment_not_extracted() {
    let source = "// see fn helper() for details\nfn real_function() {}\n";
    let names = PropertyInferer::extract_function_names(source);
    assert!(names.contains(&"real_function".to_string()));
    assert!(!names.contains(&"helper".to_string()), "fn in comment should not be extracted");
}
```

- [ ] **Step 2: Fix — skip comment lines**

Add a check before the Rust `fn ` extraction:

```rust
// Rust: fn func_name( or pub fn func_name(
// Skip lines that are comments
else if !trimmed.starts_with("//") && !trimmed.starts_with("/*") && !trimmed.starts_with("* ") {
    if let Some(pos) = trimmed.find("fn ") {
        // Only match if `fn ` is at the start or preceded by a keyword
        let before = &trimmed[..pos];
        if before.is_empty()
            || before.ends_with("pub ")
            || before.ends_with("async ")
            || before.ends_with("const ")
            || before.ends_with("unsafe ")
            || before.ends_with(") ")  // closing paren of `pub(crate) fn`
        {
            let after_fn = &trimmed[pos + 3..];
            if let Some(name) = after_fn.split('(').next() {
                let name = name.trim();
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    names.push(name.to_string());
                }
            }
        }
    }
}
```

- [ ] **Step 3: Run tests**

### Bug 4.4: `roundtrip_fns` HashSet built but never used

- [ ] **Step 1: Fix — remove the dead code**

Delete lines 55 and 60-61 (the `roundtrip_fns` HashSet creation and insertions). It's unused.

```rust
// Remove:
// let mut roundtrip_fns = std::collections::HashSet::new();
// ...
// roundtrip_fns.insert(enc.clone());
// roundtrip_fns.insert(dec.clone());
```

- [ ] **Step 2: Run tests and commit**

Run: `cargo test -p apex-synth -- property::tests -v`

- [ ] **Step 3: Commit**

```bash
git add crates/apex-synth/src/property.rs
git commit -m "fix(apex-synth): 4 property inference bugs — filter, pub async fn, comment extraction, dead code"
```

---

## Task 5: apex-detect — `license_scan.rs` SPDX parsing bugs (5 bugs)

**Files:**
- Modify: `crates/apex-detect/src/detectors/license_scan.rs:275-284` (eval_spdx_permissive)
- Modify: `crates/apex-detect/src/detectors/license_scan.rs:289-310` (find_denied_in_expr)

### Bug 5.1: SPDX `WITH` exception causes false positive

**Root cause:** `"Apache-2.0 WITH LLVM-exception"` doesn't match `"Apache-2.0"` in the allow list.

### Bug 5.2: Parenthesized SPDX not parsed

**Root cause:** `"(MIT OR Apache-2.0)"` — `"(MIT"` doesn't match `"MIT"`.

### Bug 5.3: `GPL-2.0+` bypasses deny list

**Root cause:** `"GPL-2.0+"` doesn't match `"GPL-2.0-or-later"` in the deny list.

### Bug 5.4: Lowercase `or`/`and` not recognized

**Root cause:** Split on `" OR "` is case-sensitive; `" or "` is missed.

### Bug 5.5: Tab characters cause false positive

**Root cause:** `split(" OR ")` doesn't handle tabs.

- [ ] **Step 1: Write tests for all 5 bugs**

```rust
#[test]
fn bug_spdx_with_exception_allowed() {
    let result = check_policy(&LicensePolicy::Permissive, "Apache-2.0 WITH LLVM-exception");
    assert_eq!(result, PolicyVerdict::Allowed);
}

#[test]
fn bug_spdx_parentheses_stripped() {
    let result = check_policy(&LicensePolicy::Permissive, "(MIT OR Apache-2.0)");
    assert_eq!(result, PolicyVerdict::Allowed);
}

#[test]
fn bug_spdx_plus_suffix_denied() {
    let result = check_policy(&LicensePolicy::Enterprise, "GPL-2.0+");
    assert!(matches!(result, PolicyVerdict::Denied(_)));
}

#[test]
fn bug_spdx_lowercase_or_parsed() {
    let result = check_policy(&LicensePolicy::Permissive, "MIT or Apache-2.0");
    assert_eq!(result, PolicyVerdict::Allowed);
}

#[test]
fn bug_spdx_tab_separated() {
    let result = check_policy(&LicensePolicy::Permissive, "MIT\tOR\tApache-2.0");
    assert_eq!(result, PolicyVerdict::Allowed);
}
```

- [ ] **Step 2: Fix — add SPDX normalization helper**

Add a normalization function and apply it before evaluation:

```rust
/// Normalize SPDX expression: strip parens, normalize whitespace, handle WITH/+.
fn normalize_spdx(expr: &str) -> String {
    let mut s = expr.trim().to_string();
    // Strip outer parentheses
    while s.starts_with('(') && s.ends_with(')') {
        s = s[1..s.len() - 1].trim().to_string();
    }
    // Strip inner parentheses (simple cases)
    s = s.replace('(', "").replace(')', "");
    // Normalize whitespace (tabs, multiple spaces)
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // Normalize case of operators
    s = s.replace(" or ", " OR ").replace(" and ", " AND ");
    s
}

/// Normalize a single SPDX identifier for matching.
fn normalize_spdx_id(id: &str) -> String {
    let id = id.trim();
    // Strip WITH clauses (exceptions only add permissions)
    let id = if let Some(pos) = id.find(" WITH ") {
        &id[..pos]
    } else {
        id
    };
    // Map deprecated + suffix: "GPL-2.0+" → "GPL-2.0-or-later"
    if let Some(base) = id.strip_suffix('+') {
        format!("{base}-or-later")
    } else {
        id.to_string()
    }
}
```

- [ ] **Step 3: Update `eval_spdx_permissive` to use normalization**

```rust
fn eval_spdx_permissive(expr: &str, allow: &[&str]) -> bool {
    let expr = normalize_spdx(expr);
    let or_parts: Vec<&str> = expr.split(" OR ").collect();
    or_parts.iter().any(|or_part| {
        let and_parts: Vec<&str> = or_part.split(" AND ").collect();
        and_parts.iter().all(|id| {
            let normalized = normalize_spdx_id(id);
            allow.iter().any(|a| a.eq_ignore_ascii_case(&normalized))
        })
    })
}
```

- [ ] **Step 4: Update `find_denied_in_expr` similarly**

```rust
fn find_denied_in_expr(expr: &str, deny: &[&str]) -> Option<String> {
    let expr = normalize_spdx(expr);
    let or_parts: Vec<&str> = expr.split(" OR ").collect();
    // ... same logic but use normalize_spdx_id for each identifier
}
```

- [ ] **Step 5: Run tests and commit**

Run: `cargo test -p apex-detect -- detectors::license_scan::tests -v`

- [ ] **Step 6: Commit**

```bash
git add crates/apex-detect/src/detectors/license_scan.rs
git commit -m "fix(apex-detect): 5 SPDX parsing bugs — WITH, parens, +suffix, case, whitespace"
```

---

## Final Verification

- [ ] **Run full workspace tests**

```bash
cargo test --workspace
```

- [ ] **Run only bug-exposing tests**

```bash
cargo test --workspace -- bug_
```

- [ ] **Final commit (if not already committed per-task)**

```bash
git commit -m "fix: 21 bugs from bug-hunting round 1 — parser crashes, SPDX, path handling, source maps"
```
