//! AES-256-GCM encryption for sensitive config fields (enc2 scheme).
//!
//! Encrypted values use the format:
//!   `enc2:<base64(12-byte-nonce || GCM-ciphertext+tag)>`
//!
//! The 32-byte symmetric key is stored in `~/.openpup/.keystore` with 0600
//! permissions.  It is generated once on first use and never leaves the host.
//! The LLM can read config files but only sees the `enc2:` opaque token —
//! it cannot reconstruct the plaintext without the on-disk key.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::RngCore;
use std::path::PathBuf;

const ENC2_PREFIX: &str = "enc2:";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;

// ── Keystore ──────────────────────────────────────────────────────────────────

fn keystore_path() -> Result<PathBuf> {
    Ok(crate::config::app_root()?.join(".keystore"))
}

/// Load or create the 32-byte key at `~/.openpup/.keystore`.
///
/// On creation the file is written with mode 0600 (Unix).
pub fn load_or_create_key() -> Result<[u8; KEY_LEN]> {
    let path = keystore_path()?;

    if path.exists() {
        let raw = std::fs::read(&path).context("read .keystore")?;
        let key: [u8; KEY_LEN] = raw
            .try_into()
            .map_err(|_| anyhow::anyhow!(".keystore must be exactly {KEY_LEN} bytes"))?;
        return Ok(key);
    }

    // Generate new key
    let mut key = [0u8; KEY_LEN];
    rand::rngs::OsRng.fill_bytes(&mut key);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("create .openpup dir")?;
    }
    std::fs::write(&path, key).context("write .keystore")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .context("chmod .keystore 0600")?;
    }

    Ok(key)
}

// ── Core encrypt / decrypt ────────────────────────────────────────────────────

/// Encrypt `plaintext` → `enc2:<base64(nonce || ciphertext)>`.
pub fn encrypt(plaintext: &str) -> Result<String> {
    let key_bytes = load_or_create_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;

    let mut payload = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    payload.extend_from_slice(&nonce_bytes);
    payload.extend_from_slice(&ciphertext);

    Ok(format!("{ENC2_PREFIX}{}", B64.encode(&payload)))
}

/// Decrypt `enc2:<base64>` → plaintext.
///
/// Returns an error if the value is not enc2-prefixed or decryption fails.
pub fn decrypt(value: &str) -> Result<String> {
    let b64 = value
        .strip_prefix(ENC2_PREFIX)
        .context("value is not enc2-prefixed")?;

    let payload = B64.decode(b64).context("base64 decode")?;
    anyhow::ensure!(payload.len() > NONCE_LEN, "enc2 payload too short");

    let (nonce_bytes, ciphertext) = payload.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let key_bytes = load_or_create_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let plaintext_bytes = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed: {e}"))?;

    String::from_utf8(plaintext_bytes).context("decrypted bytes are not valid UTF-8")
}

// ── Idempotent helpers ────────────────────────────────────────────────────────

/// Returns `true` if `value` is already an enc2-encrypted token.
pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(ENC2_PREFIX)
}

/// Encrypt `value` if it is not already encrypted.  Empty values are returned
/// unchanged.
pub fn ensure_encrypted(value: &str) -> Result<String> {
    if value.is_empty() || is_encrypted(value) {
        return Ok(value.to_string());
    }
    encrypt(value)
}

/// Decrypt `value` if it is enc2-encrypted.  Plain-text and empty values are
/// returned unchanged.
pub fn ensure_decrypted(value: &str) -> Result<String> {
    if value.is_empty() || !is_encrypted(value) {
        return Ok(value.to_string());
    }
    decrypt(value)
}
