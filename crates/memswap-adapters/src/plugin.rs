use std::collections::BTreeMap;
use std::path::Path;

use memswap_core::Result;

use crate::claude::ClaudeAdapter;
use crate::codex::CodexAdapter;
use crate::hermes::HermesAdapter;
use crate::model::Adapter;

/// Registry of built-in adapters, keyed by name (case-insensitive).
#[derive(Default)]
pub struct AdapterRegistry {
    pub adapters: BTreeMap<String, Box<dyn Adapter>>,
}

impl AdapterRegistry {
    pub fn builtin() -> Self {
        let mut r = AdapterRegistry::default();
        r.register(HermesAdapter);
        r.register(CodexAdapter);
        r.register(ClaudeAdapter);
        r
    }

    pub fn register<A: Adapter + 'static>(&mut self, a: A) {
        self.adapters.insert(a.name().to_lowercase(), Box::new(a));
    }

    pub fn get(&self, name: &str) -> Option<&dyn Adapter> {
        self.adapters.get(&name.to_lowercase()).map(|b| b.as_ref())
    }

    pub fn names(&self) -> Vec<String> {
        self.adapters.keys().cloned().collect()
    }

    /// Detect which harness is present under `home`.
    pub fn detect_all(&self, home: &Path) -> Vec<(String, Option<crate::model::HarnessContext>)> {
        self.adapters
            .iter()
            .map(|(name, a)| (name.clone(), a.detect(home)))
            .collect()
    }

    pub fn read_harness(&self, name: &str, home: &Path) -> Result<Vec<memswap_core::Entry>> {
        let a = self
            .get(name)
            .ok_or_else(|| memswap_core::Error::Adapter(format!("no adapter '{}'", name)))?;
        let ctx = a.detect(home).ok_or_else(|| {
            memswap_core::Error::Adapter(format!(
                "harness '{}' not detected under {}",
                name,
                home.display()
            ))
        })?;
        a.read(&ctx)
    }
}
