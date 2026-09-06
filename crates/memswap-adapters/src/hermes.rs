use std::fs;
use std::path::{Path, PathBuf};

use memswap_core::entry::{Entry, EntryKind, Scope, Source};
use memswap_core::{Error, Result};

use crate::model::{Adapter, HarnessContext, MergeStrategy, WriteReport};

/// Hermes stores memory as flat, section-delimited (`§`) UTF-8 text files:
///
/// - `~/.hermes/memories/MEMORY.md` — agent personal notes, ~2200 char cap
/// - `~/.hermes/memories/USER.md` — user profile, ~1375 char cap
/// - `~/.hermes/profiles/<name>/memories/*.md` — named profiles
///
/// There is no schema or per-entry metadata; we preserve the body losslessly.
pub struct HermesAdapter;

const MEMORY_LIMIT: usize = 2200;
const USER_LIMIT: usize = 1375;

impl HermesAdapter {
    fn memories_dir(home: &Path) -> PathBuf {
        home.join("memories")
    }
}

impl Adapter for HermesAdapter {
    fn name(&self) -> &str {
        "hermes"
    }

    fn detect(&self, home: &Path) -> Option<HarnessContext> {
        let dir = Self::memories_dir(home);
        if dir.join("MEMORY.md").exists() || dir.join("USER.md").exists() {
            let mut detected = vec![];
            if dir.join("MEMORY.md").exists() {
                detected.push(("memory".into(), dir.join("MEMORY.md")));
            }
            if dir.join("USER.md").exists() {
                detected.push(("user".into(), dir.join("USER.md")));
            }
            Some(HarnessContext {
                name: self.name().into(),
                home: home.to_path_buf(),
                profile: None,
                detected,
            })
        } else {
            None
        }
    }

    fn read(&self, ctx: &HarnessContext) -> Result<Vec<Entry>> {
        let mut out = vec![];
        for (label, path) in &ctx.detected {
            let body = fs::read_to_string(path).map_err(Error::Io)?;
            let (id, kind, title, limit) = match label.as_str() {
                "user" => (
                    "hermes/user",
                    EntryKind::User,
                    "Hermes user profile",
                    USER_LIMIT,
                ),
                _ => (
                    "hermes/memory",
                    EntryKind::Fact,
                    "Hermes memory",
                    MEMORY_LIMIT,
                ),
            };
            out.push(Entry {
                id: id.into(),
                kind,
                title: title.into(),
                body,
                scope: Scope::Global,
                project: None,
                source: Source {
                    harness: "hermes".into(),
                    path: Some(path.display().to_string()),
                    updated_at: None,
                    profile: ctx.profile.clone(),
                },
                tags: vec![],
                content_hash: String::new(),
            });
            let _ = limit;
        }
        Ok(out)
    }

    fn write(
        &self,
        ctx: &HarnessContext,
        entries: &[Entry],
        _strategy: MergeStrategy,
    ) -> Result<WriteReport> {
        let dir = Self::memories_dir(&ctx.home);
        fs::create_dir_all(&dir)?;
        let mut report = WriteReport::default();
        for e in entries {
            if e.source.harness != "hermes" {
                report.skipped.push(e.id.clone());
                continue;
            }
            let (filename, limit) = match e.id.as_str() {
                "hermes/user" => ("USER.md", USER_LIMIT),
                _ => ("MEMORY.md", MEMORY_LIMIT),
            };
            let mut body = e.body.clone();
            if body.chars().count() > limit {
                body = body.chars().take(limit).collect();
                report.truncated.push(e.id.clone());
            }
            fs::write(dir.join(filename), body)?;
            report.written += 1;
        }
        Ok(report)
    }
}
