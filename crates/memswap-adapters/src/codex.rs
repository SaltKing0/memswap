use std::fs;
use std::path::{Path, PathBuf};

use memswap_core::entry::{Entry, EntryKind, Scope, Source};
use memswap_core::{Error, Result};

use crate::model::{Adapter, HarnessContext, MergeStrategy, WriteReport};

/// Codex memory layout:
///
/// - `~/.codex/AGENTS.md` — human/global instructions (~32 KiB cap; the write path)
/// - `~/.codex/memories/MEMORY.md` — consolidated handbook (generated state)
/// - `~/.codex/memories/memory_summary.md` — always-loaded index (5k-token cap)
/// - `~/.codex/memories/rollout_summaries/*.md` — per-session summaries
/// - `~/.codex/memories/skills/<name>/SKILL.md` — procedural knowledge
///
/// The `memories/` tree is generated state owned by Codex's two-phase
/// consolidation pipeline, so memswap reads it but never writes it. The
/// supported write path is `AGENTS.md` (the human-controlled instruction file).
pub struct CodexAdapter;

const AGENTS_LIMIT: usize = 32 * 1024;
const SUMMARY_TOKEN_LIMIT: usize = 5_000;
/// Rough chars-per-token for the advisory summary budget check.
const CHARS_PER_TOKEN: usize = 4;

impl CodexAdapter {
    fn memories_dir(home: &Path) -> PathBuf {
        home.join("memories")
    }

    /// Extract the `description:` field from a Codex frontmatter block.
    fn frontmatter_description(body: &str) -> Option<String> {
        let rest = body.strip_prefix("---")?;
        let end = rest.find("\n---")?;
        for line in rest[..end].lines() {
            if let Some(v) = line.strip_prefix("description:") {
                return Some(v.trim().to_string());
            }
        }
        None
    }
}

impl Adapter for CodexAdapter {
    fn name(&self) -> &str {
        "codex"
    }

    fn detect(&self, home: &Path) -> Option<HarnessContext> {
        let agents = home.join("AGENTS.md");
        let mem = Self::memories_dir(home);
        let mut detected = vec![];
        if agents.exists() {
            detected.push(("agents".into(), agents));
        }
        if mem.join("MEMORY.md").exists() {
            detected.push(("memory".into(), mem.join("MEMORY.md")));
        }
        if mem.join("memory_summary.md").exists() {
            detected.push(("summary".into(), mem.join("memory_summary.md")));
        }
        let rollout = mem.join("rollout_summaries");
        if rollout.is_dir() {
            detected.push(("rollout_summaries".into(), rollout));
        }
        let skills = mem.join("skills");
        if skills.is_dir() {
            detected.push(("skills".into(), skills));
        }
        if detected.is_empty() {
            None
        } else {
            Some(HarnessContext {
                name: "codex".into(),
                home: home.to_path_buf(),
                profile: None,
                detected,
            })
        }
    }

