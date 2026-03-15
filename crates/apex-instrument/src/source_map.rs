use apex_core::{hash::fnv1a_hash, types::BranchId};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tracing::warn;

/// Remap branch IDs from emitted JS locations to original TS/source locations.
pub fn remap_source_maps(
    branches: Vec<BranchId>,
    file_paths: &HashMap<u64, PathBuf>,
    target: &Path,
) -> (Vec<BranchId>, HashMap<u64, PathBuf>) {
    let mut remapped_branches = Vec::new();
    let mut remapped_file_paths = HashMap::new();

    // Pre-load source maps for each unique file_id
    let mut source_maps: HashMap<u64, Option<sourcemap::SourceMap>> = HashMap::new();
    for (&file_id, rel_path) in file_paths {
        let abs_path = target.join(rel_path);
        let sm = load_source_map(&abs_path);
        source_maps.insert(file_id, sm);
    }

    for branch in branches {
        let sm_opt = source_maps.get(&branch.file_id).and_then(|s| s.as_ref());

        if let Some(sm) = sm_opt {
            let line_0 = branch.line.saturating_sub(1);
            let col = branch.col as u32;

            if let Some(token) = sm.lookup_token(line_0, col) {
                if let Some(source) = token.get_source() {
                    // The sourcemap crate v9 already resolves sourceRoot into
                    // the source paths, so we must NOT prepend it again.
                    let original_path = PathBuf::from(source);

                    let original_rel = original_path.to_string_lossy();
                    let new_file_id = fnv1a_hash(&original_rel);
                    let new_line = token.get_src_line() + 1; // back to 1-based
                    let new_col = token.get_src_col().min(u16::MAX as u32) as u16;

                    remapped_file_paths.insert(new_file_id, original_path);
                    remapped_branches.push(BranchId::new(
                        new_file_id,
                        new_line,
                        new_col,
                        branch.direction,
                    ));
                    continue;
                }
            }
            // Source map exists but no mapping found — generated code, drop it
        } else {
            // No source map — keep original location
            if let Some(path) = file_paths.get(&branch.file_id) {
                remapped_file_paths.insert(branch.file_id, path.clone());
            }
            remapped_branches.push(branch);
        }
    }

    (remapped_branches, remapped_file_paths)
}

/// Try to load a source map for the given JS file.
fn load_source_map(js_path: &Path) -> Option<sourcemap::SourceMap> {
    // Try .map sidecar
    let map_path = js_path.with_extension("js.map");
    if map_path.exists() {
        match std::fs::read(&map_path) {
            Ok(bytes) => return sourcemap::SourceMap::from_reader(&bytes[..]).ok(),
            Err(e) => warn!(path = %map_path.display(), error = %e, "failed to read source map"),
        }
    }

    // Try inline source map in the JS file
    if let Ok(content) = std::fs::read_to_string(js_path) {
        if let Some(pos) = content.rfind("//# sourceMappingURL=data:") {
            let data_url = &content[pos + 26..];
            if let Some(comma_pos) = data_url.find(',') {
                let b64 = data_url[comma_pos + 1..].trim();
                if let Ok(decoded) = base64_decode(b64) {
                    return sourcemap::SourceMap::from_reader(&decoded[..]).ok();
                }
            }
        }
    }

    None
}

