use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::core::error::{NyayaError, Result};

/// Mask an API key for display: shows first 4 + last 4 chars with "..." in between.
pub fn mask_key(key: &str) -> String {
    let len = key.len();
    if len <= 8 {
        return "*".repeat(len);
    }
    format!("{}...{}", &key[..4], &key[len - 4..])
}

/// Simple file-based encrypted credential store.
///
/// Uses XOR encryption with a machine-derived key. This is NOT
/// cryptographically strong — it is an obfuscation layer to prevent
/// casual reading of keys on disk. Production deployments should use
/// OS keychains or a proper secrets manager.
pub struct EncryptedFileStore {
    dir: PathBuf,
    derive_key: Vec<u8>,
}

impl EncryptedFileStore {
    /// Create a new store under `data_dir/credentials/`.
    pub fn new(data_dir: &Path) -> Result<Self> {
        let dir = data_dir.join("credentials");
        fs::create_dir_all(&dir)?;
        let derive_key = derive_machine_key();
        Ok(Self { dir, derive_key })
    }

    /// Store an API key for a provider (XOR-encrypted).
    pub fn set(&self, provider_id: &str, api_key: &str) -> Result<()> {
        let path = self.key_path(provider_id)?;
        let encrypted = xor_encrypt(api_key.as_bytes(), &self.derive_key);
        fs::write(path, encrypted)?;
        Ok(())
    }

    /// Retrieve a stored API key, or None if not stored.
    pub fn get(&self, provider_id: &str) -> Result<Option<String>> {
        let path = self.key_path(provider_id)?;
        if !path.exists() {
            return Ok(None);
        }
        let encrypted = fs::read(&path)?;
        let decrypted = xor_encrypt(&encrypted, &self.derive_key);
        let key = String::from_utf8(decrypted).map_err(|e| {
            NyayaError::Config(format!(
                "Credential decryption produced invalid UTF-8: {}",
                e
            ))
        })?;
        Ok(Some(key))
    }

    /// Delete a stored credential.
    pub fn delete(&self, provider_id: &str) -> Result<()> {
        let path = self.key_path(provider_id)?;
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }

    /// List all stored provider IDs.
    pub fn list(&self) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        if !self.dir.exists() {
            return Ok(ids);
        }
        for entry in fs::read_dir(&self.dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("enc") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    ids.push(stem.to_string());
                }
            }
        }
        Ok(ids)
    }

    /// Build the file path for a provider's credential file, with path traversal protection.
    fn key_path(&self, provider_id: &str) -> Result<PathBuf> {
        // Sanitize: only allow alphanumeric, hyphens, and underscores
        if !provider_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(NyayaError::Config(format!(
                "Invalid provider ID (only alphanumeric, hyphens, underscores): {}",
                provider_id
            )));
        }
        if provider_id.is_empty() {
            return Err(NyayaError::Config(
                "Provider ID must not be empty".to_string(),
            ));
        }
        Ok(self.dir.join(format!("{}.enc", provider_id)))
    }
}

/// Derive a machine-specific key from hostname + username.
/// This is NOT a strong KDF — it is an obfuscation layer.
fn derive_machine_key() -> Vec<u8> {
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown-host".to_string());
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown-user".to_string());

    let input = format!("nabaos-credential-key-v1:{}:{}", host, user);
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().to_vec()
}

/// XOR-based symmetric encrypt/decrypt. The key is cycled over the data.
fn xor_encrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ key[i % key.len()])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_encrypted_file_store_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let store = EncryptedFileStore::new(tmp.path()).unwrap();

        // Set and get
        store.set("anthropic", "sk-ant-secret-key-12345").unwrap();
        let retrieved = store.get("anthropic").unwrap();
        assert_eq!(retrieved.as_deref(), Some("sk-ant-secret-key-12345"));

        // Delete and verify gone
        store.delete("anthropic").unwrap();
        let after_delete = store.get("anthropic").unwrap();
        assert!(after_delete.is_none());
    }

    #[test]
    fn test_mask_api_key() {
        // Normal length key
        assert_eq!(mask_key("sk-ant-api03-abcdefgh12345678"), "sk-a...5678");
        // Exactly 8 chars
        assert_eq!(mask_key("12345678"), "********");
        // Short key
        assert_eq!(mask_key("abc"), "***");
        // 9 chars — just over threshold
        assert_eq!(mask_key("123456789"), "1234...6789");
    }

    #[test]
    fn test_list_stored_providers() {
        let tmp = TempDir::new().unwrap();
        let store = EncryptedFileStore::new(tmp.path()).unwrap();

        store.set("anthropic", "key1").unwrap();
        store.set("openai", "key2").unwrap();
        store.set("deepseek", "key3").unwrap();

        let mut list = store.list().unwrap();
        list.sort();
        assert_eq!(list, vec!["anthropic", "deepseek", "openai"]);
    }

    #[test]
    fn test_path_traversal_rejected() {
        let tmp = TempDir::new().unwrap();
        let store = EncryptedFileStore::new(tmp.path()).unwrap();

        assert!(store.set("../etc/passwd", "hack").is_err());
        assert!(store.set("foo/bar", "hack").is_err());
        assert!(store.set("", "hack").is_err());
    }
}
