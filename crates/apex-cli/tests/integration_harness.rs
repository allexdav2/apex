//! Integration harness: end-to-end tests for `apex audit` and `apex features`
//! using the fixture projects under `tests/fixtures/`.

use apex_cli::{run_cli, Cli};
use apex_core::config::ApexConfig;
use clap::Parser;

fn default_cfg() -> ApexConfig {
    ApexConfig::default()
}

/// Absolute path to the workspace root (two levels up from crates/apex-cli/).
fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fixture_path(name: &str) -> std::path::PathBuf {
    workspace_root().join("tests/fixtures").join(name)
}

// ---------------------------------------------------------------------------
// audit tests
// ---------------------------------------------------------------------------

/// `apex audit --lang js --target tests/fixtures/tiny-js` should succeed.
///
/// The audit command reads source files and runs static detectors — it does
/// NOT require nyc, istanbul, or bun to be installed.
#[tokio::test]
async fn test_audit_js_succeeds() {
    let target = fixture_path("tiny-js");
    assert!(
        target.exists(),
        "fixture missing: {}",
        target.display()
    );

    let cli = Cli::parse_from([
        "apex",
        "audit",
        "--lang",
        "js",
        "--target",
        target.to_str().unwrap(),
    ]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(
        result.is_ok(),
        "apex audit --lang js failed: {:?}",
        result
    );
}

// ---------------------------------------------------------------------------
// features tests
// ---------------------------------------------------------------------------

/// `apex features --lang js` should succeed and not panic.
#[tokio::test]
async fn test_features_js() {
    let cli = Cli::parse_from(["apex", "features", "--lang", "js"]);
    let result = run_cli(cli, &default_cfg()).await;
    assert!(
        result.is_ok(),
        "apex features --lang js failed: {:?}",
        result
    );
}
