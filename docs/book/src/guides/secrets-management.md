# Secrets Management

> **What you'll learn**
>
> - How the encrypted vault works
> - How to store, list, and retrieve secrets
> - How intent binding restricts which operations can access a secret
> - How vault encryption is configured
> - How to rotate secrets safely

---

## Prerequisites

- NabaOS installed (`nabaos --version`)
- A working data directory (default `~/.nabaos`)

---

## How the vault works

NabaOS stores secrets in an encrypted SQLite database at `~/.nabaos/vault.db`. Every secret is encrypted with AES-256-GCM using a key derived from your vault passphrase (via PBKDF2 with a random salt).

Key properties:

- **Encrypted at rest**: Secrets are never stored in plaintext on disk.
- **Passphrase-protected**: The vault requires a passphrase to open. Without it, secrets are unreadable.
- **Intent-bound**: Secrets can be restricted to specific operations (e.g., only `check` and `analyze` intents can access an API key).
- **Sanitized from output**: The vault builds a sanitizer that scrubs secret values from any agent output to prevent accidental leakage.

---

## Step 1: Set the vault passphrase

The vault passphrase can be provided as an environment variable or entered interactively.

**Option A: Environment variable (recommended for servers)**

```bash
export NABA_VAULT_PASSPHRASE="your-strong-passphrase-here"
```

Add it to your shell profile for persistence:

```bash
# ~/.bashrc or ~/.zshrc
export NABA_VAULT_PASSPHRASE="your-strong-passphrase-here"
```

**Option B: Interactive prompt**

If `NABA_VAULT_PASSPHRASE` is not set, the CLI will prompt you:

```
Vault passphrase: ********
```

## Step 2: Store a secret

Store secrets by piping the value through stdin:

```bash
echo "sk-ant-api03-xxxx" | nabaos secret store openai-key
```

Expected output:

```
Secret 'openai-key' stored successfully
```

### Store with intent binding

Intent binding restricts which operations can access the secret. This prevents a compromised chain from reading secrets meant for other purposes.

```bash
echo "sk-ant-api03-xxxx" | nabaos secret store openai-key --bind "check|analyze"
```

This means only chains whose intent matches `check` or `analyze` can retrieve `openai-key`. A chain with a `send` or `delete` intent will be denied access.

### More examples

Store a GitHub token bound to check and create operations:

```bash
echo "ghp_abc123def456" | nabaos secret store GITHUB_TOKEN --bind "check|create"
```

Store a Gmail OAuth token:

```bash
echo "ya29.a0AfH6SMBZ..." | nabaos secret store GMAIL_ACCESS_TOKEN --bind "check|search|send"
```

Store a Telegram bot token (no intent binding -- accessible to all operations):

```bash
echo "7123456789:AAHfiqksKZ..." | nabaos secret store TELEGRAM_BOT_TOKEN
```

## Step 3: List stored secrets

```bash
nabaos secret list
```

Expected output:

```
Stored secrets:
  openai-key          bound: check|analyze
  GITHUB_TOKEN        bound: check|create
  GMAIL_ACCESS_TOKEN  bound: check|search|send
  TELEGRAM_BOT_TOKEN  bound: (any)
```

The list shows secret names and their intent bindings, but never shows the actual values.

---

## Intent binding in detail

Intent binding is a security feature unique to NabaOS. When a chain step requests a secret, the runtime checks the current operation's intent against the secret's binding.

### How it works

1. A chain step references `{{GITHUB_TOKEN}}` in its args.
2. The runtime looks up `GITHUB_TOKEN` in the vault.
3. The vault checks the current intent (e.g., `check_infra`) against the binding (`check|create`).
4. `check_infra` starts with `check`, so access is granted.
5. If the intent were `delete_repo`, access would be denied with an error.

### Binding format

The `--bind` argument takes a pipe-separated list of intent prefixes:

```
--bind "check|analyze|create"
```

A secret with no binding (`--bind` omitted) is accessible to all intents.

### Why intent binding matters

Consider this scenario: you store your Gmail OAuth token with `--bind "check|search"`. Even if an attacker tricks the agent into running a chain that tries to send emails, the `send` intent will be denied access to the Gmail token. The secret is compartmentalized to its intended use.

---

## How secrets are used in plugins and chains

### In plugin manifests

Plugin manifests reference secrets using `{{VAR_NAME}}` syntax in headers:

```yaml
abilities:
  github.issues:
    type: cloud
    endpoint: "https://api.github.com/repos/{{owner}}/{{repo}}/issues"
    method: GET
    headers:
      Authorization: "Bearer {{GITHUB_TOKEN}}"
```

