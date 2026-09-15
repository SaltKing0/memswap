#![no_main]

//! Fuzz the INDEX.json parser and the store round-trip it feeds.
//!
//! The parser is the boundary where attacker-controlled bytes (a shared
//! `.memfile`, a hand-edited store) enter the process, so it must never
//! panic — only parse or reject. Beyond parsing, arbitrary entries that do
//! parse are pushed through a real store write/read cycle to catch
//! panics in hashing, canonicalisation or path derivation (entry ids become
//! file names via `sanitize_id`, which is a classic traversal sink).

use libfuzzer_sys::fuzz_target;
use memswap_core::{Entry, Store};

fuzz_target!(|data: &[u8]| {
    // 1. Parse: must never panic, whatever the bytes.
    let Ok(entries) = serde_json::from_slice::<Vec<Entry>>(data) else {
        return;
    };
    if entries.is_empty() {
        return;
    }

    // 2. Round-trip the parsed entries through a real store. Entry ids come
    //    from untrusted input, so this also exercises the id -> filename
    //    mapping for traversal and separator characters.
    let dir = std::env::temp_dir().join(format!("memswap-fuzz-index-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if let Ok(store) = Store::init(&dir, "fuzz", "fuzz", None) {
        let _ = store.replace_entries(&entries);
        let _ = store.read_entries();
        let _ = store.verify();
    }
    let _ = std::fs::remove_dir_all(&dir);
});