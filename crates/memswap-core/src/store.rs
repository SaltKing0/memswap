use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::entry::Entry;
use crate::error::{Error, Result};
use crate::hash::blake3_hex;
use crate::history;
use crate::manifest::Manifest;

pub fn now_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Compact UTC timestamp (RFC3339 without sub-second).
    format!("{secs}Z")
}

/// A memswap store: MANIFEST.json, INDEX.json, objects/, refs/, commits/.
#[derive(Debug, Clone)]
pub struct Store {
    pub dir: PathBuf,
}

/// Result of `verify` — structural, chain, and signature report.
#[derive(Debug, Clone)]
pub struct VerifyReport {
    pub entries: usize,
    pub objects_ok: usize,
    pub refs_ok: usize,
    pub manifest_ok: bool,
    /// Commit chain intact (hashes recompute, parents link, HEAD binds).
    pub chain_ok: bool,
    /// Number of commits in the chain.
    pub commits: usize,
    /// `None` = store is unsigned; `Some(b)` = SIG present and valid/invalid.
    pub signature_ok: Option<bool>,
    pub ok: bool,
}

impl Store {
    pub fn open(dir: &Path) -> Result<Store> {
        if !dir.join("MANIFEST.json").exists() {
            return Err(Error::NotFound(format!(
                "{} is not a memswap store (no MANIFEST.json)",
                dir.display()
            )));
        }
        Ok(Store {
            dir: dir.to_path_buf(),
        })
    }

    /// Create a new empty store at `dir`.
    pub fn init(
        dir: &Path,
        package_id: &str,
        harness_origin: &str,
        profile: Option<String>,
    ) -> Result<Store> {
        if dir.exists() && dir.read_dir()?.next().is_some() {
            return Err(Error::Invalid(format!("{} is not empty", dir.display())));
        }
        fs::create_dir_all(dir.join("objects"))?;
        fs::create_dir_all(dir.join("refs"))?;
        let manifest = Manifest::empty(package_id, harness_origin, profile);
        let store = Store {
            dir: dir.to_path_buf(),
        };
        store.write_manifest(&manifest)?;
        store.write_index(&[], "init")?;
        Ok(store)
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.dir.join("MANIFEST.json")
    }
    pub fn index_path(&self) -> PathBuf {
        self.dir.join("INDEX.json")
    }
    pub fn objects_dir(&self) -> PathBuf {
        self.dir.join("objects")
    }
    pub fn refs_dir(&self) -> PathBuf {
        self.dir.join("refs")
    }

    fn read_manifest(&self) -> Result<Manifest> {
        let raw = fs::read(self.manifest_path())?;
        Ok(serde_json::from_slice(&raw)?)
    }

    fn write_manifest(&self, m: &Manifest) -> Result<()> {
        let raw = serde_json::to_vec_pretty(m)?;
        fs::write(self.manifest_path(), raw)?;
        Ok(())
    }

    fn read_index(&self) -> Result<Vec<Entry>> {
        let raw = fs::read(self.index_path())?;
        Ok(serde_json::from_slice(&raw)?)
    }

    /// Write the index, re-bind the manifest, and append a commit capturing
    /// the new state. Every mutating path funnels through here.
    fn write_index(&self, entries: &[Entry], message: &str) -> Result<()> {
        let raw = serde_json::to_vec_pretty(entries)?;
        let hash = blake3_hex(&raw);
        fs::write(self.index_path(), &raw)?;

        // Re-bind the manifest to the new index and chain tip.
        let parent = history::head(&self.dir)?;
        let mut m = self.read_manifest()?;
        m.index_hash = hash;
        m.head_hash = String::new(); // rebound below after the commit exists
        self.write_manifest(&m)?;

        let commit = history::append(&self.dir, parent, message, entries)?;
        let mut m = self.read_manifest()?;
        m.head_hash = commit.commit_hash;
        self.write_manifest(&m)?;
        Ok(())
    }

