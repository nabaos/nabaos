# Configuration

> **What you'll learn**
>
> - Key environment variables NabaOS reads and what they control
> - How the data directory is laid out on disk
> - How to configure each supported LLM provider
> - How to select and customize a constitution
> - How to set spending budgets

NabaOS is configured primarily through environment variables. Set the variables
in your shell profile, a `.env` file, or your container orchestrator, and NabaOS
picks them up on startup.

---

## Key Environment Variables

### Required

| Variable | Description | Example |
|----------|-------------|---------|
| `NABA_LLM_PROVIDER` | Primary LLM backend: `anthropic`, `openai`, or `gemini` | `anthropic` |
| `NABA_LLM_API_KEY` | API key for the primary LLM provider | `sk-ant-api03-...` |

### Messaging Channels

| Variable | Description | Example |
|----------|-------------|---------|
| `NABA_TELEGRAM_BOT_TOKEN` | Telegram bot token from @BotFather | `7123456789:AAF...` |
| `NABA_DISCORD_BOT_TOKEN` | Discord bot token | `MTIz...` |
| `NABA_SLACK_BOT_TOKEN` | Slack bot token | `xoxb-...` |

### Paths and Storage

| Variable | Default | Description |
|----------|---------|-------------|
| `NABA_DATA_DIR` | `~/.nabaos` | Root directory for all NabaOS data |
| `NABA_MODEL_PATH` | `models/` | Path to the ONNX model directory |
| `NABA_CONSTITUTION_PATH` | *(none)* | Path to a custom constitution YAML file |
| `NABA_CONSTITUTION_TEMPLATE` | *(none)* | Use a built-in template by name instead of a file |
| `NABA_PLUGIN_DIR` | `$NABA_DATA_DIR/plugins` | Directory for installed plugins |
| `NABA_SUBPROCESS_CONFIG` | *(none)* | Path to subprocess abilities YAML config |

### Budgets and Limits

| Variable | Default | Description |
|----------|---------|-------------|
| `NABA_DAILY_BUDGET_USD` | *(unlimited)* | Maximum daily LLM spend in USD |
| `NABA_PER_TASK_BUDGET_USD` | *(unlimited)* | Maximum spend per individual task in USD |

### Web Dashboard

| Variable | Default | Description |
|----------|---------|-------------|
| `NABA_WEB_PASSWORD` | *(none -- dashboard disabled)* | Password to access the web dashboard |
| `NABA_WEB_BIND` | `127.0.0.1:8919` | Bind address for the web dashboard |
| `NABA_WEB_PORT` | `8919` | Port for the web dashboard |

### Security

| Variable | Description |
|----------|-------------|
| `NABA_VAULT_PASSPHRASE` | Passphrase for the encrypted secret vault |
| `NABA_TOTP_SECRET` | TOTP base32 secret (when using 2FA with TOTP) |

### Logging

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Standard Rust env filter for log verbosity (e.g., `debug`, `info`, `warn`, `error`, or per-module like `nabaos=debug,tower=warn`) |

### Advanced

| Variable | Default | Description |
|----------|---------|-------------|
| `NABA_CHEAP_LLM_MODEL` | `claude-haiku-4-5` | Model name for cheap LLM calls (Tier 3) |
| `NABA_FALLBACK_LLM_PROVIDER` | Same as `NABA_LLM_PROVIDER` | Fallback LLM provider |
| `NABA_FALLBACK_LLM_API_KEY` | *(none)* | API key for fallback provider |

For the complete list of all environment variables, see the
[Environment Variables Reference](../reference/environment-variables.md).

---

## Data Directory Layout

All persistent data lives under `NABA_DATA_DIR` (default `~/.nabaos/`):

```text
~/.nabaos/
  |-- nabaos.db               # Main SQLite database (fingerprint cache, intent cache)
  |-- cache.db                # Semantic cache database
  |-- cost.db                 # Cost tracking database
  |-- training.db             # Training queue for model fine-tuning
  |-- vault.db                # Encrypted secret vault
  |-- agents.db               # Installed agent registry
  |-- permissions.db          # Agent permission grants
  |-- profile.toml            # Module profile (output of `nabaos setup`)
  |-- agents/                 # Installed agent data
  |   |-- morning-briefing/
  |   |   |-- agent.wasm
  |   |   |-- manifest.json
  |   |   +-- data/
  |   +-- email-triage/
  |       +-- ...
  |-- plugins/                # Installed plugins
  |   |-- weather/
  |   |   |-- manifest.yaml
  |   |   +-- weather.wasm
  |   +-- ...
  +-- logs/                   # Log files (when running as server)
```

The SQLite databases are created automatically on first use. You can safely
delete any `.db` file to reset that subsystem -- the cache will rebuild as
you use the system.

---

## LLM Provider Setup

### Anthropic (Recommended)

```bash
export NABA_LLM_PROVIDER=anthropic
export NABA_LLM_API_KEY=sk-ant-api03-your-key-here
```

