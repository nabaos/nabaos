//! Encrypted secret vault — AES-256-GCM via ring.
//!
//! Stores secrets in SQLite, encrypted with a master key derived from a passphrase
//! using PBKDF2. Each secret can be bound to specific intents (e.g., github_token
//! only accessible for check_infra or monitor_infra intents).

use ring::aead::{self, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};
use rusqlite::Connection;
use std::num::NonZeroU32;
use std::path::Path;

use crate::core::error::{NyayaError, Result};

const PBKDF2_ITERATIONS: u32 = 100_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

/// The secret vault
pub struct Vault {
    db: Connection,
    key: LessSafeKey,
    rng: SystemRandom,
    /// Compiled regex patterns for output sanitization
    sanitizer_patterns: Vec<regex::Regex>,
}

impl Vault {
    /// Open or create the vault with a passphrase
    pub fn open(db_path: &Path, passphrase: &str) -> Result<Self> {
        let db = Connection::open(db_path)
            .map_err(|e| NyayaError::Vault(format!("Failed to open vault DB: {}", e)))?;

        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS vault_meta (
                key TEXT PRIMARY KEY,
                value BLOB NOT NULL
            );
            CREATE TABLE IF NOT EXISTS secrets (
                name TEXT PRIMARY KEY,
                encrypted_value BLOB NOT NULL,
                nonce BLOB NOT NULL,
                intent_binding TEXT,
                created_at INTEGER NOT NULL,
                last_accessed_at INTEGER
            );",
        )
        .map_err(|e| NyayaError::Vault(format!("Failed to create vault tables: {}", e)))?;

