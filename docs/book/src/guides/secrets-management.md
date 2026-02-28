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

---

## How the vault works

NabaOS stores secrets in an encrypted SQLite database (`vault.db`). Every secret is encrypted with AES-256-GCM using a key derived from your vault passphrase (via PBKDF2 with a random 16-byte salt).

Key properties:

- **Encrypted at rest**: Secrets are never stored in plaintext on disk.
- **Passphrase-protected**: The vault requires a passphrase to open. Without it, secrets are unreadable.
- **Intent-bound**: Secrets can be restricted to specific operations (e.g., only `check` and `analyze` intents can access an API key).
- **Sanitized from output**: The vault builds a sanitizer that scrubs secret values from any agent output to prevent accidental leakage.

---

## Step 1: Set the vault passphrase

```bash
export NABA_VAULT_PASSPHRASE="your-strong-passphrase-here"
```

If `NABA_VAULT_PASSPHRASE` is not set, the CLI will prompt you interactively.

## Step 2: Store a secret

Store secrets by piping the value through stdin:

```bash
echo "sk-ant-api03-xxxx" | nabaos config vault store openai-key
```

### Store with intent binding

Intent binding restricts which operations can access the secret:

```bash
echo "sk-ant-api03-xxxx" | nabaos config vault store openai-key --bind "check|analyze"
```

## Step 3: List stored secrets

```bash
nabaos config vault list
```

Expected output:

```
Stored secrets:
  openai-key          bound: check|analyze
  GITHUB_TOKEN        bound: check|create
  TELEGRAM_BOT_TOKEN  bound: (any)
```

---

## Vault encryption details

| Property | Value |
|----------|-------|
| Algorithm | AES-256-GCM |
| Key derivation | PBKDF2-HMAC-SHA256, 100,000 iterations |
| Salt | Random 16 bytes, stored in `vault_meta` table |
| Nonce | Random 12 bytes per secret, stored alongside ciphertext |
| Library | `ring` (no hand-rolled crypto) |

---

## Rotating secrets

To rotate a secret, store a new value under the same name. The old value is overwritten:

```bash
echo "new-api-key-value" | nabaos config vault store openai-key --bind "check|analyze"
```

---

## Environment variable reference

| Variable | Required | Description |
|----------|----------|-------------|
| `NABA_VAULT_PASSPHRASE` | No | Vault passphrase (prompted interactively if not set) |

---

## Next steps

- [Plugin Development](./plugin-development.md) -- Use secrets in plugin manifests
- [Building Agents](./building-agents.md) -- Package agents that use vault secrets
- [Constitution Customization](./constitution-customization.md) -- Control which intents can access secrets
