use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsRuntime {
    Node,
    Bun,
    Deno,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgManager {
    Npm,
    Yarn,
    Pnpm,
    Bun,
    Deno,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsTestRunner {
    Jest,
    Mocha,
    Vitest,
    BunTest,
    DenoTest,
    NpmScript,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleSystem {
    CommonJS,
    ESM,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MonorepoKind {
    NpmWorkspaces,
    Yarn,
    Pnpm,
    Turborepo,
    Nx,
}

#[derive(Debug, Clone)]
pub struct JsEnvironment {
    pub runtime: JsRuntime,
    pub pkg_manager: PkgManager,
    pub test_runner: JsTestRunner,
    pub module_system: ModuleSystem,
    pub is_typescript: bool,
    pub source_maps: bool,
    pub monorepo: Option<MonorepoKind>,
}

impl JsEnvironment {
    /// Detect the JS/TS project environment from the filesystem.
    pub fn detect(target: &Path) -> Option<JsEnvironment> {
        if !target.join("package.json").exists() {
            return None;
        }

        let runtime = detect_runtime(target);
        let pkg_manager = detect_pkg_manager(target, runtime);
        let test_runner = detect_test_runner(target);
        let module_system = detect_module_system(target);
        let is_typescript = detect_typescript(target);
        let source_maps = is_typescript;
        let monorepo = detect_monorepo(target);

        Some(JsEnvironment {
            runtime,
            pkg_manager,
            test_runner,
            module_system,
            is_typescript,
            source_maps,
            monorepo,
        })
    }
}

fn detect_runtime(target: &Path) -> JsRuntime {
    if target.join("bun.lockb").exists() || target.join("bunfig.toml").exists() {
        JsRuntime::Bun
    } else if target.join("deno.json").exists() || target.join("deno.jsonc").exists() {
        JsRuntime::Deno
    } else {
        JsRuntime::Node
    }
}

fn detect_pkg_manager(target: &Path, runtime: JsRuntime) -> PkgManager {
    if runtime == JsRuntime::Bun {
        return PkgManager::Bun;
    }
    if runtime == JsRuntime::Deno {
        return PkgManager::Deno;
    }
    if target.join("yarn.lock").exists() {
        return PkgManager::Yarn;
    }
    if target.join("pnpm-lock.yaml").exists() {
        return PkgManager::Pnpm;
    }
    PkgManager::Npm
}

/// Detect test runner from package.json content.
///
/// Priority: if the project has a custom `"test"` script that isn't just
/// a bare `jest`/`mocha`/`vitest` invocation, use `npm test` — the project
/// author knows best.  Only fall back to direct runner invocation when there
/// is no npm test script or it's a simple wrapper.
pub fn detect_test_runner(target: &Path) -> JsTestRunner {
    let pkg_content = std::fs::read_to_string(target.join("package.json")).unwrap_or_default();

    let runtime = detect_runtime(target);

    if runtime == JsRuntime::Deno {
        if pkg_content.contains("\"vitest\"") {
            return JsTestRunner::Vitest;
        }
        return JsTestRunner::DenoTest;
    }

    if runtime == JsRuntime::Bun {
        if pkg_content.contains("\"vitest\"") {
            return JsTestRunner::Vitest;
        }
        return JsTestRunner::BunTest;
    }

    // Check if there's a custom npm test script.  If it's something other
    // than a bare test-runner invocation, honour it — the project knows its
    // own test harness better than we do.
    if let Some(test_script) = extract_npm_test_script(&pkg_content) {
        let bare = test_script.trim();
        // Only override if the script is NOT just a bare runner name
        let is_bare_runner = matches!(
            bare,
            "jest" | "mocha" | "vitest" | "vitest run" | "jest --passWithNoTests"
        );
        if !is_bare_runner {
            return JsTestRunner::NpmScript;
        }
    }

    if pkg_content.contains("\"jest\"") {
        return JsTestRunner::Jest;
    }
    if pkg_content.contains("\"mocha\"") {
        return JsTestRunner::Mocha;
    }
    if pkg_content.contains("\"vitest\"") {
        return JsTestRunner::Vitest;
    }
    // Fallback: if there's a test script at all, use it
    if pkg_content.contains("\"scripts\"") && pkg_content.contains("\"test\"") {
        return JsTestRunner::NpmScript;
    }
    JsTestRunner::Jest
}

/// Extract the value of `"test"` from the `"scripts"` object in package.json.
///
/// Simple line-based extraction — avoids pulling in serde_json.
fn extract_npm_test_script(pkg_content: &str) -> Option<String> {
    let mut in_scripts = false;
    let mut brace_depth = 0;

    for line in pkg_content.lines() {
        let trimmed = line.trim();

        if trimmed.contains("\"scripts\"") && trimmed.contains('{') {
            in_scripts = true;
            brace_depth = 1;
            // The scripts block might start on this same line
            if let Some(val) = extract_test_value(trimmed) {
                return Some(val);
            }
            continue;
        }

        if in_scripts {
            brace_depth += trimmed.matches('{').count() as i32;
            brace_depth -= trimmed.matches('}').count() as i32;
            if brace_depth <= 0 {
                break;
            }
            if let Some(val) = extract_test_value(trimmed) {
                return Some(val);
            }
        }
    }
    None
}

/// Extract value from a line like `"test": "hereby runtests-parallel"`.
fn extract_test_value(line: &str) -> Option<String> {
    let trimmed = line.trim().trim_end_matches(',');
    // Match "test": "value"
    let after_key = trimmed
        .strip_prefix("\"test\":")
        .or_else(|| trimmed.strip_prefix("\"test\" :"))
        .map(|s| s.trim())?;
    // Strip surrounding quotes
    let value = after_key.trim_matches('"');
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

fn detect_module_system(target: &Path) -> ModuleSystem {
    let pkg_content = std::fs::read_to_string(target.join("package.json")).unwrap_or_default();
    let has_type_module =
        pkg_content.contains("\"type\": \"module\"") || pkg_content.contains("\"type\":\"module\"");

    let src_dir = target.join("src");
    let has_mjs = src_dir.join("index.mjs").exists();
    let has_cjs = src_dir.join("index.cjs").exists();

    match (has_type_module, has_mjs, has_cjs) {
        (true, _, true) => ModuleSystem::Mixed,
        (true, _, _) => ModuleSystem::ESM,
        (false, true, _) => ModuleSystem::Mixed,
        _ => ModuleSystem::CommonJS,
    }
}

fn detect_typescript(target: &Path) -> bool {
    if let Ok(entries) = std::fs::read_dir(target) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with("tsconfig") && name_str.ends_with(".json") {
                return true;
            }
        }
    }
    let src_dir = target.join("src");
    if let Ok(entries) = std::fs::read_dir(&src_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.ends_with(".ts") || name_str.ends_with(".tsx") {
                return true;
            }
        }
    }
    false
}

fn detect_monorepo(target: &Path) -> Option<MonorepoKind> {
    let pkg_content = std::fs::read_to_string(target.join("package.json")).unwrap_or_default();

    if target.join("nx.json").exists() {
        return Some(MonorepoKind::Nx);
    }
    if target.join("turbo.json").exists() {
        return Some(MonorepoKind::Turborepo);
    }
    if target.join("pnpm-workspace.yaml").exists() {
        return Some(MonorepoKind::Pnpm);
    }
    if pkg_content.contains("\"workspaces\"") {
        if target.join("yarn.lock").exists() {
            return Some(MonorepoKind::Yarn);
        }
        return Some(MonorepoKind::NpmWorkspaces);
    }
    None
}

/// Return the test command for the given environment.
pub fn test_command(env: &JsEnvironment) -> (String, Vec<String>) {
    match env.test_runner {
        JsTestRunner::Jest => (
            "npx".to_string(),
            vec!["jest".to_string(), "--passWithNoTests".to_string()],
        ),
        JsTestRunner::Mocha => ("npx".to_string(), vec!["mocha".to_string()]),
        JsTestRunner::Vitest => (
            "npx".to_string(),
            vec!["vitest".to_string(), "run".to_string()],
        ),
        JsTestRunner::BunTest => ("bun".to_string(), vec!["test".to_string()]),
        JsTestRunner::DenoTest => ("deno".to_string(), vec!["test".to_string()]),
        JsTestRunner::NpmScript => ("npm".to_string(), vec!["test".to_string()]),
    }
}

/// Return the install command for the given environment.
pub fn install_command(env: &JsEnvironment) -> &'static str {
    match env.pkg_manager {
        PkgManager::Npm => "npm",
        PkgManager::Yarn => "yarn",
        PkgManager::Pnpm => "pnpm",
        PkgManager::Bun => "bun",
        PkgManager::Deno => "deno",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detect_none_without_package_json() {
        let dir = tempdir().unwrap();
        assert!(JsEnvironment::detect(dir.path()).is_none());
    }

    #[test]
    fn detect_basic_npm_project() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "test", "devDependencies": {"jest": "^29"}}"#,
        )
        .unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.runtime, JsRuntime::Node);
        assert_eq!(env.pkg_manager, PkgManager::Npm);
        assert_eq!(env.test_runner, JsTestRunner::Jest);
        assert_eq!(env.module_system, ModuleSystem::CommonJS);
        assert!(!env.is_typescript);
        assert!(env.monorepo.is_none());
    }

    #[test]
    fn detect_typescript_via_tsconfig() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "ts-proj"}"#).unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert!(env.is_typescript);
        assert!(env.source_maps);
    }

    #[test]
    fn detect_typescript_via_tsconfig_build() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "ts"}"#).unwrap();
        std::fs::write(dir.path().join("tsconfig.build.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert!(env.is_typescript);
    }

    #[test]
    fn detect_typescript_via_ts_files() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "ts"}"#).unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/index.ts"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert!(env.is_typescript);
    }

    #[test]
    fn detect_bun_runtime() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "bun-proj"}"#).unwrap();
        std::fs::write(dir.path().join("bun.lockb"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.runtime, JsRuntime::Bun);
        assert_eq!(env.pkg_manager, PkgManager::Bun);
        assert_eq!(env.test_runner, JsTestRunner::BunTest);
    }

    #[test]
    fn detect_yarn_pkg_manager() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "yarn-proj", "devDependencies": {"jest": "^29"}}"#,
        )
        .unwrap();
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.pkg_manager, PkgManager::Yarn);
    }

    #[test]
    fn detect_pnpm_pkg_manager() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "pnpm"}"#).unwrap();
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.pkg_manager, PkgManager::Pnpm);
    }

    #[test]
    fn detect_esm_module_system() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "esm", "type": "module"}"#,
        )
        .unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.module_system, ModuleSystem::ESM);
    }

    #[test]
    fn detect_vitest_runner() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"devDependencies": {"vitest": "^1"}}"#,
        )
        .unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.test_runner, JsTestRunner::Vitest);
    }

    #[test]
    fn detect_npm_workspaces_monorepo() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "root", "workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::NpmWorkspaces));
    }

    #[test]
    fn detect_turborepo() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "root"}"#).unwrap();
        std::fs::write(dir.path().join("turbo.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::Turborepo));
    }

    #[test]
    fn detect_nx_monorepo() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "root"}"#).unwrap();
        std::fs::write(dir.path().join("nx.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::Nx));
    }

    #[test]
    fn detect_pnpm_workspace_monorepo() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "root"}"#).unwrap();
        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*",
        )
        .unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.monorepo, Some(MonorepoKind::Pnpm));
    }

    #[test]
    fn test_command_jest() {
        let env = JsEnvironment {
            runtime: JsRuntime::Node,
            pkg_manager: PkgManager::Npm,
            test_runner: JsTestRunner::Jest,
            module_system: ModuleSystem::CommonJS,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let (bin, args) = test_command(&env);
        assert_eq!(bin, "npx");
        assert_eq!(args, vec!["jest", "--passWithNoTests"]);
    }

    #[test]
    fn test_command_bun() {
        let env = JsEnvironment {
            runtime: JsRuntime::Bun,
            pkg_manager: PkgManager::Bun,
            test_runner: JsTestRunner::BunTest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let (bin, args) = test_command(&env);
        assert_eq!(bin, "bun");
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn install_command_variants() {
        assert_eq!(
            install_command(&JsEnvironment {
                runtime: JsRuntime::Node,
                pkg_manager: PkgManager::Npm,
                test_runner: JsTestRunner::Jest,
                module_system: ModuleSystem::CommonJS,
                is_typescript: false,
                source_maps: false,
                monorepo: None,
            }),
            "npm"
        );

        assert_eq!(
            install_command(&JsEnvironment {
                runtime: JsRuntime::Bun,
                pkg_manager: PkgManager::Bun,
                test_runner: JsTestRunner::BunTest,
                module_system: ModuleSystem::ESM,
                is_typescript: false,
                source_maps: false,
                monorepo: None,
            }),
            "bun"
        );
    }

    #[test]
    fn detect_deno_runtime() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "deno-proj"}"#).unwrap();
        std::fs::write(dir.path().join("deno.json"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.runtime, JsRuntime::Deno);
        assert_eq!(env.pkg_manager, PkgManager::Deno);
        assert_eq!(env.test_runner, JsTestRunner::DenoTest);
    }

    #[test]
    fn detect_deno_runtime_jsonc() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "deno-proj"}"#).unwrap();
        std::fs::write(dir.path().join("deno.jsonc"), "{}").unwrap();
        let env = JsEnvironment::detect(dir.path()).unwrap();
        assert_eq!(env.runtime, JsRuntime::Deno);
    }

    #[test]
    fn test_command_deno() {
        let env = JsEnvironment {
            runtime: JsRuntime::Deno,
            pkg_manager: PkgManager::Deno,
            test_runner: JsTestRunner::DenoTest,
            module_system: ModuleSystem::ESM,
            is_typescript: false,
            source_maps: false,
            monorepo: None,
        };
        let (bin, args) = test_command(&env);
        assert_eq!(bin, "deno");
        assert_eq!(args, vec!["test"]);
    }

    #[test]
    fn install_command_deno() {
        assert_eq!(
            install_command(&JsEnvironment {
                runtime: JsRuntime::Deno,
                pkg_manager: PkgManager::Deno,
                test_runner: JsTestRunner::DenoTest,
                module_system: ModuleSystem::ESM,
                is_typescript: false,
                source_maps: false,
                monorepo: None,
            }),
            "deno"
        );
    }
}
