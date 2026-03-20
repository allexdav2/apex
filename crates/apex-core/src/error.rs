use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApexError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Instrumentation failed: {0}")]
    Instrumentation(String),

    #[error("Language runner error: {0}")]
    LanguageRunner(String),

    #[error("Sandbox error: {0}")]
    Sandbox(String),

    #[error("Sandbox operation not supported: {0}")]
    NotSupported(String),

    #[error("Agent error: {0}")]
    Agent(String),

    #[error("Solver error: {0}")]
    Solver(String),

    #[error("Synthesis error: {0}")]
    Synthesis(String),

    #[error("Coverage oracle error: {0}")]
    Oracle(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Subprocess error: exit={exit_code}, stderr={stderr}")]
    Subprocess { exit_code: i32, stderr: String },

    #[error("Timeout after {0}ms")]
    Timeout(u64),

    #[error("Detector error: {0}")]
    Detect(String),

    #[error("Agent dispatch error: {0}")]
    AgentDispatch(String),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ApexError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_instrumentation() {
        let e = ApexError::Instrumentation("bad probe".into());
        let msg = e.to_string();
        assert!(msg.contains("Instrumentation failed"));
        assert!(msg.contains("bad probe"));
    }

    #[test]
    fn display_language_runner() {
        let e = ApexError::LanguageRunner("python crashed".into());
        let msg = e.to_string();
        assert!(msg.contains("Language runner error"));
        assert!(msg.contains("python crashed"));
    }

    #[test]
    fn display_sandbox() {
        let e = ApexError::Sandbox("jail escape".into());
        let msg = e.to_string();
        assert!(msg.contains("Sandbox error"));
        assert!(msg.contains("jail escape"));
    }

    #[test]
    fn display_not_supported() {
        let e = ApexError::NotSupported("no firecracker".into());
        let msg = e.to_string();
        assert!(msg.contains("not supported"));
        assert!(msg.contains("no firecracker"));
    }

    #[test]
    fn display_agent() {
        let e = ApexError::Agent("llm fail".into());
        let msg = e.to_string();
        assert!(msg.contains("Agent error"));
        assert!(msg.contains("llm fail"));
    }

    #[test]
    fn display_solver() {
        let e = ApexError::Solver("unsat".into());
        let msg = e.to_string();
        assert!(msg.contains("Solver error"));
        assert!(msg.contains("unsat"));
    }

    #[test]
    fn display_synthesis() {
        let e = ApexError::Synthesis("codegen failed".into());
        let msg = e.to_string();
        assert!(msg.contains("Synthesis error"));
        assert!(msg.contains("codegen failed"));
    }

    #[test]
    fn display_oracle() {
        let e = ApexError::Oracle("bitmap mismatch".into());
        let msg = e.to_string();
        assert!(msg.contains("oracle error"));
        assert!(msg.contains("bitmap mismatch"));
    }

    #[test]
    fn display_config() {
        let e = ApexError::Config("missing key".into());
        let msg = e.to_string();
        assert!(msg.contains("Configuration error"));
        assert!(msg.contains("missing key"));
    }

    #[test]
    fn display_subprocess() {
        let e = ApexError::Subprocess {
            exit_code: 42,
            stderr: "segfault".into(),
        };
        let msg = e.to_string();
        assert!(msg.contains("exit=42"));
        assert!(msg.contains("stderr=segfault"));
    }

    #[test]
    fn display_timeout() {
        let e = ApexError::Timeout(5000);
        let msg = e.to_string();
        assert!(msg.contains("Timeout after 5000ms"));
    }

    #[test]
    fn display_detect() {
        let e = ApexError::Detect("cargo-audit failed".into());
        let msg = e.to_string();
        assert!(msg.contains("Detector error"));
        assert!(msg.contains("cargo-audit failed"));
    }

    #[test]
    fn display_other() {
        let e = ApexError::Other("something else".into());
        let msg = e.to_string();
        assert!(msg.contains("something else"));
    }

    #[test]
    fn display_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let e = ApexError::Io(io_err);
        let msg = e.to_string();
        assert!(msg.contains("I/O error"));
        assert!(msg.contains("file gone"));
    }

    #[test]
    fn display_serialization() {
        let serde_err = serde_json::from_str::<()>("invalid").unwrap_err();
        let e = ApexError::Serialization(serde_err);
        let msg = e.to_string();
        assert!(msg.contains("Serialization error"));
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "no access");
        let apex: ApexError = io_err.into();
        assert!(matches!(apex, ApexError::Io(_)));
        assert!(apex.to_string().contains("no access"));
    }

    #[test]
    fn from_serde_json_error() {
        let serde_err = serde_json::from_str::<()>("{{bad}}").unwrap_err();
        let apex: ApexError = serde_err.into();
        assert!(matches!(apex, ApexError::Serialization(_)));
    }
}
