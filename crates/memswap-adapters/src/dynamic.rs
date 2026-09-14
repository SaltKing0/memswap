//! Dynamic plugin loading (dlopen). External adapters are shared libraries
//! exporting a small C ABI with JSON as the wire format — no ABI drift when
//! the entry model evolves:
//!
//! ```c
//! const char* memswap_plugin_name(void);          // static, do not free
//! int         memswap_plugin_version(void);       // ABI version, currently 1
//! int         memswap_plugin_detect(const char* home, char** out_json);
//! int         memswap_plugin_read (const char* home, char** out_json);
//! int         memswap_plugin_write(const char* home, const char* entries_json,
//!                                  const char* strategy, char** out_json);
//! void        memswap_plugin_free(char* ptr);
//! ```
//!
//! `detect`/`read`/`write` return 0 on success and set `*out_json` to a
//! malloc'd NUL-terminated UTF-8 JSON string the host frees with
//! `memswap_plugin_free`. Non-zero return = error (message may be absent).
//! Plugins are loaded from `~/.config/memswap/adapters/*.so` (or
//! `$MEMSWAP_ADAPTERS`, colon-separated).

use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};

use memswap_core::entry::{Entry, EntryKind, Scope, Source};
use memswap_core::{Error, Result};

use crate::model::{Adapter, HarnessContext, MergeStrategy, WriteReport};

/// Current plugin ABI version.
pub const PLUGIN_ABI_VERSION: i32 = 1;

type PluginName = unsafe extern "C" fn() -> *const c_char;
type PluginVersion = unsafe extern "C" fn() -> i32;
type PluginDetect = unsafe extern "C" fn(*const c_char, *mut *mut c_char) -> i32;
type PluginRead = unsafe extern "C" fn(*const c_char, *mut *mut c_char) -> i32;
type PluginWrite =
    unsafe extern "C" fn(*const c_char, *const c_char, *const c_char, *mut *mut c_char) -> i32;
type PluginFree = unsafe extern "C" fn(*mut c_char);

/// A loaded dynamic adapter.
pub struct DynamicAdapter {
    /// Kept alive for the adapter's lifetime: dropping it would unload the
    /// code the fn pointers below point into.
    #[allow(dead_code)]
    lib: libloading::Library,
    name: String,
    detect: PluginDetect,
    read: PluginRead,
    write: PluginWrite,
    free: PluginFree,
}

// The dlopen'd symbols are process-global; sharing &DynamicAdapter across
// threads is safe as long as the plugin itself is thread-safe (documented
// contract). libloading::Library is Send on all supported platforms.
unsafe impl Send for DynamicAdapter {}
unsafe impl Sync for DynamicAdapter {}

impl DynamicAdapter {
    /// Load a plugin from a shared-library path and validate its ABI.
    pub fn load(path: &Path) -> Result<DynamicAdapter> {
        unsafe {
            let lib = libloading::Library::new(path).map_err(|e| {
                Error::Adapter(format!("failed to load plugin {}: {e}", path.display()))
            })?;
            let name_fn: libloading::Symbol<PluginName> =
                lib.get(b"memswap_plugin_name").map_err(|e| {
                    Error::Adapter(format!(
                        "plugin {}: missing memswap_plugin_name: {e}",
                        path.display()
                    ))
                })?;
            let version_fn: libloading::Symbol<PluginVersion> =
                lib.get(b"memswap_plugin_version").map_err(|e| {
                    Error::Adapter(format!(
                        "plugin {}: missing memswap_plugin_version: {e}",
                        path.display()
                    ))
                })?;
            let name = CStr::from_ptr(name_fn())
                .to_str()
                .map_err(|_| Error::Adapter("plugin name is not valid UTF-8".into()))?
                .to_string();
            let version = version_fn();
            if version != PLUGIN_ABI_VERSION {
                return Err(Error::Adapter(format!(
                    "plugin '{name}' reports ABI v{version}, loader supports v{PLUGIN_ABI_VERSION}"
                )));
            }
            let detect: libloading::Symbol<PluginDetect> =
                lib.get(b"memswap_plugin_detect").map_err(|e| {
                    Error::Adapter(format!("plugin {name}: missing memswap_plugin_detect: {e}"))
                })?;
            let read: libloading::Symbol<PluginRead> =
                lib.get(b"memswap_plugin_read").map_err(|e| {
                    Error::Adapter(format!("plugin {name}: missing memswap_plugin_read: {e}"))
                })?;
            let write: libloading::Symbol<PluginWrite> =
                lib.get(b"memswap_plugin_write").map_err(|e| {
                    Error::Adapter(format!("plugin {name}: missing memswap_plugin_write: {e}"))
                })?;
            let free: libloading::Symbol<PluginFree> =
                lib.get(b"memswap_plugin_free").map_err(|e| {
                    Error::Adapter(format!("plugin {name}: missing memswap_plugin_free: {e}"))
                })?;
            // Detach the raw fn pointers before moving `lib` into the struct
            // (Symbol borrows the Library, so deref first).
            let detect = *detect;
            let read = *read;
            let write = *write;
            let free = *free;
            Ok(DynamicAdapter {
                lib,
                name,
                detect,
                read,
                write,
                free,
            })
        }
    }

