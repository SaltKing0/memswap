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

    /// Detect one harness under `home`, returning its context or None.
    /// Small helper so CLI code doesn't juggle get()+detect() pairs.
    pub fn detect_one(&self, name: &str, home: &Path) -> Option<crate::model::HarnessContext> {
        self.get(name)?.detect(home)
    }

    /// Write entries into one harness: detect + write with the given
    /// strategy. Mirrors read_harness for the write direction.
    pub fn write_adapter(
        &self,
        name: &str,
        home: &Path,
        entries: &[memswap_core::Entry],
        strategy: crate::model::MergeStrategy,
    ) -> Result<crate::model::WriteReport> {
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
        a.write(&ctx, entries, strategy)
    }

    /// Built-in adapters plus every dynamic plugin found under
    /// `MEMSWAP_ADAPTERS` (a directory of shared libraries).
    pub fn with_plugins() -> Self {
        let mut r = Self::builtin();
        for a in crate::dynamic::load_all() {
            r.register(a);
        }
        r
    }
}
