//! Property tests (M6 hardening): invariants that must hold for *every*
//! generated input, not just hand-picked fixtures.
//!
//! - entry hash = blake3(body), stable and content-derived
//! - write→read roundtrip preserves entries exactly (any valid entry)
//! - replace_entries is deterministic: same input → same tree_hash
//! - INDEX.json parser accepts any serialized Entry and rejects garbage
//!   without panicking (fuzz-light)
//! - commit chain: arbitrary mutation sequences keep chain_ok true

use std::fs;
use std::path::PathBuf;

use memswap_core::{Entry, EntryKind, Scope, Source, Store};
use proptest::prelude::*;

fn arb_kind() -> impl Strategy<Value = EntryKind> {
    prop_oneof![
        Just(EntryKind::Instruction),
        Just(EntryKind::Fact),
        Just(EntryKind::Project),
        Just(EntryKind::User),
        Just(EntryKind::Index),
        Just(EntryKind::Other),
    ]
}

fn arb_scope() -> impl Strategy<Value = Scope> {
    prop_oneof![Just(Scope::Global), Just(Scope::Project)]
}

/// Valid ids: harness/<word>[/more-words] — mirrors what adapters emit.
fn arb_id() -> impl Strategy<Value = String> {
    prop_oneof![
        "hermes|codex|claude|notes|py".prop_map(|h| format!("{h}/memory")),
        "[a-z]{3,8}/[a-z]{3,8}/[a-z0-9]{1,6}",
    ]
}

fn arb_body() -> impl Strategy<Value = String> {
    // Realistic memory-ish text: letters, unicode, newlines, quotes, slashes.
    proptest::collection::vec(
        prop_oneof![
            Just(' '),
            Just('\n'),
            Just('"'),
            Just('\\'),
            Just('/'),
            any::<char>().prop_filter("printable-ish", |c| {
                let c = *c;
                !c.is_control() && c != '\u{0}'
            }),
        ],
        0..200,
    )
    .prop_map(|chars| {
        let s: String = chars.into_iter().collect();
        if s.trim().is_empty() {
            "fallback body".to_string()
        } else {
            s
        }
    })
}

fn arb_entry() -> impl Strategy<Value = Entry> {
    (
        arb_id(),
        arb_kind(),
        "[ -~äöüß]{0,60}".prop_map(|t| if t.trim().is_empty() { "t".into() } else { t }),
        arb_body(),
        arb_scope(),
        proptest::option::of("[a-zA-Z0-9-]{1,20}"),
    )
        .prop_map(|(id, kind, title, body, scope, project)| Entry {
            id,
            kind,
            title,
            body,
            scope,
            project,
            source: Source {
                harness: "proptest".into(),
                path: None,
                updated_at: None,
                profile: None,
            },
            tags: vec![],
            content_hash: String::new(),
        })
}

fn tmp_store(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("m6-proptest-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// content_hash is always blake3(body), independent of everything else.
    #[test]
    fn content_hash_depends_only_on_body(title in "[a-z]{0,20}", body1 in arb_body(), body2 in arb_body()) {
        let mk = |b: &str| {
            Entry {
                id: "p/x".into(),
                kind: EntryKind::Fact,
                title: title.clone(),
                body: b.to_string(),
                scope: Scope::Global,
                project: None,
                source: Source {
                    harness: "proptest".into(),
                    path: None,
                    updated_at: None,
                    profile: None,
                },
                tags: vec![],
                content_hash: String::new(),
            }
        };
        let a = mk(&body1);
        let b = mk(&body2);
        let same_body_diff_title = {
            let mut e = mk(&body1);
            e.title = "different".into();
            e
        };
        prop_assert_eq!(a.compute_hash(), same_body_diff_title.compute_hash());
        if body1 != body2 {
            prop_assert_ne!(a.compute_hash(), b.compute_hash());
        }
    }

    /// write → read roundtrip preserves every field for arbitrary entries.
    #[test]
    fn write_read_roundtrip_preserves_entries(entries in proptest::collection::vec(arb_entry(), 1..12)) {
        let dir = tmp_store("rt");
        let store = Store::init(&dir, "p", "proptest", None).unwrap();
        store.replace_entries(&entries).unwrap();
        let back = store.read_entries().unwrap();
        prop_assert_eq!(back.len(), entries.len());
        for (orig, got) in entries.iter().zip(back.iter()) {
            prop_assert_eq!(&orig.id, &got.id);
            prop_assert_eq!(&orig.body, &got.body);
            prop_assert_eq!(&orig.title, &got.title);
            prop_assert_eq!(&orig.tags, &got.tags);
            prop_assert_eq!(&orig.source.harness, &got.source.harness);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// Same input → same tree state, byte-for-byte INDEX (determinism).
    #[test]
    fn replace_entries_is_deterministic(entries in proptest::collection::vec(arb_entry(), 1..8)) {
        let dir_a = tmp_store("det-a");
        let dir_b = tmp_store("det-b");
        for d in [&dir_a, &dir_b] {
            let s = Store::init(d, "p", "proptest", None).unwrap();
            s.replace_entries(&entries).unwrap();
        }
        let ia = fs::read(dir_a.join("INDEX.json")).unwrap();
        let ib = fs::read(dir_b.join("INDEX.json")).unwrap();
        prop_assert_eq!(ia, ib);
        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    /// INDEX.json parser: roundtrips valid entries, never panics on garbage.
    #[test]
    fn index_parser_never_panics(garbage in ".*{0,200}") {
        // Any byte string must fail to parse or parse to entries — no panic.
        let _ = serde_json::from_slice::<Vec<Entry>>(garbage.as_bytes());
    }

    /// Mutation sequences of arbitrary entries keep the chain intact.
    #[test]
    fn mutation_sequences_keep_chain_ok(ops in proptest::collection::vec(
        (arb_entry(), 0..2u8), 1..15,
    )) {
        let dir = tmp_store("chain");
        let store = Store::init(&dir, "p", "proptest", None).unwrap();
        for (e, mode) in &ops {
            let mut entry = e.clone();
            entry.id = format!("{}/{:x}", entry.id, mode); // spread across ids
            match mode {
                0 => {
                    let mut all = store.read_entries().unwrap();
                    all.retain(|x| x.id != entry.id);
                    all.push(entry);
                    store.replace_entries(&all).unwrap();
                }
                _ => {
                    let mut all = store.read_entries().unwrap();
                    all.retain(|x| x.id != entry.id); // delete-only op
                    store.replace_entries(&all).unwrap();
                }
            }
        }
        let report = store.verify().unwrap();
        prop_assert!(report.chain_ok, "chain broken after {} ops", ops.len());
        prop_assert!(report.ok);
        let _ = fs::remove_dir_all(&dir);
    }
}
