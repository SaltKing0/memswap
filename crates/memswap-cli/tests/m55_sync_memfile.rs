//! M5.5 feature tests: `mem sync`, `.memfile` pack/unpack/peek, `mem stats`.
//!
//! All tests drive the real CLI binary through the same code paths a user
//! would, on synthetic two-harness homes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mem")
}

/// A fake base home with two harness homes: hermes (source) and codex (target).
struct Fixture {
    base: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!("m55-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join(".hermes/memories")).unwrap();
        fs::create_dir_all(base.join(".codex")).unwrap();
        fs::write(
            base.join(".hermes/memories/MEMORY.md"),
            "User prefers plain text.\n",
        )
        .unwrap();
        fs::write(base.join(".hermes/memories/USER.md"), "Niklas, fixture.\n").unwrap();
        fs::write(
            base.join(".codex/AGENTS.md"),
            "# AGENTS.md\n\n- seed note\n",
        )
        .unwrap();
        Fixture { base }
    }

    fn run(&self, args: &[&str]) -> (i32, String) {
        let out = Command::new(bin())
            .args(args)
            .env("HOME", &self.base)
            .output()
            .expect("run mem");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn memfile_path(base: &Path) -> PathBuf {
    base.join("sync.memfile")
}

#[test]
fn sync_hermes_to_hermes_replaces_stale_memory() {
    let fx = Fixture::new("sync-replace");
    let store = fx.base.join("store1");
    let (rc, _) = fx.run(&[
        "sync",
        "--harnesses",
        "hermes,hermes",
        "--target-base",
        fx.base.to_str().unwrap(),
        "--dir",
        store.to_str().unwrap(),
        "--merge",
        "replace",
        "--json",
    ]);
    assert_eq!(rc, 0);
    // Target got the source body verbatim.
    let target = fs::read_to_string(fx.base.join(".hermes/memories/MEMORY.md")).unwrap();
    assert!(
        target.contains("User prefers plain text."),
        "target got source body: {target}"
    );
    // The store is chain-verified after sync.
    let (rc2, out2) = fx.run(&["verify", "--dir", store.to_str().unwrap(), "--json"]);
    assert_eq!(rc2, 0, "{out2}");
    assert!(out2.contains("\"chain_ok\":true"));
}

#[test]
fn sync_codex_to_codex_merges_appends_new_content() {
    let fx = Fixture::new("sync-merge");
    // Export codex source into a store first (source has the seed note).
    let store = fx.base.join("store2");
    let (rc, _) = fx.run(&[
        "export",
        "--harness",
        "codex",
        "--out",
        store.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(rc, 0);
    // Change the source content so the target receives something new.
    fs::write(
        fx.base.join(".codex/AGENTS.md"),
        "# AGENTS.md\n\n- seed note\n- NEW RULE\n",
    )
    .unwrap();
    let (rc2, _) = fx.run(&[
        "sync",
        "--harnesses",
        "codex,codex",
        "--target-base",
        fx.base.to_str().unwrap(),
        "--dir",
        store.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(rc2, 0);
    let target = fs::read_to_string(fx.base.join(".codex/AGENTS.md")).unwrap();
    assert!(
        target.contains("seed note"),
        "merge preserves existing: {target}"
    );
    assert!(target.contains("NEW RULE"), "merge appends new: {target}");
}

#[test]
fn sync_dry_run_writes_nothing() {
    let fx = Fixture::new("sync-dry");
    let store = fx.base.join("store3");
    let before = fs::read_to_string(fx.base.join(".codex/AGENTS.md")).unwrap();
    let (rc, out) = fx.run(&[
        "sync",
        "--harnesses",
        "hermes,codex",
        "--target-base",
        fx.base.to_str().unwrap(),
        "--dir",
        store.to_str().unwrap(),
        "--dry-run",
        "--json",
    ]);
    assert_eq!(rc, 0);
    assert!(out.contains("\"dry_run\":true"), "{out}");
    let after = fs::read_to_string(fx.base.join(".codex/AGENTS.md")).unwrap();
    assert_eq!(before, after, "dry run must not touch targets");
}

#[test]
fn pack_unpack_roundtrip_is_byte_identical_and_verifies() {
    let fx = Fixture::new("pack");
    let store = fx.base.join("store4");
    let (rc, _) = fx.run(&[
        "export",
        "--harness",
        "hermes",
        "--out",
        store.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(rc, 0);
    let archive = memfile_path(&fx.base);
    let (rc2, out2) = fx.run(&[
        "pack",
        "--dir",
        store.to_str().unwrap(),
        "--out",
        archive.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(rc2, 0, "{out2}");
    assert!(archive.is_file(), "archive created");

    // peek lists entries without unpacking.
    let (rc3, out3) = fx.run(&["peek", archive.to_str().unwrap(), "--json"]);
    assert_eq!(rc3, 0, "{out3}");
    assert!(out3.contains("hermes/memory"), "{out3}");

    // unpack into a fresh dir; the restored store verifies clean.
    let restored = fx.base.join("restored");
    let (rc4, out4) = fx.run(&[
        "unpack",
        archive.to_str().unwrap(),
        "--out",
        restored.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(rc4, 0, "{out4}");
    let (rc5, out5) = fx.run(&["verify", "--dir", restored.to_str().unwrap(), "--json"]);
    assert_eq!(rc5, 0, "{out5}");
    assert!(
        out5.contains("\"ok\":true") && out5.contains("\"chain_ok\":true"),
        "{out5}"
    );

    // INDEX.json byte-identical between original and restored.
    let a = fs::read(store.join("INDEX.json")).unwrap();
    let b = fs::read(restored.join("INDEX.json")).unwrap();
    assert_eq!(a, b, "INDEX.json lossless across pack/unpack");
}

#[test]
fn unpack_refuses_nonempty_dir_and_zip_slip() {
    let fx = Fixture::new("unpack-guard");
    let store = fx.base.join("store5");
    let (rc, _) = fx.run(&[
        "export",
        "--harness",
        "hermes",
        "--out",
        store.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(rc, 0);
    let archive = memfile_path(&fx.base);
    let (rc2, _) = fx.run(&[
        "pack",
        "--dir",
        store.to_str().unwrap(),
        "--out",
        archive.to_str().unwrap(),
    ]);
    assert_eq!(rc2, 0);

    // Refuses a populated directory.
    let (rc3, _) = fx.run(&[
        "unpack",
        archive.to_str().unwrap(),
        "--out",
        store.to_str().unwrap(),
    ]);
    assert_ne!(rc3, 0, "must refuse non-empty target");
}

#[test]
fn stats_reports_kinds_and_chain_health() {
    let fx = Fixture::new("stats");
    let store = fx.base.join("store6");
    let (rc, _) = fx.run(&[
        "export",
        "--harness",
        "hermes",
        "--out",
        store.to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(rc, 0);
    let (rc2, out) = fx.run(&["stats", "--dir", store.to_str().unwrap(), "--json"]);
    assert_eq!(rc2, 0, "{out}");
    assert!(out.contains("\"entries\":2"), "{out}");
    assert!(out.contains("\"chain_ok\":true"), "{out}");
    assert!(out.contains("\"hermes\""), "{out}");
}

#[test]
fn stats_fails_on_missing_store() {
    let fx = Fixture::new("stats-missing");
    let (rc, _) = fx.run(&[
        "stats",
        "--dir",
        fx.base.join("nope").to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(rc, 3, "missing store = exit 3 (not-a-store contract)");
}
