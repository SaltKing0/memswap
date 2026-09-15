use serde::{Deserialize, Serialize};

/// The schema version this build writes. v1 = pre-canonicalisation stores
/// (bodies may carry CRLF); v2 = canonical LF bodies, so hashes are
/// platform-independent. See `crate::migrate`.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// The container's trust root. `index_hash` binds MANIFEST to the INDEX; the
/// per-entry `content_hash` (in INDEX) binds each body to its object.
/// `head_hash` binds the manifest to the tip of the commit chain.
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
    /// blake3 of the tip commit (empty when the store has no history yet).
    #[serde(default)]
    pub head_hash: String,
}

impl Manifest {
    pub fn empty(package_id: &str, harness_origin: &str, profile: Option<String>) -> Self {
        Manifest {
            schema_version: CURRENT_SCHEMA_VERSION,
            package_id: package_id.to_string(),
            created_at: crate::store::now_iso8601(),
            harness_origin: harness_origin.to_string(),
            profile,
            index_hash: String::new(),
            head_hash: String::new(),
        }
    }
}
