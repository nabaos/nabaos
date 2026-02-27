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
4. Choose a **username** for your bot. It must end in `bot` (e.g., `my_nyaya_agent_bot`).
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

To persist these, add them to your shell profile (`~/.bashrc`, `~/.zshrc`) or a `.env` file:

```bash
# ~/.bashrc or ~/.zshrc
export NABA_TELEGRAM_BOT_TOKEN="7123456789:AAHfiqksKZ8WmR2zSjiQ7_v4TpcG2cCkHHI"
export NABA_ALLOWED_CHAT_IDS="123456789"
```

## Step 4: Start the Telegram bot

Run the bot in standalone mode:

```bash
nabaos telegram
```

Expected output:

```
  2026-02-24T10:00:00  INFO  Starting Telegram bot...
  2026-02-24T10:00:01  INFO  Bot username: @my_nyaya_agent_bot
  2026-02-24T10:00:01  INFO  Allowed chat IDs: [123456789]
  2026-02-24T10:00:01  INFO  Listening for messages...
```

Or run it as part of the daemon (which also handles scheduled jobs and the web dashboard):

```bash
nabaos daemon
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

TOTP (Time-based One-Time Password) is compatible with Google Authenticator, Authy, and similar apps.

**Generate a TOTP secret:**

```bash
nabaos telegram-setup-2fa totp
```

Expected output:

```
=== TOTP Setup ===
Secret: JBSWY3DPEHPK3PXP
URI: otpauth://totp/NabaOS?secret=JBSWY3DPEHPK3PXP&issuer=NabaOS

Scan the QR code or enter the secret manually in your authenticator app.

Set these environment variables:
  export NABA_TELEGRAM_2FA=totp
  export NABA_TOTP_SECRET="JBSWY3DPEHPK3PXP"
```

**Configure the environment:**

```bash
export NABA_TELEGRAM_2FA="totp"
export NABA_TOTP_SECRET="JBSWY3DPEHPK3PXP"
```

**How it works:**

1. When you send a message to the bot for the first time (or after the session expires), it replies: `Two-factor authentication required. Please enter your 6-digit TOTP code.`
2. Open your authenticator app, find the `NabaOS` entry, and send the 6-digit code.
3. If the code is valid, the bot creates a session and processes your message.
4. Subsequent messages within the session do not require re-authentication.

### Option B: Password

For simpler setups, you can use a static password:

```bash
nabaos telegram-setup-2fa password
```

Expected output:

```
=== Password Setup ===
Enter a password for Telegram 2FA:
> ********

Password hash: $argon2id$v=19$m=65536,t=3,p=4$...

Set these environment variables:
  export NABA_TELEGRAM_2FA=password
  export NABA_2FA_PASSWORD_HASH="$argon2id$v=19$m=65536,t=3,p=4$..."
```

**Configure the environment:**

```bash
export NABA_TELEGRAM_2FA="password"
export NABA_2FA_PASSWORD_HASH="$argon2id$v=19$..."
```

### Option C: No 2FA

By default, 2FA is disabled. To explicitly disable it:

```bash
export NABA_TELEGRAM_2FA="none"
```

---

## Environment variable reference

| Variable | Required | Description |
|----------|----------|-------------|
| `NABA_TELEGRAM_BOT_TOKEN` | Yes | Bot token from @BotFather |
| `NABA_ALLOWED_CHAT_IDS` | No | Comma-separated list of allowed chat IDs |
| `NABA_TELEGRAM_2FA` | No | 2FA method: `none`, `totp`, `password`, `weblink` |
| `NABA_TOTP_SECRET` | If TOTP | Base32-encoded TOTP secret |
| `NABA_2FA_PASSWORD_HASH` | If password | Argon2id password hash |
| `NABA_SECURITY_BOT_TOKEN` | No | Separate bot token for security alerts |
| `NABA_ALERT_CHAT_ID` | No | Chat ID for security alert notifications |

---

## Security alert bot (optional)

You can run a separate Telegram bot dedicated to security alerts. This keeps security notifications in a separate chat from regular agent interactions.

1. Create a second bot via @BotFather (e.g., `my_nyaya_security_bot`).
2. Set the environment variables:

```bash
export NABA_SECURITY_BOT_TOKEN="7987654321:BBCdefghijk..."
export NABA_ALERT_CHAT_ID="123456789"
```

3. The security bot sends alerts for:
   - Blocked requests (constitution violations)
   - Anomalous behavior patterns
   - Failed authentication attempts
   - Credential detection in messages

---

## Running in production

For production deployments, run the daemon which manages Telegram, scheduled jobs, and optionally the web dashboard:

```bash
export NABA_TELEGRAM_BOT_TOKEN="..."
export NABA_ALLOWED_CHAT_IDS="..."
export NABA_TELEGRAM_2FA="totp"
export NABA_TOTP_SECRET="..."
export NABA_WEB_PASSWORD="your-dashboard-password"

nabaos daemon
```

Expected output:

```
[daemon] Starting Telegram bot...
[daemon] Bot username: @my_nyaya_agent_bot
[daemon] Starting web dashboard on http://127.0.0.1:8919...
[daemon] Scheduler running (3 scheduled jobs)
[daemon] Ready.
```

---

## Troubleshooting

**Bot does not respond to messages:**
- Verify `NABA_TELEGRAM_BOT_TOKEN` is correct.
- Check that your chat ID is in `NABA_ALLOWED_CHAT_IDS`.
- Look at the terminal output for errors.
- Make sure no other process is using the same bot token.

**2FA code is rejected:**
- TOTP codes are time-sensitive. Make sure your device clock is accurate.
- Verify `NABA_TOTP_SECRET` matches the secret shown during setup.

**Bot responds slowly:**
- The first query after startup may be slower due to model loading.
- Cache hits are near-instant (<300ms). Cache misses go to the LLM.

---

## Next steps

- [Discord Setup](./discord-setup.md) -- Set up a Discord bot channel
- [Web Dashboard](./web-dashboard.md) -- Access the web interface
- [Secrets Management](./secrets-management.md) -- Store your bot tokens securely in the vault
