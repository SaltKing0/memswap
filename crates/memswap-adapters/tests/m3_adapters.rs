//! M3 golden tests: Codex and Claude Code adapters against realistic fixture
//! homes. Round-trip losslessness, idempotence, type mapping, and budget caps.

use std::fs;
use std::path::Path;

use memswap_adapters::model::{Adapter, MergeStrategy};
use memswap_adapters::{ClaudeAdapter, CodexAdapter};
use memswap_core::{Entry, EntryKind, Store};

// ---------------------------------------------------------------------------
// Codex fixtures
// ---------------------------------------------------------------------------

const CODEX_AGENTS: &str = "# Global instructions\nAlways run cargo test before pushing.\n";
const CODEX_MEMORY: &str = "# Handbook\n## rust\n- cargo fmt before clippy\n";
const CODEX_SUMMARY: &str = "User prefers Rust. Tests before code.\n";
const CODEX_ROLLOUT: &str = "---\ndescription: fixed the store hash bug\ntask: fix hash\ntask_group: memswap\ntask_outcome: success\ncwd: /home/u/memswap\nkeywords: rust, blake3\n---\nRoot cause: moved value before hashing.\n";
const CODEX_SKILL: &str = "---\ndescription: how to cut a release\ncwd: /home/u/memswap\n---\n1. cargo test 2. tag 3. push\n";

fn codex_home(base: &Path) -> std::path::PathBuf {
    let home = base.join(".codex");
    fs::create_dir_all(home.join("memories/rollout_summaries")).unwrap();
    fs::create_dir_all(home.join("memories/skills/release")).unwrap();
    fs::write(home.join("AGENTS.md"), CODEX_AGENTS).unwrap();
    fs::write(home.join("memories/MEMORY.md"), CODEX_MEMORY).unwrap();
    fs::write(home.join("memories/memory_summary.md"), CODEX_SUMMARY).unwrap();
    fs::write(
        home.join("memories/rollout_summaries/2026-09-08T10-00-00-ab12.md"),
        CODEX_ROLLOUT,
    )
    .unwrap();
    fs::write(home.join("memories/skills/release/SKILL.md"), CODEX_SKILL).unwrap();
    home
}

#[test]
fn codex_read_maps_all_five_sources() {
    let tmp = tempfile::tempdir().unwrap();
    let home = codex_home(tmp.path());
    let adapter = CodexAdapter;
    let ctx = adapter.detect(&home).expect("codex detected");
    let entries = adapter.read(&ctx).unwrap();

    let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    assert!(ids.contains(&"codex/agents"));
    assert!(ids.contains(&"codex/memory"));
    assert!(ids.contains(&"codex/memory_summary"));
    assert!(ids.contains(&"codex/rollout/2026-09-08T10-00-00-ab12"));
    assert!(ids.contains(&"codex/skill/release"));

    let agents = entries.iter().find(|e| e.id == "codex/agents").unwrap();
    assert_eq!(agents.body, CODEX_AGENTS);
    assert!(matches!(agents.kind, EntryKind::Instruction));

    let rollout = entries
        .iter()
        .find(|e| e.id.starts_with("codex/rollout/"))
        .unwrap();
    assert_eq!(
        rollout.title, "fixed the store hash bug",
        "frontmatter description becomes the title"
    );

    let skill = entries
        .iter()
        .find(|e| e.id == "codex/skill/release")
        .unwrap();
    assert_eq!(skill.title, "how to cut a release");
}

#[test]
fn codex_roundtrip_agents_is_lossless_and_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let home = codex_home(tmp.path());
    let adapter = CodexAdapter;
    let ctx = adapter.detect(&home).unwrap();
    let entries = adapter.read(&ctx).unwrap();

    let store = Store::init(&tmp.path().join("s"), "memory", "codex", None).unwrap();
    store.replace_entries(&entries).unwrap();
    assert!(store.verify().unwrap().ok);

    // Import into a fresh codex home: AGENTS.md written, memories/ untouched.
    let home2 = codex_home(tmp.path());
    let ctx2 = adapter.detect(&home2).unwrap();
    let report = adapter
        .write(&ctx2, &store.read_entries().unwrap(), MergeStrategy::Merge)
        .unwrap();
    assert_eq!(
        fs::read_to_string(home2.join("AGENTS.md")).unwrap(),
        CODEX_AGENTS,
        "identical content must not be duplicated (idempotent merge)"
    );
    assert!(
        fs::read_to_string(home2.join("memories/MEMORY.md")).unwrap() == CODEX_MEMORY,
        "generated memories/ must never be rewritten"
    );
    assert!(report.written <= 1);

    // Replace strategy overwrites cleanly.
    let report = adapter
        .write(
            &ctx2,
            &store.read_entries().unwrap(),
            MergeStrategy::Replace,
        )
        .unwrap();
    assert_eq!(report.written, 1);
    assert_eq!(
        fs::read_to_string(home2.join("AGENTS.md")).unwrap(),
        CODEX_AGENTS
    );
}

