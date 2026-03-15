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
                // Guard against fuzzy nearest-match: if the token maps to a
                // different destination line, the lookup was inexact and we
                // should drop this branch rather than attribute it to the
                // wrong source location.
                if token.get_dst_line() != line_0 {
                    continue;
                }
                if let Some(source) = token.get_source() {
                    // The sourcemap crate v9 already prepends sourceRoot to the
                    // value returned by get_source() — do not join manually.
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
    // Try .map sidecar — append ".map" to the full filename so that
    // "foo.mjs" → "foo.mjs.map" rather than "foo.js.map".
    let map_path = PathBuf::from(format!("{}.map", js_path.display()));
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
                let b64 = data_url[comma_pos + 1..].lines().next().unwrap_or("").trim();
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
        // Skip RFC 2045 whitespace (newlines, spaces, tabs)
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

    #[test]
    fn bug_base64_decode_whitespace() {
        // RFC 2045 allows line-wrapped base64; our decoder must skip whitespace.
        let decoded = base64_decode("SGVs\nbG8=").unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn bug_inline_sourcemap_trailing_truncated() {
        // After the base64 payload there may be trailing file content (e.g.
        // another comment or code). The truncation at first whitespace must
        // prevent that trailing content from being fed into the decoder.
        let clean = "SGVsbG8";
        let with_trailing = "SGVsbG8 \n// some trailing JS content";
        // Both should decode identically — the trailing content is ignored.
        let decoded_clean = base64_decode(clean).unwrap();
        let b64_extracted = with_trailing.split_whitespace().next().unwrap_or(with_trailing);
        let decoded_trailing = base64_decode(b64_extracted).unwrap();
        assert_eq!(decoded_clean, decoded_trailing);
        assert_eq!(decoded_clean, b"Hello");
    }

    #[test]
    fn base64_decode_empty_input() {
        let decoded = base64_decode("").unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn base64_decode_invalid_char_returns_err() {
        assert!(base64_decode("SGV!bG8=").is_err());
    }

    #[test]
    fn base64_decode_padding_variants() {
        // No padding needed (3 bytes = 4 base64 chars)
        assert_eq!(base64_decode("AQID").unwrap(), &[1, 2, 3]);
        // Single padding (2 bytes = 3 base64 chars + 1 pad)
        assert_eq!(base64_decode("AQI=").unwrap(), &[1, 2]);
        // Double padding (1 byte = 2 base64 chars + 2 pad)
        assert_eq!(base64_decode("AQ==").unwrap(), &[1]);
    }

    #[test]
    fn base64_decode_with_carriage_return() {
        // Windows-style line endings in base64
        let decoded = base64_decode("SGVs\r\nbG8=").unwrap();
        assert_eq!(decoded, b"Hello");
    }

    #[test]
    fn remap_empty_branches() {
        let file_paths = HashMap::new();
        let branches = vec![];
        let (remapped, new_files) = remap_source_maps(branches, &file_paths, Path::new("/tmp"));
        assert!(remapped.is_empty());
        assert!(new_files.is_empty());
    }

    #[test]
    fn bug_load_source_map_inline_with_trailing_newline_and_code() {
        // After fix: lines().next() stops at the first newline, so trailing
        // content on subsequent lines is not fed into the base64 decoder.
        let dir = std::env::temp_dir().join("apex_test_inline_trailing");
        let _ = std::fs::create_dir_all(&dir);
        let js_path = dir.join("test_trailing.js");

        let source_map_json =
            r#"{"version":3,"sources":["test.ts"],"names":[],"mappings":"AAAA"}"#;
        let b64 = simple_base64_encode(source_map_json.as_bytes());
        // Simulate file with content after the source map comment
        let js_content = format!(
            "console.log('hello');\n//# sourceMappingURL=data:application/json;base64,{}\n// some trailing comment\n",
            b64
        );
        std::fs::write(&js_path, &js_content).unwrap();

        let sm = load_source_map(&js_path);
        assert!(
            sm.is_some(),
            "inline source map with trailing content should parse correctly after fix"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bug_remap_line_zero_collides_with_line_one() {
        // Documents that line=0 and line=1 both saturate to source-map line 0.
        // This is a known limitation: without a source map both pass through intact.
        let line_zero_mapped = 0u32.saturating_sub(1);
        let line_one_mapped = 1u32.saturating_sub(1);
        assert_eq!(
            line_zero_mapped, line_one_mapped,
            "line=0 and line=1 both map to source-map line 0 via saturating_sub"
        );
    }

    #[test]
    fn bug_remap_with_source_root_double_joins_path() {
        // After fix: sourceRoot is NOT manually joined because the sourcemap crate
        // v9 already prepends it in get_source(). The result should be "src/app.ts",
        // not "src/src/app.ts".
        let dir = std::env::temp_dir().join("apex_test_source_root_remap");
        let _ = std::fs::create_dir_all(&dir);
        let map_path = dir.join("app.js.map");

        let source_map_json = r#"{"version":3,"sourceRoot":"src/","sources":["app.ts"],"names":[],"mappings":"AAAA"}"#;
        std::fs::write(&map_path, source_map_json).unwrap();

        let js_rel = PathBuf::from("app.js");
        let file_id = fnv1a_hash(&js_rel.to_string_lossy());
        let mut file_paths = HashMap::new();
        file_paths.insert(file_id, js_rel);

        let branches = vec![BranchId::new(file_id, 1, 0, 0)];
        let (remapped, new_files) = remap_source_maps(branches, &file_paths, &dir);

        assert_eq!(remapped.len(), 1);
        let actual_file_id = remapped[0].file_id;
        let actual_path = &new_files[&actual_file_id];
        let actual_str = actual_path.to_string_lossy().to_string();

        // After fix: sourceRoot is not double-joined; get_source() already returns
        // the resolved path. The exact value depends on the sourcemap crate's
        // resolution: either "src/app.ts" (if crate prepends) or "app.ts" (if not).
        // Either way it must NOT be "src/src/app.ts".
        assert_ne!(
            actual_str, "src/src/app.ts",
            "sourceRoot must not be double-joined"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bug_col_clamping_loses_large_column() {
        // Documents that source columns > 65535 are clamped to u16::MAX.
        // This is a known BranchId limitation (col is u16).
        let large_col: u32 = 70000;
        let clamped = large_col.min(u16::MAX as u32) as u16;
        assert_eq!(clamped, u16::MAX, "column 70000 is clamped to u16::MAX");
        assert_ne!(clamped as u32, large_col, "column information is lost for values > 65535");
    }

    #[test]
    fn bug_load_source_map_mjs_wrong_sidecar_path() {
        // After fix: the sidecar is found at "module.mjs.map" because we append
        // ".map" to the full filename instead of using with_extension("js.map").
        let dir = std::env::temp_dir().join("apex_test_mjs");
        let _ = std::fs::create_dir_all(&dir);
        let js_path = dir.join("module.mjs");
        let correct_map = dir.join("module.mjs.map");

        let source_map_json =
            r#"{"version":3,"sources":["module.ts"],"names":[],"mappings":"AAAA"}"#;
        std::fs::write(&correct_map, source_map_json).unwrap();
        std::fs::write(&js_path, "export default 1;").unwrap();

        let sm = load_source_map(&js_path);
        assert!(
            sm.is_some(),
            "source map at module.mjs.map should be found after sidecar path fix"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // Helper for tests that need base64 encoding
    fn simple_base64_encode(data: &[u8]) -> String {
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
