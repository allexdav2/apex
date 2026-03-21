//! Coverage injection for wrapping user test commands.
//!
//! `inject_coverage` takes a language, a command to run, and an output directory,
//! then returns environment variables and a (possibly modified) argument list that
//! will produce coverage data when the command executes.

use apex_core::types::Language;
use std::path::Path;

/// Result of coverage injection: environment variables to set, and the
/// (possibly rewritten) command arguments.
pub struct CoverageInjection {
    /// Extra environment variables to set before running the command.
    pub env_vars: Vec<(String, String)>,
    /// The rewritten command + arguments to execute.
    pub args: Vec<String>,
}

/// Inject coverage instrumentation into a command invocation.
///
/// For each supported language this sets the right env vars and/or rewrites the
/// command so the test runner produces coverage output in `output_dir`.
pub fn inject_coverage(lang: Language, cmd: &[String], output_dir: &Path) -> CoverageInjection {
    match lang {
        Language::Python => {
            let mut args = vec![
                "coverage".into(),
                "run".into(),
                "-m".into(),
            ];
            args.extend(cmd.iter().cloned());
            CoverageInjection {
                env_vars: vec![(
                    "COVERAGE_FILE".into(),
                    output_dir.join(".coverage").display().to_string(),
                )],
                args,
            }
        }

        Language::JavaScript => CoverageInjection {
            env_vars: vec![(
                "NODE_V8_COVERAGE".into(),
                output_dir.display().to_string(),
            )],
            args: cmd.to_vec(),
        },

        Language::Go => {
            let mut args = cmd.to_vec();
            args.push(format!(
                "-coverprofile={}",
                output_dir.join("coverage.out").display()
            ));
            CoverageInjection {
                env_vars: vec![],
                args,
            }
        }

        Language::Rust => CoverageInjection {
            env_vars: vec![
                ("RUSTFLAGS".into(), "-C instrument-coverage".into()),
                (
                    "LLVM_PROFILE_FILE".into(),
                    output_dir
                        .join("default_%m_%p.profraw")
                        .display()
                        .to_string(),
                ),
            ],
            args: cmd.to_vec(),
        },

        Language::Java | Language::Kotlin => {
            // JaCoCo agent injection — prepend -javaagent to the java/gradle/mvn command.
            let jacoco_dest = output_dir.join("jacoco.exec");
            let agent_arg = format!(
                "-javaagent:jacoco-agent.jar=destfile={}",
                jacoco_dest.display()
            );
            let mut args = Vec::with_capacity(cmd.len() + 1);
            if let Some(first) = cmd.first() {
                args.push(first.clone());
                // For gradle/mvn, pass as JVM arg
                if first == "gradle" || first == "gradlew" || first == "./gradlew" {
                    args.push(format!("-Djacoco={}", jacoco_dest.display()));
                } else if first == "mvn" || first == "./mvnw" {
                    args.push(format!("-Djacoco.destFile={}", jacoco_dest.display()));
                } else {
                    args.push(agent_arg);
                }
                args.extend(cmd[1..].iter().cloned());
            }
            CoverageInjection {
                env_vars: vec![("JACOCO_AGENT_DEST".into(), jacoco_dest.display().to_string())],
                args,
            }
        }

        Language::Swift => CoverageInjection {
            env_vars: vec![],
            args: {
                let mut a = cmd.to_vec();
                a.push("--enable-code-coverage".into());
                a
            },
        },

        Language::CSharp => {
            let mut args = cmd.to_vec();
            args.extend([
                "--collect:\"XPlat Code Coverage\"".into(),
                format!(
                    "--results-directory:{}",
                    output_dir.display()
                ),
            ]);
            CoverageInjection {
                env_vars: vec![],
                args,
            }
        }

        Language::Ruby => {
            // SimpleCov is typically require'd in test_helper; we set the output
            // dir via env so it lands where we expect.
            CoverageInjection {
                env_vars: vec![(
                    "COVERAGE_DIR".into(),
                    output_dir.display().to_string(),
                )],
                args: cmd.to_vec(),
            }
        }

        Language::C | Language::Cpp => CoverageInjection {
            env_vars: vec![
                ("CFLAGS".into(), "--coverage".into()),
                ("CXXFLAGS".into(), "--coverage".into()),
                ("GCOV_PREFIX".into(), output_dir.display().to_string()),
            ],
            args: cmd.to_vec(),
        },

        Language::Wasm => {
            // No standard coverage injection for wasm; pass through unchanged.
            CoverageInjection {
                env_vars: vec![],
                args: cmd.to_vec(),
            }
        }
    }
}

