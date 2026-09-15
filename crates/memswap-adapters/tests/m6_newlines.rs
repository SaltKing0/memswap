//! Cross-platform line-ending equivalence (M6 hardening).
//!
//! Found by the golden corpus failing on Windows CI: fixture files checked
//! out with CRLF produced different content hashes than the same files with
//! LF. That makes the store platform-dependent, so a `.memfile` exported on
//! Windows could not be verified on Linux — the opposite of what an
//! interchange format is for.
//!
//! These tests pin the fix: the canonical form is LF, and CRLF input must
//! hash identically to LF input.

use std::fs;
use std::path::Path;

use memswap_adapters::model::Adapter;
use memswap_adapters::HermesAdapter;

const LF_BODY: &str = "First line.\nSecond line.\n§\nThird line.\n";

fn hermes_home_with(base: &Path, memory: &str) -> std::path::PathBuf {
    let home = base.join(".hermes");
    fs::create_dir_all(home.join("memories")).unwrap();
    fs::write(home.join("memories/MEMORY.md"), memory).unwrap();
    fs::write(home.join("memories/USER.md"), "user\n").unwrap();
    home
}

fn export_hashes(home: &Path) -> Vec<(String, String)> {
    let ctx = HermesAdapter.detect(home).expect("hermes home detected");
    let entries = HermesAdapter.read(&ctx).expect("read");
    let mut out: Vec<(String, String)> = entries
        .into_iter()
        .map(|e| {
            let hash = e.compute_hash();
            (e.id, hash)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn crlf_and_lf_inputs_hash_identically() {
    let tmp = tempfile::tempdir().unwrap();
    let lf_home = hermes_home_with(&tmp.path().join("lf"), LF_BODY);
    let crlf_body = LF_BODY.replace('\n', "\r\n");
    let crlf_home = hermes_home_with(&tmp.path().join("crlf"), &crlf_body);

    let lf = export_hashes(&lf_home);
    let crlf = export_hashes(&crlf_home);
    assert_eq!(
        lf, crlf,
        "CRLF input must produce the same ids and content hashes as LF input"
    );
}

#[test]
fn classic_mac_cr_input_hashes_identically() {
    let tmp = tempfile::tempdir().unwrap();
    let lf_home = hermes_home_with(&tmp.path().join("lf"), LF_BODY);
    let cr_body = LF_BODY.replace('\n', "\r");
    let cr_home = hermes_home_with(&tmp.path().join("cr"), &cr_body);

    assert_eq!(
        export_hashes(&lf_home),
        export_hashes(&cr_home),
        "lone-CR input must normalize to the same canonical form"
    );
}

#[test]
fn exported_body_never_contains_carriage_return() {
    let tmp = tempfile::tempdir().unwrap();
    let crlf_body = LF_BODY.replace('\n', "\r\n");
    let home = hermes_home_with(tmp.path(), &crlf_body);

    let ctx = HermesAdapter.detect(&home).unwrap();
    for entry in HermesAdapter.read(&ctx).unwrap() {
        assert!(
            !entry.body.contains('\r'),
            "entry {} still carries CR after normalization",
            entry.id
        );
    }
}
