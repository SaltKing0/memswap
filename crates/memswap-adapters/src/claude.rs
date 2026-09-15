use std::fs;
use std::path::{Path, PathBuf};

use memswap_core::entry::{Entry, EntryKind, Scope, Source};
use memswap_core::{Error, Result};

use crate::model::{Adapter, HarnessContext, MergeStrategy, WriteReport};

/// Claude Code memory layout:
///
/// - `~/.claude/CLAUDE.md` — human-authored global instructions
/// - `~/.claude/projects/<mapped-cwd>/memory/MEMORY.md` — per-project
///   always-loaded index (200-line / 25 KiB cap)
/// - `~/.claude/projects/<mapped-cwd>/memory/<type>_<slug>.md` — one file per
///   discrete memory, YAML frontmatter + freeform body; native types
///   `user | feedback | project | reference` map onto the canonical taxonomy
///   (user→User, feedback→Instruction, project→Project, reference→Fact) and
///   are preserved in tags.
///
/// The `<mapped-cwd>` key is the project path with `/` and `.` replaced by
/// `-` (leading `-` kept), e.g. `/home/u/My.Proj` → `-home-u-My-Proj`.
pub struct ClaudeAdapter;

const INDEX_LINE_CAP: usize = 200;
const INDEX_BYTE_CAP: usize = 25 * 1024;

/// Map a project path to Claude's `projects/` directory key.
pub fn map_project_key(cwd: &str) -> String {
    cwd.replace(['/', '.'], "-")
}

