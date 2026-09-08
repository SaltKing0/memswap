//! memswap-adapters: turn each harness's on-disk memory into the canonical
//! memswap model and back. Two tiers: built-in adapters (compiled in) and a
//! dlopen C-ABI dynamic plugin contract (M4).

pub mod claude;
pub mod codex;
pub mod hermes;
pub mod model;
pub mod plugin;

pub use claude::ClaudeAdapter;
pub use codex::CodexAdapter;
pub use hermes::HermesAdapter;
pub use model::{Adapter, HarnessContext, MergeStrategy, WriteReport};
pub use plugin::AdapterRegistry;
