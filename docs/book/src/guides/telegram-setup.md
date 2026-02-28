# Telegram Bot Setup

> **What you'll learn**
>
> - How to create a Telegram bot and obtain a bot token
> - How to configure NabaOS to connect to your bot
> - How to restrict access to specific chat IDs
> - How to enable two-factor authentication (TOTP or password)
> - How to test the bot with a message

---

## Prerequisites

- NabaOS installed (`nabaos --version`)
- A Telegram account
- An LLM provider configured (`NABA_LLM_API_KEY`)

---

## Step 1: Create a bot via BotFather

1. Open Telegram and search for **@BotFather**.
2. Send the command `/newbot`.
3. Choose a **display name** for your bot (e.g., `My NabaOS Agent`).
4. Choose a **username** for your bot. It must end in `bot` (e.g., `my_nabaos_bot`).
5. BotFather will reply with your **bot token**. It looks like this:

```
7123456789:AAHfiqksKZ8WmR2zSjiQ7_v4TpcG2cCkHHI
```

**Keep this token secret.** Anyone with this token can control your bot.

## Step 2: Get your chat ID

You need your Telegram chat ID to restrict who can talk to the bot.

1. Search for **@userinfobot** on Telegram and start a conversation.
2. It will reply with your user ID (a number like `123456789`).
3. For group chats, add @userinfobot to the group -- it will report the group's chat ID (a negative number like `-1001234567890`).

## Step 3: Set environment variables

Export the bot token and allowed chat IDs:

```bash
export NABA_TELEGRAM_BOT_TOKEN="7123456789:AAHfiqksKZ8WmR2zSjiQ7_v4TpcG2cCkHHI"
export NABA_ALLOWED_CHAT_IDS="123456789"
```

For multiple allowed chats, separate IDs with commas:

```bash
export NABA_ALLOWED_CHAT_IDS="123456789,-1001234567890"
```

## Step 4: Start the Telegram bot

Run the bot in standalone mode:

```bash
nabaos start --telegram-only
```

Or run it as part of the full server (which also handles scheduled jobs and the web dashboard):

```bash
nabaos start
```

## Step 5: Test the bot

Open Telegram and send a message to your bot:

```
check the weather in Delhi
```

The bot should respond through the NabaOS pipeline -- classifying the intent, checking the constitution, querying the cache or LLM, and returning a result.

If you send a message from a chat ID that is not in `NABA_ALLOWED_CHAT_IDS`, the bot will ignore it silently.

---

## Enabling two-factor authentication

NabaOS supports 2FA on the Telegram channel. When enabled, users must authenticate before the bot processes their messages.

### Option A: TOTP (recommended)

```bash
nabaos config security 2fa totp
```

Configure the environment:

```bash
export NABA_TOTP_SECRET="JBSWY3DPEHPK3PXP"
```

### Option B: Password

```bash
nabaos config security 2fa password
```

---

## Running in production

For production deployments, run the server which manages Telegram, scheduled jobs, and optionally the web dashboard:

```bash
export NABA_TELEGRAM_BOT_TOKEN="..."
export NABA_ALLOWED_CHAT_IDS="..."
export NABA_TOTP_SECRET="..."
export NABA_WEB_PASSWORD="your-dashboard-password"

nabaos start
```

Expected output:

```
[start] Starting Telegram bot...
[start] Bot username: @my_nabaos_bot
[start] Starting web dashboard on http://127.0.0.1:8919...
[start] Scheduler running (3 scheduled jobs)
[start] Ready.
```

---

## Environment variable reference

| Variable | Required | Description |
|----------|----------|-------------|
| `NABA_TELEGRAM_BOT_TOKEN` | Yes | Bot token from @BotFather |
| `NABA_ALLOWED_CHAT_IDS` | No | Comma-separated list of allowed chat IDs |
| `NABA_TOTP_SECRET` | If TOTP | Base32-encoded TOTP secret |

---

## Next steps

- [Discord Setup](./discord-setup.md) -- Set up a Discord bot channel
- [Web Dashboard](./web-dashboard.md) -- Access the web interface
- [Secrets Management](./secrets-management.md) -- Store your bot tokens securely in the vault
