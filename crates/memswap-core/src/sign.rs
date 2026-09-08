//! Optional detached ed25519 signatures over the store's MANIFEST+INDEX.
//!
//! Default mode is hash-only (no key material needed). When a `SIG` file is
//! present, `verify` checks it. The SIG carries the verifying public key so a
//! store is self-contained; pinning the key out-of-band is what actually
//! defends against key substitution (documented in SPEC §5).

use std::fs;
use std::path::Path;

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::hash::blake3_hex;

/// The on-disk `SIG` document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureDoc {
    pub alg: String, // "ed25519"
    /// Hex verifying key. Self-contained but substitutable; pin it out-of-band
    /// for real trust.
    pub public_key: String,
    /// Hex ed25519 signature over the 32-byte digest below.
    pub signature: String,
    /// blake3(MANIFEST.json bytes || INDEX.json bytes), hex.
    pub message: String,
}

pub const SIG_FILE: &str = "SIG";

/// A freshly generated keypair (hex-encoded).
pub struct KeyPair {
    pub secret_hex: String,
    pub public_hex: String,
}

/// Generate a new ed25519 keypair.
pub fn keygen() -> Result<KeyPair> {
    let sk = SigningKey::generate(&mut OsRng);
    Ok(KeyPair {
        secret_hex: hex(&sk.to_bytes()),
        public_hex: hex(&sk.verifying_key().to_bytes()),
    })
}

/// The digest a signature covers.
fn store_digest(dir: &Path) -> Result<Vec<u8>> {
    let manifest = fs::read(dir.join("MANIFEST.json"))?;
    let index = fs::read(dir.join("INDEX.json"))?;
    let mut msg = manifest;
    msg.extend(index);
    Ok(msg)
}

/// Sign the store at `dir` with the hex secret key; writes `SIG`.
pub fn sign_store(dir: &Path, secret_hex: &str) -> Result<SignatureDoc> {
    let secret = decode32(secret_hex)
        .map_err(|_| Error::Invalid("key must be 64 hex chars (32 bytes)".into()))?;
    let sk = SigningKey::from_bytes(&secret);
    let msg = store_digest(dir)?;
    let sig = sk.sign(&msg);
    let doc = SignatureDoc {
        alg: "ed25519".into(),
        public_key: hex(&sk.verifying_key().to_bytes()),
        signature: hex(&sig.to_bytes()),
        message: blake3_hex(&msg),
    };
    fs::write(dir.join(SIG_FILE), serde_json::to_vec_pretty(&doc)?)?;
    Ok(doc)
}

/// Verify `SIG` against the current MANIFEST+INDEX.
/// Returns `Ok(None)` when no SIG is present, `Ok(Some(true/false))` otherwise.
pub fn verify_signature(dir: &Path) -> Result<Option<bool>> {
    let raw = match fs::read(dir.join(SIG_FILE)) {
        Ok(r) => r,
        Err(_) => return Ok(None),
    };
    let doc: SignatureDoc = serde_json::from_slice(&raw).map_err(Error::Json)?;
    // Recompute the digest; a mismatch means the signed content changed.
    let msg = store_digest(dir)?;
    if blake3_hex(&msg) != doc.message {
        return Ok(Some(false));
    }
    let vk_bytes = decode32(&doc.public_key)
        .map_err(|_| Error::Invalid("SIG public_key must be 64 hex chars".into()))?;
    let vk = VerifyingKey::from_bytes(&vk_bytes)
        .map_err(|_| Error::Invalid("SIG public_key is not a valid ed25519 key".into()))?;
    let sig_bytes = decode_fixed::<64>(&doc.signature)
        .map_err(|_| Error::Invalid("SIG signature must be 128 hex chars".into()))?;
    let sig = Signature::from_bytes(&sig_bytes);
    Ok(Some(vk.verify(&msg, &sig).is_ok()))
}

/// Load a hex secret key from a file (whitespace-trimmed).
pub fn load_secret(path: &Path) -> Result<String> {
    let s = fs::read_to_string(path)?.trim().to_string();
    if decode32(&s).is_err() {
        return Err(Error::Invalid(format!(
            "{} is not a 64-hex-char ed25519 secret key",
            path.display()
        )));
    }
    Ok(s)
}

/// Decode a hex string into a fixed-size byte array (32 or 64 bytes).
fn decode_fixed<const N: usize>(s: &str) -> std::result::Result<[u8; N], ()> {
    let s = s.trim();
    if s.len() != N * 2 {
        return Err(());
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| ())?;
    }
    Ok(out)
}

fn decode32(s: &str) -> std::result::Result<[u8; 32], ()> {
    decode_fixed::<32>(s)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
