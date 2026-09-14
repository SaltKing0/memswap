"""memswap Python binding (ctypes) over libmemswap_ffi.

Zero dependencies. Usage:

    from memswap import Store
    st = Store("/tmp/mystore", create=True)
    st.write_entries([...])          # list of entry dicts
    for e in st.read_entries(): ...  # list of entry dicts
    st.verify()                      # -> dict or raises MemswapError
    st.log()                         # -> list of commit dicts
"""

from __future__ import annotations

import ctypes
import json
import os
from pathlib import Path

_LIB_NAMES = {
    "Linux": "libmemswap_ffi.so",
    "Darwin": "libmemswap_ffi.dylib",
    "Windows": "memswap_ffi.dll",
}

_OK, _ERR, _VERIFY_FAIL = 0, 1, 4


def _lib_name() -> str:
    if os.name == "nt":
        return _LIB_NAMES["Windows"]
    return _LIB_NAMES.get(os.uname().sysname, _LIB_NAMES["Linux"])


def _search_roots() -> list[Path]:
    """Candidate roots, in priority order.

    1. MEMSWAP_FFI env var (absolute lib path).
    2. Bundled wheel lib dir (pip install memswap).
    3. Repo target/{release,debug} (git checkout of memswap).
    """
    roots: list[Path] = []
    here = Path(__file__).resolve().parent
    roots.append(here / "lib")  # bundled in wheels
    # git checkout: bindings/python/memswap/__init__.py -> repo root
    repo = here.parents[2]
    name = _lib_name()
    roots.append(repo / "target" / "release" / name)
    roots.append(repo / "target" / "debug" / name)
    return roots


def _find_lib() -> str:
    env = os.environ.get("MEMSWAP_FFI")
    if env:
        return env
    name = _lib_name()
    for root in _search_roots():
        # Wheel layout: lib/<name>; repo layout: root itself is the file.
        candidate = root / name if root.is_dir() else root
        if candidate.exists():
            return str(candidate)
    raise FileNotFoundError(
        "libmemswap_ffi not found; build it (cargo build -p memswap-ffi), "
        "reinstall the wheel, or set MEMSWAP_FFI=/path/to/libmemswap_ffi.so"
    )


class MemswapError(RuntimeError):
    """A memswap FFI call returned an error; `.details` holds the JSON doc."""


class MemswapVerifyError(MemswapError):
    """Store verification failed (tamper/corruption), exit-code 4 class."""


class _Ffi:
    _inst = None

    def __init__(self) -> None:
        self.lib = ctypes.CDLL(_find_lib())
        c = self.lib
        c.memswap_version.restype = ctypes.c_char_p
        c.memswap_free.argtypes = [ctypes.c_char_p]
        # (path, out) -> i32
        c.memswap_verify_store.argtypes = [ctypes.c_char_p]
        c.memswap_verify_store.restype = ctypes.c_int
        for name in ("memswap_store_info", "memswap_read_entries", "memswap_log"):
            fn = getattr(c, name)
            fn.argtypes = [ctypes.c_char_p, ctypes.POINTER(ctypes.c_char_p)]
            fn.restype = ctypes.c_int
        c.memswap_read_entries.argtypes = [
            ctypes.c_char_p,
            ctypes.c_int,
            ctypes.POINTER(ctypes.c_char_p),
        ]
        c.memswap_write_entries.argtypes = [
            ctypes.c_char_p,
            ctypes.c_char_p,
            ctypes.POINTER(ctypes.c_char_p),
        ]
        c.memswap_write_entries.restype = ctypes.c_int

    @classmethod
    def get(cls) -> "_Ffi":
        if cls._inst is None:
            cls._inst = cls()
        return cls._inst

    def call(self, fn_name: str, *args) -> object:
        """Call a (…, out) -> i32 FFI function and decode its JSON result.

        The caller knows the expected shape (dict for object docs, list for
        arrays) from the FFI contract; the runtime just decodes JSON.
        """
        lib = self.lib
        fn = getattr(lib, fn_name)
        out = ctypes.c_char_p()
        rc = fn(*args, ctypes.byref(out))
        raw = out.value or b""
        lib.memswap_free(out)
        doc: object = json.loads(raw.decode("utf-8")) if raw else {}
        if rc == _VERIFY_FAIL:
            raise MemswapVerifyError(f"verification failed: {doc}")
        if rc == _ERR:
            err = doc.get("error", "unknown error") if isinstance(doc, dict) else doc
            raise MemswapError(str(err))
        return doc

    def version(self) -> str:
        return self.lib.memswap_version().decode("utf-8")


class Store:
    """A memswap store accessed through the C ABI."""

    def __init__(self, path: str | os.PathLike, create: bool = False) -> None:
        self._ffi = _Ffi.get()
        self._path = str(path).encode("utf-8")

    @property
    def version(self) -> str:
        return self._ffi.version()

    def read_entries(self) -> list[dict]:
        out = self._ffi.call("memswap_read_entries", self._path, 0)
        assert isinstance(out, list)
        return out

    def write_entries(self, entries: list[dict]) -> dict:
        payload = json.dumps(entries).encode("utf-8")
        doc = self._ffi.call("memswap_write_entries", self._path, payload)
        assert isinstance(doc, dict)
        return doc

    def verify(self) -> dict:
        """Verify the store; raises MemswapVerifyError on tamper."""
        rc = self._ffi.lib.memswap_verify_store(self._path)
        if rc == _OK:
            doc = self._ffi.call("memswap_store_info", self._path)
            assert isinstance(doc, dict)
            return doc
        if rc == _VERIFY_FAIL:
            raise MemswapVerifyError("store failed verification")
        raise MemswapError("store could not be opened")

    def log(self) -> list[dict]:
        out = self._ffi.call("memswap_log", self._path)
        assert isinstance(out, list)
        return out