/// Auto-detect language from the first token of a command.
///
/// Returns `None` if the command is unrecognised.
pub fn detect_language_from_command(cmd: &[String]) -> Option<Language> {
    let first = cmd.first()?.rsplit('/').next()?;
    match first {
        "pytest" | "python" | "python3" | "pip" | "pipenv" | "poetry" => Some(Language::Python),
        "npm" | "npx" | "node" | "jest" | "vitest" | "mocha" | "yarn" | "pnpm" | "bun" => {
            Some(Language::JavaScript)
        }
        "go" => Some(Language::Go),
        "cargo" | "rustc" => Some(Language::Rust),
        "dotnet" | "csc" => Some(Language::CSharp),
        "swift" | "xcodebuild" => Some(Language::Swift),
        "bundle" | "rspec" | "rake" | "ruby" => Some(Language::Ruby),
        "gradle" | "gradlew" | "./gradlew" | "mvn" | "./mvnw" | "java" | "javac" => {
            Some(Language::Java)
        }
        "gcc" | "g++" | "cc" | "make" | "cmake" | "clang" => Some(Language::C),
        "clang++" | "c++" => Some(Language::Cpp),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn s(v: &str) -> String {
        v.to_string()
    }

    fn cmd(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| s(a)).collect()
    }

    // -----------------------------------------------------------------------
    // inject_coverage per language
    // -----------------------------------------------------------------------

    #[test]
    fn test_inject_python() {
        let out = PathBuf::from("/tmp/cov");
        let inj = inject_coverage(Language::Python, &cmd(&["pytest", "-q"]), &out);
        assert_eq!(inj.args, cmd(&["coverage", "run", "-m", "pytest", "-q"]));
        assert!(inj
            .env_vars
            .iter()
            .any(|(k, v)| k == "COVERAGE_FILE" && v.contains(".coverage")));
    }

    #[test]
    fn test_inject_javascript() {
        let out = PathBuf::from("/tmp/cov");
        let inj = inject_coverage(Language::JavaScript, &cmd(&["npm", "test"]), &out);
        assert_eq!(inj.args, cmd(&["npm", "test"]));
        assert!(inj
            .env_vars
            .iter()
            .any(|(k, _)| k == "NODE_V8_COVERAGE"));
    }

    #[test]
    fn test_inject_go() {
        let out = PathBuf::from("/tmp/cov");
        let inj = inject_coverage(Language::Go, &cmd(&["go", "test", "./..."]), &out);
        assert_eq!(inj.args.len(), 4);
        assert!(inj.args[3].starts_with("-coverprofile="));
        assert!(inj.env_vars.is_empty());
    }

    #[test]
    fn test_inject_rust() {
        let out = PathBuf::from("/tmp/cov");
        let inj = inject_coverage(Language::Rust, &cmd(&["cargo", "test"]), &out);
        assert_eq!(inj.args, cmd(&["cargo", "test"]));
        assert!(inj
            .env_vars
            .iter()
            .any(|(k, v)| k == "RUSTFLAGS" && v.contains("instrument-coverage")));
        assert!(inj
            .env_vars
            .iter()
            .any(|(k, v)| k == "LLVM_PROFILE_FILE" && v.contains("profraw")));
    }

    #[test]
    fn test_inject_csharp() {
        let out = PathBuf::from("/tmp/cov");
        let inj = inject_coverage(Language::CSharp, &cmd(&["dotnet", "test"]), &out);
        assert!(inj.args.iter().any(|a| a.contains("XPlat Code Coverage")));
        assert!(inj
            .args
            .iter()
            .any(|a| a.starts_with("--results-directory:")));
    }

    #[test]
    fn test_inject_swift() {
        let out = PathBuf::from("/tmp/cov");
        let inj = inject_coverage(Language::Swift, &cmd(&["swift", "test"]), &out);
        assert!(inj
            .args
            .iter()
            .any(|a| a == "--enable-code-coverage"));
    }

    #[test]
    fn test_inject_ruby() {
        let out = PathBuf::from("/tmp/cov");
        let inj = inject_coverage(Language::Ruby, &cmd(&["rspec"]), &out);
        assert_eq!(inj.args, cmd(&["rspec"]));
        assert!(inj.env_vars.iter().any(|(k, _)| k == "COVERAGE_DIR"));
    }

    #[test]
    fn test_inject_c() {
        let out = PathBuf::from("/tmp/cov");
        let inj = inject_coverage(Language::C, &cmd(&["make", "test"]), &out);
        assert_eq!(inj.args, cmd(&["make", "test"]));
        assert!(inj
            .env_vars
            .iter()
            .any(|(k, v)| k == "CFLAGS" && v == "--coverage"));
    }

    #[test]
    fn test_inject_java() {
        let out = PathBuf::from("/tmp/cov");
        let inj = inject_coverage(Language::Java, &cmd(&["gradle", "test"]), &out);
        assert!(inj.args.iter().any(|a| a.starts_with("-Djacoco=")));
    }

    // -----------------------------------------------------------------------
    // auto-detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_python() {
        assert_eq!(
            detect_language_from_command(&cmd(&["pytest", "-q"])),
            Some(Language::Python)
        );
        assert_eq!(
            detect_language_from_command(&cmd(&["python3", "-m", "unittest"])),
            Some(Language::Python)
        );
    }

    #[test]
    fn test_detect_javascript() {
        assert_eq!(
            detect_language_from_command(&cmd(&["npm", "test"])),
            Some(Language::JavaScript)
        );
        assert_eq!(
            detect_language_from_command(&cmd(&["jest"])),
            Some(Language::JavaScript)
        );
    }

    #[test]
    fn test_detect_go() {
        assert_eq!(
            detect_language_from_command(&cmd(&["go", "test", "./..."])),
            Some(Language::Go)
        );
    }

    #[test]
    fn test_detect_rust() {
        assert_eq!(
            detect_language_from_command(&cmd(&["cargo", "test"])),
            Some(Language::Rust)
        );
    }

    #[test]
    fn test_detect_csharp() {
        assert_eq!(
            detect_language_from_command(&cmd(&["dotnet", "test"])),
            Some(Language::CSharp)
        );
    }

    #[test]
    fn test_detect_swift() {
        assert_eq!(
            detect_language_from_command(&cmd(&["swift", "test"])),
            Some(Language::Swift)
        );
    }

    #[test]
    fn test_detect_ruby() {
        assert_eq!(
            detect_language_from_command(&cmd(&["rspec"])),
            Some(Language::Ruby)
        );
        assert_eq!(
            detect_language_from_command(&cmd(&["bundle", "exec", "rspec"])),
            Some(Language::Ruby)
        );
    }

    #[test]
    fn test_detect_java() {
        assert_eq!(
            detect_language_from_command(&cmd(&["gradle", "test"])),
            Some(Language::Java)
        );
        assert_eq!(
            detect_language_from_command(&cmd(&["mvn", "test"])),
            Some(Language::Java)
        );
    }

    #[test]
    fn test_detect_c() {
        assert_eq!(
            detect_language_from_command(&cmd(&["gcc", "-o", "test"])),
            Some(Language::C)
        );
        assert_eq!(
            detect_language_from_command(&cmd(&["make", "test"])),
            Some(Language::C)
        );
    }

    #[test]
    fn test_detect_unknown() {
        assert_eq!(
            detect_language_from_command(&cmd(&["unknown-tool"])),
            None
        );
    }

    #[test]
    fn test_detect_empty() {
        assert_eq!(detect_language_from_command(&[]), None);
    }

    #[test]
    fn test_detect_with_path() {
        // Commands may have full paths like /usr/bin/python3
        assert_eq!(
            detect_language_from_command(&cmd(&["/usr/bin/python3", "test.py"])),
            Some(Language::Python)
        );
    }
}
