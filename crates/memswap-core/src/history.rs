//! Git-like commit history over the store's INDEX state.
//!
//! Every mutating operation appends a `Commit` that binds:
//! - `tree_hash`  = blake3(canonical JSON of the entry snapshot),
//! - `commit_hash` = blake3(canonical JSON of the commit with `commit_hash` empty),
//! - `parent_hash` = previous tip ("" for the root commit).
//!
//! Commits live in `commits/<commit_hash>.json`; `refs/HEAD` points at the tip.
//! History is append-only: rewriting a past commit breaks the chain and is
//! reported by `verify`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::entry::Entry;
use crate::error::{Error, Result};
use crate::hash::blake3_hex;

/// One versioned snapshot of the store's index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    /// blake3 over the canonical serialization of this commit with
    /// `commit_hash` set to "".
    pub commit_hash: String,
    /// Previous tip; empty for the root commit.
    pub parent_hash: String,
    /// blake3 over the canonical JSON of `entries`.
    pub tree_hash: String,
    /// RFC3339 UTC timestamp of the write.
    pub timestamp: String,
    /// Human-readable operation ("export hermes (2 entries)", "write <id>").
    pub message: String,
    /// Full entry snapshot at this commit (stores are small; keeps every
    /// commit self-contained so `diff` never needs object archaeology).
    pub entries: Vec<Entry>,
}

impl Commit {
    /// Compute the commit hash over the canonical form (hash field empty).
    pub fn compute_hash(&self) -> String {
        let mut probe = self.clone();
        probe.commit_hash = String::new();
        let bytes = serde_json::to_vec(&probe).expect("commit serializes");
        blake3_hex(&bytes)
    }

    /// The entry state as an id -> content_hash map.
    pub fn state(&self) -> std::collections::BTreeMap<String, String> {
        self.entries
            .iter()
            .map(|e| (e.id.clone(), e.content_hash.clone()))
            .collect()
    }
}

/// Structural diff between two commits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffReport {
    pub from: String,
    pub to: String,
    /// Entry ids present in `to` but not `from`.
    pub added: Vec<String>,
    /// Entry ids present in `from` but not `to`.
    pub removed: Vec<String>,
    /// Entry ids present in both whose `content_hash` changed.
    pub changed: Vec<String>,
}

impl DiffReport {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty()
    }
}

pub(crate) fn commit_dir(dir: &Path) -> PathBuf {
    dir.join("commits")
}

pub(crate) fn head_path(dir: &Path) -> PathBuf {
    dir.join("refs").join("HEAD")
}

/// Read the current tip, if any.
pub fn head(dir: &Path) -> Result<Option<String>> {
    match fs::read_to_string(head_path(dir)) {
        Ok(s) => {
            let h = s.trim().to_string();
            if h.is_empty() {
                Ok(None)
            } else {
                Ok(Some(h))
            }
        }
        Err(_) => Ok(None),
    }
}

/// Append a commit capturing `entries` as the new state; returns the commit.
/// Callers must have already persisted INDEX/objects/refs.
pub fn append(
    dir: &Path,
    parent: Option<String>,
    message: &str,
    entries: &[Entry],
) -> Result<Commit> {
    let tree_hash = blake3_hex(&serde_json::to_vec(entries)?);
    let mut commit = Commit {
        commit_hash: String::new(),
        parent_hash: parent.unwrap_or_default(),
        tree_hash,
        timestamp: crate::store::now_iso8601(),
        message: message.to_string(),
        entries: entries.to_vec(),
    };
    commit.commit_hash = commit.compute_hash();

    let cdir = commit_dir(dir);
    fs::create_dir_all(&cdir)?;
    fs::write(
        cdir.join(&commit.commit_hash),
        serde_json::to_vec_pretty(&commit)?,
    )?;
    fs::write(head_path(dir), &commit.commit_hash)?;
    Ok(commit)
}

/// Walk the chain from HEAD (newest first).
pub fn log(dir: &Path) -> Result<Vec<Commit>> {
    let mut out = Vec::new();
    let mut cur = head(dir)?;
    while let Some(h) = cur {
        let path = commit_dir(dir).join(&h);
        let raw = fs::read(&path).map_err(|_| {
            Error::Invalid(format!(
                "commit {h} referenced by chain but missing on disk"
            ))
        })?;
        let commit: Commit = serde_json::from_slice(&raw)?;
        cur = if commit.parent_hash.is_empty() {
            None
        } else {
            Some(commit.parent_hash.clone())
        };
        out.push(commit);
    }
    Ok(out)
}

