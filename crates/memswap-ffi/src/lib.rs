//! memswap C ABI (cdylib). M4 will expand this to a full plugin/FFI surface;
//! M1 ships a minimal version + verify seam so Python (Hermes, via ctypes) and
//! Node (Claude Code / Codex, via napi-rs) can call in today.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::path::Path;

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
    match memswap_core::Store::open(Path::new(p)) {
        Ok(store) => match store.verify() {
            Ok(r) => {
                if r.ok {
                    0
                } else {
                    4
                }
            }
            Err(_) => 1,
        },
        Err(_) => 1,
    }
}

/// Return a JSON document describing the store at `path` into a caller-freed buffer.
/// On success returns 0 and sets *out to a malloc'd C string (free with memswap_free).
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
    let doc = match memswap_core::Store::open(Path::new(p)) {
        Ok(store) => match store.verify() {
            Ok(r) => format!(
                r#"{{"ok":{},"entries":{},"objects_ok":{},"refs_ok":{},"manifest_ok":{}}}"#,
                r.ok, r.entries, r.objects_ok, r.refs_ok, r.manifest_ok
            ),
            Err(e) => format!(r#"{{"error":"{e}"}}"#),
        },
        Err(e) => format!(r#"{{"error":"{e}"}}"#),
    };
    let c = match CString::new(doc) {
        Ok(c) => c,
        Err(_) => return 1,
    };
    *out = c.into_raw();
    0
}

/// Free a buffer returned by memswap_store_info.
///
/// # Safety
/// `ptr` must be a pointer previously returned by `memswap_store_info` and not
/// already freed.
#[no_mangle]
pub unsafe extern "C" fn memswap_free(ptr: *mut c_char) {
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
}
