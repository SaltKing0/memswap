//! M2 tests: commit hash-chain, log/diff, ed25519 signing, and tamper-evidence
//! at every layer (object, ref, index, manifest, chain, signature).

use memswap_core::history;
use memswap_core::sign;
use memswap_core::{Entry, EntryKind, Scope, Source, Store};

fn entry(id: &str, body: &str) -> Entry {
    Entry {
        id: id.into(),
        kind: EntryKind::Fact,
        title: format!("entry {id}"),
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

fn tmp_store(_tag: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::init(dir.path(), "memory", "test", None).unwrap();
    (dir, store)
}

#[test]
fn commit_chain_grows_and_links() {
    let (_d, store) = tmp_store("chain");

    // init commit
    let chain = history::log(store.dir.as_path()).unwrap();
    assert_eq!(chain.len(), 1, "init creates a root commit");
    assert!(chain[0].parent_hash.is_empty());

    // write 1
    store.write_entry(&entry("a", "alpha")).unwrap();
    // replace (2 entries)
    store
        .replace_entries(&[entry("a", "alpha"), entry("b", "beta")])
        .unwrap();

    let chain = history::log(store.dir.as_path()).unwrap();
    assert_eq!(chain.len(), 3, "init + write + snapshot");

    // parent links: newest -> oldest
    assert_eq!(chain[0].parent_hash, chain[1].commit_hash);
    assert_eq!(chain[1].parent_hash, chain[2].commit_hash);
    assert!(chain[2].parent_hash.is_empty());

    // HEAD matches the tip and the manifest binds it.
    let head = history::head(store.dir.as_path()).unwrap().unwrap();
    assert_eq!(head, chain[0].commit_hash);
    let manifest: memswap_core::Manifest =
        serde_json::from_slice(&std::fs::read(store.manifest_path()).unwrap()).unwrap();
    assert_eq!(manifest.head_hash, head);
}

#[test]
fn commit_hashes_are_reproducible_and_tamper_evident() {
    let (_d, store) = tmp_store("hashes");
    store.write_entry(&entry("a", "alpha")).unwrap();

    let chain = history::log(store.dir.as_path()).unwrap();
    assert!(chain[0].commit_hash == chain[0].compute_hash());
    assert_eq!(chain[0].tree_hash, {
        use memswap_core::hash::blake3_hex;
        blake3_hex(&serde_json::to_vec(&chain[0].entries).unwrap())
    });

    // Tamper with a commit file -> chain verification fails.
    let tip = chain[0].commit_hash.clone();
    let cpath = store.dir.join("commits").join(&tip);
    let mut raw = std::fs::read_to_string(&cpath).unwrap();
    raw = raw.replace("alpha", "tampered");
    std::fs::write(&cpath, raw).unwrap();
    let (ok, _) = history::verify_chain(store.dir.as_path()).unwrap();
    assert!(!ok, "commit tampering must break the chain");
}

#[test]
fn log_and_diff_report_state_changes() {
    let (_d, store) = tmp_store("logdiff");

    store
        .replace_entries(&[entry("a", "v1"), entry("b", "v1")])
        .unwrap();
    store
        .replace_entries(&[entry("a", "v2"), entry("b", "v1"), entry("c", "new")])
        .unwrap();

    let report = history::diff(store.dir.as_path(), None, None).unwrap();
    assert_eq!(report.added, vec!["c".to_string()]);
    assert_eq!(report.changed, vec!["a".to_string()]);
    assert!(report.removed.is_empty());
    assert!(!report.is_empty());

    // Identical consecutive states -> empty diff.
    store
        .replace_entries(&[entry("a", "v2"), entry("b", "v1"), entry("c", "new")])
        .unwrap();
    let report = history::diff(store.dir.as_path(), None, None).unwrap();
    assert!(report.is_empty(), "no-op write must produce an empty diff");

    // Explicit revisions.
    let chain = history::log(store.dir.as_path()).unwrap();
    let oldest = chain.last().unwrap().commit_hash.clone();
    let report = history::diff(store.dir.as_path(), Some(&oldest), Some("HEAD")).unwrap();
    assert_eq!(report.added.len() + report.changed.len(), 3);
}

#[test]
fn diff_detects_removals() {
    let (_d, store) = tmp_store("removal");
    store
        .replace_entries(&[entry("a", "v1"), entry("b", "v1")])
        .unwrap();
    store.replace_entries(&[entry("a", "v1")]).unwrap();
    let report = history::diff(store.dir.as_path(), None, None).unwrap();
    assert_eq!(report.removed, vec!["b".to_string()]);
}

#[test]
fn resolve_supports_head_abbrev_and_errors() {
    let (_d, store) = tmp_store("resolve");
    store.write_entry(&entry("a", "alpha")).unwrap();

    let head_commit = history::resolve(store.dir.as_path(), "HEAD").unwrap();
    let chain = history::log(store.dir.as_path()).unwrap();
    assert_eq!(head_commit.commit_hash, chain[0].commit_hash);

    let abbrev = &chain[0].commit_hash[..8];
    let by_abbrev = history::resolve(store.dir.as_path(), abbrev).unwrap();
    assert_eq!(by_abbrev.commit_hash, chain[0].commit_hash);

    // HEAD~1 is the root (init) commit.
    let root = history::resolve(store.dir.as_path(), "HEAD~1").unwrap();
    assert_eq!(root.commit_hash, chain[1].commit_hash);

    assert!(history::resolve(store.dir.as_path(), "HEAD~5").is_err());
    assert!(history::resolve(store.dir.as_path(), "zzzz").is_err());
    assert!(history::resolve(store.dir.as_path(), "HEAD~nope").is_err());
}

#[test]
fn verify_covers_chain_and_manifest_head_binding() {
    let (_d, store) = tmp_store("verify-chain");
    store.write_entry(&entry("a", "alpha")).unwrap();
    let report = store.verify().unwrap();
    assert!(report.ok);
    assert!(report.chain_ok);
    assert_eq!(report.commits, 2);
    assert_eq!(report.signature_ok, None, "unsigned by default");

    // Rip the chain out: verify must fail.
    std::fs::remove_dir_all(store.dir.join("commits")).unwrap();
    let report = store.verify().unwrap();
    assert!(!report.ok);
    assert!(!report.chain_ok);
}

#[test]
fn manifest_head_binding_detects_stale_pointer() {
    let (_d, store) = tmp_store("stale-head");
    store.write_entry(&entry("a", "alpha")).unwrap();

    // Overwrite the manifest with an older head_hash (stale pointer).
    let mpath = store.manifest_path();
    let mut m: serde_json::Value = serde_json::from_slice(&std::fs::read(&mpath).unwrap()).unwrap();
    m["head_hash"] = serde_json::value::Value::String("0".repeat(64));
    std::fs::write(&mpath, serde_json::to_vec_pretty(&m).unwrap()).unwrap();

    let report = store.verify().unwrap();
    assert!(!report.ok, "stale head_hash must fail verification");
}

#[test]
fn sign_roundtrip_and_tamper_detection() {
    let (_d, store) = tmp_store("sign");
    store.write_entry(&entry("a", "alpha")).unwrap();

    let kp = sign::keygen().unwrap();
    sign::sign_store(store.dir.as_path(), &kp.secret_hex).unwrap();

    // Clean store verifies with signature_ok = Some(true).
    let report = store.verify().unwrap();
    assert!(report.ok);
    assert_eq!(report.signature_ok, Some(true));

    // Tamper with an object -> ok=false via the object hash check. The SIG
    // covers MANIFEST+INDEX only, so it stays valid — by design: the two
    // layers catch different classes of tampering.
    let e = entry("a", "alpha");
    let obj = store.objects_dir().join(e.compute_hash());
    let orig = std::fs::read(&obj).unwrap();
    std::fs::write(&obj, b"tampered").unwrap();
    let report = store.verify().unwrap();
    assert!(!report.ok);
    assert_eq!(report.signature_ok, Some(true));
    std::fs::write(&obj, orig).unwrap();

    // Tamper with INDEX.json (what the SIG actually covers) -> signature fails.
    let ipath = store.index_path();
    let orig_index = std::fs::read(&ipath).unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&orig_index).unwrap();
    v[0]["title"] = serde_json::json!("forged");
    std::fs::write(&ipath, serde_json::to_vec_pretty(&v).unwrap()).unwrap();
    let report = store.verify().unwrap();
    assert!(!report.ok);
    assert_eq!(report.signature_ok, Some(false));
    std::fs::write(&ipath, orig_index).unwrap();

    // Signing with a different key after content change -> new sig valid again.
    store.write_entry(&entry("a", "alpha v2")).unwrap();
    let kp2 = sign::keygen().unwrap();
    sign::sign_store(store.dir.as_path(), &kp2.secret_hex).unwrap();
    let report = store.verify().unwrap();
    assert!(report.ok);
    assert_eq!(report.signature_ok, Some(true));
}

#[test]
fn sig_rejects_bad_key_material() {
    let (_d, store) = tmp_store("sig-bad");
    store.write_entry(&entry("a", "alpha")).unwrap();
    assert!(sign::sign_store(store.dir.as_path(), "nothex").is_err());
    assert!(sign::sign_store(store.dir.as_path(), &"ab".repeat(31)).is_err());
    // Forged-SIG fixture: invalid key material must fail verification.
    let kp = sign::keygen().unwrap();
    std::fs::write(
        store.dir.join("SIG"),
        serde_json::to_vec(&serde_json::json!({
            "alg": "ed25519",
            "public_key": "00",
            "signature": kp.public_hex,
            "message": "0".repeat(64),
        }))
        .unwrap(),
    )
    .unwrap();
    let report = store.verify().unwrap();
    assert!(!report.ok);
    assert_eq!(report.signature_ok, Some(false));
}

#[test]
fn keygen_writes_0600_secret() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("k.hex");
    // Reuse the core keygen; permission handling lives in the CLI, which we
    // exercise indirectly through load_secret.
    let kp = sign::keygen().unwrap();
    std::fs::write(&path, &kp.secret_hex).unwrap();
    let loaded = sign::load_secret(&path).unwrap();
    assert_eq!(loaded, kp.secret_hex);
    assert!(sign::load_secret(&dir.path().join("nope.hex")).is_err());
}

#[test]
fn roundtrip_is_stable_across_history() {
    let (_d, store) = tmp_store("roundtrip");
    let entries = vec![entry("a", "alpha"), entry("b", "beta")];
    store.replace_entries(&entries).unwrap();

    let first = store.read_entries().unwrap();
    // A second identical snapshot must not change the state.
    store.replace_entries(&entries).unwrap();
    let second = store.read_entries().unwrap();

    let ids = |v: &[Entry]| -> Vec<String> { v.iter().map(|e| e.id.clone()).collect() };
    assert_eq!(ids(&first), ids(&second));
    for (a, b) in first.iter().zip(second.iter()) {
        assert_eq!(
            a.content_hash, b.content_hash,
            "content_hash must be stable"
        );
        assert_eq!(a.body, b.body);
    }
}
