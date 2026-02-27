//! WhatsApp Cloud API channel — sends and receives messages via Meta's Graph API.

use crate::core::error::{NyayaError, Result};
use serde::Deserialize;

const GRAPH_API_BASE: &str = "https://graph.facebook.com/v21.0";

pub struct WhatsAppChannel {
    token: Option<String>,
    phone_id: Option<String>,
    verify_token: Option<String>,
    client: reqwest::blocking::Client,
}

// Webhook payload types
#[derive(Debug, Deserialize)]
pub struct WhatsAppWebhookPayload {
    pub object: String,
    pub entry: Vec<WebhookEntry>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookEntry {
    pub id: String,
    pub changes: Vec<WebhookChange>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookChange {
    pub value: WebhookValue,
}

#[derive(Debug, Deserialize)]
pub struct WebhookValue {
    pub messaging_product: Option<String>,
    #[serde(default)]
    pub messages: Option<Vec<IncomingMessage>>,
    #[serde(default)]
    pub contacts: Option<Vec<WebhookContact>>,
}

#[derive(Debug, Deserialize)]
pub struct IncomingMessage {
    pub from: String,
    pub id: String,
    pub timestamp: String,
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub text: Option<TextBody>,
}

#[derive(Debug, Deserialize)]
pub struct TextBody {
    pub body: String,
}

#[derive(Debug, Deserialize)]
pub struct WebhookContact {
    pub wa_id: String,
    #[serde(default)]
    pub profile: Option<ContactProfile>,
}

#[derive(Debug, Deserialize)]
pub struct ContactProfile {
    pub name: Option<String>,
}

/// Extracted message from webhook payload
#[derive(Debug, Clone)]
pub struct ParsedMessage {
    pub from: String,
    pub text: String,
    pub message_id: String,
    pub contact_name: Option<String>,
}

impl Default for WhatsAppChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl WhatsAppChannel {
    pub fn new() -> Self {
        Self {
            token: std::env::var("NABA_WHATSAPP_TOKEN").ok(),
            phone_id: std::env::var("NABA_WHATSAPP_PHONE_ID").ok(),
            verify_token: std::env::var("NABA_WHATSAPP_VERIFY_TOKEN").ok(),
            client: reqwest::blocking::Client::new(),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.token.is_some() && self.phone_id.is_some()
    }

    /// Verify webhook subscription (GET request from Meta).
    /// Returns the challenge string if verification passes.
    pub fn verify_webhook(&self, mode: &str, token: &str, challenge: &str) -> Option<String> {
        if mode == "subscribe" && Some(token.to_string()) == self.verify_token {
            Some(challenge.to_string())
        } else {
            None
        }
    }

    /// Parse incoming webhook POST payload into messages.
    pub fn parse_webhook(&self, body: &str) -> Result<Vec<ParsedMessage>> {
        let payload: WhatsAppWebhookPayload = serde_json::from_str(body)
            .map_err(|e| NyayaError::Config(format!("WhatsApp webhook parse error: {}", e)))?;

        let mut messages = Vec::new();
        for entry in &payload.entry {
            for change in &entry.changes {
                // Build contact name lookup
                let contact_names: std::collections::HashMap<String, String> = change
                    .value
                    .contacts
                    .as_ref()
                    .map(|contacts| {
                        contacts
                            .iter()
                            .filter_map(|c| {
                                c.profile
                                    .as_ref()
                                    .and_then(|p| p.name.as_ref())
                                    .map(|name| (c.wa_id.clone(), name.clone()))
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                if let Some(ref msgs) = change.value.messages {
                    for msg in msgs {
                        if let Some(ref text) = msg.text {
                            messages.push(ParsedMessage {
                                from: msg.from.clone(),
                                text: text.body.clone(),
                                message_id: msg.id.clone(),
                                contact_name: contact_names.get(&msg.from).cloned(),
                            });
                        }
                    }
                }
            }
        }
        Ok(messages)
    }

    /// Send a text message to a WhatsApp number.
    pub fn send_message(&self, to: &str, text: &str) -> Result<()> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| NyayaError::Config("WhatsApp token not configured".into()))?;
        let phone_id = self
            .phone_id
            .as_ref()
            .ok_or_else(|| NyayaError::Config("WhatsApp phone ID not configured".into()))?;

        let url = format!("{}/{}/messages", GRAPH_API_BASE, phone_id);
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": to,
            "type": "text",
            "text": { "body": text }
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .map_err(|e| NyayaError::Config(format!("WhatsApp send error: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(NyayaError::Config(format!(
                "WhatsApp API error {}: {}",
                status, body
            )));
        }

        tracing::info!(to = to, len = text.len(), "WhatsApp: message sent");
        Ok(())
    }

    /// Mark a message as read.
    pub fn mark_read(&self, message_id: &str) -> Result<()> {
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| NyayaError::Config("WhatsApp token not configured".into()))?;
        let phone_id = self
            .phone_id
            .as_ref()
            .ok_or_else(|| NyayaError::Config("WhatsApp phone ID not configured".into()))?;

        let url = format!("{}/{}/messages", GRAPH_API_BASE, phone_id);
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "status": "read",
            "message_id": message_id
        });

        self.client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .map_err(|e| NyayaError::Config(format!("WhatsApp mark read error: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_webhook_valid() {
        let mut ch = WhatsAppChannel::new();
        ch.verify_token = Some("test_verify".to_string());
        let result = ch.verify_webhook("subscribe", "test_verify", "challenge123");
        assert_eq!(result, Some("challenge123".to_string()));
    }

    #[test]
    fn test_verify_webhook_invalid() {
        let mut ch = WhatsAppChannel::new();
        ch.verify_token = Some("correct_token".to_string());
        let result = ch.verify_webhook("subscribe", "wrong_token", "challenge");
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_webhook_text_message() {
        let ch = WhatsAppChannel::new();
        let payload = serde_json::json!({
            "object": "whatsapp_business_account",
            "entry": [{
                "id": "123",
                "changes": [{
                    "value": {
                        "messaging_product": "whatsapp",
                        "contacts": [{
                            "wa_id": "919999999999",
                            "profile": {"name": "Alice"}
                        }],
                        "messages": [{
                            "from": "919999999999",
                            "id": "wamid.abc",
                            "timestamp": "1234567890",
                            "type": "text",
                            "text": {"body": "Hello bot"}
                        }]
                    }
                }]
            }]
        });
        let messages = ch.parse_webhook(&payload.to_string()).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].from, "919999999999");
        assert_eq!(messages[0].text, "Hello bot");
        assert_eq!(messages[0].contact_name.as_deref(), Some("Alice"));
    }

    #[test]
    fn test_send_message_requires_config() {
        let ch = WhatsAppChannel {
            token: None,
            phone_id: None,
            verify_token: None,
            client: reqwest::blocking::Client::new(),
        };
        let result = ch.send_message("+1234", "hello");
        assert!(result.is_err());
    }

    #[test]
    fn test_is_configured() {
        let ch = WhatsAppChannel {
            token: Some("t".into()),
            phone_id: Some("p".into()),
            verify_token: None,
            client: reqwest::blocking::Client::new(),
        };
        assert!(ch.is_configured());
        let ch2 = WhatsAppChannel {
            token: None,
            phone_id: Some("p".into()),
            verify_token: None,
            client: reqwest::blocking::Client::new(),
        };
        assert!(!ch2.is_configured());
    }
}
