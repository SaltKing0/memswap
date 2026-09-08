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

# export your Hermes memory into a portable store
./target/release/mem export --harness hermes --out memory.memfile

# verify it's intact and tamper-evident
./target/release/mem verify --dir memory.memfile

# import it into a Hermes home
./target/release/mem import --harness hermes --dir memory.memfile --dry-run
```

## CLI

| Command | Description |
|---|---|
| `mem init` | Create an empty store |
| `mem export --harness <h>` | Export a harness's memory into a store |
| `mem import --harness <h>` | Import a store into a harness memory dir |
| `mem verify --dir <d>` | Verify integrity / tamper-evidence (exit 4 on failure) |
| `mem doctor` | Probe harnesses and report status |
| `mem adapters list` | List built-in adapters |
| `mem log` / `mem diff` | shipped |
| `mem keygen` / `mem sign` | shipped |
| `mem migrate` | M6 |

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
  plus a dlopen C-ABI plugin contract (M4).
- **CLI** (`memswap-cli`): the `mem` binary.
- **FFI** (`memswap-ffi`): C ABI cdylib for embedding into Python/Node hosts.

## Roadmap

- **M1 (now):** core store + Hermes adapter + CLI + golden/round-trip/tamper tests.
- **M2:** git-like commit hash-chain, `log`/`diff`, ed25519 signing. **Shipped.**
- **M3:** Codex + Claude Code adapters, merge strategies, golden files.
- **M4:** plugin ABI + FFI expansion.
- **M5:** packaging (brew, crates.io, pip/npm wrappers).

See `spec/SPEC.md` for the format, `CONTRIBUTING.md` for governance.

## License

Apache-2.0. See [LICENSE](LICENSE).
