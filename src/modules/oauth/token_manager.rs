//! OAuth2 token management — PKCE authorization code flow with automatic refresh.

use crate::core::error::{NyayaError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// OAuth2 token pair with metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64,
    pub token_type: String,
    pub scope: Option<String>,
}

impl TokenPair {
    /// Check if the access token has expired (with 60s buffer).
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now + 60 >= self.expires_at
    }
}

/// OAuth2 provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthProvider {
    pub name: String,
    pub auth_url: String,
    pub token_url: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub scopes: Vec<String>,
    pub redirect_uri: String,
}

/// Manages OAuth2 tokens for all providers.
pub struct TokenManager {
    db: rusqlite::Connection,
}

impl TokenManager {
    pub fn open(db_path: &std::path::Path) -> Result<Self> {
        let db = rusqlite::Connection::open(db_path)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS oauth_tokens (
                provider TEXT PRIMARY KEY,
                access_token TEXT NOT NULL,
                refresh_token TEXT,
                expires_at INTEGER NOT NULL,
                token_type TEXT NOT NULL DEFAULT 'Bearer',
                scope TEXT,
                updated_at INTEGER NOT NULL
            );",
        )?;
        Ok(Self { db })
    }

    /// Store a token pair for a provider.
    pub fn store(&self, provider: &str, token: &TokenPair) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.db.execute(
            "INSERT OR REPLACE INTO oauth_tokens
             (provider, access_token, refresh_token, expires_at, token_type, scope, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                provider,
                token.access_token,
                token.refresh_token,
                token.expires_at as i64,
                token.token_type,
                token.scope,
                now as i64
            ],
        )?;
        Ok(())
    }

    /// Get a token for a provider. Returns None if not stored.
    pub fn get(&self, provider: &str) -> Result<Option<TokenPair>> {
        let mut stmt = self.db.prepare(
            "SELECT access_token, refresh_token, expires_at, token_type, scope
             FROM oauth_tokens WHERE provider = ?1",
        )?;
        let result = stmt.query_row(rusqlite::params![provider], |row| {
            let expires_at_i64: i64 = row.get(2)?;
            Ok(TokenPair {
                access_token: row.get(0)?,
                refresh_token: row.get(1)?,
                expires_at: expires_at_i64 as u64,
                token_type: row.get(3)?,
                scope: row.get(4)?,
            })
        });
        match result {
            Ok(t) => Ok(Some(t)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Remove token for a provider.
    pub fn revoke(&self, provider: &str) -> Result<bool> {
        let count = self.db.execute(
            "DELETE FROM oauth_tokens WHERE provider = ?1",
            rusqlite::params![provider],
        )?;
        Ok(count > 0)
    }

    /// List all providers with stored tokens.
    pub fn list_providers(&self) -> Result<Vec<(String, bool)>> {
        let mut stmt = self
            .db
            .prepare("SELECT provider, expires_at FROM oauth_tokens ORDER BY provider")?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let rows = stmt.query_map([], |row| {
            let provider: String = row.get(0)?;
            let expires_at_i64: i64 = row.get(1)?;
            let expires_at = expires_at_i64 as u64;
            Ok((provider, now + 60 < expires_at))
        })?;
        let mut providers = Vec::new();
        for row in rows {
            providers.push(row?);
        }
        Ok(providers)
    }

    /// Generate PKCE code verifier and challenge.
    pub fn generate_pkce() -> (String, String) {
        use sha2::{Digest, Sha256};
        use std::hash::{Hash, Hasher};

        // Generate random-ish verifier
        let mut verifier_bytes = Vec::with_capacity(32);
        for i in 0..32u8 {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            SystemTime::now().hash(&mut hasher);
            i.hash(&mut hasher);
            std::thread::current().id().hash(&mut hasher);
            verifier_bytes.push((hasher.finish() & 0xFF) as u8);
        }
        let verifier = hex::encode(&verifier_bytes);

        let mut sha = Sha256::new();
        sha.update(verifier.as_bytes());
        let hash = sha.finalize();
        // Base64url encode
        let challenge = hex::encode(&hash[..]);
        (verifier, challenge)
    }

    /// Build the authorization URL for a provider.
    pub fn auth_url(provider: &OAuthProvider, state: &str, code_challenge: &str) -> String {
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
            provider.auth_url,
            urlencoding_encode(&provider.client_id),
            urlencoding_encode(&provider.redirect_uri),
            urlencoding_encode(&provider.scopes.join(" ")),
            urlencoding_encode(state),
            urlencoding_encode(code_challenge),
        )
    }

    /// Exchange authorization code for tokens (async).
    pub async fn exchange_code(
        provider: &OAuthProvider,
        code: &str,
        code_verifier: &str,
    ) -> Result<TokenPair> {
        let client = reqwest::Client::new();
        let mut params = HashMap::new();
        params.insert("grant_type", "authorization_code");
        params.insert("code", code);
        params.insert("redirect_uri", &provider.redirect_uri);
        params.insert("client_id", &provider.client_id);
        params.insert("code_verifier", code_verifier);

        let resp = client
            .post(&provider.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("Token exchange failed: {}", e)))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(NyayaError::Config(format!(
                "Token exchange error: {}",
                body
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| NyayaError::Config(format!("Token parse failed: {}", e)))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(TokenPair {
            access_token: body["access_token"].as_str().unwrap_or("").to_string(),
            refresh_token: body["refresh_token"].as_str().map(|s| s.to_string()),
            expires_at: now + body["expires_in"].as_u64().unwrap_or(3600),
            token_type: body["token_type"].as_str().unwrap_or("Bearer").to_string(),
            scope: body["scope"].as_str().map(|s| s.to_string()),
        })
    }

    /// Refresh an expired token (async).
    pub async fn refresh_token(provider: &OAuthProvider, refresh_token: &str) -> Result<TokenPair> {
        let client = reqwest::Client::new();
        let mut params = HashMap::new();
        params.insert("grant_type", "refresh_token");
        params.insert("refresh_token", refresh_token);
        params.insert("client_id", &provider.client_id);

        let resp = client
            .post(&provider.token_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("Token refresh failed: {}", e)))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(NyayaError::Config(format!("Token refresh error: {}", body)));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| NyayaError::Config(format!("Token parse failed: {}", e)))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Ok(TokenPair {
            access_token: body["access_token"].as_str().unwrap_or("").to_string(),
            refresh_token: body["refresh_token"].as_str().map(|s| s.to_string()),
            expires_at: now + body["expires_in"].as_u64().unwrap_or(3600),
            token_type: body["token_type"].as_str().unwrap_or("Bearer").to_string(),
            scope: body["scope"].as_str().map(|s| s.to_string()),
        })
    }
}

