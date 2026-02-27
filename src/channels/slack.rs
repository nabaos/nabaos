//! Slack Web API channel — outbound messaging via chat.postMessage.

use crate::core::error::{NyayaError, Result};
use serde::{Deserialize, Serialize};

const SLACK_API_BASE: &str = "https://slack.com/api";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackBlock {
    #[serde(rename = "type")]
    pub block_type: SlackBlockType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<SlackText>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlackBlockType {
    Section,
    Header,
    Divider,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackText {
    #[serde(rename = "type")]
    pub text_type: String,
    pub text: String,
}

pub struct SlackChannel {
    token: Option<String>,
    signing_secret: Option<String>,
    client: reqwest::Client,
}

impl Default for SlackChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl SlackChannel {
    pub fn new() -> Self {
        Self {
            token: std::env::var("NABA_SLACK_BOT_TOKEN").ok(),
            signing_secret: std::env::var("NABA_SLACK_SIGNING_SECRET").ok(),
            client: reqwest::Client::new(),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.token.is_some()
    }

    pub fn has_signing_secret(&self) -> bool {
        self.signing_secret.is_some()
    }

    pub async fn send_message(&self, channel: &str, text: &str) -> Result<()> {
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| NyayaError::Config("NABA_SLACK_BOT_TOKEN not set".to_string()))?;
        let body = serde_json::json!({
            "channel": channel,
            "text": text,
        });
        let resp = self
            .client
            .post(format!("{}/chat.postMessage", SLACK_API_BASE))
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("Slack API error: {e}")))?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| NyayaError::Config(format!("Slack response parse error: {e}")))?;
        if json.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let err = json
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(NyayaError::Config(format!(
                "Slack API returned error: {err}"
            )));
        }
        Ok(())
    }

    pub async fn send_blocks(
        &self,
        channel: &str,
        text: &str,
        blocks: &[SlackBlock],
    ) -> Result<()> {
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| NyayaError::Config("NABA_SLACK_BOT_TOKEN not set".to_string()))?;
        let body = serde_json::json!({
            "channel": channel,
            "text": text,
            "blocks": blocks,
        });
        let resp = self
            .client
            .post(format!("{}/chat.postMessage", SLACK_API_BASE))
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await
            .map_err(|e| NyayaError::Config(format!("Slack API error: {e}")))?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| NyayaError::Config(format!("Slack response parse error: {e}")))?;
        if json.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let err = json
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            return Err(NyayaError::Config(format!(
                "Slack API returned error: {err}"
            )));
        }
        Ok(())
    }

    pub fn verify_signature(&self, timestamp: &str, body: &str, signature: &str) -> bool {
        let secret = match self.signing_secret.as_deref() {
            Some(s) => s,
            None => return false,
        };
        let basestring = format!("v0:{}:{}", timestamp, body);
        use ring::hmac;
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
        let tag = hmac::sign(&key, basestring.as_bytes());
        let expected = format!("v0={}", hex::encode(tag.as_ref()));
        // Constant-time comparison of hex-encoded HMAC signatures
        expected.as_bytes() == signature.as_bytes()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slack_not_configured_without_token() {
        std::env::remove_var("NABA_SLACK_BOT_TOKEN");
        let channel = SlackChannel::new();
        assert!(!channel.is_configured());
    }

    #[tokio::test]
    async fn test_send_message_without_token_returns_error() {
        std::env::remove_var("NABA_SLACK_BOT_TOKEN");
        let channel = SlackChannel::new();
        let result = channel.send_message("#general", "hello").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("NABA_SLACK_BOT_TOKEN"));
    }

    #[test]
    fn test_slack_block_serialization() {
        let block = SlackBlock {
            block_type: SlackBlockType::Section,
            text: Some(SlackText {
                text_type: "mrkdwn".into(),
                text: "Hello *world*".into(),
            }),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "section");
        assert_eq!(json["text"]["type"], "mrkdwn");
        assert_eq!(json["text"]["text"], "Hello *world*");
    }

    #[test]
    fn test_slack_divider_block_no_text() {
        let block = SlackBlock {
            block_type: SlackBlockType::Divider,
            text: None,
        };
        let json = serde_json::to_value(&block).unwrap();
        assert_eq!(json["type"], "divider");
        assert!(json.get("text").is_none());
    }

    #[test]
    fn test_verify_signature_no_secret() {
        std::env::remove_var("NABA_SLACK_SIGNING_SECRET");
        let channel = SlackChannel::new();
        assert!(!channel.verify_signature("12345", "body", "v0=abc"));
    }

    #[test]
    fn test_verify_signature_valid() {
        // Compute expected HMAC for known inputs
        let secret = "test_signing_secret";
        let timestamp = "1234567890";
        let body = r#"{"type":"event_callback"}"#;
        let basestring = format!("v0:{}:{}", timestamp, body);
        use ring::hmac;
        let key = hmac::Key::new(hmac::HMAC_SHA256, secret.as_bytes());
        let tag = hmac::sign(&key, basestring.as_bytes());
        let valid_sig = format!("v0={}", hex::encode(tag.as_ref()));

        let channel = SlackChannel {
            token: None,
            signing_secret: Some(secret.to_string()),
            client: reqwest::Client::new(),
        };
        assert!(channel.verify_signature(timestamp, body, &valid_sig));
        assert!(!channel.verify_signature(timestamp, body, "v0=invalid"));
    }
}
