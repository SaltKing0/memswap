use std::path::Path;

use memswap_core::Result;

use crate::model::{Adapter, HarnessContext, MergeStrategy, WriteReport};

/// Codex memory layout (M3): `~/.codex/AGENTS.md` (global instructions, ~32 KiB
/// cap) + generated `~/.codex/memories/` (MEMORY.md handbook, memory_summary.md,
/// rollout_summaries/*.md, skills/). `memories/` is generated state and treated
/// as read-only by memswap; the supported write path is AGENTS.md.
///
/// Stub in M1 — implemented in M3 per the build plan.
pub struct CodexAdapter;

impl Adapter for CodexAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    fn detect(&self, home: &Path) -> Option<HarnessContext> {
        let agents = home.join("AGENTS.md");
        if agents.exists() || home.join("memories").exists() {
            Some(HarnessContext {
                name: "codex".into(),
                home: home.to_path_buf(),
                profile: None,
                detected: vec![("agents".into(), agents)],
            })
        } else {
            None
        }
    }

    fn read(&self, _ctx: &HarnessContext) -> Result<Vec<memswap_core::Entry>> {
        Err(memswap_core::Error::Adapter(
            "codex adapter not implemented in M1 (planned for M3)".into(),
        ))
    }

    fn write(
        &self,
        _ctx: &HarnessContext,
        _entries: &[memswap_core::Entry],
        _strategy: MergeStrategy,
    ) -> Result<WriteReport> {
        Err(memswap_core::Error::Adapter(
            "codex adapter not implemented in M1 (planned for M3)".into(),
        ))
    }
}
