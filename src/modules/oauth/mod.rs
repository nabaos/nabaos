// OAuth framework module — connector types, token management, and configuration.

pub mod calendar;
pub mod drive;
pub mod gmail;
pub mod notion;
pub mod slack;
pub mod token_manager;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Supported OAuth connector types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorType {
    Gmail,
    Calendar,
    Drive,
    Slack,
    Notion,
}

impl ConnectorType {
    /// Return the names of all supported connector types.
    pub fn all_names() -> &'static [&'static str] {
        &["gmail", "calendar", "drive", "slack", "notion"]
    }

    /// Parse a connector type from its lowercase name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "gmail" => Some(ConnectorType::Gmail),
            "calendar" => Some(ConnectorType::Calendar),
            "drive" => Some(ConnectorType::Drive),
            "slack" => Some(ConnectorType::Slack),
            "notion" => Some(ConnectorType::Notion),
            _ => None,
        }
    }
}

/// An OAuth token with optional refresh and expiry metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>,
    pub scopes: Vec<String>,
}

impl OAuthToken {
    /// Returns true if the token has expired (with a 60-second safety buffer).
    /// Tokens without an expiry are considered non-expired.
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            None => false,
            Some(exp) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                now + 60 >= exp
            }
        }
    }
}

/// Configuration for a single OAuth connector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorConfig {
    pub enabled: bool,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

/// Top-level OAuth configuration holding all connector configs.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuthConfig {
    pub connectors: HashMap<String, ConnectorConfig>,
}

impl OAuthConfig {
    /// Load OAuth configuration from environment variables.
    ///
    /// Checks for patterns like:
    ///   NABA_OAUTH_GMAIL=1   (enables the connector)
    ///   NABA_GMAIL_CLIENT_ID=...
    ///   NABA_GMAIL_CLIENT_SECRET=...
    pub fn from_env_safe() -> Self {
        let mut connectors = HashMap::new();

        for name in ConnectorType::all_names() {
            let upper = name.to_uppercase();
            let enabled_key = format!("NABA_OAUTH_{}", upper);
            let client_id_key = format!("NABA_{}_CLIENT_ID", upper);
            let client_secret_key = format!("NABA_{}_CLIENT_SECRET", upper);

            let enabled = std::env::var(&enabled_key)
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false);

            let client_id = std::env::var(&client_id_key).ok();
            let client_secret = std::env::var(&client_secret_key).ok();

            if enabled || client_id.is_some() || client_secret.is_some() {
                connectors.insert(
                    name.to_string(),
                    ConnectorConfig {
                        enabled,
                        client_id,
                        client_secret,
                    },
                );
            }
        }

        OAuthConfig { connectors }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oauth_config_from_env() {
        let config = OAuthConfig::from_env_safe();
        // Without env vars, should have no enabled connectors
        assert!(config.connectors.is_empty() || !config.connectors.values().any(|c| c.enabled));
    }

    #[test]
    fn test_connector_names() {
        let names = ConnectorType::all_names();
        assert!(names.contains(&"gmail"));
        assert!(names.contains(&"calendar"));
        assert!(names.contains(&"slack"));
        assert!(names.contains(&"notion"));
    }

    #[test]
    fn test_token_storage_roundtrip() {
        let token = OAuthToken {
            access_token: "test_access".into(),
            refresh_token: Some("test_refresh".into()),
            expires_at: Some(1700000000),
            scopes: vec!["read".into()],
        };
        let json = serde_json::to_string(&token).unwrap();
        let parsed: OAuthToken = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.access_token, "test_access");
        assert_eq!(parsed.refresh_token, Some("test_refresh".into()));
    }

    #[test]
    fn test_token_is_expired() {
        let expired = OAuthToken {
            access_token: "x".into(),
            refresh_token: None,
            expires_at: Some(1000),
            scopes: vec![],
        };
        assert!(expired.is_expired());

        let valid = OAuthToken {
            access_token: "x".into(),
            refresh_token: None,
            expires_at: Some(u64::MAX),
            scopes: vec![],
        };
        assert!(!valid.is_expired());

        let no_expiry = OAuthToken {
            access_token: "x".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
        };
        assert!(!no_expiry.is_expired());
    }
}
