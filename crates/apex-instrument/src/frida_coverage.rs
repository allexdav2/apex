//! Frida Stalker binary coverage collection (feature-gated: `frida`).
//!
//! When built with `--features frida`, this module uses `frida-gum`'s Stalker
//! engine to trace basic-block execution in a spawned binary process, then maps
//! hit addresses back to source locations via DWARF debug info.
//!
//! Without the feature, all public functions return an error directing the user
//! to recompile with `--features frida`.

use std::path::Path;

use apex_core::error::{ApexError, Result};

// ── Public types ────────────────────────────────────────────────────────────

/// Coverage data collected from Frida Stalker binary instrumentation.
#[derive(Debug, Clone, Default)]
pub struct FridaCoverageResult {
    /// Basic blocks observed: `(address, hit_count)`.
    pub basic_blocks: Vec<(u64, u64)>,
    /// Source mappings derived from DWARF: `(address, file_path, line)`.
    pub source_mappings: Vec<(u64, String, u32)>,
    /// Total number of basic blocks discovered in the target module(s).
    pub total_blocks: usize,
    /// Number of basic blocks that were executed at least once.
    pub covered_blocks: usize,
}

// ── Feature-gated implementation ────────────────────────────────────────────

#[cfg(feature = "frida")]
mod frida_impl {
    use super::*;
    use frida_gum::Gum;

    /// Collect basic-block coverage from `binary` by spawning it with `args`
    /// under Frida Stalker.
    ///
    /// `module_filter` restricts tracing to modules whose name contains the
    /// given substring (e.g. the target library name). When `None`, all
    /// modules in the process are traced.
    pub async fn collect_frida_coverage(
        binary: &Path,
        args: &[String],
        module_filter: Option<&str>,
    ) -> Result<FridaCoverageResult> {
        // 1. Initialise Frida GUM runtime.
        let gum = Gum::obtain();

        // 2. Spawn the target process.
        // 3. Attach Stalker with an optional module whitelist.
        // 4. Collect basic-block hit counts.
        // 5. Map addresses to source via DWARF (addr2line + gimli).

        // TODO: full implementation once frida-gum integration is validated
        let _ = (gum, binary, args, module_filter);
        Err(ApexError::Instrumentation(
            "Frida coverage collection is not yet fully implemented".into(),
        ))
    }
}

#[cfg(not(feature = "frida"))]
mod frida_impl {
    use super::*;

    /// Stub — returns an error directing the user to enable the `frida` feature.
    pub async fn collect_frida_coverage(
        _binary: &Path,
        _args: &[String],
        _module_filter: Option<&str>,
    ) -> Result<FridaCoverageResult> {
        Err(ApexError::Instrumentation(
            "Frida coverage requires --features frida. \
             Install: cargo build --features frida"
                .into(),
        ))
    }
}

pub use frida_impl::collect_frida_coverage;

// ── DWARF address-to-source mapping ─────────────────────────────────────────

/// Map raw binary addresses to source file + line using DWARF debug info.
///
/// On macOS the function also looks for companion `.dSYM` bundles. Returns
/// only those addresses for which debug info was found.
#[cfg(feature = "frida")]
pub fn map_addresses_to_source(binary: &Path, addresses: &[u64]) -> Vec<(u64, String, u32)> {
    use addr2line::Context;
    use object::read::File as ObjectFile;

    let Ok(data) = std::fs::read(binary) else {
        return Vec::new();
    };
    let Ok(obj) = ObjectFile::parse(&*data) else {
        return Vec::new();
    };
    let Ok(ctx) = Context::new(&obj) else {
        return Vec::new();
    };

    let mut mappings = Vec::new();
    for &addr in addresses {
        if let Ok(Some(loc)) = ctx.find_location(addr) {
            if let (Some(file), Some(line)) = (loc.file, loc.line) {
                mappings.push((addr, file.to_string(), line));
            }
        }
    }
    mappings
}

/// Stub — always returns an empty vec when the `frida` feature is disabled.
#[cfg(not(feature = "frida"))]
pub fn map_addresses_to_source(_binary: &Path, _addresses: &[u64]) -> Vec<(u64, String, u32)> {
    Vec::new()
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frida_not_available_returns_error() {
        #[cfg(not(feature = "frida"))]
        {
            let result = collect_frida_coverage(Path::new("/bin/true"), &[], None).await;
            assert!(result.is_err());
            let msg = format!("{}", result.unwrap_err());
            assert!(msg.contains("frida"), "error should mention frida: {msg}");
        }

        // When compiled with the frida feature the stub path is not taken,
        // so this test is a no-op — real integration tests would require a
        // running Frida server.
        #[cfg(feature = "frida")]
        {
            // Placeholder: ensure the function signature compiles.
            let _ = collect_frida_coverage(Path::new("/bin/true"), &[], None).await;
        }
    }

    #[test]
    fn map_addresses_empty_returns_empty() {
        let result = map_addresses_to_source(Path::new("/nonexistent"), &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn frida_coverage_result_default() {
        let r = FridaCoverageResult::default();
        assert!(r.basic_blocks.is_empty());
        assert!(r.source_mappings.is_empty());
        assert_eq!(r.total_blocks, 0);
        assert_eq!(r.covered_blocks, 0);
    }

    #[test]
    fn frida_coverage_result_clone() {
        let r = FridaCoverageResult {
            basic_blocks: vec![(0x1000, 5), (0x2000, 0)],
            source_mappings: vec![(0x1000, "main.c".into(), 42)],
            total_blocks: 2,
            covered_blocks: 1,
        };
        let r2 = r.clone();
        assert_eq!(r2.basic_blocks.len(), 2);
        assert_eq!(r2.source_mappings.len(), 1);
        assert_eq!(r2.total_blocks, 2);
        assert_eq!(r2.covered_blocks, 1);
    }

    #[test]
    fn map_addresses_nonexistent_binary_returns_empty() {
        let result =
            map_addresses_to_source(Path::new("/nonexistent/binary"), &[0x1000, 0x2000, 0x3000]);
        assert!(result.is_empty());
    }
}
