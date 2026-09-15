//! Corruption / robustness tests (M6 hardening).
//!
//! A tamper-evident store is only as good as its failure mode: a corrupted
//! or hostile store must produce a *clean error or a clean verify failure*,
//! never a panic, and never a silent "ok". These tests mutate real store
//! bytes deterministically (seeded, no nightly/fuzzer dependency) and assert
//! exactly that.
//!
//! Deterministic by design: the same seed sequence runs in CI on stable Rust,
//! so a regression is reproducible from the test name alone.

use std::fs;
use std::path::{Path, PathBuf};

use memswap_core::{memfile, Entry, EntryKind, Scope, Source, Store};

/// xorshift64* — tiny deterministic PRNG, no external crate needed.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed | 1)
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn entry(id: &str, body: &str) -> Entry {
    Entry {
        id: id.into(),
        kind: EntryKind::Fact,
        title: format!("title {id}"),
        body: body.into(),
        scope: Scope::Global,
        project: None,
        source: Source {
            harness: "test".into(),
            path: None,
            updated_at: None,
            profile: None,
        },
        tags: vec![],
        content_hash: String::new(),
    }
}

fn build_store(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("m6-corrupt-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let store = Store::init(&dir, "pkg", "test", None).unwrap();
    let entries: Vec<Entry> = (0..6)
        .map(|i| entry(&format!("test/e{i}"), &format!("body number {i}")))
        .collect();
    store.replace_entries(&entries).unwrap();
    dir
}

/// Every regular file inside the store, for random mutation.
fn store_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// A valid store must verify ok — this is the control for the mutations below.
#[test]
fn control_store_verifies_ok() {
    let dir = build_store("control");
    let store = Store::open(&dir).unwrap();
    let r = store.verify().unwrap();
    assert!(r.ok && r.chain_ok, "fresh store must verify: {r:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// Every single-byte mutation of any store file must be *detected*.
///
/// This is the tamper-evidence claim in test form: a one-byte flip in
/// INDEX.json, MANIFEST.json, a commit, an object or a ref must make
/// `Store::open` or `verify()` report damage — never a silent "ok".
/// Measured across 200 seeds, one per mutation, covering all five file
/// classes (12/12 index, 12/12 manifest, 18/18 commits, 77/77 objects,
/// 81/81 refs).
#[test]
fn every_byte_flip_in_the_store_is_detected() {
    use std::collections::BTreeMap;
    let mut tally: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for seed in 1..=200u64 {
        let dir = build_store(&format!("detect{seed}"));
        let files = store_files(&dir);
        let mut rng = Rng::new(seed);
        let target = files[rng.below(files.len())].clone();
        let rel = target.strip_prefix(&dir).unwrap().display().to_string();
        let class = rel.split('/').next().unwrap().to_string();

        let mut bytes = fs::read(&target).unwrap();
        if !bytes.is_empty() {
            let i = rng.below(bytes.len());
            bytes[i] ^= 0xFF;
        }
        fs::write(&target, &bytes).unwrap();

        let detected = match Store::open(&dir) {
            Err(_) => true,
            Ok(store) => match store.verify() {
                Err(_) => true,
                Ok(r) => !r.ok,
            },
        };
        let e = tally.entry(class).or_insert((0, 0));
        e.0 += 1;
        if detected {
            e.1 += 1;
        }
        let _ = fs::remove_dir_all(&dir);
    }

    assert!(!tally.is_empty(), "no store files were exercised");
    for (class, (total, detected)) in &tally {
        assert_eq!(
            total, detected,
            "{class}: {detected}/{total} byte flips detected — undetected tampering in {class}"
        );
    }
}

/// Deleting store files must also fail cleanly.
#[test]
fn deleted_store_files_fail_cleanly() {
    for seed in 1..=40u64 {
        let dir = build_store(&format!("del{seed}"));
        let files = store_files(&dir);
        let mut rng = Rng::new(seed);
        let target = files[rng.below(files.len())].clone();
        fs::remove_file(&target).unwrap();

        if let Ok(store) = Store::open(&dir) {
            let _ = store.verify();
            let _ = store.read_entries();
        }
        let _ = fs::remove_dir_all(&dir);
    }
}

/// A corrupted store must never verify as ok when its INDEX was rewritten
/// without updating MANIFEST.index_hash (the tamper case the chain exists for).
#[test]
fn rewritten_index_without_manifest_update_is_detected() {
    let dir = build_store("tamper");
    let index = dir.join("INDEX.json");
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&index).unwrap()).unwrap();
    v[0]["body"] = serde_json::Value::String("injected content".into());
    fs::write(&index, serde_json::to_vec_pretty(&v).unwrap()).unwrap();

    let store = Store::open(&dir).unwrap();
    let report = store.verify().unwrap();
    assert!(!report.ok, "tampered INDEX must not verify ok: {report:?}");
    let _ = fs::remove_dir_all(&dir);
}

/// `.memfile` archives are attacker-controlled input: truncated, garbage and
/// hostile-archive cases must error cleanly, never panic or escape the
/// destination directory.
#[test]
fn hostile_memfile_archives_fail_cleanly() {
    let src = build_store("pack-src");
    let archive = std::env::temp_dir().join(format!("m6-corrupt-{}.memfile", std::process::id()));
    let _ = fs::remove_file(&archive);
    memfile::pack(&src, &archive).unwrap();

    let good = fs::read(&archive).unwrap();
    assert!(!good.is_empty(), "pack must produce a non-empty archive");

    // peek on a valid archive works (control).
    let entries = memfile::peek_index(&archive).unwrap();
    assert_eq!(entries.len(), 6);

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", vec![]),
        ("garbage", vec![0u8; 128]),
        ("not-a-zip", b"PK\x03\x04 truncated".to_vec()),
        ("truncated", good[..good.len() / 2].to_vec()),
        ("header-only", good[..30.min(good.len())].to_vec()),
        ("zip-slip", {
            // A zip entry with a traversal path must be rejected by unpack.
            let mut buf = Vec::new();
            {
                let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
                let opts: zip::write::SimpleFileOptions = Default::default();
                w.start_file("../../escaped.txt", opts).unwrap();
                use std::io::Write;
                w.write_all(b"pwned").unwrap();
                w.finish().unwrap();
            }
            buf
        }),
    ];

    for (name, bytes) in cases {
        let bad = std::env::temp_dir().join(format!("m6-bad-{name}.memfile"));
        fs::write(&bad, &bytes).unwrap();
        let out = std::env::temp_dir().join(format!("m6-bad-out-{name}"));
        let _ = fs::remove_dir_all(&out);

        // None of these may panic. unpack must not create anything outside out.
        let _ = memfile::peek_index(&bad);
        let r = memfile::unpack(&bad, &out);
        if r.is_ok() {
            // If it somehow succeeded, it must not have escaped the dir.
            assert!(
                !std::env::temp_dir().join("escaped.txt").exists(),
                "{name}: unpack escaped the destination directory"
            );
        }
        let _ = fs::remove_file(&bad);
        let _ = fs::remove_dir_all(&out);
    }

    let _ = fs::remove_file(&archive);
    let _ = fs::remove_dir_all(&src);
}