        // Get or create salt
        let salt = match db.query_row(
            "SELECT value FROM vault_meta WHERE key = 'salt'",
            [],
            |row| row.get::<_, Vec<u8>>(0),
        ) {
            Ok(s) => s,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let rng = SystemRandom::new();
                let mut salt = vec![0u8; SALT_LEN];
                rng.fill(&mut salt)
                    .map_err(|_| NyayaError::Vault("RNG failure".into()))?;
                db.execute(
                    "INSERT INTO vault_meta (key, value) VALUES ('salt', ?1)",
                    rusqlite::params![salt],
                )
                .map_err(|e| NyayaError::Vault(format!("Failed to store salt: {}", e)))?;
                salt
            }
            Err(e) => return Err(NyayaError::Vault(format!("Failed to read salt: {}", e))),
        };

        // Derive key from passphrase
        let mut key_bytes = [0u8; KEY_LEN];
        pbkdf2::derive(
            pbkdf2::PBKDF2_HMAC_SHA256,
            NonZeroU32::new(PBKDF2_ITERATIONS).unwrap(),
            &salt,
            passphrase.as_bytes(),
            &mut key_bytes,
        );

        let unbound_key = UnboundKey::new(&AES_256_GCM, &key_bytes)
            .map_err(|_| NyayaError::Vault("Failed to create encryption key".into()))?;
        let key = LessSafeKey::new(unbound_key);

        Ok(Self {
            db,
            key,
            rng: SystemRandom::new(),
            sanitizer_patterns: Vec::new(),
        })
    }

    /// Store an encrypted secret
    pub fn store_secret(
        &self,
        name: &str,
        value: &str,
        intent_binding: Option<&str>,
    ) -> Result<()> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        self.rng
            .fill(&mut nonce_bytes)
            .map_err(|_| NyayaError::Vault("RNG failure".into()))?;

        let nonce = Nonce::assume_unique_for_key(nonce_bytes);
        let mut in_out = value.as_bytes().to_vec();

        self.key
            .seal_in_place_append_tag(nonce, aead::Aad::empty(), &mut in_out)
            .map_err(|_| NyayaError::Vault("Encryption failed".into()))?;

        let now = now_millis();
        self.db.execute(
            "INSERT OR REPLACE INTO secrets (name, encrypted_value, nonce, intent_binding, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![name, in_out, nonce_bytes.to_vec(), intent_binding, now],
        ).map_err(|e| NyayaError::Vault(format!("Failed to store secret: {}", e)))?;

        tracing::info!(name = %name, "Secret stored");
        Ok(())
    }

    /// Retrieve and decrypt a secret, checking intent binding
    pub fn get_secret(&self, name: &str, current_intent: Option<&str>) -> Result<String> {
        let (encrypted, nonce_bytes, binding): (Vec<u8>, Vec<u8>, Option<String>) = self
            .db
            .query_row(
                "SELECT encrypted_value, nonce, intent_binding FROM secrets WHERE name = ?1",
                rusqlite::params![name],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    NyayaError::Vault(format!("Secret '{}' not found", name))
                }
                _ => NyayaError::Vault(format!("Failed to read secret: {}", e)),
            })?;

        // Check intent binding — if a binding exists, require a matching intent
        if let Some(ref binding) = binding {
            match current_intent {
                Some(intent) => {
                    let allowed: Vec<&str> = binding.split('|').collect();
                    if !allowed.contains(&intent) {
                        return Err(NyayaError::Vault(format!(
                            "Secret '{}' not accessible for intent '{}' (bound to: {})",
                            name, intent, binding
                        )));
                    }
                }
                None => {
                    // Intent binding exists but no intent provided — deny access
                    return Err(NyayaError::Vault(format!(
                        "Secret '{}' requires intent binding (bound to: {}) but no intent was provided",
                        name, binding
                    )));
                }
            }
        }

        // Decrypt
        let nonce_arr: [u8; NONCE_LEN] = nonce_bytes
            .try_into()
            .map_err(|_| NyayaError::Vault("Invalid nonce length".into()))?;
        let nonce = Nonce::assume_unique_for_key(nonce_arr);
        let mut in_out = encrypted;

        let plaintext = self
            .key
            .open_in_place(nonce, aead::Aad::empty(), &mut in_out)
            .map_err(|_| NyayaError::Vault("Decryption failed (wrong passphrase?)".into()))?;

        // Update last_accessed_at
        let _ = self.db.execute(
            "UPDATE secrets SET last_accessed_at = ?1 WHERE name = ?2",
            rusqlite::params![now_millis(), name],
        );

        String::from_utf8(plaintext.to_vec())
            .map_err(|_| NyayaError::Vault("Decrypted value is not valid UTF-8".into()))
    }

    /// List all secret names (not values)
    pub fn list_secrets(&self) -> Result<Vec<SecretInfo>> {
        let mut stmt = self
            .db
            .prepare("SELECT name, intent_binding, created_at FROM secrets ORDER BY name")
            .map_err(|e| NyayaError::Vault(format!("Failed to list secrets: {}", e)))?;

        let rows = stmt
            .query_map([], |row| {
                Ok(SecretInfo {
                    name: row.get(0)?,
                    intent_binding: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })
            .map_err(|e| NyayaError::Vault(format!("Query failed: {}", e)))?;

        let mut secrets = Vec::new();
        for row in rows {
            secrets.push(row.map_err(|e| NyayaError::Vault(format!("Row error: {}", e)))?);
        }
        Ok(secrets)
    }

    /// Delete a secret
    pub fn delete_secret(&self, name: &str) -> Result<bool> {
        let changes = self
            .db
            .execute(
                "DELETE FROM secrets WHERE name = ?1",
                rusqlite::params![name],
            )
            .map_err(|e| NyayaError::Vault(format!("Failed to delete secret: {}", e)))?;
        Ok(changes > 0)
    }

    /// Decrypt a secret without checking intent binding (internal use only).
    fn decrypt_secret_raw(&self, name: &str) -> Result<String> {
        let (encrypted, nonce_bytes): (Vec<u8>, Vec<u8>) = self
            .db
            .query_row(
                "SELECT encrypted_value, nonce FROM secrets WHERE name = ?1",
                rusqlite::params![name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    NyayaError::Vault(format!("Secret '{}' not found", name))
                }
                _ => NyayaError::Vault(format!("Failed to read secret: {}", e)),
            })?;

        let nonce_arr: [u8; NONCE_LEN] = nonce_bytes
            .try_into()
            .map_err(|_| NyayaError::Vault("Invalid nonce length".into()))?;
        let nonce = Nonce::assume_unique_for_key(nonce_arr);
        let mut in_out = encrypted;

        let plaintext = self
            .key
            .open_in_place(nonce, aead::Aad::empty(), &mut in_out)
            .map_err(|_| NyayaError::Vault("Decryption failed (wrong passphrase?)".into()))?;

        String::from_utf8(plaintext.to_vec())
            .map_err(|_| NyayaError::Vault("Decrypted value is not valid UTF-8".into()))
    }

    /// Build output sanitizer patterns from all stored secrets.
    /// Call after storing/deleting secrets to update the sanitizer.
    pub fn rebuild_sanitizer(&mut self) -> Result<()> {
        let secrets = self.list_secrets()?;
        let mut patterns = Vec::new();

        for info in &secrets {
            // Use raw decrypt to bypass intent binding (sanitizer needs all secrets)
            if let Ok(value) = self.decrypt_secret_raw(&info.name) {
                if let Ok(pattern) = regex::Regex::new(&regex::escape(&value)) {
                    patterns.push(pattern);
                }
            }
        }

        self.sanitizer_patterns = patterns;
        Ok(())
    }

    /// Sanitize output by replacing any secret values with [REDACTED]
    pub fn sanitize_output(&self, output: &str) -> String {
        let mut result = output.to_string();
        for pattern in &self.sanitizer_patterns {
            result = pattern.replace_all(&result, "[REDACTED]").to_string();
        }
        result
    }
}

