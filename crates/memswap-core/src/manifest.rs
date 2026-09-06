use serde::{Deserialize, Serialize};

/// The container's trust root. `index_hash` binds MANIFEST to the INDEX; the
/// per-entry `content_hash` (in INDEX) binds each body to its object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub package_id: String,
    pub created_at: String,
    pub harness_origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// blake3 of the canonical INDEX.json bytes at write time.
    pub index_hash: String,
}

impl Manifest {
    pub fn empty(package_id: &str, harness_origin: &str, profile: Option<String>) -> Self {
        Manifest {
            schema_version: 1,
            package_id: package_id.to_string(),
            created_at: crate::store::now_iso8601(),
            harness_origin: harness_origin.to_string(),
            profile,
            index_hash: String::new(),
        }
    }
}