    fn call_detect(&self, home: &Path) -> Option<serde_json::Value> {
        let c_home = match CString::new(home.to_string_lossy().as_ref()) {
            Ok(s) => s,
            Err(_) => return None,
        };
        let mut out: *mut c_char = std::ptr::null_mut();
        let rc = unsafe { (self.detect)(c_home.as_ptr(), &mut out) };
        if rc != 0 || out.is_null() {
            return None;
        }
        let json = take_c_string(out, self.free);
        serde_json::from_str(&json).ok()
    }

    fn call_read(&self, home: &Path) -> Result<Vec<Entry>> {
        let c_home = CString::new(home.to_string_lossy().as_ref())
            .map_err(|_| Error::Invalid("home path contains NUL".into()))?;
        let mut out: *mut c_char = std::ptr::null_mut();
        let rc = unsafe { (self.read)(c_home.as_ptr(), &mut out) };
        if rc != 0 {
            return Err(Error::Adapter(format!(
                "plugin '{}' read failed (rc={rc})",
                self.name
            )));
        }
        let json = take_c_string(out, self.free);
        parse_entries(&json)
    }

    fn call_write(
        &self,
        home: &Path,
        entries: &[Entry],
        strategy: MergeStrategy,
    ) -> Result<WriteReport> {
        let c_home = CString::new(home.to_string_lossy().as_ref())
            .map_err(|_| Error::Invalid("home path contains NUL".into()))?;
        let entries_json =
            serde_json::to_string(&entries.iter().map(entry_to_json).collect::<Vec<_>>())
                .map_err(Error::Json)?;
        let strategy_str = match strategy {
            MergeStrategy::Replace => "replace",
            MergeStrategy::Merge => "merge",
            MergeStrategy::Keep => "keep",
        };
        let c_entries = CString::new(entries_json)
            .map_err(|_| Error::Invalid("entries JSON contains NUL".into()))?;
        let c_strategy = CString::new(strategy_str).unwrap();
        let mut out: *mut c_char = std::ptr::null_mut();
        let rc = unsafe {
            (self.write)(
                c_home.as_ptr(),
                c_entries.as_ptr(),
                c_strategy.as_ptr(),
                &mut out,
            )
        };
        if rc != 0 {
            return Err(Error::Adapter(format!(
                "plugin '{}' write failed (rc={rc})",
                self.name
            )));
        }
        let json = take_c_string(out, self.free);
        parse_write_report(&json)
    }
}

/// Copy out the JSON string and hand ownership back to the plugin.
fn take_c_string(ptr: *mut c_char, free: PluginFree) -> String {
    unsafe {
        let s = CStr::from_ptr(ptr).to_string_lossy().into_owned();
        (free)(ptr);
        s
    }
}

// --- JSON <-> entry model (the ABI wire format) ---

fn entry_to_json(e: &Entry) -> serde_json::Value {
    serde_json::json!({
        "id": e.id,
        "kind": e.kind,
        "title": e.title,
        "body": e.body,
        "scope": e.scope,
        "project": e.project,
        "source": {
            "harness": e.source.harness,
            "path": e.source.path,
            "updated_at": e.source.updated_at,
            "profile": e.source.profile,
        },
        "tags": e.tags,
        "content_hash": e.content_hash,
    })
}