#[test]
fn codex_merge_appends_new_instructions() {
    let tmp = tempfile::tempdir().unwrap();
    let home = codex_home(tmp.path());
    let adapter = CodexAdapter;
    let ctx = adapter.detect(&home).unwrap();

    let incoming = Entry {
        id: "codex/agents".into(),
        kind: EntryKind::Instruction,
        title: "Codex AGENTS.md".into(),
        body: "New rule: never force-push main.\n".into(),
        scope: memswap_core::Scope::Global,
        project: None,
        source: memswap_core::Source {
            harness: "codex".into(),
            path: None,
            updated_at: None,
            profile: None,
        },
        tags: vec![],
        content_hash: String::new(),
    };
    adapter
        .write(&ctx, &[incoming], MergeStrategy::Merge)
        .unwrap();
    let merged = fs::read_to_string(home.join("AGENTS.md")).unwrap();
    assert!(
        merged.contains("Always run cargo test") && merged.contains("never force-push main"),
        "merge must keep existing and append incoming: {merged}"
    );
}

#[test]
fn codex_detects_nothing_without_codex_home() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(CodexAdapter.detect(&tmp.path().join(".codex")).is_none());
}

// ---------------------------------------------------------------------------
// Claude Code fixtures
// ---------------------------------------------------------------------------

const CLAUDE_GLOBAL_MD: &str = "# Personal prefs\nPrefer explicit types.\n";
const CLAUDE_INDEX: &str = "# Memory Index\n- user_background.md — who the user is\n- feedback_tests.md — run tests first\n";
const CLAUDE_USER_FILE: &str = "---\ntype: user\ndescription: Background, current role\n---\nNiklas, builds agent harnesses.\n";
const CLAUDE_FEEDBACK_FILE: &str = "---\ntype: feedback\ndescription: always run tests before committing\n---\nRun the full suite, not just affected tests.\n";

fn claude_home(base: &Path) -> std::path::PathBuf {
    let home = base.join(".claude");
    let proj_key = memswap_adapters::claude::map_project_key("/home/u/proj");
    let memdir = home.join("projects").join(&proj_key).join("memory");
    fs::create_dir_all(&memdir).unwrap();
    fs::write(home.join("CLAUDE.md"), CLAUDE_GLOBAL_MD).unwrap();
    fs::write(memdir.join("MEMORY.md"), CLAUDE_INDEX).unwrap();
    fs::write(memdir.join("user_background.md"), CLAUDE_USER_FILE).unwrap();
    fs::write(memdir.join("feedback_tests.md"), CLAUDE_FEEDBACK_FILE).unwrap();
    home
}

#[test]
fn claude_read_maps_project_memories_and_types() {
    let tmp = tempfile::tempdir().unwrap();
    let home = claude_home(tmp.path());
    let adapter = ClaudeAdapter;
    let ctx = adapter.detect(&home).expect("claude detected");
    let entries = adapter.read(&ctx).unwrap();

    let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
    assert!(ids.contains(&"claude/claude_md"));
    assert!(ids.contains(&"claude/project/-home-u-proj/index"));
    assert!(ids.contains(&"claude/user_background"));
    assert!(ids.contains(&"claude/feedback_tests"));

    let user = entries
        .iter()
        .find(|e| e.id == "claude/user_background")
        .unwrap();
    assert!(
        matches!(user.kind, EntryKind::User),
        "user type maps to User"
    );
    assert!(user.tags.contains(&"claude-type:user".to_string()));
    assert_eq!(user.body, "Niklas, builds agent harnesses.\n");
    assert!(matches!(user.scope, memswap_core::Scope::Project));

    let feedback = entries
        .iter()
        .find(|e| e.id == "claude/feedback_tests")
        .unwrap();
    assert!(
        matches!(feedback.kind, EntryKind::Instruction),
        "feedback maps to Instruction"
    );
    assert_eq!(feedback.title, "always run tests before committing");
}

