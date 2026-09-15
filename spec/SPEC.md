# memswap — Portable Agent Memory Interchange Format

**Status:** Draft (v0.1) for implementation
**Spec version:** 1.0 (draft)
**License:** Apache-2.0
**Canonical repo:** https://github.com/SaltKing0/memswap

> **The portable memory interchange format.** Move your agent's memory between
> Hermes, Codex, and Claude Code losslessly, and pin the facts compaction must
> never erase. The memory analogue of [AGENTS.md](https://agents.md/).

The key words "MUST", "MUST NOT", "REQUIRED", "SHALL", "SHALL NOT", "SHOULD",
"SHOULD NOT", "RECOMMENDED", "MAY", and "OPTIONAL" in this document are to be
interpreted as described in [RFC 2119](https://www.rfc-editor.org/rfc/rfc2119).

---

## 1. Scope

This specification defines the **memswap container**: a portable, versioned,
provenance-verified on-disk representation of an agent's memory, together with
the rules for moving it losslessly between agent harnesses (Hermes, OpenAI
Codex, Claude Code) and between machines.

memswap is an **interchange format**, not a memory system. It does not define
how to store, rank, retrieve, summarize, or expire context *inside* one runtime
(that is the domain of mem0, Zep, Letta, LangMem). It defines how memory
**travels between** them.

## 2. Design principles

1. **Plain text is the source of truth.** Every memory is a human-readable
   markdown body. No opaque binary is required to read memory.
2. **Machine-verifiable by default.** The container carries a content-addressed
   provenance hash-chain so tamper, corruption, and drift are *detectable*.
3. **Canonical form is a directory; transport form is a single `.memfile` zip.**
   Both share one schema; the zip is a byte-exact, checksummed archive.
4. **Additive metadata.** Importers fill what they can from the real source and
   mark the rest absent. Nothing is fabricated on import.
5. **Canonical taxonomy is `episodic / semantic / procedural`.** Each harness's
   native type vocabulary is a *mapping*, never a requirement.
6. **Bounded by default.** Budgets are advisory and validated on export.

## 3. Container layout

```
<name>.memfile/
├── MANIFEST.json      # trust root: schema_version, package_id, created_at,
│                      #   harness_origin, profile, index_hash
├── INDEX.json         # array of entries (metadata + content_hash)
├── objects/           # content-addressed blobs, blake3(body); body = UTF-8 markdown
├── refs/              # <entry-id> -> content_hash (git-like thin pointers)
└── commits/           # append-only commit chain: <commit_hash>.json
└── SIG                # OPTIONAL detached ed25519 signature over MANIFEST+INDEX
```

- `MANIFEST.json` and `INDEX.json` are the authoritative inputs.
- `objects/`, `refs/`, and `SIG` are derived/regenerable; integrity verification
  never depends on them being present.
- All hashing is **blake3**. `content_hash = blake3(body)`.

## 4. Entry model

Each entry in `INDEX.json` is:

| Field | Req | Type | Description |
|---|---|---|---|
| `id` | R | string | Stable logical id, e.g. `hermes/memory`, `hermes/user`. |
| `kind` | R | enum | `instruction \| fact \| project \| user \| index \| other` |
| `title` | R | string | Human/LLM-readable one-liner. |
| `body` | D | string | Markdown body (stored in `objects/<content_hash>`, resolved on read). |
| `scope` | R | enum | `global \| project` |
| `project` | O | string | Project key/path when `scope=project`. |
| `source` | R | object | `{ harness, path?, updated_at?, profile? }` — provenance. |
| `tags` | O | array | Lowercase tags. |
| `content_hash` | R | string | blake3(body), hex. |

### 4.1 Text canonicalisation

The canonical form of every text entry is **LF-only**. Adapters normalise CRLF
and lone CR to LF on read, so a memory written on Windows hashes identically to
the same memory written on Linux or macOS. `content_hash` is therefore
platform-independent, and a `.memfile` produced on one platform verifies on any
other. Writers emit LF.

### 4.2 Canonical kind taxonomy

The spec's canonical kinds are the harness-independent classification. Harness
native vocabularies (Claude `user|feedback|project|reference`, Codex
`task/task_outcome`, Hermes `memory|user`) are preserved in `source` / tags and
used for export placement only.

## 5. Integrity & tamper evidence

`verify` recomputes and checks, in order:

1. **Per-object:** `blake3(object bytes) == entry.content_hash` — detects body
   tampering/corruption.
2. **Per-ref:** `refs/<id>` points at an existing object whose hash matches —
   detects broken or retargeted pointers.
3. **Manifest binding:** `MANIFEST.index_hash == blake3(INDEX.json bytes)` —
   detects index tampering.
4. **Commit chain:** for every commit reachable from HEAD,
   `commit_hash == blake3(canonical commit with hash field empty)`,
   `tree_hash == blake3(canonical entry snapshot)`, and each `parent_hash`
   links to the previous commit, terminating at a root commit;
   `MANIFEST.head_hash` MUST equal the tip commit's hash. Detects history
   rewriting, dangling parents, and stale manifests.
5. **Signature (OPTIONAL):** when a `SIG` file is present, it MUST carry
   `alg="ed25519"`, a hex verifying key, and a hex signature over
   `blake3(MANIFEST.json bytes || INDEX.json bytes)`. The SIG covers the
   manifest and index only — object integrity is enforced by rule 1. The
   verifying key is self-contained but substitutable; relying parties that
   need authenticity MUST pin the key out-of-band.

## 6. Interop with real harness formats

| Harness | Location | Adapter maps |
|---|---|---|
| **Hermes** | `~/.hermes/memories/{MEMORY.md,USER.md}` (flat `§`-delimited, char-capped) | `hermes/memory` (fact), `hermes/user` (user) — **implemented** |
| **Codex** | `~/.codex/AGENTS.md` (global instructions, ~32 KiB) + generated `~/.codex/memories/` (MEMORY.md handbook, memory_summary.md 5k-token index, rollout_summaries/, skills/) | `codex/agents` (instruction, only write target), `codex/memory`, `codex/memory_summary`, `codex/rollout/*`, `codex/skill/*` — **implemented; `memories/` is generated state, read-only** |
| **Claude Code** | `~/.claude/CLAUDE.md` + `~/.claude/projects/<mapped>/memory/` (MEMORY.md index 200-line/25 KiB cap, `<type>_<slug>.md` per-memory files with YAML frontmatter; native types user/feedback/project/reference) | `claude/claude_md`, `claude/project/<key>/index`, `claude/<name>` with `claude-type:*` tags — **implemented; index entries are derived views, never written back** |

Round-trip **MUST** be lossless: `export -> import -> export` is idempotent and
byte-identical. `content_hash` MUST be stable across export/import.

### 6.1 Dynamic adapters (plugins)

Beyond the built-in adapters, memswap loads shared libraries at runtime from
the directory named by `MEMSWAP_ADAPTERS` (colon/semicolon-separated paths).
A plugin is a cdylib exporting the `memswap_plugin_*` C ABI:

| Symbol | Signature | Meaning |
|---|---|---|
| `memswap_plugin_name` | `() -> const char*` | Static adapter name |
| `memswap_plugin_version` | `() -> int32` | ABI version, must equal 1 |
| `memswap_plugin_detect` | `(home, char** out) -> i32` | 0 + JSON `{"detected":[...]}` or nonzero if absent |
| `memswap_plugin_read` | `(home, char** out) -> i32` | 0 + JSON array of canonical entries |
| `memswap_plugin_write` | `(home, entries_json, strategy, char** out) -> i32` | 0 + JSON `{"written","truncated","skipped"}` |
| `memswap_plugin_free` | `(ptr) -> void` | Frees any buffer the plugin handed out |

JSON is the wire format so plugins never link against memswap internals —
no ABI drift when the entry model evolves. The loader rejects plugins with a
mismatched `memswap_plugin_version` and reports (does not swallow) missing
symbols. `crates/sample-plugin` is the reference implementation (a toy
"notes" harness reading `<home>/notes/*.md`).

## 7. Versioning & migration

- `schema_version` is an integer in `MANIFEST.json`.
- Additive/relaxing changes = minor; breaking changes = major.
- `mem migrate --from v1 --to v2` rewrites a store between versions.

## 8. Conformance

A conforming implementation MUST:
- read and write the container layout in §3;
- compute and verify blake3 hashes per §5;
- preserve entry provenance (`source`) losslessly across import/export;
- pass the round-trip and tamper tests in `crates/memswap-adapters/tests/`.

---

*This is a draft spec for the M1 scaffold. Open an RFC before changing anything
marked MUST. See `CONTRIBUTING.md` and the `rfcs/` directory.*