/// Resolve `HEAD`, `HEAD~N`, or a (possibly abbreviated) commit hash.
pub fn resolve(dir: &Path, spec: &str) -> Result<Commit> {
    let spec = spec.trim();
    if spec.eq_ignore_ascii_case("HEAD") {
        let h = head(dir)?.ok_or_else(|| Error::NotFound("no commits yet".into()))?;
        return load(dir, &h);
    }
    if let Some(rest) = spec.strip_prefix("HEAD~") {
        let n: usize = rest
            .parse()
            .map_err(|_| Error::Invalid(format!("bad revision spec '{spec}' (expected HEAD~N)")))?;
        let chain = log(dir)?;
        chain.get(n).cloned().ok_or_else(|| {
            Error::NotFound(format!(
                "HEAD~{n} does not exist (history has {} commits)",
                chain.len()
            ))
        })
    } else {
        // Full or abbreviated hash.
        let cdir = commit_dir(dir);
        let exact = cdir.join(spec);
        if exact.exists() {
            return load(dir, spec);
        }
        let mut matches = Vec::new();
        for entry in fs::read_dir(&cdir)?.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(spec) {
                matches.push(name);
            }
        }
        match matches.len() {
            1 => load(dir, &matches[0]),
            0 => Err(Error::NotFound(format!("no commit matching '{spec}'"))),
            _ => Err(Error::Invalid(format!(
                "abbreviated hash '{spec}' is ambiguous ({} matches)",
                matches.len()
            ))),
        }
    }
}

fn load(dir: &Path, hash: &str) -> Result<Commit> {
    let raw = fs::read(commit_dir(dir).join(hash))
        .map_err(|_| Error::NotFound(format!("commit {hash} not found")))?;
    Ok(serde_json::from_slice(&raw)?)
}

/// Diff two revisions (`None` = HEAD).
pub fn diff(dir: &Path, from: Option<&str>, to: Option<&str>) -> Result<DiffReport> {
    let from_spec = from.unwrap_or("HEAD~1");
    let to_spec = to.unwrap_or("HEAD");
    let a = resolve(dir, from_spec)?;
    let b = resolve(dir, to_spec)?;
    let sa = a.state();
    let sb = b.state();
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();
    for (id, h) in &sb {
        match sa.get(id) {
            None => added.push(id.clone()),
            Some(old) if old != h => changed.push(id.clone()),
            Some(_) => {}
        }
    }
    for id in sa.keys() {
        if !sb.contains_key(id) {
            removed.push(id.clone());
        }
    }
    Ok(DiffReport {
        from: a.commit_hash,
        to: b.commit_hash,
        added,
        removed,
        changed,
    })
}

/// Recompute every commit's hashes and parent links from HEAD down to the root.
/// Returns `(chain_ok, commits_checked)`. A broken chain (missing or corrupt
/// commit file, bad hash, broken parent link) is a `false`, never an error —
/// verification must be able to *report* tampering.
pub fn verify_chain(dir: &Path) -> Result<(bool, usize)> {
    let mut ok = true;
    let mut count = 0usize;
    let mut cur = head(dir)?;
    let mut last_parent: Option<String> = None;
    while let Some(h) = cur {
        let raw = match fs::read(commit_dir(dir).join(&h)) {
            Ok(r) => r,
            Err(_) => {
                ok = false;
                break;
            }
        };
        let commit: Commit = match serde_json::from_slice(&raw) {
            Ok(c) => c,
            Err(_) => {
                ok = false;
                break;
            }
        };
        if commit.compute_hash() != commit.commit_hash {
            ok = false;
        }
        if commit.tree_hash != blake3_hex(&serde_json::to_vec(&commit.entries)?) {
            ok = false;
        }
        last_parent = Some(commit.parent_hash.clone());
        cur = if commit.parent_hash.is_empty() {
            None
        } else {
            Some(commit.parent_hash.clone())
        };
        count += 1;
    }
    // The chain must terminate at a root commit; a dangling parent means a
    // missing commit file.
    if let Some(p) = last_parent {
        if !p.is_empty() {
            ok = false;
        }
    }
    Ok((ok, count))
}
