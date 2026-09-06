//! memswap-core: the portable agent-memory interchange format.
//!
//! M1 vertical slice: a content-addressed store (MANIFEST.json + INDEX.json +
//! objects/ + refs/) with blake3 tamper-evidence and `verify`. The full git-like
//! commit hash-chain (body_hash -> tree_hash -> commit_hash) is M2.

pub mod entry;
pub mod error;
pub mod hash;
pub mod manifest;
pub mod store;

pub use entry::{Entry, EntryKind, Scope, Source};
pub use error::{Error, Result};
pub use manifest::Manifest;
pub use store::{Store, VerifyReport};