Get an API key at [console.anthropic.com](https://console.anthropic.com/).
NabaOS uses Claude Haiku for cheap Tier 3 calls and Claude Opus for complex
Tier 4 tasks by default. You can override the model names:

```bash
export NABA_CHEAP_LLM_MODEL=claude-haiku-4-5
```

### OpenAI

```bash
export NABA_LLM_PROVIDER=openai
export NABA_LLM_API_KEY=sk-your-key-here
```

Get an API key at [platform.openai.com](https://platform.openai.com/).

### Google Gemini

```bash
export NABA_LLM_PROVIDER=gemini
export NABA_LLM_API_KEY=your-gemini-key-here
```

### Local Model (No API Key Needed)

If you are running a local LLM server (e.g., Ollama, llama.cpp, vLLM) that
exposes an OpenAI-compatible API:

```bash
export NABA_LLM_PROVIDER=openai
export NABA_LLM_API_KEY=not-needed
export NABA_CHEAP_LLM_MODEL=llama3
```

With a local model, Tier 3 calls stay entirely on your machine. Combined with
the caching pipeline, this means virtually zero data leaves your hardware.

---

## Constitution Selection

A constitution defines the rules your agent operates under: which domains are
allowed, what actions are blocked, and what spending limits apply.

### Use a Built-in Template

NabaOS ships with 8 constitution templates for different use cases. List them:

```bash
nabaos config rules templates
```

**Expected output:**

```text
Available constitution templates:
  default              -- General-purpose balanced constitution
  content-creator      -- Content creation workflows
  dev-assistant        -- Developer assistant (code/git/CI domain)
  full-autonomy        -- Minimal restrictions for advanced users
  home-assistant       -- Smart home (IoT/calendar domain)
  hr-assistant         -- Human resources workflows
  research-assistant   -- Research: papers, data analysis, experiments
  trading              -- Financial markets monitoring and trading
```

Activate a template:

```bash
export NABA_CONSTITUTION_TEMPLATE=dev-assistant
```

### Use a Custom Constitution File

Generate a template as a starting point, then edit it:

```bash
nabaos config rules use-template dev-assistant --output ~/.nabaos/constitution.yaml
# Edit the file to customize rules
export NABA_CONSTITUTION_PATH=~/.nabaos/constitution.yaml
```

View the active constitution at any time:

```bash
nabaos config rules show
```

For details on writing custom rules, see
[Constitution Customization](../guides/constitution-customization.md).

---

## Budget Configuration

Spending limits prevent runaway LLM costs. They apply to Tier 3 (cheap LLM)
and Tier 4 (deep agent) calls. Cached requests (Tiers 0-2.5) are always free.

### Daily Budget

Set a maximum daily spend across all LLM calls:

```bash
export NABA_DAILY_BUDGET_USD=5.00
```

When the daily budget is exhausted, NabaOS returns cached results where possible
and rejects requests that would require an LLM call, with a clear error message.

### Per-Task Budget

Set a maximum spend for any single task:

```bash
export NABA_PER_TASK_BUDGET_USD=2.00
```

This is especially useful for Tier 4 deep agent calls, which can cost $1-5 per
task. Tasks that would exceed the per-task budget are blocked and the user is
notified.

### View Current Spending

Check your cost summary at any time:

```bash
nabaos status
```

**Expected output:**

```text
=== Cost Summary (All Time) ===
  Total LLM calls:   47
  Total cache hits:   312
  Cache hit rate:     86.9%
  Estimated savings:  $14.20
  Total spend:        $2.15

=== Last 24 Hours ===
  Total LLM calls:   3
  Total cache hits:   28
  Cache hit rate:     90.3%
  Estimated savings:  $1.05
  Total spend:        $0.12
```

---

## Example: Minimal Setup

The absolute minimum to get NabaOS running with LLM support:

```bash
export NABA_LLM_PROVIDER=anthropic
export NABA_LLM_API_KEY=sk-ant-api03-your-key-here
nabaos setup --non-interactive
nabaos ask "check my email"
```

## Example: Production Setup

A more complete configuration for daily use:

```bash
# LLM
export NABA_LLM_PROVIDER=anthropic
export NABA_LLM_API_KEY=sk-ant-api03-your-key-here
export NABA_DAILY_BUDGET_USD=10.00
export NABA_PER_TASK_BUDGET_USD=3.00

# Constitution
export NABA_CONSTITUTION_TEMPLATE=dev-assistant

# Web dashboard
export NABA_WEB_PASSWORD=a-strong-random-password

# Telegram
export NABA_TELEGRAM_BOT_TOKEN=7123456789:AAFyour-token-here

# Vault
export NABA_VAULT_PASSPHRASE=another-strong-passphrase

# Start
nabaos start
```

---

## What to Do Next

| Goal | Next page |
|------|-----------|
| Understand the caching pipeline | [Five-Tier Pipeline](../concepts/five-tier-pipeline.md) |
| Write constitution rules | [Constitution Customization](../guides/constitution-customization.md) |
| Set up Telegram | [Telegram Setup](../guides/telegram-setup.md) |
| Deploy with Docker Compose | [Docker Deployment](../operations/docker-deployment.md) |
| Store secrets securely | [Secrets Management](../guides/secrets-management.md) |
