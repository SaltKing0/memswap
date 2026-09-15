//! `mem migrate` — schema upgrades (M7).
//!
//! The interesting case is a *legacy* store: one written before bodies were
//! canonicalised to LF. These tests build one by hand (CRLF bodies, hashes
//! computed over the CRLF bytes, `schema_version: 1`) and check that migration
//! rewrites it to the canonical form, leaves a store that still verifies, and
//! is visible in `mem log`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn mem() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_mem"))
}

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let d = std::env::temp_dir().join(format!("m7-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        Tmp(d)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> (i32, String) {
    let out = Command::new(mem()).args(args).output().unwrap();
    let mut s = String::from_utf8_lossy(&out.stdout).to_string();
    s.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), s)
}

/// Build a store the way a pre-canonicalisation build would have: bodies keep
/// their CRLF, and every hash is computed over those exact bytes.
fn legacy_store(dir: &Path, bodies: &[(&str, &str)]) {
    fs::create_dir_all(dir.join("objects")).unwrap();
    fs::create_dir_all(dir.join("refs")).unwrap();

    let mut index = Vec::new();
    for (id, body) in bodies {
        let hash = blake3_hex(body.as_bytes());
        fs::write(dir.join("objects").join(&hash), body.as_bytes()).unwrap();
        fs::write(dir.join("refs").join(id.replace('/', "__")), &hash).unwrap();
        index.push(serde_json::json!({
            "id": id,
            "kind": "fact",
            "title": id,
            "body": "",
            "scope": "global",
            "source": { "harness": "hermes" },
            "content_hash": hash,
        }));
    }
    let raw = serde_json::to_vec_pretty(&index).unwrap();
    let index_hash = blake3_hex(&raw);
    fs::write(dir.join("INDEX.json"), &raw).unwrap();
    fs::write(
        dir.join("MANIFEST.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "package_id": "legacy",
            "created_at": "0Z",
            "harness_origin": "hermes",
            "index_hash": index_hash,
            "head_hash": "",
        }))
        .unwrap(),
    )
    .unwrap();
}

fn blake3_hex(bytes: &[u8]) -> String {
    // The store's hash function; kept local so the fixture does not depend on
    // the crate under test to describe itself.
    memswap_core::hash::blake3_hex(bytes)
}

fn schema_version(dir: &Path) -> u32 {
    let raw = fs::read_to_string(dir.join("MANIFEST.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    v["schema_version"].as_u64().unwrap() as u32
}

#[test]
fn legacy_crlf_store_is_rewritten_to_canonical_lf() {
    let t = Tmp::new("crlf");
    let store = t.path().join("store");
    legacy_store(
        &store,
        &[
            ("hermes/memory", "line one\r\nline two\r\n"),
            ("hermes/user", "already canonical\n"),
        ],
    );

    let (code, out) = run(&[
        "migrate",
        "--dir",
        store.to_str().unwrap(),
        "--to",
        "2",
        "--json",
    ]);
    assert_eq!(code, 0, "migrate failed: {out}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["from"], 1);
    assert_eq!(v["to"], 2);
    assert_eq!(v["entries"], 2);
    // Only the CRLF entry needed rewriting; the LF one was already canonical.
    assert_eq!(v["entries_rewritten"], 1, "report: {out}");
    assert_eq!(v["ok"], true, "post-migration verify: {out}");
    assert_eq!(v["chain_ok"], true, "chain: {out}");
    assert_eq!(schema_version(&store), 2);

    // The store now verifies, and the CRLF object is gone.
    let (code, out) = run(&["verify", "--dir", store.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0, "verify after migrate: {out}");
    let crlf_hash = blake3_hex(b"line one\r\nline two\r\n");
    assert!(
        !store.join("objects").join(&crlf_hash).exists(),
        "pre-migration object should have been pruned"
    );

    // And the rewrite is visible in history rather than silent.
    let (code, out) = run(&["log", "--dir", store.to_str().unwrap()]);
    assert_eq!(code, 0);
    assert!(
        out.contains("migrate") || out.contains("snapshot"),
        "log: {out}"
    );
}

#[test]
fn dry_run_reports_without_writing() {
    let t = Tmp::new("dry");
    let store = t.path().join("store");
    legacy_store(&store, &[("hermes/memory", "a\r\nb\r\n")]);

    let (code, out) = run(&[
        "migrate",
        "--dir",
        store.to_str().unwrap(),
        "--to",
        "2",
        "--dry-run",
        "--json",
    ]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["dry_run"], true);
    assert_eq!(v["entries_rewritten"], 1);

    // Nothing was touched.
    assert_eq!(
        schema_version(&store),
        1,
        "dry run must not bump the version"
    );
    let crlf_hash = blake3_hex(b"a\r\nb\r\n");
    assert!(store.join("objects").join(&crlf_hash).exists());
}

#[test]
fn current_store_is_a_noop() {
    let t = Tmp::new("noop");
    let store = t.path().join("store");
    let (code, _) = run(&["init", "--dir", store.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (code, out) = run(&["migrate", "--dir", store.to_str().unwrap(), "--json"]);
    assert_eq!(code, 0, "{out}");
    let v: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["noop"], true);
    assert_eq!(v["from"], 2);
    assert_eq!(v["to"], 2);
}

#[test]
fn backwards_and_future_migrations_are_refused() {
    let t = Tmp::new("refuse");
    let store = t.path().join("store");
    let (code, _) = run(&["init", "--dir", store.to_str().unwrap()]);
    assert_eq!(code, 0);

    let (code, out) = run(&[
        "migrate",
        "--dir",
        store.to_str().unwrap(),
        "--to",
        "1",
        "--json",
    ]);
    assert_ne!(code, 0, "downgrade must fail: {out}");
    assert!(out.contains("backwards"), "{out}");

    let (code, out) = run(&[
        "migrate",
        "--dir",
        store.to_str().unwrap(),
        "--to",
        "99",
        "--json",
    ]);
    assert_ne!(code, 0, "future version must fail: {out}");
    assert!(out.contains("newer than this build"), "{out}");
}

#[test]
fn migrating_a_missing_store_is_reported() {
    let t = Tmp::new("missing");
    let (code, out) = run(&[
        "migrate",
        "--dir",
        t.path().join("nope").to_str().unwrap(),
        "--json",
    ]);
    assert_eq!(code, 3, "{out}");
}