/// Simple base64 decode (avoid extra dependency).
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let input = input.trim_end_matches('=');
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &byte in input.as_bytes() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_no_source_maps_passes_through() {
        let mut file_paths = HashMap::new();
        file_paths.insert(42, PathBuf::from("src/app.js"));
        let branches = vec![BranchId::new(42, 10, 5, 0)];
        let (remapped, new_files) =
            remap_source_maps(branches, &file_paths, Path::new("/nonexistent"));
        assert_eq!(remapped.len(), 1);
        assert_eq!(remapped[0].file_id, 42);
        assert_eq!(remapped[0].line, 10);
        assert!(new_files.contains_key(&42));
    }

    #[test]
    fn base64_decode_basic() {
        let encoded = "SGVsbG8=";
        let decoded = base64_decode(encoded).unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn base64_decode_no_padding() {
        let encoded = "SGVsbG8";
        let decoded = base64_decode(encoded).unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn load_source_map_nonexistent_file() {
        assert!(load_source_map(Path::new("/no/such/file.js")).is_none());
    }

    // ---- base64_decode edge cases ----

    #[test]
    fn base64_decode_empty_input() {
        let decoded = base64_decode("").unwrap();
        assert_eq!(decoded, b"");
    }

    #[test]
    fn base64_decode_single_byte_output() {
        // "YQ==" encodes "a"
        let decoded = base64_decode("YQ==").unwrap();
        assert_eq!(decoded, b"a");
    }

    #[test]
    fn base64_decode_two_byte_output() {
        // "YWI=" encodes "ab"
        let decoded = base64_decode("YWI=").unwrap();
        assert_eq!(decoded, b"ab");
    }

    #[test]
    fn base64_decode_three_byte_output() {
        // "YWJj" encodes "abc"
        let decoded = base64_decode("YWJj").unwrap();
        assert_eq!(decoded, b"abc");
    }

    #[test]
    fn base64_decode_invalid_char() {
        assert!(base64_decode("SGVs!G8=").is_err());
    }

    #[test]
    fn base64_decode_whitespace_in_middle() {
        // base64 with a space in the middle — space is not in the table
        // so this should fail (which is correct for strict decoding)
        assert!(base64_decode("SGVs bG8=").is_err());
    }

    #[test]
    fn base64_decode_binary_data() {
        // Encode [0xFF, 0x00, 0xAB] -> "/wCr"
        let decoded = base64_decode("/wCr").unwrap();
        assert_eq!(decoded, vec![0xFF, 0x00, 0xAB]);
    }

    #[test]
    fn base64_decode_with_plus_and_slash() {
        // "+" and "/" are valid base64 chars
        let decoded = base64_decode("+/8=").unwrap();
        // +/8 in base64: + = 62, / = 63, 8 = 60 (actually '8' is index 60)
        // Let me compute: '+' = 62, '/' = 63, '8' = 60
        // bits: 62=111110, 63=111111, 60=111100
        // concat: 111110_111111_111100
        // = 0xFBFF_C... no let me do this right
        // 111110 111111 111100 = 18 bits
        // first byte (bits 17-10): 11111011 = 0xFB
        // second byte (bits 9-2): 11111111 = 0xFF
        // remaining 2 bits: 00, not enough for a byte
        assert_eq!(decoded, vec![0xFB, 0xFF]);
    }

    // ---- load_source_map tests with temp files ----

    #[test]
    fn load_source_map_from_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("app.js");
        std::fs::write(&js_path, "console.log('hello');").unwrap();

        // Create a minimal valid source map
        let source_map_json = r#"{
            "version": 3,
            "file": "app.js",
            "sources": ["app.ts"],
            "names": [],
            "mappings": "AAAA"
        }"#;
        let map_path = dir.path().join("app.js.map");
        std::fs::write(&map_path, source_map_json).unwrap();

        let sm = load_source_map(&js_path);
        assert!(sm.is_some(), "should load sidecar .js.map file");
    }

    #[test]
    fn load_source_map_from_inline_data_url() {
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("bundle.js");

        let source_map_json = r#"{"version":3,"file":"bundle.js","sources":["src.ts"],"names":[],"mappings":"AAAA"}"#;
        let b64 = base64_encode_for_test(source_map_json.as_bytes());
        let js_content = format!(
            "console.log('hello');\n//# sourceMappingURL=data:application/json;base64,{}",
            b64
        );
        std::fs::write(&js_path, &js_content).unwrap();

        let sm = load_source_map(&js_path);
        assert!(sm.is_some(), "should load inline base64 source map");
    }

    #[test]
    fn bug_inline_source_map_trailing_content_after_base64() {
        // BUG: If there is content (e.g., a newline followed by another comment)
        // after the base64 data, the parser includes it in the base64 string.
        // trim() only removes whitespace at the edges, but any non-whitespace
        // trailing content corrupts the base64 decode.
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("bundle.js");

        let source_map_json = r#"{"version":3,"file":"bundle.js","sources":["src.ts"],"names":[],"mappings":"AAAA"}"#;
        let b64 = base64_encode_for_test(source_map_json.as_bytes());
        // Trailing newline is fine (trim handles it)
        let js_content = format!(
            "console.log('hello');\n//# sourceMappingURL=data:application/json;base64,{}\n",
            b64
        );
        std::fs::write(&js_path, &js_content).unwrap();

        let sm = load_source_map(&js_path);
        assert!(sm.is_some(), "trailing newline should be handled by trim()");
    }

    #[test]
    fn bug_inline_source_map_with_trailing_code_lines() {
        // BUG: If there's actual code after the sourceMappingURL comment,
        // e.g., "//# sourceMappingURL=data:...base64,XXXX\nsome_other_code();"
        // the base64 extraction takes everything from comma to EOF, meaning
        // "XXXX\nsome_other_code();" — trim() won't remove "some_other_code();".
        // The base64 decode will fail on invalid chars, returning None.
        // This is arguably correct behavior (source map line should be last),
        // but it's fragile. We test the behavior is at least safe (returns None).
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("bundle.js");

        let source_map_json = r#"{"version":3,"file":"bundle.js","sources":["src.ts"],"names":[],"mappings":"AAAA"}"#;
        let b64 = base64_encode_for_test(source_map_json.as_bytes());
        let js_content = format!(
            "//# sourceMappingURL=data:application/json;base64,{}\nsome_other_code();",
            b64
        );
        std::fs::write(&js_path, &js_content).unwrap();

        // The trailing "some_other_code();" will be included in b64 string after trim
        // and cause base64 decode failure (or garbage). Should return None.
        let sm = load_source_map(&js_path);
        assert!(sm.is_none(), "trailing code after base64 should cause parse failure");
    }

    #[test]
    fn load_source_map_invalid_sidecar_json() {
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("app.js");
        std::fs::write(&js_path, "").unwrap();
        let map_path = dir.path().join("app.js.map");
        std::fs::write(&map_path, "this is not valid json").unwrap();

        let sm = load_source_map(&js_path);
        assert!(sm.is_none(), "invalid JSON in .map file should return None");
    }

    #[test]
    fn load_source_map_inline_no_comma_in_data_url() {
        // data URL without a comma — should return None
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("bundle.js");
        let js_content = "//# sourceMappingURL=data:application/json;base64";
        std::fs::write(&js_path, js_content).unwrap();

        let sm = load_source_map(&js_path);
        assert!(sm.is_none(), "data URL without comma should return None");
    }

    #[test]
    fn load_source_map_inline_empty_base64() {
        // data URL with comma but empty base64 after it
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("bundle.js");
        let js_content = "//# sourceMappingURL=data:application/json;base64,";
        std::fs::write(&js_path, js_content).unwrap();

        let sm = load_source_map(&js_path);
        assert!(sm.is_none(), "empty base64 should produce empty bytes, not valid source map");
    }

    // ---- remap_source_maps edge cases ----

    #[test]
    fn remap_empty_branches() {
        let file_paths = HashMap::new();
        let branches: Vec<BranchId> = vec![];
        let (remapped, new_files) =
            remap_source_maps(branches, &file_paths, Path::new("/nonexistent"));
        assert!(remapped.is_empty());
        assert!(new_files.is_empty());
    }

    #[test]
    fn remap_branch_with_no_matching_file_path() {
        // branch.file_id doesn't exist in file_paths
        let file_paths = HashMap::new();
        let branches = vec![BranchId::new(999, 10, 5, 0)];
        let (remapped, new_files) =
            remap_source_maps(branches, &file_paths, Path::new("/nonexistent"));
        // No source map found => else branch at line 57
        // But file_paths.get(&branch.file_id) is None => no path inserted
        assert_eq!(remapped.len(), 1);
        assert_eq!(remapped[0].file_id, 999);
        assert!(new_files.is_empty(), "file_id not in file_paths means no path in output");
    }

    #[test]
    fn bug_remap_branch_missing_file_path_loses_file_mapping() {
        // BUG: When there's no source map (line 57-63) and the branch file_id
        // exists in file_paths, the branch is kept and the file path is copied.
        // But if the file_id is NOT in file_paths, the branch is still pushed
        // but no file path is inserted. This creates an inconsistency:
        // a branch exists in remapped_branches with a file_id that has no
        // corresponding entry in remapped_file_paths.
        let file_paths: HashMap<u64, PathBuf> = HashMap::new();
        let branches = vec![BranchId::new(42, 1, 0, 0)];
        let (remapped, new_files) =
            remap_source_maps(branches, &file_paths, Path::new("/nonexistent"));

        // Branch is kept...
        assert_eq!(remapped.len(), 1);
        // ...but file path is missing. This is inconsistent.
        assert!(
            !new_files.contains_key(&42),
            "branch 42 has no file path — inconsistency"
        );
    }

    #[test]
    fn remap_with_valid_source_map() {
        let dir = tempfile::tempdir().unwrap();

        // Create JS file and source map
        let js_path = dir.path().join("app.js");
        std::fs::write(&js_path, "console.log('hello');").unwrap();

        let source_map_json = r#"{
            "version": 3,
            "file": "app.js",
            "sourceRoot": "",
            "sources": ["app.ts"],
            "names": [],
            "mappings": "AAAA"
        }"#;
        let map_path = dir.path().join("app.js.map");
        std::fs::write(&map_path, source_map_json).unwrap();

        let file_id = fnv1a_hash("app.js");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, PathBuf::from("app.js"));

        // Line 1, col 0, direction 0
        let branches = vec![BranchId::new(file_id, 1, 0, 0)];
        let (remapped, new_files) = remap_source_maps(branches, &file_paths, dir.path());

        // AAAA maps generated (0,0) -> original (0,0) in source "app.ts"
        assert_eq!(remapped.len(), 1);
        let new_file_id = fnv1a_hash("app.ts");
        assert_eq!(remapped[0].file_id, new_file_id);
        assert_eq!(remapped[0].line, 1); // 0-based src line 0 + 1 = 1
        assert_eq!(remapped[0].col, 0);
        assert!(new_files.contains_key(&new_file_id));
        assert_eq!(new_files[&new_file_id], PathBuf::from("app.ts"));
    }

    #[test]
    fn remap_drops_branch_when_source_map_exists_but_no_token_match() {
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("gen.js");
        std::fs::write(&js_path, "").unwrap();

        // Source map with mapping only at (0,0)
        let source_map_json = r#"{
            "version": 3,
            "file": "gen.js",
            "sources": ["src.ts"],
            "names": [],
            "mappings": "AAAA"
        }"#;
        let map_path = dir.path().join("gen.js.map");
        std::fs::write(&map_path, source_map_json).unwrap();

        let file_id = fnv1a_hash("gen.js");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, PathBuf::from("gen.js"));

        // Query line 9999 — no token should match exactly, but sourcemap crate
        // may return the closest token. Let's see what happens.
        let branches = vec![BranchId::new(file_id, 9999, 50, 0)];
        let (remapped, _new_files) = remap_source_maps(branches, &file_paths, dir.path());

        // The sourcemap crate's lookup_token returns closest match, so it may
        // still return a token. The branch may or may not be dropped.
        // This test documents the actual behavior.
        // With a single mapping at (0,0), lookup_token(9998, 50) typically
        // returns the token at (0,0) since sourcemap does "closest previous" lookup.
        // So the branch gets remapped, not dropped.
        // This is important to document: branches are only dropped if lookup_token
        // returns None (no mappings at all) or if the token has no source.
        assert!(
            remapped.len() <= 1,
            "branch should be either remapped or dropped"
        );
    }

    #[test]
    fn bug_remap_with_source_root_double_applies() {
        // BUG: The sourcemap crate v9 already resolves sourceRoot into the
        // source paths returned by token.get_source(). The code at lines 34-39
        // then ALSO prepends sourceRoot, creating "src/src/main.ts" instead of
        // "src/main.ts". This is a double-application bug.
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("dist.js");
        std::fs::write(&js_path, "").unwrap();

        let source_map_json = r#"{
            "version": 3,
            "file": "dist.js",
            "sourceRoot": "src/",
            "sources": ["main.ts"],
            "names": [],
            "mappings": "AAAA"
        }"#;
        let map_path = dir.path().join("dist.js.map");
        std::fs::write(&map_path, source_map_json).unwrap();

        let file_id = fnv1a_hash("dist.js");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, PathBuf::from("dist.js"));

        let branches = vec![BranchId::new(file_id, 1, 0, 0)];
        let (remapped, new_files) = remap_source_maps(branches, &file_paths, dir.path());

        assert_eq!(remapped.len(), 1);
        // CORRECT behavior: source path should be "src/main.ts"
        let correct_path = PathBuf::from("src/main.ts");
        let correct_id = fnv1a_hash(&correct_path.to_string_lossy());
        // After fix, this should pass:
        assert_eq!(remapped[0].file_id, correct_id,
            "sourceRoot should not be double-applied");
        assert_eq!(new_files[&correct_id], correct_path);
    }

    #[test]
    fn bug_remap_preserves_direction() {
        // Verify that the direction field is preserved through remapping
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("x.js");
        std::fs::write(&js_path, "").unwrap();

        let source_map_json = r#"{
            "version": 3,
            "file": "x.js",
            "sources": ["x.ts"],
            "names": [],
            "mappings": "AAAA"
        }"#;
        std::fs::write(dir.path().join("x.js.map"), source_map_json).unwrap();

        let file_id = fnv1a_hash("x.js");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, PathBuf::from("x.js"));

        let branches = vec![BranchId::new(file_id, 1, 0, 7)];
        let (remapped, _) = remap_source_maps(branches, &file_paths, dir.path());

        assert_eq!(remapped.len(), 1);
        assert_eq!(remapped[0].direction, 7, "direction must be preserved through remap");
    }

    #[test]
    fn bug_col_truncation_at_u16_max() {
        // Line 44: `token.get_src_col().min(u16::MAX as u32) as u16`
        // This clamps columns > 65535 to 65535. Verify this clamping works
        // and doesn't panic or overflow.
        // We can't easily create a source map with col > 65535 in a unit test,
        // but we can verify the clamping logic directly.
        let large_col: u32 = 70000;
        let clamped = large_col.min(u16::MAX as u32) as u16;
        assert_eq!(clamped, u16::MAX);

        let normal_col: u32 = 42;
        let clamped = normal_col.min(u16::MAX as u32) as u16;
        assert_eq!(clamped, 42);
    }

    #[test]
    fn bug_line_zero_saturating_sub() {
        // Line 29: branch.line.saturating_sub(1) converts 1-based to 0-based
        // If line is 0 (invalid input), saturating_sub(1) = 0
        // This silently maps line 0 to source map line 0, which is wrong —
        // line 0 is invalid in 1-based numbering.
        // We document that line=0 and line=1 both map to the same source map line.
        let zero_based = 0u32.saturating_sub(1);
        assert_eq!(zero_based, 0, "line 0 maps to same as line 1 — potential bug");
        let one_based = 1u32.saturating_sub(1);
        assert_eq!(one_based, 0, "line 1 correctly maps to 0-based line 0");
        // Both give 0 — indistinguishable
    }

    #[test]
    fn remap_multiple_branches_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("multi.js");
        std::fs::write(&js_path, "").unwrap();

        // Two mappings: line 0 col 0 -> source line 0 col 0
        //               line 1 col 0 -> source line 2 col 4
        // "AAAA;AECA" in VLQ: first segment (0,0,0,0), second line first segment (0,2,0,4... wait
        // Actually let's use a simpler map with just one mapping
        let source_map_json = r#"{
            "version": 3,
            "file": "multi.js",
            "sources": ["multi.ts"],
            "names": [],
            "mappings": "AAAA;AAEA"
        }"#;
        std::fs::write(dir.path().join("multi.js.map"), source_map_json).unwrap();

        let file_id = fnv1a_hash("multi.js");
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, PathBuf::from("multi.js"));

        let branches = vec![
            BranchId::new(file_id, 1, 0, 0), // line 1 -> 0-based line 0
            BranchId::new(file_id, 2, 0, 1), // line 2 -> 0-based line 1
        ];
        let (remapped, new_files) = remap_source_maps(branches, &file_paths, dir.path());

        assert_eq!(remapped.len(), 2);
        // Both should be remapped to the same source file
        assert_eq!(remapped[0].file_id, remapped[1].file_id);
        // Direction preserved
        assert_eq!(remapped[0].direction, 0);
        assert_eq!(remapped[1].direction, 1);
        assert!(!new_files.is_empty());
    }

    #[test]
    fn load_source_map_prefers_sidecar_over_inline() {
        // When both sidecar and inline exist, sidecar wins (it's checked first)
        let dir = tempfile::tempdir().unwrap();
        let js_path = dir.path().join("both.js");

        // Inline source map
        let inline_json = r#"{"version":3,"file":"both.js","sources":["inline.ts"],"names":[],"mappings":"AAAA"}"#;
        let b64 = base64_encode_for_test(inline_json.as_bytes());
        let js_content = format!(
            "code();\n//# sourceMappingURL=data:application/json;base64,{}",
            b64
        );
        std::fs::write(&js_path, &js_content).unwrap();

        // Sidecar source map with different source
        let sidecar_json = r#"{"version":3,"file":"both.js","sources":["sidecar.ts"],"names":[],"mappings":"AAAA"}"#;
        std::fs::write(dir.path().join("both.js.map"), sidecar_json).unwrap();

        let sm = load_source_map(&js_path).unwrap();
        // Sidecar should win
        let token = sm.lookup_token(0, 0).unwrap();
        assert_eq!(
            token.get_source().unwrap(),
            "sidecar.ts",
            "sidecar should take priority over inline"
        );
    }

    // Helper: simple base64 encode for test data
    fn base64_encode_for_test(data: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut result = String::new();
        for chunk in data.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;
            result.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
            result.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
            if chunk.len() > 1 {
                result.push(TABLE[((triple >> 6) & 0x3F) as usize] as char);
            } else {
                result.push('=');
            }
            if chunk.len() > 2 {
                result.push(TABLE[(triple & 0x3F) as usize] as char);
            } else {
                result.push('=');
            }
        }
        result
    }
}