fn parse_frontmatter(body: &str) -> (std::collections::BTreeMap<String, String>, String) {
    let mut meta = std::collections::BTreeMap::new();
    let Some(rest) = body.strip_prefix("---") else {
        return (meta, body.to_string());
    };
    let Some(end) = rest.find("\n---") else {
        return (meta, body.to_string());
    };
    for line in rest[..end].lines() {
        if let Some((k, v)) = line.split_once(':') {
            meta.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    let after = &rest[end + 4..];
    (meta, after.trim_start_matches('\n').to_string())
}

fn native_kind(t: &str) -> (EntryKind, &'static str) {
    match t {
        "user" => (EntryKind::User, "user"),
        "feedback" => (EntryKind::Instruction, "feedback"),
        "project" => (EntryKind::Project, "project"),
        // reference and anything unknown map to Fact
        _ => (EntryKind::Fact, "reference"),
    }
}

fn read_one_memory(path: &Path, project: Option<&str>) -> Result<Option<Entry>> {
    let raw = memswap_core::text::read_text(path).map_err(Error::Io)?;
    let (meta, body) = parse_frontmatter(&raw);
    let name = path
        .file_stem()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .to_string();
    // type_<slug>.md convention; fall back to frontmatter `type`, else fact.
    let native = name
        .split_once('_')
        .map(|(t, _)| t)
        .or_else(|| meta.get("type").map(|s| s.as_str()))
        .unwrap_or("reference");
    let (kind, tag) = native_kind(native);
    let title = meta
        .get("description")
        .cloned()
        .unwrap_or_else(|| name.clone());
    let id = format!("claude/{}", name);
    Ok(Some(Entry {
        id,
        kind,
        title,
        body,
        scope: if project.is_some() {
            Scope::Project
        } else {
            Scope::Global
        },
        project: project.map(|s| s.to_string()),
        source: Source {
            harness: "claude".into(),
            path: Some(path.display().to_string()),
            updated_at: None,
            profile: None,
        },
        tags: vec![format!("claude-type:{tag}")],
        content_hash: String::new(),
    }))
}

impl ClaudeAdapter {
    fn global_claude_md(home: &Path) -> PathBuf {
        home.join("CLAUDE.md")
    }

    fn projects_dir(home: &Path) -> PathBuf {
        home.join("projects")
    }
}

impl Adapter for ClaudeAdapter {
    fn name(&self) -> &str {
        "claude"
    }

    fn detect(&self, home: &Path) -> Option<HarnessContext> {
        let mut detected = vec![];
        let claude_md = Self::global_claude_md(home);
        if claude_md.exists() {
            detected.push(("claude_md".into(), claude_md));
        }
        let projects = Self::projects_dir(home);
        if projects.is_dir() {
            // Every project dir with a memory/ subdirectory counts.
            let mut dirs: Vec<_> = fs::read_dir(&projects)
                .ok()?
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join("memory").is_dir())
                .collect();
            dirs.sort();
            for d in dirs {
                let key = d.file_name().unwrap().to_string_lossy().to_string();
                detected.push((format!("project:{key}"), d.join("memory")));
            }
        }
        // Global auto-memory.
        let global_mem = home.join("memory");
        if global_mem.is_dir() {
            detected.push(("global_memory".into(), global_mem));
        }
        if detected.is_empty() {
            None
        } else {
            Some(HarnessContext {
                name: "claude".into(),
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
                "claude_md" => {
                    let body = memswap_core::text::read_text(path).map_err(Error::Io)?;
                    out.push(Entry {
                        id: "claude/claude_md".into(),
                        kind: EntryKind::Instruction,
                        title: "Claude Code global instructions (CLAUDE.md)".into(),
                        body,
                        scope: Scope::Global,
                        project: None,
                        source: Source {
                            harness: "claude".into(),
                            path: Some(path.display().to_string()),
                            updated_at: None,
                            profile: None,
                        },
                        tags: vec![],
                        content_hash: String::new(),
                    });
                }
                "global_memory" => {
                    out.extend(read_memory_dir(path, None)?);
                }
                _ => {
                    // project:<key> — the key is the mapped cwd.
                    if let Some(key) = label.strip_prefix("project:") {
                        out.extend(read_memory_dir(path, Some(key))?);
                    }
                }
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
        let mut report = WriteReport::default();

        // 1. CLAUDE.md — global instructions.
        let claude_md: Vec<&Entry> = entries
            .iter()
            .filter(|e| e.id == "claude/claude_md")
            .collect();
        if let Some(e) = claude_md.first() {
            let path = Self::global_claude_md(&ctx.home);
            match strategy {
                MergeStrategy::Replace => {
                    fs::write(&path, &e.body)?;
                    report.written += 1;
                }
                MergeStrategy::Merge => {
                    let existing = memswap_core::text::read_text(&path).unwrap_or_default();
                    if existing.is_empty() {
                        fs::write(&path, &e.body)?;
                        report.written += 1;
                    } else if !existing.contains(&e.body) {
                        let mut merged = existing;
                        if !merged.ends_with('\n') {
                            merged.push('\n');
                        }
                        merged.push_str(&e.body);
                        fs::write(&path, merged)?;
                        report.written += 1;
                    } else {
                        report.skipped.push(e.id.clone());
                    }
                }
                MergeStrategy::Keep => {
                    if !path.exists() {
                        fs::write(&path, &e.body)?;
                        report.written += 1;
                    } else {
                        report.skipped.push(e.id.clone());
                    }
                }
            }
        }

        // 2. Per-memory files -> project memory dirs. Index entries are
        // derived views and are never written back as files.
        for e in entries
            .iter()
            .filter(|e| e.id.starts_with("claude/") && !e.tags.contains(&"index".to_string()))
        {
            if e.id == "claude/claude_md" {
                continue;
            }
            let name = e.id.strip_prefix("claude/").unwrap();
            // Determine target dir: project-scoped entries go to their project.
            let dir: PathBuf = match (&e.scope, &e.project) {
                (Scope::Project, Some(key)) => {
                    Self::projects_dir(&ctx.home).join(key).join("memory")
                }
                _ => ctx.home.join("memory"),
            };
            fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{name}.md"));
            let native_type = e
                .tags
                .iter()
                .find(|t| t.starts_with("claude-type:"))
                .map(|t| &t["claude-type:".len()..])
                .unwrap_or("reference");
            let body = render_memory_file(native_type, &e.title, &e.body);
            match strategy {
                MergeStrategy::Replace => {
                    fs::write(&path, &body)?;
                    report.written += 1;
                }
                MergeStrategy::Merge | MergeStrategy::Keep => {
                    if path.exists() {
                        report.skipped.push(e.id.clone());
                    } else {
                        fs::write(&path, &body)?;
                        report.written += 1;
                    }
                }
            }
        }

        Ok(report)
    }
}

/// Render a per-memory file with YAML frontmatter, Claude-style.
fn render_memory_file(native_type: &str, title: &str, body: &str) -> String {
    format!("---\ntype: {native_type}\ndescription: {title}\n---\n{body}")
}

/// Read one memory directory: MEMORY.md index (if present) + per-memory files.
fn read_memory_dir(dir: &Path, project: Option<&str>) -> Result<Vec<Entry>> {
    let mut out = vec![];
    let index = dir.join("MEMORY.md");
    if index.exists() {
        let body = memswap_core::text::read_text(&index).map_err(Error::Io)?;
        out.push(Entry {
            id: match project {
                Some(k) => format!("claude/project/{k}/index"),
                None => "claude/memory_index".into(),
            },
            kind: EntryKind::Index,
            title: match project {
                Some(_) => "Claude Code project memory index (MEMORY.md)".into(),
                None => "Claude Code global memory index (MEMORY.md)".into(),
            },
            body,
            scope: if project.is_some() {
                Scope::Project
            } else {
                Scope::Global
            },
            project: project.map(|s| s.to_string()),
            source: Source {
                harness: "claude".into(),
                path: Some(index.display().to_string()),
                updated_at: None,
                profile: None,
            },
            tags: vec!["index".into()],
            content_hash: String::new(),
        });
    }
    // Per-memory files: <type>_<slug>.md, skipping MEMORY.md itself.
    let mut files: Vec<_> = fs::read_dir(dir)
        .map_err(Error::Io)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension().is_some_and(|x| x == "md")
                && p.file_name().is_some_and(|n| n != "MEMORY.md")
        })
        .collect();
    files.sort();
    for p in files {
        if let Some(e) = read_one_memory(&p, project)? {
            out.push(e);
        }
    }
    Ok(out)
}

/// Advisory index-budget check (200 lines / 25 KiB). Exposed for tests.
pub fn index_over_budget(body: &str) -> bool {
    body.lines().count() > INDEX_LINE_CAP || body.len() > INDEX_BYTE_CAP
}
