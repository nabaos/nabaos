// Slack OAuth connector stub.

use super::ConnectorConfig;

/// Required OAuth scopes for Slack access.
pub const SCOPES: &[&str] = &["channels:read", "chat:write", "users:read"];

/// Slack connector for messaging via the Slack API.
pub struct SlackConnector {
    config: ConnectorConfig,
}

impl SlackConnector {
    /// Create a new Slack connector from its configuration.
    pub fn new(config: ConnectorConfig) -> Self {
        Self { config }
    }

    /// Returns true if client credentials are present and the connector is enabled.
    pub fn is_configured(&self) -> bool {
        self.config.enabled
            && self.config.client_id.is_some()
            && self.config.client_secret.is_some()
    }

    /// Build the Slack OAuth2 authorization URL.
    pub fn auth_url(&self) -> Option<String> {
        let client_id = self.config.client_id.as_ref()?;
        let scopes = SCOPES.join(",");
        Some(format!(
            "https://slack.com/oauth/v2/authorize?client_id={}&scope={}&response_type=code",
            client_id, scopes
        ))
    }
}