At runtime, `{{GITHUB_TOKEN}}` is resolved from the vault (subject to intent binding).

### In chain steps

Chain steps can reference secrets in their arguments:

```yaml
steps:
  - id: fetch_data
    ability: cloud.request
    args:
      endpoint: "https://api.example.com/data"
      headers:
        Authorization: "Bearer {{MY_API_KEY}}"
    output_key: result
```

---

## Vault encryption details

| Property | Value |
|----------|-------|
| Algorithm | AES-256-GCM |
| Key derivation | PBKDF2-HMAC-SHA256, 100,000 iterations |
| Salt | Random 32 bytes, stored in `vault_meta` table |
| Nonce | Random 12 bytes per secret, stored alongside ciphertext |
| Library | `ring` (no hand-rolled crypto) |

The vault database (`vault.db`) contains two tables:
- `vault_meta`: stores the PBKDF2 salt
- `secrets`: stores name, encrypted value, nonce, and intent binding

Even with access to `vault.db`, secrets cannot be decrypted without the passphrase.

---

## Rotating secrets

To rotate a secret, store a new value under the same name. The old value is overwritten:

```bash
echo "new-api-key-value" | nabaos secret store openai-key --bind "check|analyze"
```

Expected output:

```
Secret 'openai-key' stored successfully
```

The new value takes effect immediately for all subsequent operations.

### Rotation best practices

1. **Generate the new credential** from the provider (e.g., rotate your API key in the OpenAI dashboard).
2. **Store the new value** in the vault.
3. **Test** that the agent can still make API calls.
4. **Revoke the old credential** at the provider.

---

## Deleting secrets

To remove a secret from the vault entirely:

```bash
# Currently done via the API or programmatically
# The vault supports delete operations internally
```

---

## Output sanitization

The vault automatically sanitizes agent output to prevent secret leakage. When the vault loads, it builds a sanitizer that replaces any occurrence of secret values in output text with `[REDACTED]`.

For example, if the vault contains `openai-key = "sk-ant-api03-xxxx"` and the agent accidentally includes it in a response, the output becomes:

```
The API key is [REDACTED]
```

This works across all channels (Telegram, Discord, web dashboard, CLI).

---

## Complete working example

Here is a full workflow for setting up secrets for a GitHub monitoring agent:

```bash
# 1. Set vault passphrase
export NABA_VAULT_PASSPHRASE="my-secure-passphrase"

# 2. Store the GitHub token (bound to check and create operations)
echo "ghp_abc123def456ghi789" | nabaos secret store GITHUB_TOKEN --bind "check|create"

# 3. Store the notification webhook (bound to notify operations)
echo "https://hooks.slack.com/services/T00/B00/xxxx" | nabaos secret store SLACK_WEBHOOK --bind "notify|send"

# 4. Store the LLM API key (bound to analyze and generate operations)
echo "sk-ant-api03-production-key" | nabaos secret store LLM_API_KEY --bind "analyze|generate"

# 5. Verify all secrets are stored
nabaos secret list
```

Expected output:

```
Stored secrets:
  GITHUB_TOKEN   bound: check|create
  SLACK_WEBHOOK  bound: notify|send
  LLM_API_KEY    bound: analyze|generate
```

Now these secrets are available to plugins and chains that reference them with `{{GITHUB_TOKEN}}`, `{{SLACK_WEBHOOK}}`, and `{{LLM_API_KEY}}`.

---

## Environment variable reference

| Variable | Required | Description |
|----------|----------|-------------|
| `NABA_VAULT_PASSPHRASE` | No | Vault passphrase (prompted interactively if not set) |
| `NABA_DATA_DIR` | No | Data directory containing `vault.db` (default: `~/.nabaos`) |

---

## Troubleshooting

**"Decryption failed (wrong passphrase?)":**
- The passphrase does not match the one used when the vault was created.
- If you have forgotten the passphrase, the secrets are unrecoverable. Delete `vault.db` and re-create the vault.

**"Secret 'X' not found":**
- The secret has not been stored. Use `nabaos secret list` to see available secrets.
- Secret names are case-sensitive. `GITHUB_TOKEN` and `github_token` are different.

**"Intent binding denied access to secret 'X'":**
- The current operation's intent does not match the secret's binding.
- Check the binding with `nabaos secret list` and adjust if needed.

---

## Next steps

- [Plugin Development](./plugin-development.md) -- Use secrets in plugin manifests
- [Building Agents](./building-agents.md) -- Package agents that use vault secrets
- [Constitution Customization](./constitution-customization.md) -- Control which intents can access secrets
