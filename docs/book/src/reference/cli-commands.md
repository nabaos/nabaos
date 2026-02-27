# CLI Commands

NabaOS ships as a single binary called `nyaya`. All commands share two
global options:

```
nyaya [OPTIONS] <COMMAND>

Options:
  --data-dir <PATH>   Data directory  [env: NABA_DATA_DIR] [default: ~/.nabaos]
  --model-dir <PATH>  Model directory [env: NABA_MODEL_PATH] [default: models/setfit-w5h2]
  -h, --help          Print help
  -V, --version       Print version
```

---

## Classification & Query

### `classify`

Classify a query into a W5H2 intent using the local SetFit ONNX model.

```
nyaya classify <QUERY>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `QUERY`  | yes      | The text to classify |

**Example:**

```bash
nyaya classify "check my email for messages from Alice"
# Query:      check my email for messages from Alice
# Intent:     check|email
# Action:     check
# Target:     email
# Confidence: 94.2%
# Latency:    3.1ms
```

### `query`

Full query pipeline: fingerprint cache -> SetFit classification -> constitution
check -> intent cache lookup. This is the standard entry point for processing
user queries outside the orchestrator.

```
nyaya query <QUERY>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `QUERY`  | yes      | The query to process |

**Example:**

```bash
nyaya query "what is the price of NVDA"
# === Tier 1: Fingerprint Cache HIT ===
# Intent:     check|price
# Confidence: 95.0%
# Latency:    0.042ms
```

### `orchestrate`

Run a query through the full two-speed orchestrator pipeline, including LLM
routing, NabaOS block parsing, security scanning, and training signal
generation.

```
nyaya orchestrate <QUERY>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `QUERY`  | yes      | The query to process |

**Example:**

```bash
nyaya orchestrate "summarize the top 3 news stories today"
```

### `security-scan`

Scan text for credential leaks, PII, and prompt injection patterns. Does not
route to any LLM -- runs entirely locally.

```
nyaya security-scan <TEXT>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `TEXT`   | yes      | Text to scan |

**Example:**

```bash
nyaya security-scan "my api key is sk-ant-abc123 and my SSN is 123-45-6789"
# === Security Scan ===
# Credentials: 1 found
# PII:         1 found
# Types:       ["api_key", "ssn"]
# === Redacted Output ===
# my api key is [REDACTED:api_key] and my SSN is [REDACTED:ssn]
```

---

## Cache & Cost

### `cache stats`

Display hit/miss statistics for the fingerprint cache (tier 1) and intent
cache (tier 2).

```
nyaya cache stats
```

**Example:**

```bash
nyaya cache stats
# === Cache Statistics ===
# Fingerprint Cache (Tier 1):
#   Entries: 142
#   Hits:    1038
# Intent Cache (Tier 2):
#   Total entries:   27
#   Enabled entries: 25
#   Total hits:      314
```

### `costs`

Show cost tracking summary: total spend, cache savings, token usage. Displays
both all-time and last-24-hour figures.

```
nyaya costs
```

---

## Secret Vault

### `secret store`

Store a secret in the encrypted vault. The secret value is read from stdin.
Optionally bind the secret to specific intents so it can only be accessed
during matching operations.

```
nyaya secret store <NAME> [--bind <INTENTS>]
```

| Argument       | Required | Description |
|----------------|----------|-------------|
| `NAME`         | yes      | Secret name (key) |
| `--bind`       | no       | Pipe-separated intent binding, e.g. `"check_infra\|monitor_infra"` |

Requires `NABA_VAULT_PASSPHRASE` or prompts interactively.

**Example:**

```bash
echo "xoxb-my-slack-token" | nyaya secret store SLACK_TOKEN --bind "notify|channel"
```

### `secret list`

List all stored secret names with their intent bindings and creation
timestamps.

```
nyaya secret list
```

---

## Constitution

### `constitution check`

Check a query against the active constitution. The query is first classified
into a W5H2 intent, then matched against constitution rules.

