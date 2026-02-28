# Discord Integration

> **What you'll learn**
>
> - How to create a Discord application and bot account
> - How to invite the bot to your server
> - How to configure NabaOS to connect to Discord
> - What Discord can and cannot do (outbound-only)

---

## Prerequisites

- NabaOS installed (`nabaos --version`)
- A Discord account with "Manage Server" permission on the target server
- An LLM provider configured (`NABA_LLM_API_KEY`)

---

## Important: Outbound-Only

The NabaOS Discord integration is **outbound-only**. It can send messages to Discord channels but does **not** support:

- Slash commands
- Inbound message handling (reading DMs or channel messages)
- Interactive features (buttons, reactions)

Discord is used as a notification delivery channel. If you need full interactive bot capabilities, use the [Telegram channel](./telegram-setup.md) or the [Web Dashboard](./web-dashboard.md).

---

## Step 1: Create a Discord application

1. Go to the [Discord Developer Portal](https://discord.com/developers/applications).
2. Click **New Application**.
3. Name your application (e.g., `NabaOS Agent`) and click **Create**.

## Step 2: Create a bot account

1. In the Developer Portal, select your application.
2. Go to the **Bot** section in the left sidebar.
3. Click **Add Bot**, then confirm with **Yes, do it!**
4. Under the bot's username, click **Reset Token** to generate a new token.
5. Copy the **bot token**.

**Keep this token secret.**

## Step 3: Invite the bot to your server

1. Go to the **OAuth2** section, then **URL Generator**.
2. Under **Scopes**, select:
   - `bot`
3. Under **Bot Permissions**, select:
   - Send Messages
   - Embed Links
4. Copy the generated URL and open it in your browser.
5. Select the server to add the bot to and click **Authorize**.

## Step 4: Configure environment variables

```bash
export NABA_DISCORD_BOT_TOKEN="MTIzNDU2Nzg5MDEy.GAbcDE.a1b2c3d4e5f6..."
```

## Step 5: Start the server

The Discord channel is activated when the server detects `NABA_DISCORD_BOT_TOKEN` in the environment:

```bash
nabaos start
```

Expected output:

```
[start] Starting Telegram bot...
[start] Starting Discord bot...
[start] Discord bot connected
[start] Ready.
```

---

## Using Discord as a notification channel

Discord is used via the `channel.send` ability in chains:

```yaml
steps:
  - id: send_to_discord
    ability: channel.send
    args:
      channel: "discord"
      message: "Daily report: {{report}}"
```

This sends a message to the configured Discord channel via the serenity HTTP client.

---

## Environment variable reference

| Variable | Required | Description |
|----------|----------|-------------|
| `NABA_DISCORD_BOT_TOKEN` | Yes | Discord bot token from Developer Portal |

---

## Next steps

- [Telegram Setup](./telegram-setup.md) -- Set up the Telegram channel with full interactive support and 2FA
- [Web Dashboard](./web-dashboard.md) -- Monitor your agent from a browser
- [Constitution Customization](./constitution-customization.md) -- Control what the bot can do