#[test]
fn claude_roundtrip_preserves_frontmatter_and_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let home = claude_home(tmp.path());
    let adapter = ClaudeAdapter;
    let ctx = adapter.detect(&home).unwrap();
    let entries = adapter.read(&ctx).unwrap();

    let store = Store::init(&tmp.path().join("s"), "memory", "claude", None).unwrap();
    store.replace_entries(&entries).unwrap();
    assert!(store.verify().unwrap().ok);

    let home2 = claude_home(tmp.path());
    let ctx2 = adapter.detect(&home2).unwrap();
    adapter
        .write(&ctx2, &store.read_entries().unwrap(), MergeStrategy::Merge)
        .unwrap();

    // Per-memory files unchanged (skipped because they exist and match).
    assert_eq!(
        fs::read_to_string(home2.join("projects/-home-u-proj/memory/user_background.md")).unwrap(),
        CLAUDE_USER_FILE
    );
    assert_eq!(
        fs::read_to_string(home2.join("CLAUDE.md")).unwrap(),
        CLAUDE_GLOBAL_MD
    );

    // Import into a *fresh* claude home (no memory dir) recreates everything.
    let fresh = tempfile::tempdir().unwrap();
    let home3 = fresh.path().join(".claude");
    fs::create_dir_all(&home3).unwrap();
    fs::write(home3.join("CLAUDE.md"), "").unwrap(); // makes the home detectable
    let ctx3 = adapter
        .detect(&home3)
        .expect("CLAUDE.md alone is detectable");
    let report = adapter
        .write(
            &ctx3,
            &store.read_entries().unwrap(),
            MergeStrategy::Replace,
        )
        .unwrap();
    assert!(report.written >= 3);
    let memdir3 = home3.join("projects/-home-u-proj/memory");
    assert!(memdir3.join("user_background.md").exists());
    let rebuilt = fs::read_to_string(memdir3.join("user_background.md")).unwrap();
    assert!(
        rebuilt.starts_with("---\ntype: user\n"),
        "frontmatter must be re-rendered: {rebuilt}"
    );
    assert!(rebuilt.contains("Niklas, builds agent harnesses."));
}

#[test]
fn claude_index_budget_caps() {
    // 201 lines -> over budget.
    let long = "x\n".repeat(201);
    assert!(memswap_adapters::claude::index_over_budget(&long));
    // 200 short lines -> fine.
    assert!(!memswap_adapters::claude::index_over_budget(
        &"x\n".repeat(200)
    ));
    // 26 KiB in one line -> over budget.
    let wide = "x".repeat(25 * 1024 + 1);
    assert!(memswap_adapters::claude::index_over_budget(&wide));
}

#[test]
fn claude_project_key_mapping() {
    assert_eq!(
        memswap_adapters::claude::map_project_key("/home/u/My.Proj"),
        "-home-u-My-Proj"
    );
    assert_eq!(
        memswap_adapters::claude::map_project_key("/a/b.c/d"),
        "-a-b-c-d"
    );
}

// ---------------------------------------------------------------------------
// Cross-harness: one store, three harnesses
// ---------------------------------------------------------------------------

#[test]
fn hermes_to_codex_transfer_via_store() {
    // Export from Hermes, import the *user-relevant* instruction into Codex.
    let tmp = tempfile::tempdir().unwrap();
    let hermes_home = tmp.path().join(".hermes");
    fs::create_dir_all(hermes_home.join("memories")).unwrap();
    fs::write(
        hermes_home.join("memories/MEMORY.md"),
        "Prefer concise answers.\n§\nWrite tests before code.\n",
    )
    .unwrap();

    let hermes = memswap_adapters::HermesAdapter;
    let ctx = hermes.detect(&hermes_home).unwrap();
    let entries = hermes.read(&ctx).unwrap();

    let store = Store::init(&tmp.path().join("s"), "memory", "hermes", None).unwrap();
    store.replace_entries(&entries).unwrap();
    assert!(store.verify().unwrap().ok);

    // Codex side: the store's hermes/memory entry is not codex/agents, so a
    // codex import skips it (generated-state rule) rather than corrupting.
    let codex = CodexAdapter;
    let chome = codex_home(tmp.path());
    let cctx = codex.detect(&chome).unwrap();
    let report = codex
        .write(&cctx, &store.read_entries().unwrap(), MergeStrategy::Merge)
        .unwrap();
    assert!(
        report.skipped.iter().any(|id| id == "hermes/memory"),
        "foreign-harness entries must be skipped, not injected into generated state"
    );
    assert_eq!(
        fs::read_to_string(chome.join("AGENTS.md")).unwrap(),
        CODEX_AGENTS,
        "codex AGENTS.md untouched by foreign entries"
    );
}
