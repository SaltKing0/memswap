//! memswap-core: the portable agent-memory interchange format.
//!
//! Content-addressed store (MANIFEST.json + INDEX.json + objects/ + refs/)
//! with blake3 tamper-evidence, a git-like commit hash-chain
//! (body_hash -> tree_hash -> commit_hash with parent_hash chains), and
//! optional detached ed25519 signatures.

pub mod entry;
pub mod error;
pub mod hash;
pub mod history;
pub mod manifest;
pub mod memfile;
pub mod migrate;
pub mod sign;
pub mod store;
pub mod text;

pub use entry::{Entry, EntryKind, Scope, Source};
pub use error::{Error, Result};
pub use history::{Commit, DiffReport};
pub use manifest::Manifest;
pub use sign::SignatureDoc;
pub use store::{Store, VerifyReport};
