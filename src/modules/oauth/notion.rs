// Notion OAuth connector stub.

use super::ConnectorConfig;

/// Required OAuth scopes for Notion access.
pub const SCOPES: &[&str] = &["read_content", "update_content", "insert_content"];

/// Notion connector for reading and writing pages via the Notion API.
pub struct NotionConnector {
    config: ConnectorConfig,
}

impl NotionConnector {
    /// Create a new Notion connector from its configuration.
    pub fn new(config: ConnectorConfig) -> Self {
        Self { config }
    }

    /// Returns true if client credentials are present and the connector is enabled.
    pub fn is_configured(&self) -> bool {
        self.config.enabled
            && self.config.client_id.is_some()
            && self.config.client_secret.is_some()
    }

    /// Build the Notion OAuth2 authorization URL.
    pub fn auth_url(&self) -> Option<String> {
        let client_id = self.config.client_id.as_ref()?;
        Some(format!(
            "https://api.notion.com/v1/oauth/authorize?client_id={}&response_type=code&owner=user",
            client_id
        ))
    }
}