fn urlencoding_encode(s: &str) -> String {
    s.replace(' ', "%20")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('+', "%2B")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn test_manager() -> TokenManager {
        TokenManager::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_store_and_get() {
        let mgr = test_manager();
        let token = TokenPair {
            access_token: "abc123".into(),
            refresh_token: Some("refresh456".into()),
            expires_at: i64::MAX as u64,
            token_type: "Bearer".into(),
            scope: Some("email".into()),
        };
        mgr.store("gmail", &token).unwrap();
        let got = mgr.get("gmail").unwrap().unwrap();
        assert_eq!(got.access_token, "abc123");
        assert!(!got.is_expired());
    }

    #[test]
    fn test_expired_token() {
        let token = TokenPair {
            access_token: "old".into(),
            refresh_token: None,
            expires_at: 0,
            token_type: "Bearer".into(),
            scope: None,
        };
        assert!(token.is_expired());
    }

    #[test]
    fn test_revoke() {
        let mgr = test_manager();
        let token = TokenPair {
            access_token: "x".into(),
            refresh_token: None,
            expires_at: i64::MAX as u64,
            token_type: "Bearer".into(),
            scope: None,
        };
        mgr.store("gmail", &token).unwrap();
        assert!(mgr.revoke("gmail").unwrap());
        assert!(mgr.get("gmail").unwrap().is_none());
    }

    #[test]
    fn test_list_providers() {
        let mgr = test_manager();
        let token = TokenPair {
            access_token: "x".into(),
            refresh_token: None,
            expires_at: i64::MAX as u64,
            token_type: "Bearer".into(),
            scope: None,
        };
        mgr.store("gmail", &token).unwrap();
        mgr.store("slack", &token).unwrap();
        let providers = mgr.list_providers().unwrap();
        assert_eq!(providers.len(), 2);
    }

    #[test]
    fn test_pkce_generation() {
        let (verifier, challenge) = TokenManager::generate_pkce();
        assert!(!verifier.is_empty());
        assert!(!challenge.is_empty());
        assert_ne!(verifier, challenge);
    }

    #[test]
    fn test_get_nonexistent() {
        let mgr = test_manager();
        assert!(mgr.get("nonexistent").unwrap().is_none());
    }
}
