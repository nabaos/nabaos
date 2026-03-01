//! Email inbound channel — IMAP client for generic email providers.

use crate::core::error::{NyayaError, Result};
use serde::{Deserialize, Serialize};

/// Shared email message type (used by both Gmail and IMAP paths).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailMessage {
    pub from: String,
    pub to: String,
    pub subject: String,
    pub date: String,
    pub body: String,
    pub message_id: String,
}

/// IMAP connection configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    pub password: String,
    #[serde(default = "default_mailbox")]
    pub mailbox: String,
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
}

fn default_port() -> u16 {
    993
}
fn default_mailbox() -> String {
    "INBOX".to_string()
}
fn default_poll_interval() -> u64 {
    60
}

pub struct ImapChannel {
    config: Option<ImapConfig>,
}

impl ImapChannel {
    /// Create from environment variables.
    pub fn from_env() -> Self {
        let config = match (
            std::env::var("NABA_IMAP_HOST"),
            std::env::var("NABA_IMAP_USER"),
            std::env::var("NABA_IMAP_PASS"),
        ) {
            (Ok(host), Ok(user), Ok(pass)) => Some(ImapConfig {
                host,
                port: std::env::var("NABA_IMAP_PORT")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(993),
                username: user,
                password: pass,
                mailbox: std::env::var("NABA_IMAP_MAILBOX").unwrap_or_else(|_| "INBOX".to_string()),
                poll_interval_secs: std::env::var("NABA_IMAP_POLL_INTERVAL")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(60),
            }),
            _ => None,
        };
        Self { config }
    }

    pub fn is_configured(&self) -> bool {
        self.config.is_some()
    }

    pub fn config(&self) -> Option<&ImapConfig> {
        self.config.as_ref()
    }

    /// Fetch unseen messages from the configured IMAP mailbox.
    pub async fn fetch_unseen(&self) -> Result<Vec<EmailMessage>> {
        let config = self
            .config
            .as_ref()
            .ok_or_else(|| NyayaError::Config("IMAP not configured".to_string()))?;

        // Connect via TCP, then upgrade to TLS.
        let tcp_stream = tokio::net::TcpStream::connect((config.host.as_str(), config.port))
            .await
            .map_err(|e| NyayaError::Config(format!("IMAP TCP connect error: {e}")))?;

        let tls = async_native_tls::TlsConnector::new();
        let tls_stream = tls
            .connect(&config.host, tcp_stream)
            .await
            .map_err(|e| NyayaError::Config(format!("IMAP TLS error: {e}")))?;

        let mut client = async_imap::Client::new(tls_stream);

        // Read the server greeting.
        let _greeting = client
            .read_response()
            .await
            .ok_or_else(|| NyayaError::Config("IMAP: no greeting from server".to_string()))?
            .map_err(|e| NyayaError::Config(format!("IMAP greeting error: {e}")))?;

        let mut session = client
            .login(&config.username, &config.password)
            .await
            .map_err(|e| NyayaError::Config(format!("IMAP login error: {}", e.0)))?;

        session
            .select(&config.mailbox)
            .await
            .map_err(|e| NyayaError::Config(format!("IMAP select error: {e}")))?;

        let unseen = session
            .search("UNSEEN")
            .await
            .map_err(|e| NyayaError::Config(format!("IMAP search error: {e}")))?;

        if unseen.is_empty() {
            session.logout().await.ok();
            return Ok(Vec::new());
        }

        let seq_set: String = unseen
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let messages_stream = session
            .fetch(&seq_set, "ENVELOPE BODY[TEXT]")
            .await
            .map_err(|e| NyayaError::Config(format!("IMAP fetch error: {e}")))?;

        use futures::StreamExt;
        let fetched: Vec<_> = messages_stream.collect().await;
        let mut messages = Vec::new();

        for item in fetched {
            let fetch = match item {
                Ok(f) => f,
                Err(e) => {
                    tracing::warn!("IMAP fetch item error: {e}");
                    continue;
                }
            };
            if let Some(envelope) = fetch.envelope() {
                let from = envelope
                    .from
                    .as_ref()
                    .and_then(|addrs| addrs.first())
                    .map(|a| {
                        let mailbox = a
                            .mailbox
                            .as_ref()
                            .map(|m| String::from_utf8_lossy(m).to_string())
                            .unwrap_or_default();
                        let host = a
                            .host
                            .as_ref()
                            .map(|h| String::from_utf8_lossy(h).to_string())
                            .unwrap_or_default();
                        format!("{}@{}", mailbox, host)
                    })
                    .unwrap_or_default();
                let subject = envelope
                    .subject
                    .as_ref()
                    .map(|s| String::from_utf8_lossy(s).to_string())
                    .unwrap_or_default();
                let date = envelope
                    .date
                    .as_ref()
                    .map(|d| String::from_utf8_lossy(d).to_string())
                    .unwrap_or_default();
                let message_id = envelope
                    .message_id
                    .as_ref()
                    .map(|m| String::from_utf8_lossy(m).to_string())
                    .unwrap_or_default();
                let body = fetch
                    .text()
                    .map(|b| String::from_utf8_lossy(b).to_string())
                    .unwrap_or_default();

                messages.push(EmailMessage {
                    from,
                    to: String::new(),
                    subject,
                    date,
                    body,
                    message_id,
                });
            }
        }

        session.logout().await.ok();
        Ok(messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_imap_config_defaults() {
        let config: ImapConfig = serde_json::from_str(
            r#"{
            "host": "imap.example.com",
            "username": "user",
            "password": "pass"
        }"#,
        )
        .unwrap();
        assert_eq!(config.port, 993);
        assert_eq!(config.mailbox, "INBOX");
        assert_eq!(config.poll_interval_secs, 60);
    }

    #[test]
    fn test_imap_channel_not_configured() {
        unsafe { std::env::remove_var("NABA_IMAP_HOST"); }
        unsafe { std::env::remove_var("NABA_IMAP_USER"); }
        unsafe { std::env::remove_var("NABA_IMAP_PASS"); }
        let channel = ImapChannel::from_env();
        assert!(!channel.is_configured());
    }

    #[tokio::test]
    async fn test_fetch_unseen_not_configured_returns_error() {
        unsafe { std::env::remove_var("NABA_IMAP_HOST"); }
        let channel = ImapChannel::from_env();
        let result = channel.fetch_unseen().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("IMAP not configured"));
    }

    #[test]
    fn test_email_message_serialization() {
        let msg = EmailMessage {
            from: "alice@example.com".to_string(),
            to: "bob@example.com".to_string(),
            subject: "Hello".to_string(),
            date: "2026-02-26".to_string(),
            body: "Hi Bob".to_string(),
            message_id: "<abc@example.com>".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: EmailMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.from, "alice@example.com");
        assert_eq!(parsed.subject, "Hello");
    }
}
