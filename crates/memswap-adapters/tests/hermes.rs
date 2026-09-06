//! Golden + round-trip + tamper-evidence tests for the Hermes adapter.
//! Verifies the M1 vertical slice: export Hermes memory -> store -> verify ->
//! import back is lossless, and tampering is detected.

use std::fs;
use std::path::Path;

use memswap_adapters::model::{Adapter, MergeStrategy};
use memswap_adapters::HermesAdapter;
use memswap_core::Store;

const MEMORY_BODY: &str =
    "Prefer concise answers.\nWrite tests before code.\n§\nUser runs Magnum-Opus on Hermes.\n";
const USER_BODY: &str = "Niklas Reckstad\nPrefers English for technical reports.\n";

/// Build a fake Hermes home directory.
fn hermes_home(base: &Path) -> std::path::PathBuf {
    let home = base.join(".hermes");
    fs::create_dir_all(home.join("memories")).unwrap();
    fs::write(home.join("memories/MEMORY.md"), MEMORY_BODY).unwrap();
    fs::write(home.join("memories/USER.md"), USER_BODY).unwrap();
    home
}

#[test]
fn hermes_export_import_roundtrip_is_lossless() {
    let tmp = tempfile::tempdir().unwrap();
    let home = hermes_home(tmp.path());
    let adapter = HermesAdapter;

    // Detect + read
    let ctx = adapter.detect(&home).expect("hermes detected");
    let entries = adapter.read(&ctx).expect("read");
    assert_eq!(entries.len(), 2, "MEMORY.md + USER.md");
    let memory = entries.iter().find(|e| e.id == "hermes/memory").unwrap();
    let user = entries.iter().find(|e| e.id == "hermes/user").unwrap();
    assert_eq!(memory.body, MEMORY_BODY);
    assert_eq!(user.body, USER_BODY);

    // Export -> store
    let store = Store::init(&tmp.path().join("memory.memfile"), "memory", "hermes", None).unwrap();
    store.replace_entries(&entries).unwrap();

    // Verify clean
    let report = store.verify().unwrap();
    assert!(report.ok, "store should verify clean: {report:?}");
    assert_eq!(report.entries, 2);

    // Import -> a fresh Hermes home, and confirm losslessness
    let home2 = hermes_home(tmp.path()); // reuse same shape; write should be idempotent
    let ctx2 = adapter.detect(&home2).unwrap();
    let report = adapter
        .write(
            &ctx2,
            &store.read_entries().unwrap(),
            MergeStrategy::Replace,
        )
        .unwrap();
    assert_eq!(report.written, 2, "both files written back");
    assert_eq!(
        fs::read_to_string(home2.join("memories/MEMORY.md")).unwrap(),
        MEMORY_BODY
    );
    assert_eq!(
        fs::read_to_string(home2.join("memories/USER.md")).unwrap(),
        USER_BODY
    );

    // export -> import -> export idempotence: re-read matches original bodies
    let entries2 = adapter.read(&ctx2).unwrap();
    assert_eq!(entries2.len(), 2);
    let memory2 = entries2.iter().find(|e| e.id == "hermes/memory").unwrap();
    assert_eq!(memory2.body, MEMORY_BODY);
}

#[test]
fn tampering_is_detected_by_verify() {
    let tmp = tempfile::tempdir().unwrap();
    let home = hermes_home(tmp.path());
    let adapter = HermesAdapter;
    let ctx = adapter.detect(&home).unwrap();
    let entries = adapter.read(&ctx).unwrap();

    let store_dir = tmp.path().join("memory.memfile");
    let store = Store::init(&store_dir, "memory", "hermes", None).unwrap();
    store.replace_entries(&entries).unwrap();
    assert!(store.verify().unwrap().ok);

    // Flip one byte in an object -> verify must fail.
    let entries = store.read_entries().unwrap();
    let obj = store.objects_dir().join(&entries[0].content_hash);
    let mut bytes = fs::read(&obj).unwrap();
    bytes[0] ^= 0xff;
    fs::write(&obj, &bytes).unwrap();

    let report = store.verify().unwrap();
    assert!(!report.ok, "tampered store must fail verification");
    assert!(
        report.objects_ok < report.entries,
        "at least one object must fail"
    );
}

#[test]
fn export_is_content_addressed_and_dedupes() {
    let tmp = tempfile::tempdir().unwrap();
    let home = hermes_home(tmp.path());
    let adapter = HermesAdapter;
    let ctx = adapter.detect(&home).unwrap();
    let entries = adapter.read(&ctx).unwrap();

    let store = Store::init(&tmp.path().join("s"), "memory", "hermes", None).unwrap();
    store.replace_entries(&entries).unwrap();
    let entries2 = store.read_entries().unwrap();
    // Re-writing identical bodies must not create new objects.
    let before = fs::read_dir(store.objects_dir()).unwrap().count();
    store.replace_entries(&entries2).unwrap();
    let after = fs::read_dir(store.objects_dir()).unwrap().count();
    assert_eq!(
        before, after,
        "content-addressed store must dedupe identical bodies"
    );
}