```
nyaya constitution check <QUERY>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `QUERY`  | yes      | The query to check |

**Example:**

```bash
nyaya constitution check "delete all my emails"
# Query:       delete all my emails
# Intent:      delete|email
# Enforcement: Block
# Allowed:     BLOCKED
# Matched:     block_destructive_keywords
# Reason:      Destructive operations require explicit confirmation
```

### `constitution show`

Display the active constitution: its name and all rules with their
enforcement levels, triggers, and reasons.

```
nyaya constitution show
```

### `constitution templates`

List all 21 built-in constitution templates.

```
nyaya constitution templates
```

Available templates: `default`, `solopreneur`, `freelancer`,
`digital-marketer`, `student`, `sales`, `customer-support`, `legal`,
`ecommerce`, `hr`, `finance`, `healthcare`, `engineering`, `media`,
`government`, `ngo`, `logistics`, `research`, `consulting`, `creative`,
`agriculture`.

### `constitution use-template`

Generate a constitution YAML file from a named template. Outputs to stdout
by default, or to a file with `--output`.

```
nyaya constitution use-template <NAME> [--output <PATH>]
```

| Argument     | Required | Description |
|--------------|----------|-------------|
| `NAME`       | yes      | Template name |
| `-o, --output` | no    | Output file path (default: stdout) |

**Example:**

```bash
nyaya constitution use-template solopreneur -o my-constitution.yaml
```

---

## Plugin Management

### `plugin install`

Install a plugin from a manifest YAML file. Copies the manifest and any
associated shared library into the plugin directory.

```
nyaya plugin install <MANIFEST>
```

| Argument   | Required | Description |
|------------|----------|-------------|
| `MANIFEST` | yes      | Path to the plugin `manifest.yaml` |

### `plugin list`

List all installed plugins with their names, sources, trust levels, and
descriptions.

```
nyaya plugin list
```

### `plugin remove`

Remove an installed plugin by name.

```
nyaya plugin remove <NAME>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `NAME`   | yes      | Plugin name to remove |

### `plugin register-subprocess`

Register one or more subprocess abilities from a YAML configuration file.

```
nyaya plugin register-subprocess <CONFIG>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `CONFIG` | yes      | Path to subprocess abilities YAML |

---

## Scheduling

### `schedule add`

Schedule a chain to run at a recurring interval.

```
nyaya schedule add <CHAIN_ID> <INTERVAL>
```

| Argument    | Required | Description |
|-------------|----------|-------------|
| `CHAIN_ID`  | yes      | Chain ID to schedule |
| `INTERVAL`  | yes      | Interval string: `"10m"`, `"1h"`, `"30s"` |

**Example:**

```bash
nyaya schedule add check_email 10m
# Scheduled 'check_email' every 10m (job: a1b2c3d4...)
```

### `schedule list`

List all scheduled jobs showing chain ID, interval, run count, and enabled
status.

```
nyaya schedule list
```

### `schedule run-due`

Immediately process all jobs that are due for execution.

```
nyaya schedule run-due
```

### `schedule disable`

Disable a scheduled job by its job ID.

```
nyaya schedule disable <JOB_ID>
```

---

## Abilities

### `abilities`

List all available abilities (built-in, plugin, subprocess, and cloud)
with their source and description.

```
nyaya abilities
```

---

## WASM Sandbox

### `run`

Execute a WASM agent module inside the sandboxed runtime. Requires both a
`.wasm` file and an agent manifest JSON.

```
nyaya run <WASM> --manifest <MANIFEST>
```

| Argument       | Required | Description |
|----------------|----------|-------------|
| `WASM`         | yes      | Path to the `.wasm` module |
| `--manifest`   | yes      | Path to the agent manifest JSON |

**Example:**

```bash
nyaya run agents/weather.wasm --manifest agents/weather.json
```

---

## Services

### `telegram`

Start the Telegram bot. Requires `NABA_TELEGRAM_BOT_TOKEN`.

```
nyaya telegram
```

### `telegram-setup-2fa`

Set up two-factor authentication for the Telegram bot. Supports TOTP
(authenticator app) or password methods.

```
nyaya telegram-setup-2fa <METHOD>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `METHOD` | yes      | `totp` or `password` |

