//! Golden-file corpus (M6 hardening): checked-in fixture harness homes.
//!
//! Export over a fixture must produce byte-identical INDEX.json on every
//! run and on every platform — this is the drift alarm for harness layout
//! changes (Codex/Claude/Hermes release often). If one of these tests fails
//! after a memswap change, the adapter's mapping changed; if it fails after
//! nothing changed locally, the *harness* changed its layout and the
//! adapter needs an update before users lose data fidelity.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_mem")
}

/// Copy a checked-in fixture home into a temp dir and run `mem export`.
struct Golden {
    work: PathBuf,
}

impl Golden {
    fn new(name: &str) -> Self {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        // Unique per Golden instance: tests in one binary share a PID and run
        // in parallel, so a PID-only path makes sibling tests clobber each
        // other's working dir.
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let work =
            std::env::temp_dir().join(format!("m6-golden-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&work);
        fs::create_dir_all(&work).unwrap();
        copy_dir(&fixture, &work);
        Golden { work }
    }

    fn export(&self, harness: &str) -> PathBuf {
        let store = self.work.join("store");
        let out = Command::new(bin())
            .args([
                "export",
                "--harness",
                harness,
                "--out",
                store.to_str().unwrap(),
            ])
            .env("HOME", &self.work)
            .output()
            .expect("run mem export");
        assert!(
            out.status.success(),
            "export failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        store
    }
}

impl Drop for Golden {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.work);
    }
}

fn copy_dir(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap().flatten() {
        let p = entry.path();
        let rel = p.strip_prefix(src).unwrap();
        if p.is_dir() {
            copy_dir(&p, &dst.join(rel));
        } else {
            // Fixtures cannot ship files literally named AGENTS.md / CLAUDE.md
            // (they are agent-instruction files and are write-protected), so
            // they are checked in with a `.fixture` suffix and materialized
            // under their real name here.
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let real = name.strip_suffix(".fixture").unwrap_or(&name).to_string();
            let target = dst.join(rel).with_file_name(real);
            fs::copy(&p, &target).unwrap();
        }
    }
}

fn index_of(store: &Path) -> String {
    fs::read_to_string(store.join("INDEX.json")).unwrap()
}

/// `source.path` records where the entry was read from, so it carries the
/// absolute fixture path (temp dir + pid). Normalize it to a stable
/// placeholder so the golden file is portable across machines and runs.
///
/// Two platform hazards are handled here:
/// 1. Windows paths use `\`, and JSON escapes them again (`\\`) — the
///    replacement must match the *escaped* form or it silently does nothing.
/// 2. The separator itself differs, so both sides are unified to `/`.
///    (Fixture bodies contain no backslashes; if one ever does, it must be
///    added to the fixture in its `/` form.)
fn normalize(index: &str, work: &Path) -> String {
    let raw = work.display().to_string();
    let escaped = raw.replace('\\', "\\\\");
    index
        .replace(&escaped, "<HOME>")
        .replace(&raw, "<HOME>")
        // After the escaped-path replacement the remaining separators are
        // still doubled, so collapse pairs before the single-char pass.
        .replace("\\\\", "/")
        .replace('\\', "/")
}

/// The golden INDEX for a fixture lives next to it: `<fixture>.golden.json`.
/// First run materializes it; later runs must match byte-for-byte.
fn check_golden(fixture_name: &str, index: &str) {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(format!("{fixture_name}.golden.json"));
    if !golden.exists() {
        fs::write(&golden, index).unwrap();
        panic!(
            "golden materialized on first run — re-run this test: {}",
            golden.display()
        );
    }
    // Same separator unification as `normalize`, so goldens stay portable.
    let expected = fs::read_to_string(&golden).unwrap().replace('\\', "/");
    assert_eq!(
        index,
        expected,
        "INDEX.json drifted from golden file {}",
        golden.display()
    );
}

/// The golden comparison must be separator-agnostic, otherwise the corpus
/// passes on Linux/macOS and fails on Windows CI for a reason that has
/// nothing to do with adapter behaviour. Verified on the actual Windows
/// shapes rather than assumed.
#[test]
fn normalization_handles_windows_paths() {
    // A Windows INDEX.json, built the way serde would emit it: the real path
    // with every backslash escaped. Constructing it here instead of
    // hand-writing the escape levels keeps the test honest about the shape
    // (hand-written escapes are exactly how this assertion was wrong first).
    let work = Path::new(r"C:\Users\runneradmin\AppData\Local\Temp\m6-golden-hermes-home-123-0");
    let real = format!(r"{}\.hermes\memories\MEMORY.md", work.display());
    let index = format!(
        r#"[{{"source":{{"path":"{}"}}}}]"#,
        real.replace('\\', "\\\\")
    );
    let got = normalize(&index, work);
    assert!(
        got.contains("<HOME>/.hermes/memories/MEMORY.md"),
        "windows path not normalized: {got}"
    );
    assert!(!got.contains('\\'), "backslashes survived: {got}");

    // POSIX shape must keep working.
    let posix =
        r#"[{"source":{"path":"/tmp/m6-golden-hermes-home-9-0/.hermes/memories/MEMORY.md"}}]"#;
    let got = normalize(posix, Path::new("/tmp/m6-golden-hermes-home-9-0"));
    assert!(got.contains("<HOME>/.hermes/memories/MEMORY.md"), "{got}");
}

#[test]
fn hermes_fixture_export_is_stable() {
    let g = Golden::new("hermes-home");
    let store = g.export("hermes");
    check_golden("hermes-home", &normalize(&index_of(&store), &g.work));
}

#[test]
fn codex_fixture_export_is_stable() {
    let g = Golden::new("codex-home");
    let store = g.export("codex");
    check_golden("codex-home", &normalize(&index_of(&store), &g.work));
}

#[test]
fn claude_fixture_export_is_stable() {
    let g = Golden::new("claude-home");
    let store = g.export("claude");
    check_golden("claude-home", &normalize(&index_of(&store), &g.work));
}

/// Re-export twice into two stores: byte-identical (no timestamps leaking
/// into the index — provenance `updated_at` comes from the fixture files,
/// not the wall clock).
#[test]
fn export_is_idempotent_across_runs() {
    let g = Golden::new("hermes-home");
    let a = normalize(&index_of(&g.export("hermes")), &g.work);
    let b = normalize(&index_of(&g.export("hermes")), &g.work);
    assert_eq!(a, b, "two exports of the same fixture must be identical");
}