    /// Write (or replace) an entry. Stores the body as a content-addressed
    /// object, updates the ref, rewrites INDEX.json, and appends a commit.
    pub fn write_entry(&self, entry: &Entry) -> Result<()> {
        let hash = entry.compute_hash();
        if entry.content_hash.is_empty() {
            let mut e = entry.clone();
            e.content_hash = hash.clone();
            return self.write_entry(&e);
        }
        if entry.content_hash != hash {
            return Err(Error::Invalid(format!(
                "entry {} content_hash {} does not match body hash {}",
                entry.id, entry.content_hash, hash
            )));
        }
        self.store_object(entry)?;
        let mut entries = self.read_index()?;
        entries.retain(|e| e.id != entry.id);
        entries.push(entry.clone());
        self.write_index(&entries, &format!("write {}", entry.id))?;
        Ok(())
    }

    /// Replace the whole index with the given entries (used by import/export).
    /// Assigns content_hash when the adapter left it empty.
    pub fn replace_entries(&self, entries: &[Entry]) -> Result<()> {
        let mut normalized = Vec::with_capacity(entries.len());
        for e in entries {
            let mut e = e.clone();
            if e.content_hash.is_empty() {
                e.content_hash = e.compute_hash();
            } else if e.content_hash != e.compute_hash() {
                return Err(Error::Invalid(format!(
                    "entry {} content_hash mismatch",
                    e.id
                )));
            }
            self.store_object(&e)?;
            normalized.push(e);
        }
        let n = normalized.len();
        self.write_index(&normalized, &format!("snapshot ({n} entries)"))?;
        Ok(())
    }

    /// Persist body object + ref for one normalized entry.
    fn store_object(&self, e: &Entry) -> Result<()> {
        let obj = self.objects_dir().join(&e.content_hash);
        if !obj.exists() {
            fs::write(obj, e.body.as_bytes())?;
        }
        fs::write(self.refs_dir().join(sanitize_id(&e.id)), &e.content_hash)?;
        Ok(())
    }

    /// Load all entries with bodies resolved from the object store.
    pub fn read_entries(&self) -> Result<Vec<Entry>> {
        let index = self.read_index()?;
        let mut out = Vec::with_capacity(index.len());
        for mut e in index {
            let obj = self.objects_dir().join(&e.content_hash);
            e.body = fs::read_to_string(&obj).map_err(|_| {
                Error::NotFound(format!(
                    "object {} missing for entry {}",
                    e.content_hash, e.id
                ))
            })?;
            out.push(e);
        }
        Ok(out)
    }

    /// Verify structural integrity, the commit chain, and the signature.
    pub fn verify(&self) -> Result<VerifyReport> {
        let manifest = self.read_manifest()?;
        let index_raw = fs::read(self.index_path())?;
        let manifest_ok = manifest.index_hash == blake3_hex(&index_raw);
        let entries = self.read_index()?;
        let mut objects_ok = 0;
        let mut refs_ok = 0;
        let mut all_ok = manifest_ok;
        for e in &entries {
            let obj = self.objects_dir().join(&e.content_hash);
            match fs::read(&obj) {
                Ok(bytes) if blake3_hex(&bytes) == e.content_hash => objects_ok += 1,
                _ => all_ok = false,
            }
            let refp = self.refs_dir().join(sanitize_id(&e.id));
            match fs::read_to_string(&refp) {
                Ok(h) if h.trim() == e.content_hash && obj.exists() => refs_ok += 1,
                _ => all_ok = false,
            }
        }

        // Commit chain: recompute every hash and parent link.
        let (chain_ok, commits) = history::verify_chain(&self.dir)?;
        if !chain_ok {
            all_ok = false;
        }
        // The manifest's head_hash must bind to the chain tip.
        if let Some(tip) = history::head(&self.dir)? {
            if manifest.head_hash != tip {
                all_ok = false;
            }
        } else if !manifest.head_hash.is_empty() {
            all_ok = false;
        }

        // Optional detached signature.
        let signature_ok = crate::sign::verify_signature(&self.dir)?;
        if signature_ok == Some(false) {
            all_ok = false;
        }

        Ok(VerifyReport {
            entries: entries.len(),
            objects_ok,
            refs_ok,
            manifest_ok,
            chain_ok,
            commits,
            signature_ok,
            ok: all_ok && objects_ok == entries.len() && refs_ok == entries.len(),
        })
    }
}

/// Filesystem-safe id (ids may contain '/', e.g. "hermes/user").
fn sanitize_id(id: &str) -> String {
    id.replace('/', "__")
}
