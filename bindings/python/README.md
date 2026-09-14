# memswap (Python)

Python binding for [memswap](https://github.com/SaltKing0/memswap) — the
portable agent-memory interchange format. Move memory between Hermes, Codex
CLI, and Claude Code losslessly.

```bash
pip install memswap
```

```python
from memswap import Store

st = Store("memory.memfile", create=True)
st.write_entries([{
    "id": "notes/idea",
    "kind": "fact",
    "title": "idea",
    "body": "Ship memswap v0.1",
    "scope": "global",
    "source": {"harness": "py"},
    "tags": [],
    "content_hash": "",
}])
for e in st.read_entries():
    print(e["id"], "=", e["body"])
print(st.verify())   # {"ok": true, "chain_ok": true, ...}
```

The package ships a prebuilt `libmemswap_ffi` shared library per platform
wheel plus a zero-dependency ctypes binding. Override the library location
with `MEMSWAP_FFI=/path/to/libmemswap_ffi.so`.

Verify what you move: every store is blake3 content-addressed, hash-chained,
and optionally ed25519-signed (`mem keygen` / `mem sign`).
