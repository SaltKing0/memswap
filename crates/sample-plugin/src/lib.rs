//! Sample dynamic plugin: a toy "notes" harness proving the plugin ABI.
//! Exports the `memswap_plugin_*` C ABI; reads/writes plain markdown files
//! from `<home>/notes/*.md`. Built as a cdylib alongside the loader tests.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// ABI version this plugin implements.
const ABI_VERSION: i32 = 1;

static NAME_CSTR: &[u8] = b"notes\0";

#[no_mangle]
pub extern "C" fn memswap_plugin_name() -> *const c_char {
    NAME_CSTR.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn memswap_plugin_version() -> i32 {
    ABI_VERSION
}

/// # Safety
/// `home` must be a valid NUL-terminated C string; `out` must be writable.
#[no_mangle]
pub unsafe extern "C" fn memswap_plugin_detect(home: *const c_char, out: *mut *mut c_char) -> i32 {
    if home.is_null() || out.is_null() {
        return 1;
    }
    let home = CStr::from_ptr(home).to_string_lossy();
    let dir = std::path::Path::new(home.as_ref()).join("notes");
    if !dir.is_dir() {
        return 1; // not detected
    }
    let json = format!(
        r#"{{"detected":[{{"label":"notes_dir","path":"{}"}}]}}"#,
        dir.display()
    );
    match CString::new(json) {
        Ok(c) => {
            unsafe { *out = c.into_raw() };
            0
        }
        Err(_) => 1,
    }
}

/// # Safety
/// `home` must be a valid NUL-terminated C string; `out` must be writable.
#[no_mangle]
pub unsafe extern "C" fn memswap_plugin_read(home: *const c_char, out: *mut *mut c_char) -> i32 {
    if home.is_null() || out.is_null() {
        return 1;
    }
    let home = CStr::from_ptr(home).to_string_lossy();
    let dir = std::path::Path::new(home.as_ref()).join("notes");
    let mut entries = vec![];
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut files: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for p in files {
            if p.extension().is_none_or(|x| x != "md") {
                continue;
            }
            let body = match std::fs::read_to_string(&p) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let name = p
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            entries.push(serde_json::json!({
                "id": format!("notes/{name}"),
                "kind": "fact",
                "title": name,
                "body": body,
                "scope": "global",
                "source": { "harness": "notes", "path": p.display().to_string() },
                "tags": [],
                "content_hash": "",
            }));
        }
    }
    match CString::new(serde_json::to_string(&entries).unwrap_or_default()) {
        Ok(c) => {
            unsafe { *out = c.into_raw() };
            0
        }
        Err(_) => 1,
    }
}

/// # Safety
/// All pointer args must be valid NUL-terminated C strings; `out` writable.
#[no_mangle]
pub unsafe extern "C" fn memswap_plugin_write(
    home: *const c_char,
    entries_json: *const c_char,
    strategy: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    if home.is_null() || entries_json.is_null() || strategy.is_null() || out.is_null() {
        return 1;
    }
    let home = CStr::from_ptr(home).to_string_lossy();
    let strat = CStr::from_ptr(strategy).to_string_lossy();
    let dir = std::path::Path::new(home.as_ref()).join("notes");
    if std::fs::create_dir_all(&dir).is_err() {
        return 1;
    }
    let Ok(entries) = serde_json::from_str::<serde_json::Value>(
        CStr::from_ptr(entries_json).to_string_lossy().as_ref(),
    ) else {
        return 1;
    };
    let arr = entries.as_array().cloned().unwrap_or_default();
    let mut written = 0;
    let mut skipped = vec![];
    for e in &arr {
        let Some(id) = e.get("id").and_then(|x| x.as_str()) else {
            continue;
        };
        let Some(body) = e.get("body").and_then(|x| x.as_str()) else {
            continue;
        };
        let name = id.strip_prefix("notes/").unwrap_or(id);
        // Sanitize: no path separators.
        if name.contains('/') || name.contains("..") {
            skipped.push(id.to_string());
            continue;
        }
        let path = dir.join(format!("{name}.md"));
        let exists = path.exists();
        match strat.as_ref() {
            "keep" if exists => skipped.push(id.to_string()),
            "merge" if exists => skipped.push(id.to_string()),
            _ => {
                if std::fs::write(&path, body).is_ok() {
                    written += 1;
                } else {
                    skipped.push(id.to_string());
                }
            }
        }
    }
    let report = serde_json::json!({ "written": written, "truncated": [], "skipped": skipped });
    match CString::new(report.to_string()) {
        Ok(c) => {
            unsafe { *out = c.into_raw() };
            0
        }
        Err(_) => 1,
    }
}

/// # Safety
/// `ptr` must be a pointer previously handed out by this plugin.
#[no_mangle]
pub unsafe extern "C" fn memswap_plugin_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}
