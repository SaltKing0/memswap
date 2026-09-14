// memswap Node.js binding over libmemswap_ffi using koffi (prebuilt FFI,
// no node-gyp). Zero first-party npm dependencies — koffi is the only dep.
//
// Usage:
//   import { Store, version } from "./memswap.js";
//   const st = new Store("/tmp/mystore");
//   st.writeEntries([...]);      // array of entry objects
//   const entries = st.readEntries();
//   st.verify();                 // throws MemswapError / MemswapVerifyError
//   st.log();
//
// Build the native lib first: cargo build -p memswap-ffi
// Locate it via MEMSWAP_FFI env var or <repo>/target/{debug,release}/.

import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import process from "node:process";

const require = createRequire(import.meta.url);
const koffi = require("koffi");

const here = dirname(fileURLToPath(import.meta.url)); // bindings/node
const repoRoot = join(here, "..", "..");

const SUFFIX =
  process.platform === "win32"
    ? "dll"
    : process.platform === "darwin"
      ? "dylib"
      : "so";

function findLib() {
  const env = process.env.MEMSWAP_FFI;
  if (env) return env;
  for (const profile of ["release", "debug"]) {
    const p = join(
      repoRoot,
      "target",
      profile,
      (process.platform === "win32" ? "" : "lib") + "memswap_ffi." + SUFFIX,
    );
    if (existsSync(p)) return p;
  }
  throw new Error(
    `libmemswap_ffi not found; build it (cargo build -p memswap-ffi) or set MEMSWAP_FFI=/path/to/libmemswap_ffi.${SUFFIX}`,
  );
}

const OK = 0,
  ERR = 1,
  VERIFY_FAIL = 4;

let cached = null;
function ffi() {
  if (cached) return cached;
  const lib = koffi.load(findLib());
  // The `!` qualifier on char* out-params creates an anonymous disposable
  // type: the pointer is decoded to a JS string, then koffi.free(ptr) runs.
  // koffi.free calls the C runtime's free() — which is the same allocator
  // Rust's global allocator routes CString::into_raw through on glibc, so
  // this releases Rust-allocated strings correctly on Linux/macOS.
  const fns = {
    version: lib.func("const char *memswap_version()"),
    verify_store: lib.func("int memswap_verify_store(const char *path)"),
    store_info: lib.func("int memswap_store_info(const char *path, _Out_ char *! *out)"),
    read_entries: lib.func("int memswap_read_entries(const char *path, int create, _Out_ char *! *out)"),
    write_entries: lib.func("int memswap_write_entries(const char *path, const char *entries, _Out_ char *! *out)"),
    log: lib.func("int memswap_log(const char *path, _Out_ char *! *out)"),
  };
  cached = fns;
  return cached;
}

// Call a (…, _Out_ char** ) -> int function; returns the parsed JSON doc.
function callJson(fnName, ...args) {
  const fns = ffi();
  const out = [null]; // koffi fills out[0] with the decoded (and freed) string
  const rc = fns[fnName](...args, out);
  const text = out[0] ?? "";
  const doc = text ? JSON.parse(text) : {};
  if (rc === VERIFY_FAIL)
    throw new MemswapVerifyError(`verification failed: ${JSON.stringify(doc)}`, doc);
  if (rc === ERR) throw new MemswapError(doc?.error ?? "unknown error", doc);
  return doc;
}

export class MemswapError extends Error {
  constructor(message, details) {
    super(message);
    this.name = "MemswapError";
    this.details = details;
  }
}

export class MemswapVerifyError extends MemswapError {}

export class Store {
  constructor(path) {
    this.path = String(path);
  }

  readEntries() {
    return callJson("read_entries", this.path, 0);
  }

  writeEntries(entries) {
    return callJson("write_entries", this.path, JSON.stringify(entries));
  }

  verify() {
    const rc = ffi().verify_store(this.path);
    if (rc === OK) return callJson("store_info", this.path);
    if (rc === VERIFY_FAIL) throw new MemswapVerifyError("store failed verification");
    throw new MemswapError("store could not be opened");
  }

  log() {
    return callJson("log", this.path);
  }
}

export function version() {
  return ffi().version(); // koffi auto-decodes const char* -> string
}