fn parse_entries(json: &str) -> Result<Vec<Entry>> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| Error::Adapter(format!("plugin returned bad JSON: {e}")))?;
    let arr = v
        .as_array()
        .ok_or_else(|| Error::Adapter("plugin read must return a JSON array".into()))?;
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        out.push(entry_from_json(item)?);
    }
    Ok(out)
}

fn entry_from_json(v: &serde_json::Value) -> Result<Entry> {
    let get = |k: &str| {
        v.get(k)
            .and_then(|x| x.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::Adapter(format!("plugin entry missing '{k}'")))
    };
    let kind = match get("kind")?.as_str() {
        "instruction" => EntryKind::Instruction,
        "fact" => EntryKind::Fact,
        "project" => EntryKind::Project,
        "user" => EntryKind::User,
        "index" => EntryKind::Index,
        _ => EntryKind::Other,
    };
    let scope = match get("scope")?.as_str() {
        "project" => Scope::Project,
        _ => Scope::Global,
    };
    let src = v.get("source").cloned().unwrap_or_default();
    Ok(Entry {
        id: get("id")?,
        kind,
        title: get("title")?,
        body: get("body")?,
        scope,
        project: v.get("project").and_then(|x| x.as_str()).map(String::from),
        source: Source {
            harness: src
                .get("harness")
                .and_then(|x| x.as_str())
                .unwrap_or("plugin")
                .to_string(),
            path: src.get("path").and_then(|x| x.as_str()).map(String::from),
            updated_at: src
                .get("updated_at")
                .and_then(|x| x.as_str())
                .map(String::from),
            profile: src
                .get("profile")
                .and_then(|x| x.as_str())
                .map(String::from),
        },
        tags: v
            .get("tags")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        content_hash: v
            .get("content_hash")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
    })
}

fn parse_write_report(json: &str) -> Result<WriteReport> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| Error::Adapter(format!("plugin returned bad JSON: {e}")))?;
    Ok(WriteReport {
        written: v.get("written").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
        truncated: str_vec(v.get("truncated")),
        skipped: str_vec(v.get("skipped")),
    })
}

fn str_vec(v: Option<&serde_json::Value>) -> Vec<String> {
    v.and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Where plugin shared libraries are discovered.
pub fn plugin_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![];
    if let Ok(var) = std::env::var("MEMSWAP_ADAPTERS") {
        for p in var.split(':').filter(|s| !s.is_empty()) {
            dirs.push(PathBuf::from(p));
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(
            PathBuf::from(home)
                .join(".config")
                .join("memswap")
                .join("adapters"),
        );
    }
    dirs
}

/// Discover and load every plugin found in the plugin directories.
pub fn load_all() -> Vec<DynamicAdapter> {
    let mut out = vec![];
    for dir in plugin_dirs() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            let is_lib = p
                .extension()
                .is_some_and(|ext| ext == "so" || ext == "dylib" || ext == "dll");
            if is_lib {
                if let Ok(a) = DynamicAdapter::load(&p) {
                    out.push(a);
                }
            }
        }
    }
    out
}

impl Adapter for DynamicAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn detect(&self, home: &Path) -> Option<HarnessContext> {
        let v = self.call_detect(home)?;
        let detected = v.get("detected").and_then(|x| x.as_array()).map(|a| {
            a.iter()
                .filter_map(|d| {
                    let label = d.get("label")?.as_str()?;
                    let path = d.get("path")?.as_str()?;
                    Some((label.to_string(), PathBuf::from(path)))
                })
                .collect::<Vec<_>>()
        })?;
        Some(HarnessContext {
            name: self.name.clone(),
            home: home.to_path_buf(),
            profile: None,
            detected,
        })
    }

    fn read(&self, ctx: &HarnessContext) -> Result<Vec<Entry>> {
        self.call_read(&ctx.home)
    }

    fn write(
        &self,
        ctx: &HarnessContext,
        entries: &[Entry],
        strategy: MergeStrategy,
    ) -> Result<WriteReport> {
        self.call_write(&ctx.home, entries, strategy)
    }
}
