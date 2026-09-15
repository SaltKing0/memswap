//! Schema migrations.
//!
//! A store declares its shape in `MANIFEST.json#schema_version`. Migration is
//! explicit and forward-only: `mem migrate --to N` walks the registered steps
//! in order, rewrites the affected state, and appends a commit so the change
//! is visible in `mem log` rather than happening silently behind the user's
//! back.
//!
//! ## v1 → v2: canonical line endings
//!
//! v1 stores were written before entry bodies were canonicalised to LF. A body
//! that came off a Windows filesystem therefore carried CRLF, and because
//! `content_hash` is a hash of the body bytes, the same memory hashed
//! differently per platform — which defeats the point of an interchange
//! format. v2 rewrites every body to LF and rehashes it, so a v2 store is
//! byte-identical wherever it was produced.

use std::fs;

use crate::error::{Error, Result};
use crate::manifest::CURRENT_SCHEMA_VERSION;
use crate::store::Store;
use crate::text::normalize_newlines;

/// One registered migration step.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    pub from: u32,
    pub to: u32,
    pub description: &'static str,
}

/// Every step this build knows, in ascending order.
pub const MIGRATIONS: &[Migration] = &[Migration {
    from: 1,
    to: 2,
    description: "canonicalise entry bodies to LF line endings",
}];

/// What a migration did (or would do, with `dry_run`).
#[derive(Debug, Clone)]
pub struct MigrateReport {
    pub from: u32,
    pub to: u32,
    pub entries: usize,
    /// Entries whose body changed, i.e. whose `content_hash` changed.
    pub entries_rewritten: usize,
    /// Content-addressed objects no longer referenced after the rewrite.
    pub objects_pruned: usize,
    pub applied: Vec<String>,
    pub dry_run: bool,
    /// True when the store was already at `to` and nothing was touched.
    pub noop: bool,
}

/// Resolve the ordered steps from `from` to `to`, or explain why they cannot
/// be resolved. Refuses downgrades and unknown/future versions rather than
/// guessing.
pub fn plan(from: u32, to: u32) -> Result<Vec<Migration>> {
    if from == to {
        return Ok(Vec::new());
    }
    if to > CURRENT_SCHEMA_VERSION {
        return Err(Error::Invalid(format!(
            "schema v{to} is newer than this build understands (v{CURRENT_SCHEMA_VERSION}); upgrade memswap"
        )));
    }
    if to < from {
        return Err(Error::Invalid(format!(
            "cannot migrate backwards (v{from} -> v{to}); migrations are forward-only"
        )));
    }
    let mut steps = Vec::new();
    let mut at = from;
    while at < to {
        let step = MIGRATIONS
            .iter()
            .find(|m| m.from == at)
            .copied()
            .ok_or_else(|| {
                Error::Invalid(format!("no migration registered from schema v{at} (store is from a newer or unknown build)"))
            })?;
        steps.push(step);
        at = step.to;
    }
    Ok(steps)
}

/// Migrate `store` to `to` (default: this build's version). With `dry_run`,
/// reports what would change and writes nothing.
pub fn migrate(store: &Store, to: u32, dry_run: bool) -> Result<MigrateReport> {
    let from = store.manifest()?.schema_version;
    let steps = plan(from, to)?;

    if steps.is_empty() {
        return Ok(MigrateReport {
            from,
            to,
            entries: store.read_entries()?.len(),
            entries_rewritten: 0,
            objects_pruned: 0,
            applied: Vec::new(),
            dry_run,
            noop: true,
        });
    }

    // v1 -> v2 rewrites bodies; the entry set itself is unchanged.
    let entries = store.read_entries()?;
    let total = entries.len();
    let mut rewritten = Vec::with_capacity(total);
    let mut changed = 0usize;
    for mut e in entries {
        let canonical = normalize_newlines(&e.body);
        if canonical != e.body {
            e.body = canonical;
            e.content_hash = String::new(); // recomputed on write
            changed += 1;
        }
        rewritten.push(e);
    }

    let before: std::collections::HashSet<String> = store
        .objects_dir()
        .read_dir()?
        .flatten()
        .filter_map(|d| d.file_name().into_string().ok())
        .collect();

    if dry_run {
        return Ok(MigrateReport {
            from,
            to,
            entries: total,
            entries_rewritten: changed,
            objects_pruned: 0,
            applied: steps.iter().map(describe).collect(),
            dry_run: true,
            noop: false,
        });
    }

    // Rewriting through the store keeps every invariant: objects are written
    // content-addressed, the index is rebound, and a commit is appended.
    store.replace_entries(&rewritten)?;
    store.set_schema_version(to)?;

    // Drop objects the rewrite orphaned. They are unreferenced by definition,
    // so this cannot break `verify` — it just stops a migrated store from
    // carrying the pre-migration bytes forever.
    let live: std::collections::HashSet<String> =
        rewritten.iter().map(|e| e.content_hash.clone()).collect();
    let mut pruned = 0usize;
    for name in before {
        if !live.contains(&name) && fs::remove_file(store.objects_dir().join(&name)).is_ok() {
            pruned += 1;
        }
    }

    Ok(MigrateReport {
        from,
        to,
        entries: total,
        entries_rewritten: changed,
        objects_pruned: pruned,
        applied: steps.iter().map(describe).collect(),
        dry_run: false,
        noop: false,
    })
}

fn describe(m: &Migration) -> String {
    format!("v{} -> v{}: {}", m.from, m.to, m.description)
}
