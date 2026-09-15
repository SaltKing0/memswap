#![no_main]

//! Fuzz the MANIFEST.json parser and the manifest/history binding.
//!
//! MANIFEST.json carries `index_hash` and `head_hash`, i.e. it is what binds
//! the store to its commit chain. A panic here would mean a hostile or
//! damaged manifest can take the process down instead of being reported as
//! tampering.

use libfuzzer_sys::fuzz_target;
use memswap_core::{Manifest, Store};

fuzz_target!(|data: &[u8]| {
    // Parse: never panic.
    let Ok(manifest) = serde_json::from_slice::<Manifest>(data) else {
        return;
    };

    // A parsed manifest is written into a real store and then verified, so
    // the hash-binding code paths see the fuzzed values (including absurd
    // hashes, empty ids and mismatched schema versions). Round-tripping
    // through the serializer keeps the parsed value in the path.
    let Ok(round_tripped) = serde_json::to_vec(&manifest) else {
        return;
    };

    let dir = std::env::temp_dir().join(format!("memswap-fuzz-manifest-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if let Ok(store) = Store::init(&dir, "fuzz", "fuzz", None) {
        let _ = std::fs::write(store.manifest_path(), &round_tripped);
        if let Ok(store) = Store::open(&dir) {
            let _ = store.verify();
            let _ = store.read_entries();
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
});