#[derive(Debug)]
pub struct SecretInfo {
    pub name: String,
    pub intent_binding: Option<String>,
    pub created_at: i64,
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vault() -> Vault {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("vault.db");
        // We need to leak the tempdir to keep it alive
        let db_path_owned = db_path.to_path_buf();
        std::mem::forget(dir);
        Vault::open(&db_path_owned, "test-passphrase").unwrap()
    }

    #[test]
    fn test_store_and_retrieve() {
        let vault = test_vault();
        vault
            .store_secret("api_key", "sk-secret-123", None)
            .unwrap();

        let value = vault.get_secret("api_key", None).unwrap();
        assert_eq!(value, "sk-secret-123");
    }

    #[test]
    fn test_intent_binding() {
        let vault = test_vault();
        vault
            .store_secret(
                "github_token",
                "ghp_abc123",
                Some("check_infra|monitor_infra"),
            )
            .unwrap();

        // Allowed intent
        let value = vault
            .get_secret("github_token", Some("check_infra"))
            .unwrap();
        assert_eq!(value, "ghp_abc123");

        // Disallowed intent
        let err = vault.get_secret("github_token", Some("check_email"));
        assert!(err.is_err());
    }

    #[test]
    fn test_list_secrets() {
        let vault = test_vault();
        vault.store_secret("key1", "val1", None).unwrap();
        vault
            .store_secret("key2", "val2", Some("check_email"))
            .unwrap();

        let list = vault.list_secrets().unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_delete_secret() {
        let vault = test_vault();
        vault.store_secret("temp_key", "temp_val", None).unwrap();
        assert!(vault.delete_secret("temp_key").unwrap());
        assert!(!vault.delete_secret("nonexistent").unwrap());
    }

    #[test]
    fn test_wrong_passphrase() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("vault.db");

        // Store with one passphrase
        {
            let vault = Vault::open(&db_path, "correct-pass").unwrap();
            vault.store_secret("key", "value", None).unwrap();
        }

        // Try to read with wrong passphrase
        {
            let vault = Vault::open(&db_path, "wrong-pass").unwrap();
            let result = vault.get_secret("key", None);
            assert!(result.is_err());
        }
    }

    #[test]
    fn test_sanitize_output() {
        let mut vault = test_vault();
        vault
            .store_secret("api_key", "sk-secret-123", None)
            .unwrap();
        vault.rebuild_sanitizer().unwrap();

        let output = "Response from API with key sk-secret-123 was successful";
        let sanitized = vault.sanitize_output(output);
        assert!(!sanitized.contains("sk-secret-123"));
        assert!(sanitized.contains("[REDACTED]"));
    }
}
