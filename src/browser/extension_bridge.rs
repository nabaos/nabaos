//! Chrome Extension Bridge — WebSocket server that accepts connections from
//! a companion Chrome extension.
//!
//! The extension sends DOM snapshots, screenshots, and page metadata.
//! The bridge feeds these into the NavCascade for navigation decisions.

use serde::{Deserialize, Serialize};

use crate::core::error::{NyayaError, Result};

// ---------------------------------------------------------------------------
// Extension → Agent messages
// ---------------------------------------------------------------------------

/// Message from the Chrome extension to the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExtensionMessage {
    /// Page loaded — includes URL, title, DOM snapshot.
    PageLoaded {
        url: String,
        title: String,
        dom_snapshot: String,
    },
    /// Screenshot captured (base64 PNG).
    Screenshot { url: String, data_base64: String },
    /// User selected an element (for training data).
    ElementSelected {
        url: String,
        selector: String,
        action: String,
    },
    /// Extension heartbeat / connection alive.
    Ping,
}

// ---------------------------------------------------------------------------
// Agent → Extension messages
// ---------------------------------------------------------------------------

/// Message from the agent to the Chrome extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentMessage {
    /// Navigate to a URL.
    Navigate { url: String },
    /// Click an element.
    Click { selector: String },
    /// Type into an element.
    Type { selector: String, value: String },
    /// Scroll the page.
    Scroll { direction: String },
    /// Take a screenshot.
    RequestScreenshot,
    /// Acknowledge ping.
    Pong,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the extension bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionBridgeConfig {
    /// WebSocket bind address (default: 127.0.0.1:8920).
    #[serde(default = "default_bind")]
    pub bind_addr: String,
    /// Shared secret for authenticating the extension.
    pub auth_token: Option<String>,
}

fn default_bind() -> String {
    "127.0.0.1:8920".into()
}

impl Default for ExtensionBridgeConfig {
    fn default() -> Self {
        Self {
            bind_addr: default_bind(),
            auth_token: None,
        }
    }
}

// ---------------------------------------------------------------------------
// ExtensionBridge
// ---------------------------------------------------------------------------

/// The extension bridge server.
pub struct ExtensionBridge {
    config: ExtensionBridgeConfig,
}

impl ExtensionBridge {
    pub fn new(config: ExtensionBridgeConfig) -> Self {
        Self { config }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &ExtensionBridgeConfig {
        &self.config
    }

    /// Validate an incoming auth token against config.
    /// Uses constant-time comparison to prevent timing attacks.
    pub fn validate_auth(&self, token: &str) -> bool {
        match &self.config.auth_token {
            Some(expected) => {
                // Constant-time comparison to prevent timing attacks
                #[allow(deprecated)]
                ring::constant_time::verify_slices_are_equal(token.as_bytes(), expected.as_bytes())
                    .is_ok()
            }
            None => true, // No auth configured = accept all local connections
        }
    }

    /// Parse an incoming JSON message from the extension.
    pub fn parse_message(json: &str) -> Result<ExtensionMessage> {
        serde_json::from_str(json)
            .map_err(|e| NyayaError::Config(format!("Extension message parse: {}", e)))
    }

    /// Serialize an outgoing message for the extension.
    pub fn serialize_message(msg: &AgentMessage) -> Result<String> {
        serde_json::to_string(msg)
            .map_err(|e| NyayaError::Config(format!("Extension message serialize: {}", e)))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_message_page_loaded_serde() {
        let msg = ExtensionMessage::PageLoaded {
            url: "https://example.com".into(),
            title: "Example".into(),
            dom_snapshot: "<html></html>".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("page_loaded"));
        assert!(json.contains("example.com"));

        let parsed: ExtensionMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            ExtensionMessage::PageLoaded { url, title, .. } => {
                assert_eq!(url, "https://example.com");
                assert_eq!(title, "Example");
            }
            _ => panic!("Expected PageLoaded"),
        }
    }

    #[test]
    fn test_extension_message_screenshot_serde() {
        let msg = ExtensionMessage::Screenshot {
            url: "https://example.com".into(),
            data_base64: "iVBORw0KGgo=".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("screenshot"));

        let parsed: ExtensionMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            ExtensionMessage::Screenshot { data_base64, .. } => {
                assert_eq!(data_base64, "iVBORw0KGgo=");
            }
            _ => panic!("Expected Screenshot"),
        }
    }

    #[test]
    fn test_extension_message_ping_serde() {
        let msg = ExtensionMessage::Ping;
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: ExtensionMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, ExtensionMessage::Ping));
    }

    #[test]
    fn test_agent_message_click_serde() {
        let msg = AgentMessage::Click {
            selector: ".btn".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("click"));
        assert!(json.contains(".btn"));

        let parsed: AgentMessage = serde_json::from_str(&json).unwrap();
        match parsed {
            AgentMessage::Click { selector } => assert_eq!(selector, ".btn"),
            _ => panic!("Expected Click"),
        }
    }

    #[test]
    fn test_agent_message_pong_serde() {
        let msg = AgentMessage::Pong;
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: AgentMessage = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, AgentMessage::Pong));
    }

    #[test]
    fn test_bridge_validate_auth_with_token() {
        let bridge = ExtensionBridge::new(ExtensionBridgeConfig {
            auth_token: Some("secret-123".into()),
            ..Default::default()
        });
        assert!(bridge.validate_auth("secret-123"));
        assert!(!bridge.validate_auth("wrong-token"));
    }

    #[test]
    fn test_bridge_validate_auth_no_token() {
        let bridge = ExtensionBridge::new(ExtensionBridgeConfig::default());
        // No auth configured = accept all
        assert!(bridge.validate_auth("anything"));
        assert!(bridge.validate_auth(""));
    }

    #[test]
    fn test_bridge_parse_message() {
        let json = r#"{"type":"ping"}"#;
        let msg = ExtensionBridge::parse_message(json).unwrap();
        assert!(matches!(msg, ExtensionMessage::Ping));
    }

    #[test]
    fn test_bridge_serialize_message() {
        let msg = AgentMessage::Navigate {
            url: "https://test.com".into(),
        };
        let json = ExtensionBridge::serialize_message(&msg).unwrap();
        assert!(json.contains("navigate"));
        assert!(json.contains("test.com"));
    }

    #[test]
    fn test_bridge_config_defaults() {
        let config = ExtensionBridgeConfig::default();
        assert_eq!(config.bind_addr, "127.0.0.1:8920");
        assert!(config.auth_token.is_none());
    }
}
