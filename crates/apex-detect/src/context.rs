use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use apex_core::types::{BugReport, Language};
use apex_coverage::CoverageOracle;

use crate::config::DetectConfig;

#[derive(Clone)]
pub struct AnalysisContext {
    pub target_root: PathBuf,
    pub language: Language,
    pub oracle: Arc<CoverageOracle>,
    pub file_paths: HashMap<u64, PathBuf>,
    pub known_bugs: Vec<BugReport>,
    pub source_cache: HashMap<PathBuf, String>,
    pub fuzz_corpus: Option<PathBuf>,
    pub config: DetectConfig,
}

impl fmt::Debug for AnalysisContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnalysisContext")
            .field("target_root", &self.target_root)
            .field("language", &self.language)
            .field("file_paths", &self.file_paths.len())
            .field("source_cache", &self.source_cache.len())
            .field("fuzz_corpus", &self.fuzz_corpus)
            .finish()
    }
}