    fn read(&self, ctx: &HarnessContext) -> Result<Vec<Entry>> {
        let mut out = vec![];
        for (label, path) in &ctx.detected {
            match label.as_str() {
                "agents" => {
                    let body = fs::read_to_string(path).map_err(Error::Io)?;
                    out.push(Entry {
                        id: "codex/agents".into(),
                        kind: EntryKind::Instruction,
                        title: "Codex AGENTS.md".into(),
                        body,
                        scope: Scope::Global,
                        project: None,
                        source: Source {
                            harness: "codex".into(),
                            path: Some(path.display().to_string()),
                            updated_at: None,
                            profile: None,
                        },
                        tags: vec![],
                        content_hash: String::new(),
                    });
                }
                "memory" => {
                    let body = fs::read_to_string(path).map_err(Error::Io)?;
                    out.push(Entry {
                        id: "codex/memory".into(),
                        kind: EntryKind::Index,
                        title: "Codex memory handbook".into(),
                        body,
                        scope: Scope::Global,
                        project: None,
                        source: Source {
                            harness: "codex".into(),
                            path: Some(path.display().to_string()),
                            updated_at: None,
                            profile: None,
                        },
                        tags: vec!["generated".into()],
                        content_hash: String::new(),
                    });
                }
                "summary" => {
                    let body = fs::read_to_string(path).map_err(Error::Io)?;
                    out.push(Entry {
                        id: "codex/memory_summary".into(),
                        kind: EntryKind::Index,
                        title: "Codex memory summary (always-loaded index)".into(),
                        body,
                        scope: Scope::Global,
                        project: None,
                        source: Source {
                            harness: "codex".into(),
                            path: Some(path.display().to_string()),
                            updated_at: None,
                            profile: None,
                        },
                        tags: vec!["generated".into(), "index".into()],
                        content_hash: String::new(),
                    });
                }
                "rollout_summaries" => {
                    let mut files: Vec<_> = fs::read_dir(path)
                        .map_err(Error::Io)?
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|x| x == "md"))
                        .collect();
                    files.sort();
                    for p in files {
                        let body = fs::read_to_string(&p).map_err(Error::Io)?;
                        let title = Self::frontmatter_description(&body)
                            .unwrap_or_else(|| "rollout summary".into());
                        out.push(Entry {
                            id: format!("codex/rollout/{}", file_stem(&p)),
                            kind: EntryKind::Other,
                            title,
                            body,
                            scope: Scope::Global,
                            project: None,
                            source: Source {
                                harness: "codex".into(),
                                path: Some(p.display().to_string()),
                                updated_at: None,
                                profile: None,
                            },
                            tags: vec!["generated".into(), "rollout".into()],
                            content_hash: String::new(),
                        });
                    }
                }
                "skills" => {
                    // One entry per skill: <dir>/SKILL.md
                    let mut dirs: Vec<_> = fs::read_dir(path)
                        .map_err(Error::Io)?
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.is_dir())
                        .collect();
                    dirs.sort();
                    for d in dirs {
                        let skill = d.join("SKILL.md");
                        if !skill.exists() {
                            continue;
                        }
                        let body = fs::read_to_string(&skill).map_err(Error::Io)?;
                        let name = d.file_name().unwrap().to_string_lossy().to_string();
                        let title = Self::frontmatter_description(&body)
                            .unwrap_or_else(|| format!("skill {name}"));
                        out.push(Entry {
                            id: format!("codex/skill/{name}"),
                            kind: EntryKind::Other,
                            title,
                            body,
                            scope: Scope::Global,
                            project: None,
                            source: Source {
                                harness: "codex".into(),
                                path: Some(skill.display().to_string()),
                                updated_at: None,
                                profile: None,
                            },
                            tags: vec!["generated".into(), "skill".into()],
                            content_hash: String::new(),
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    fn write(
        &self,
        ctx: &HarnessContext,
        entries: &[Entry],
        strategy: MergeStrategy,
    ) -> Result<WriteReport> {
        let dir = ctx.home.clone();
        let mut report = WriteReport::default();

        // AGENTS.md is the only supported write target.
        let agents_entries: Vec<&Entry> =
            entries.iter().filter(|e| e.id == "codex/agents").collect();

        let path = dir.join("AGENTS.md");
        let existing = fs::read_to_string(&path).unwrap_or_default();

        match strategy {
            MergeStrategy::Keep => {
                // Only write when the file is absent.
                if existing.is_empty() {
                    if let Some(e) = agents_entries.first() {
                        fs::write(&path, clamp_chars(&e.body, AGENTS_LIMIT))?;
                        report.written += 1;
                    }
                } else {
                    for e in &agents_entries {
                        report.skipped.push(e.id.clone());
                    }
                }
            }
            MergeStrategy::Merge => {
                // Append incoming instructions that are not already present.
                if let Some(e) = agents_entries.first() {
                    let incoming = clamp_chars(&e.body, AGENTS_LIMIT);
                    if !existing.is_empty() && !existing.contains(&incoming) {
                        let mut merged = existing.clone();
                        if !merged.ends_with('\n') {
                            merged.push('\n');
                        }
                        merged.push_str(&incoming);
                        let merged = clamp_chars(&merged, AGENTS_LIMIT);
                        if merged.chars().count() >= AGENTS_LIMIT
                            && merged != clamp_chars(&merged, AGENTS_LIMIT)
                        {
                            report.truncated.push(e.id.clone());
                        }
                        fs::write(&path, merged)?;
                        report.written += 1;
                    } else if existing.contains(&incoming) {
                        for e in &agents_entries {
                            report.skipped.push(e.id.clone());
                        }
                    } else {
                        fs::write(&path, incoming)?;
                        report.written += 1;
                    }
                }
            }
            MergeStrategy::Replace => {
                if let Some(e) = agents_entries.first() {
                    let body = clamp_chars(&e.body, AGENTS_LIMIT);
                    if body.chars().count() > e.body.chars().count() {
                        report.truncated.push(e.id.clone());
                    }
                    fs::write(&path, body)?;
                    report.written += 1;
                }
            }
        }

        // Everything else (memory, summary, rollouts, skills) is generated
        // state: never written, reported as skipped.
        for e in entries {
            if e.id != "codex/agents" {
                report.skipped.push(e.id.clone());
            }
        }
        Ok(report)
    }
}

fn clamp_chars(s: &str, limit: usize) -> String {
    s.chars().take(limit).collect()
}

fn file_stem(p: &Path) -> String {
    p.file_stem()
        .unwrap_or(p.as_os_str())
        .to_string_lossy()
        .to_string()
}

/// Advisory token-budget note (5k-token cap on memory_summary.md). Exposed for
/// tests; the CLI reports summaries that exceed it.
pub fn summary_over_budget(body: &str) -> bool {
    body.chars().count() > SUMMARY_TOKEN_LIMIT * CHARS_PER_TOKEN
}
