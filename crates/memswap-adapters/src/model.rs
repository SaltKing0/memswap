use std::path::{Path, PathBuf};

use memswap_core::{Entry, Result};

/// Resolved, detected context for one harness (paths, profile).
#[derive(Debug, Clone)]
pub struct HarnessContext {
    pub name: String,
    pub home: PathBuf,
    pub profile: Option<String>,
    /// Files the adapter will read, with a short label.
    pub detected: Vec<(String, PathBuf)>,
}

/// How import merges into a live harness directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeStrategy {
    Replace,
    Merge,
    Keep,
}

/// What a write did.
#[derive(Debug, Clone, Default)]
pub struct WriteReport {
    pub written: usize,
    pub truncated: Vec<String>,
    pub skipped: Vec<String>,
}

/// The adapter contract. Implementations must be read-only safe and idempotent.
pub trait Adapter {
    fn name(&self) -> &str;
    /// None if this harness is absent from `home`.
    fn detect(&self, home: &Path) -> Option<HarnessContext>;
    fn read(&self, ctx: &HarnessContext) -> Result<Vec<Entry>>;
    fn write(
        &self,
        ctx: &HarnessContext,
        entries: &[Entry],
        strategy: MergeStrategy,
    ) -> Result<WriteReport>;
}
