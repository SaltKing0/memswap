//! memswap C ABI (cdylib) — the M4 FFI surface.
//!
//! Every string-bearing call follows one contract:
//! - inputs: NUL-terminated UTF-8 C strings;
//! - outputs: a malloc'd NUL-terminated UTF-8 C string written to `*out`,
//!   owned by the caller and released with [`memswap_free`];
//! - return: 0 = success, 1 = error (details in `*out` as `{"error":...}`),
//!   4 = verification failure.
//!
//! JSON is the wire format for structured data (entries, reports), so
//! bindings in Python (ctypes) and Node (napi-rs / ffi) stay thin.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::Path;

use memswap_core::{Entry, Store};

/// NUL-terminated version string for the C ABI.
static VERSION_CSTR: &[u8] = b"0.1.0\0";

/// Returns the memswap library version as a NUL-terminated C string.
/// The returned pointer is static; do not free it.
///
/// # Safety
/// No inputs; returns a static pointer valid for the program's lifetime.
#[no_mangle]
pub unsafe extern "C" fn memswap_version() -> *const c_char {
    VERSION_CSTR.as_ptr() as *const c_char
}

/// Free a buffer returned by any `memswap_*` call that hands ownership out.
///
/// # Safety
/// `ptr` must be a pointer previously returned by a memswap out-parameter
/// and not already freed.
#[no_mangle]
pub unsafe extern "C" fn memswap_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}

/// Verify a store at the given path.
/// Returns 0 = clean, 4 = verification/tamper failure, 1 = error.
///
/// # Safety
/// `path` must be a valid NUL-terminated C string for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn memswap_verify_store(path: *const c_char) -> i32 {
    if path.is_null() {
        return 1;
    }
    let p = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    match Store::open(Path::new(p)) {
        Ok(store) => match store.verify() {
            Ok(r) if r.ok => 0,
            Ok(_) => 4,
            Err(_) => 1,
        },
        Err(_) => 1,
    }
}

/// Return a JSON document describing the store at `path`.
/// On success returns 0 and sets *out (free with memswap_free).
///
/// # Safety
/// `path` must be a valid NUL-terminated C string; `out` must be a valid
/// `*mut *mut c_char` the caller can write to.
#[no_mangle]
pub unsafe extern "C" fn memswap_store_info(path: *const c_char, out: *mut *mut c_char) -> i32 {
    if path.is_null() || out.is_null() {
        return 1;
    }
    let p = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    let doc = match Store::open(Path::new(p)) {
        Ok(store) => match store.verify() {
            Ok(r) => format!(
                r#"{{"ok":{},"entries":{},"objects_ok":{},"refs_ok":{},"manifest_ok":{}}}"#,
                r.ok, r.entries, r.objects_ok, r.refs_ok, r.manifest_ok
            ),
            Err(e) => format!(r#"{{"error":"{e}"}}"#),
        },
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    };
    match CString::new(doc) {
        Ok(c) => {
            *out = c.into_raw();
            0
        }
        Err(_) => 1,
    }
}

/// Open (or create) a store and return all entries as a JSON array.
/// `create` != 0 allows initializing an empty store at `path`.
///
/// # Safety
/// `path` valid NUL-terminated C string; `out` writable out-parameter.
#[no_mangle]
pub unsafe extern "C" fn memswap_read_entries(
    path: *const c_char,
    create: i32,
    out: *mut *mut c_char,
) -> i32 {
    if path.is_null() || out.is_null() {
        return 1;
    }
    let p = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    let dir = Path::new(p);
    let store = match Store::open(dir) {
        Ok(s) => s,
        Err(_) if create != 0 => match Store::init(dir, "ffi", "ffi", None) {
            Ok(s) => s,
            Err(e) => return fail(&format!("init failed: {e}"), out),
        },
        Err(e) => return fail(&e.to_string(), out),
    };
    match store.read_entries() {
        Ok(entries) => emit(&entries, out),
        Err(e) => fail(&e.to_string(), out),
    }
}

/// Replace the store's entries with the given JSON array (same shape as
/// memswap_read_entries output; `content_hash` may be empty — it is computed).
/// Appends a snapshot commit.
///
/// # Safety
/// `path` and `entries_json` valid NUL-terminated C strings; `out` writable.
#[no_mangle]
pub unsafe extern "C" fn memswap_write_entries(
    path: *const c_char,
    entries_json: *const c_char,
    out: *mut *mut c_char,
) -> i32 {
    if path.is_null() || entries_json.is_null() || out.is_null() {
        return 1;
    }
    let p = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    let raw = match CStr::from_ptr(entries_json).to_str() {
        Ok(s) => s,
        Err(_) => return fail("entries_json is not valid UTF-8", out),
    };
    let dir = Path::new(p);
    let store = match Store::open(dir) {
        Ok(s) => s,
        Err(e) => return fail(&e.to_string(), out),
    };
    let parsed: Result<Vec<Entry>, _> = serde_json::from_str(raw);
    let entries = match parsed {
        Ok(v) => v,
        Err(e) => return fail(&format!("invalid entries JSON: {e}"), out),
    };
    match store.replace_entries(&entries) {
        Ok(()) => {
            let n = entries.len();
            emit(&serde_json::json!({ "written": n }), out)
        }
        Err(e) => fail(&e.to_string(), out),
    }
}

/// Return the commit log as a JSON array (newest first, like `mem log`).
///
/// # Safety
/// `path` valid NUL-terminated C string; `out` writable out-parameter.
#[no_mangle]
pub unsafe extern "C" fn memswap_log(path: *const c_char, out: *mut *mut c_char) -> i32 {
    if path.is_null() || out.is_null() {
        return 1;
    }
    let p = match CStr::from_ptr(path).to_str() {
        Ok(s) => s,
        Err(_) => return 1,
    };
    let dir = Path::new(p);
    if let Err(e) = Store::open(dir) {
        return fail(&e.to_string(), out);
    }
    match memswap_core::history::log(dir) {
        Ok(commits) => emit(&commits, out),
        Err(e) => fail(&e.to_string(), out),
    }
}

/// Write an error JSON document into `out` and return 1.
unsafe fn fail(msg: &str, out: *mut *mut c_char) -> i32 {
    let doc = serde_json::json!({ "error": msg }).to_string();
    if let Ok(c) = CString::new(doc) {
        *out = c.into_raw();
    }
    1
}

/// Serialize a JSON-serializable value into `*out` and return 0.
fn emit<T: serde::Serialize>(value: &T, out: *mut *mut c_char) -> i32 {
    match serde_json::to_string(value)
        .ok()
        .and_then(|s| CString::new(s).ok())
    {
        Some(c) => {
            unsafe { *out = c.into_raw() };
            0
        }
        None => 1,
    }
}