### `web`

Start the web dashboard server with REST API.

```
nyaya web [--bind <ADDR>]
```

| Argument | Required | Description |
|----------|----------|-------------|
| `--bind` | no       | Bind address [default: `127.0.0.1:8919`] |

### `daemon`

Run as a background daemon: processes scheduled jobs every 60 seconds,
optionally starts the Telegram bot (if `NABA_TELEGRAM_BOT_TOKEN` is set)
and web dashboard (if `NABA_WEB_PASSWORD` is set) in background threads.

```
nyaya daemon
```

---

## Setup & Deployment

### `setup`

Interactive setup wizard: scans hardware, suggests a module profile
(core, web, voice, browser, telegram, latex, mobile, oauth), and saves
the configuration.

```
nyaya setup [--non-interactive]
```

| Flag                | Description |
|---------------------|-------------|
| `--non-interactive` | Skip prompts, accept the suggested profile |

### `deploy`

Generate a Docker Compose file from the current module profile.

```
nyaya deploy [-o <PATH>]
```

| Argument      | Required | Description |
|---------------|----------|-------------|
| `-o, --output` | no      | Output path [default: `docker-compose.yml`] |

---

## LaTeX

### `latex templates`

List available LaTeX document templates.

```
nyaya latex templates
```

Templates: `invoice`, `research_paper`, `report`, `letter`.

### `latex generate`

Generate a document from a template. Reads JSON data from stdin, writes a
`.tex` file, and attempts PDF compilation if a LaTeX backend is available.

```
nyaya latex generate <TEMPLATE> -o <OUTPUT>
```

| Argument      | Required | Description |
|---------------|----------|-------------|
| `TEMPLATE`    | yes      | Template name |
| `-o, --output` | yes     | Output PDF path |

**Example:**

```bash
echo '{"company":"Acme","items":[...]}' | nyaya latex generate invoice -o invoice.pdf
```

---

## Voice

### `voice`

Transcribe an audio file to text. Requires voice input to be enabled via
`nyaya setup` or the `NABA_VOICE_MODE` environment variable.

```
nyaya voice <FILE>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `FILE`   | yes      | Path to audio file |

---

## OAuth

### `oauth status`

Show the status of all OAuth connectors (gmail, calendar, slack, notion).

```
nyaya oauth status
```

---

## Training

### `retrain`

Export training data from the training queue for SetFit fine-tuning. Prints
all queued examples with their intent labels.

```
nyaya retrain
```

---

## Agent OS

### `agent install`

Install an agent from a `.nap` package file.

```
nabaos agent install <PACKAGE>
```

| Argument  | Required | Description |
|-----------|----------|-------------|
| `PACKAGE` | yes      | Path to `.nap` package file |

### `agent list`

List all installed agents with name, version, and state.

```
nabaos agent list
```

### `agent info`

Show detailed information about an installed agent including version history.

```
nabaos agent info <NAME>
```

### `agent start` / `stop` / `disable` / `enable`

Change the lifecycle state of an installed agent.

```
nabaos agent start <NAME>
nabaos agent stop <NAME>
nabaos agent disable <NAME>
nabaos agent enable <NAME>
```

### `agent uninstall`

Uninstall an agent and remove its data.

```
nabaos agent uninstall <NAME>
```

### `agent permissions`

Show all permissions granted to an agent.

```
nabaos agent permissions <NAME>
```

### `agent package`

Package a source directory (containing `manifest.yaml`) into a `.nap` file.

```
nabaos agent package <SOURCE> -o <OUTPUT>
```

| Argument      | Required | Description |
|---------------|----------|-------------|
| `SOURCE`      | yes      | Directory containing `manifest.yaml` |
| `-o, --output` | yes     | Output `.nap` file path |

---

## Catalog

### `catalog list`

List all agents available in the local catalog.

```
nyaya catalog list
```

### `catalog search`

Search for agents by keyword.

```
nyaya catalog search <QUERY>
```

### `catalog info`

Show detailed information about a catalog entry.

```
nyaya catalog info <NAME>
```
