# Discord Bot Setup

> **What you'll learn**
>
> - How to create a Discord application and bot account
> - How to invite the bot to your server
> - How to configure NabaOS to connect to Discord
> - How slash commands are handled
> - How to test the integration

---

## Prerequisites

- NabaOS installed (`nabaos --version`)
- A Discord account with "Manage Server" permission on the target server
- An LLM provider configured (`NABA_LLM_API_KEY`)

---

## Step 1: Create a Discord application

1. Go to the [Discord Developer Portal](https://discord.com/developers/applications).
2. Click **New Application**.
3. Name your application (e.g., `NabaOS Agent`) and click **Create**.
4. Note your **Application ID** from the General Information page.

## Step 2: Create a bot account

1. In the Developer Portal, select your application.
2. Go to the **Bot** section in the left sidebar.
3. Click **Add Bot**, then confirm with **Yes, do it!**
4. Under the bot's username, click **Reset Token** to generate a new token.
5. Copy the **bot token**. It looks like this:

```
MTIzNDU2Nzg5MDEy.GAbcDE.a1b2c3d4e5f6g7h8i9j0k1l2m3n4o5p6q7
```

**Keep this token secret.**

6. Under **Privileged Gateway Intents**, enable:
   - **Message Content Intent** (required for reading message text)

## Step 3: Invite the bot to your server

1. Go to the **OAuth2** section, then **URL Generator**.
2. Under **Scopes**, select:
   - `bot`
   - `applications.commands`
3. Under **Bot Permissions**, select:
   - Send Messages
   - Read Message History
   - Use Slash Commands
   - Embed Links
4. Copy the generated URL and open it in your browser.
5. Select the server to add the bot to and click **Authorize**.

The invite URL will look like:

```
https://discord.com/api/oauth2/authorize?client_id=YOUR_APP_ID&permissions=2147485696&scope=bot%20applications.commands
```

## Step 4: Configure environment variables

```bash
export NABA_DISCORD_BOT_TOKEN="MTIzNDU2Nzg5MDEy.GAbcDE.a1b2c3d4e5f6..."
```

To persist this, add it to your shell profile or `.env` file:

```bash
# ~/.bashrc or ~/.zshrc
export NABA_DISCORD_BOT_TOKEN="MTIzNDU2Nzg5MDEy.GAbcDE.a1b2c3d4e5f6..."
```

## Step 5: Start the daemon

The Discord channel is activated when the daemon detects `NABA_DISCORD_BOT_TOKEN` in the environment:

```bash
nabaos daemon
```

Expected output:

```
[daemon] Starting Telegram bot...
[daemon] Starting Discord bot...
[daemon] Discord bot connected as NabaOS#1234
[daemon] Scheduler running
[daemon] Ready.
```

---

## Slash commands

NabaOS registers slash commands with Discord when the bot connects. Users interact with the bot using Discord's slash command interface:

| Command | Description |
|---------|-------------|
| `/query <text>` | Send a query through the NabaOS pipeline |
| `/status` | Show agent status and cache statistics |
| `/agents` | List installed agents |
| `/help` | Show available commands |

### Using slash commands

1. In any channel where the bot has access, type `/query`.
2. Discord will show the command autocomplete. Fill in the `text` parameter:

```
/query check NVDA price
```

3. The bot processes the query through the full NabaOS pipeline (constitution check, cache lookup, LLM routing) and responds in the channel.

### Direct messages

You can also message the bot directly in DMs. Send a plain text message:

```
summarize my calendar for today
```

The bot processes it the same way as a slash command query.

---

## Channel-based routing

The `channel.send` ability in chains supports Discord as a target channel:

```yaml
steps:
  - id: send_to_discord
    ability: channel.send
    args:
      channel: "discord"
      message: "Daily report: {{report}}"
```

Valid channel values are: `telegram`, `discord`, `slack`, `whatsapp`, `email`, `sms`, `webhook`.

---

## Environment variable reference

| Variable | Required | Description |
|----------|----------|-------------|
| `NABA_DISCORD_BOT_TOKEN` | Yes | Discord bot token from Developer Portal |

---

## Restricting access

Discord access control is managed through Discord's built-in permission system:

1. **Server roles**: Control which channels the bot can see and respond in.
2. **Channel permissions**: Restrict the bot to specific channels by adjusting channel-level permissions.
3. **Constitution**: The NabaOS constitution still enforces domain boundaries regardless of the channel source. A blocked action is blocked whether it comes from Telegram, Discord, or any other channel.

---

## Complete setup checklist

```
[ ] Create Discord application in Developer Portal
[ ] Create bot account and copy token
[ ] Enable Message Content Intent
[ ] Generate OAuth2 invite URL with bot + applications.commands scopes
[ ] Invite bot to your server
[ ] Set NABA_DISCORD_BOT_TOKEN environment variable
[ ] Start the daemon: nabaos daemon
[ ] Test with /query command in Discord
```

---

## Troubleshooting

**Bot appears offline in Discord:**
- Verify `NABA_DISCORD_BOT_TOKEN` is set correctly.
- Check the daemon output for connection errors.
- Ensure the Message Content Intent is enabled in the Developer Portal.

**Slash commands do not appear:**
- It can take up to an hour for global slash commands to propagate.
- Try restarting the daemon to re-register commands.
- Verify the bot was invited with the `applications.commands` scope.

**Bot does not respond in a channel:**
- Check that the bot has "Send Messages" and "Read Message History" permissions in that channel.
- Look at the daemon logs for any constitution blocks or errors.

---

## Next steps

- [Telegram Setup](./telegram-setup.md) -- Set up the Telegram channel with 2FA
- [Web Dashboard](./web-dashboard.md) -- Monitor your agent from a browser
- [Constitution Customization](./constitution-customization.md) -- Control what the bot can do
