# memswap (Node.js)

Node binding for [memswap](https://github.com/SaltKing0/memswap) — the
portable agent-memory interchange format. Move memory between Hermes, Codex
CLI, and Claude Code losslessly.

```bash
npm install memswap
```

```js
import { Store } from "memswap";

const st = new Store("memory.memfile");
st.writeEntries([{
  id: "notes/idea",
  kind: "fact",
  title: "idea",
  body: "Ship memswap v0.1",
  scope: "global",
  source: { harness: "node" },
  tags: [],
  content_hash: "",
}]);

for (const e of st.readEntries()) console.log(e.id, "=", e.body);
console.log(st.verify()); // { ok: true, chain_ok: true, ... }
```

Uses [koffi](https://koffi.dev) (prebuilt FFI, no node-gyp) to load
`libmemswap_ffi`. Point `MEMSWAP_FFI` at a specific shared library, or build
it from source: `cargo build --release -p memswap-ffi`.

Verify what you move: every store is blake3 content-addressed, hash-chained,
and optionally ed25519-signed (`mem keygen` / `mem sign`).
