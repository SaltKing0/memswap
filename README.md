# memswap

**The portable memory interchange format.** Move your agent's memory between
Hermes, Codex, and Claude Code losslessly — and pin the facts compaction must
never erase.

> memswap is the memory analogue of [AGENTS.md](https://agents.md/): a plain,
> vendor-neutral, provenance-verified interchange format that lives *between*
> agent harnesses. It is not a memory system (that's mem0 / Zep / Letta /
> LangMem) — it is the layer that lets memory *travel*.

## Why a file format?

A memory incumbent's own research says it best: Letta's
["Is a Filesystem All You Need?"](https://www.letta.com/blog/benchmarking-ai-agent-memory/)
found a plain file + filesystem tools beats specialized memory tools on LoCoMo
(74.0% vs mem0's 68.5%). The canonical state of agent memory should be simple,
portable, human-readable files — exactly what memswap stores.

## Quick start (M1: Hermes)

```sh
# build
cargo build --release

# export your Hermes memory into a portable store directory
./target/release/mem export --harness hermes --out memory-store

# pack it into a single shareable file
./target/release/mem pack --dir memory-store --out memory.memfile

# verify it's intact and tamper-evident
./target/release/mem verify --dir memory-store

# import it into a Hermes home
./target/release/mem import --harness hermes --dir memory-store --dry-run
```

## CLI

| Command | Description |
|---|---|
| `mem init` | Create an empty store |
| `mem export --harness <h>` | Export a harness's memory into a store |
| `mem import --harness <h>` | Import a store into a harness memory dir |
| `mem sync --harnesses a,b,c` | Export harness A into the store, then import into B, C — one step |
| `mem verify --dir <d>` | Verify integrity / tamper-evidence (exit 4 on failure) |
| `mem pack` / `mem unpack` / `mem peek` | `.memfile` zip transport: pack a store, restore it, list contents |
| `mem stats` | Entry counts by harness/kind/scope + chain health |
| `mem doctor` | Probe harnesses and report status |
| `mem adapters list` | List built-in adapters |
| `mem log` / `mem diff` | Commit history and entry-level diff between two commits |
| `mem keygen` / `mem sign` | ed25519 keypair, detached signature over the store |
| `mem migrate` | Schema upgrades (v1→v2 canonicalises line endings) |

Exit codes: `0` ok · `1` error · `2` usage · `3` harness not found · `4` verify
failure · `5` conflict.

## Architecture

```
Harnesses          memswap
─────────          ────────
Hermes    ─┐
Codex     ─┼── Adapter ──> MANIFEST.json + INDEX.json + objects/ + refs/
Claude    ─┘     (read/write, provenance-preserving)
                        └── blake3 content-addressed, tamper-evident
```

- **Core** (`memswap-core`): store, hashing, verify, merge.
- **Adapters** (`memswap-adapters`): Hermes (M1), Codex (M3), Claude Code (M3),
  plus a dlopen C-ABI plugin loader (`MEMSWAP_ADAPTERS` env var) and the
  `sample-plugin` reference implementation (M4).
- **CLI** (`mem`): the `mem` binary — every command takes `--json`.
- **FFI** (`memswap-ffi`): C ABI cdylib (`memswap_read_entries`,
  `memswap_write_entries`, `memswap_log`, `memswap_verify_store`,
  `memswap_store_info`, JSON wire format).
- **Bindings**: `bindings/python` (ctypes, zero deps) and `bindings/node`
  (koffi, one dep) over the FFI cdylib.

## Guarantees (test-enforced)

| Property | How it's enforced |
|---|---|
| Tamper-evidence | Every single-byte mutation in `INDEX.json`, `MANIFEST.json`, `commits/`, `objects/` and `refs/` is detected — asserted over the whole corpus, not spot-checked. |
| Platform-independent hashes | Input is canonicalised to LF on read, so the same memory hashes identically on Windows, macOS and Linux, and a `.memfile` verifies anywhere. |
| Parser robustness | 3 cargo-fuzz targets (`INDEX.json`, `MANIFEST.json`, `.memfile` archives) — no crashes, no panics, no zip-slip escapes. |
| No silent drift | Golden corpus: exporting the checked-in fixture homes must reproduce a byte-identical `INDEX.json`; any change to adapter output fails CI. |

## Roadmap

- **M1 (now):** core store + Hermes adapter + CLI + golden/round-trip/tamper tests.
- **M2:** git-like commit hash-chain, `log`/`diff`, ed25519 signing. **Shipped.**
- **M3:** Codex + Claude Code adapters, merge strategies, golden files. **Shipped.**
- **M4:** plugin ABI (dlopen, `memswap_plugin_*` C contract), sample plugin,
  FFI expansion, Python + Node bindings, `--json` hardening. **Shipped.**
- **M5:** packaging (GitHub release matrix + pip/npm wrappers; crates.io/PyPI/npm
  publish pending tokens). **Shipped.**
- **M5.5:** `mem sync` (one-step harness→store→harness), `.memfile` pack/unpack/peek
  transport, `mem stats`. **Shipped.**
- **M6:** hardening — golden-file corpus for all three adapters, proptest
  invariants, corruption suite, cargo-fuzz targets. **Shipped.**
- **M7:** `mem migrate` — forward-only schema upgrades (v1→v2 canonicalises
  entry bodies to LF). **Shipped.**

See `spec/SPEC.md` for the format, `CONTRIBUTING.md` for governance.

## License

Apache-2.0. See [LICENSE](LICENSE).
