//! Discord bot channel — outbound messaging via serenity HTTP client.
//! Requires NABA_DISCORD_BOT_TOKEN env var.

use crate::core::error::{NyayaError, Result};
use serde::{Deserialize, Serialize};

/// Discord channel for outbound messaging.
pub struct DiscordChannel {
    token: Option<String>,
}

/// Embed structure for rich Discord messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordEmbed {
    pub title: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<u32>,
}

impl Default for DiscordChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl DiscordChannel {
    pub fn new() -> Self {
        Self {
            token: std::env::var("NABA_DISCORD_BOT_TOKEN").ok(),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.token.is_some()
    }

    /// Send a plain text message to a Discord channel.
    pub async fn send_message(&self, channel_id: &str, text: &str) -> Result<()> {
        let token = self.token.as_deref().ok_or_else(|| {
            NyayaError::Config(
                "Discord bot token not configured (set NABA_DISCORD_BOT_TOKEN)".into(),
            )
        })?;

        let channel_id_u64: u64 = channel_id.parse().map_err(|_| {
            NyayaError::Config(format!("Invalid Discord channel ID: {}", channel_id))
        })?;

        let http = serenity::http::Http::new(token);
        let channel = serenity::model::id::ChannelId::new(channel_id_u64);

        channel
            .say(&http, text)
            .await
            .map_err(|e| NyayaError::Config(format!("Discord send failed: {}", e)))?;

        tracing::info!(
            channel_id = channel_id,
            len = text.len(),
            "Discord: message sent"
        );
        Ok(())
    }

    /// Send a rich embed message to a Discord channel.
    pub async fn send_embed(&self, channel_id: &str, embed: &DiscordEmbed) -> Result<()> {
        let token = self.token.as_deref().ok_or_else(|| {
            NyayaError::Config(
                "Discord bot token not configured (set NABA_DISCORD_BOT_TOKEN)".into(),
            )
        })?;

        let channel_id_u64: u64 = channel_id.parse().map_err(|_| {
            NyayaError::Config(format!("Invalid Discord channel ID: {}", channel_id))
        })?;

        let http = serenity::http::Http::new(token);
        let channel = serenity::model::id::ChannelId::new(channel_id_u64);

        channel
            .send_message(
                &http,
                serenity::builder::CreateMessage::new().embed(
                    serenity::builder::CreateEmbed::new()
                        .title(&embed.title)
                        .description(&embed.description)
                        .color(embed.color.unwrap_or(0x5865F2)),
                ),
            )
            .await
            .map_err(|e| NyayaError::Config(format!("Discord embed send failed: {}", e)))?;

        tracing::info!(channel_id = channel_id, title = %embed.title, "Discord: embed sent");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_discord_not_configured_without_env() {
        unsafe { std::env::remove_var("NABA_DISCORD_BOT_TOKEN"); }
        let channel = DiscordChannel::new();
        assert!(!channel.is_configured());
    }

    #[tokio::test]
    async fn test_send_message_without_token_returns_error() {
        unsafe { std::env::remove_var("NABA_DISCORD_BOT_TOKEN"); }
        let channel = DiscordChannel::new();
        let result = channel.send_message("123456", "test").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not configured"));
    }

    #[tokio::test]
    async fn test_send_message_invalid_channel_id() {
        let channel = DiscordChannel {
            token: Some("fake-token".into()),
        };
        let result = channel.send_message("not-a-number", "test").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Invalid Discord channel ID"));
    }

    #[test]
    fn test_discord_embed_serialization() {
        let embed = DiscordEmbed {
            title: "Test".into(),
            description: "Description".into(),
            color: Some(0xFF0000),
        };
        let json = serde_json::to_string(&embed).unwrap();
        assert!(json.contains("\"title\":\"Test\""));
        assert!(json.contains("\"color\":16711680"));
    }
}
