#![no_main]

//! Fuzz `.memfile` archive handling.
//!
//! A `.memfile` is the one artifact users are expected to *receive from
//! someone else*, so it is the highest-risk input in the whole tool: a zip
//! with hostile entry names, absurd sizes or a truncated central directory.
//! Two invariants are checked beyond "does not panic":
//!
//! 1. `unpack` must never write outside the destination directory
//!    (zip-slip), and
//! 2. whatever it does, it must be reproducible — no partial silent success.

use libfuzzer_sys::fuzz_target;
use memswap_core::memfile;
use std::path::{Path, PathBuf};

/// Every path that exists under `root`, canonicalized.
fn tree(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p.clone());
            }
            out.push(p);
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let base = std::env::temp_dir().join(format!("memswap-fuzz-memfile-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::create_dir_all(&base);

    let archive = base.join("in.memfile");
    if std::fs::write(&archive, data).is_err() {
        let _ = std::fs::remove_dir_all(&base);
        return;
    }

    // peek must never panic.
    let _ = memfile::peek_index(&archive);

    // unpack into a nested dir; anything it creates must stay inside it.
    let out = base.join("out");
    let escaped_before = tree(&base);
    let _ = memfile::unpack(&archive, &out);

    let mut escaped_after = tree(&base);
    escaped_after.retain(|p| !escaped_before.contains(p));
    for created in &escaped_after {
        assert!(
            created.starts_with(&out) || created == &archive,
            "unpack wrote outside the destination directory: {}",
            created.display()
        );
    }

    let _ = std::fs::remove_dir_all(&base);
});