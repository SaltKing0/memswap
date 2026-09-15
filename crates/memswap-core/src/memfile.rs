//! `.memfile` transport archives: a whole store packed into one zip file.
//!
//! Canonical layout is always a directory; the `.memfile` zip is a lossless
//! transport form (send a store over chat, attach to an issue, archive it).
//! `pack` copies the store's files verbatim into the archive; `unpack`
//! restores them and the caller is expected to `mem verify` the result.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// Files that make up a store, in pack order. Directories are implicit.
const STORE_FILES: &[&str] = &["MANIFEST.json", "INDEX.json", "SIG"];

/// Walk a store directory and collect every regular file, relative to it.
/// Covers objects/, refs/, commits/ at any depth, plus the top-level files.
fn collect_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for name in STORE_FILES {
        let p = dir.join(name);
        if p.is_file() {
            files.push(p.clone());
        }
    }
    for sub in ["objects", "refs", "commits"] {
        let root = dir.join(sub);
        if !root.exists() {
            continue;
        }
        let mut stack = vec![root.clone()];
        while let Some(d) = stack.pop() {
            for entry in fs::read_dir(&d)?.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    stack.push(p);
                } else if p.is_file() {
                    files.push(p);
                }
            }
        }
    }
    if files.is_empty() {
        return Err(Error::Invalid(format!(
            "{} contains no store files (missing MANIFEST.json?)",
            dir.display()
        )));
    }
    Ok(files)
}

/// Pack a store directory into `<out>.memfile` (or `out` if it already ends
/// with `.memfile`). Overwrites an existing archive.
pub fn pack(dir: &Path, out: &Path) -> Result<PathBuf> {
    let out = if out.extension().and_then(|e| e.to_str()) == Some("memfile") {
        out.to_path_buf()
    } else {
        out.with_extension("memfile")
    };
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let files = collect_files(dir)?;
    let file = File::create(&out)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for path in &files {
        let rel = path.strip_prefix(dir).map_err(|_| {
            Error::Invalid(format!("{} is not under {}", path.display(), dir.display()))
        })?;
        zip.start_file(rel.to_string_lossy().to_string(), opts)
            .map_err(|e| Error::Invalid(format!("zip: {e}")))?;
        let mut f = File::open(path)?;
        std::io::copy(&mut f, &mut zip).map_err(|e| Error::Invalid(format!("zip: {e}")))?;
    }
    zip.finish()
        .map_err(|e| Error::Invalid(format!("zip: {e}")))?;
    Ok(out)
}

/// Unpack a `.memfile` archive into `dir` (created if missing; must be empty
/// or a fresh path — refuses to clobber a populated directory).
pub fn unpack(archive: &Path, dir: &Path) -> Result<()> {
    if dir.exists() && dir.read_dir()?.next().is_some() {
        return Err(Error::Invalid(format!(
            "{} already exists and is not empty — unpack into a fresh directory",
            dir.display()
        )));
    }
    fs::create_dir_all(dir)?;
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::Invalid(format!("zip: {e}")))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| Error::Invalid(format!("zip: {e}")))?;
        let name = entry.name().to_string();
        // Zip-slip guard: refuse anything escaping the target dir.
        if name.contains("..") || name.starts_with('/') {
            return Err(Error::Invalid(format!(
                "archive contains unsafe path '{name}'"
            )));
        }
        let target = dir.join(&name);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = File::create(&target)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    if !dir.join("MANIFEST.json").is_file() {
        return Err(Error::Invalid(format!(
            "{} is not a memswap archive (no MANIFEST.json inside)",
            archive.display()
        )));
    }
    Ok(())
}

/// Read an `.memfile` archive's INDEX.json without unpacking to disk.
pub fn peek_index(archive: &Path) -> Result<Vec<crate::Entry>> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::Invalid(format!("zip: {e}")))?;
    let mut entry = zip
        .by_name("INDEX.json")
        .map_err(|_| Error::Invalid(format!("{} has no INDEX.json", archive.display())))?;
    let mut buf = String::new();
    entry.read_to_string(&mut buf)?;
    Ok(serde_json::from_str(&buf)?)
}

// Re-export Write so `std::io::copy` bounds resolve for ZipWriter.
#[allow(unused_imports)]
use std::io::Write as _;
