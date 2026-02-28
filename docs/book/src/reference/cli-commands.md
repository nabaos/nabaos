# CLI Commands

NabaOS ships as a single binary called `nabaos`. All commands share two
global options:

```
nabaos [OPTIONS] <COMMAND>

Options:
  --data-dir <PATH>   Data directory  [env: NABA_DATA_DIR] [default: ~/.nabaos]
  --model-dir <PATH>  Model directory [env: NABA_MODEL_PATH] [default: models/setfit-w5h2]
  -h, --help          Print help
  -V, --version       Print version
```

---

## Top-Level Commands

| Command | Description |
|---------|-------------|
| [`setup`](#setup) | Interactive setup wizard |
| [`start`](#start) | Start the server (Telegram, Discord, web dashboard, scheduler) |
| [`ask`](#ask) | Run a query through the full pipeline |
| [`status`](#status) | Show cost tracking and system status |
| [`config`](#config) | Configuration subcommands |
| [`admin`](#admin) | Administration and diagnostics |
| [`memory`](#memory) | View and manage agent memory |
| [`research`](#research) | Run a research query |
| [`init`](#init) | Initialize a new project directory |
| [`export`](#export) | Export and hardware analysis |
| [`pea`](#pea) | PEA (Persistent Execution Agent) management |
| [`watcher`](#watcher) | File watcher (requires `--features watcher`) |
| [`check`](#check) | Health check |

---

## setup

Interactive setup wizard: scans hardware, suggests a module profile
(core, web, voice, browser, telegram, latex, mobile, oauth), and saves
the configuration.

```
nabaos setup [--non-interactive] [--interactive] [--download-models]
```

| Flag                  | Description |
|-----------------------|-------------|
| `--non-interactive`   | Skip prompts, accept the suggested profile |
| `--interactive`       | Force interactive mode |
| `--download-models`   | Download ONNX models during setup |

**Example:**

```bash
nabaos setup
# Scanning hardware...
# Detected: 8 cores, 16GB RAM, no GPU
# Suggested profile: core, web, telegram
# Accept? [Y/n]
```

---

## start

Start the server. Runs scheduled jobs, and optionally starts the Telegram
bot, Discord bot, and web dashboard based on environment variables.

```
nabaos start [--telegram-only] [--web-only] [--bind <ADDR>]
```

| Flag               | Description |
|--------------------|-------------|
| `--telegram-only`  | Start only the Telegram bot |
| `--web-only`       | Start only the web dashboard |
| `--bind <ADDR>`    | Bind address for web dashboard [default: `127.0.0.1:8919`] |

**Example:**

```bash
nabaos start
# [start] Starting Telegram bot...
# [start] Bot username: @my_nabaos_bot
# [start] Starting Discord bot...
# [start] Starting web dashboard on http://127.0.0.1:8919...
# [start] Scheduler running (3 scheduled jobs)
# [start] Ready.
```

---

## ask

Run a query through the full pipeline: fingerprint cache, BERT
classification, SetFit intent classification, constitution check,
semantic cache, LLM routing, and response generation.

```
nabaos ask <QUERY>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `QUERY`  | yes      | The query to process |

**Example:**

```bash
nabaos ask "what is the price of NVDA"
```

---

## status

Show cost tracking summary: total spend, cache savings, token usage.
Displays both all-time and last-24-hour figures.

```
nabaos status [--abilities] [--full] [QUERY]
```

| Flag           | Description |
|----------------|-------------|
| `--abilities`  | List all available abilities |
| `--full`       | Show full details |
| `QUERY`        | Optional query to show status for |

---

## config

Configuration subcommands are organized into subgroups:

### config persona

Manage agent personas and the catalog.

```
nabaos config persona list
nabaos config persona info <NAME>
nabaos config persona catalog list
nabaos config persona catalog search <QUERY>
nabaos config persona catalog info <NAME>
nabaos config persona catalog install <NAME>
```

**Example:**

```bash
nabaos config persona catalog list
# Available personas:
#   research-assistant    Research and analysis workflows
#   dev-assistant         Developer productivity assistant
#   home-assistant        Smart home management
#   ...
```

### config rules

Manage constitution rules.

```
nabaos config rules check <QUERY>
nabaos config rules show
nabaos config rules templates
nabaos config rules use-template <NAME> [--output <PATH>]
```

| Subcommand     | Description |
|----------------|-------------|
| `check`        | Check a query against the active constitution |
| `show`         | Display the active constitution and all rules |
| `templates`    | List the 8 built-in constitution templates |
| `use-template` | Generate a YAML file from a named template |

Templates: `default`, `content-creator`, `dev-assistant`, `full-autonomy`,
`home-assistant`, `hr-assistant`, `research-assistant`, `trading`.

**Example:**

```bash
nabaos config rules check "delete all files"
# Query:       delete all files
# Intent:      delete|file
# Enforcement: Block
# Matched:     block_destructive_keywords
```

### config workflow

Manage workflows.

```
nabaos config workflow list
nabaos config workflow start <NAME>
nabaos config workflow status <ID>
nabaos config workflow cancel <ID>
nabaos config workflow tui
nabaos config workflow visualize <NAME>
nabaos config workflow suggest
nabaos config workflow create <NAME>
nabaos config workflow templates
```

### config resource

Manage external resources.

```
nabaos config resource list
nabaos config resource status
nabaos config resource leases
nabaos config resource discover
nabaos config resource auto-add
```

### config style

Manage response styles.

```
nabaos config style list
nabaos config style set <KEY> <VALUE>
nabaos config style clear
nabaos config style show
```

### config skill

Manage skills.

```
nabaos config skill forge
nabaos config skill list
```

### config schedule

Manage scheduled jobs.

```
nabaos config schedule add <CHAIN_ID> <INTERVAL>
nabaos config schedule list
nabaos config schedule run-due
nabaos config schedule disable <JOB_ID>
```

| Argument    | Required | Description |
|-------------|----------|-------------|
| `CHAIN_ID`  | yes      | Chain ID to schedule |
| `INTERVAL`  | yes      | Interval string: `"10m"`, `"1h"`, `"30s"` |

**Example:**

```bash
nabaos config schedule add check_email 10m
# Scheduled 'check_email' every 10m (job: a1b2c3d4...)
```

### config vault

Manage the encrypted secret vault.

```
echo "secret-value" | nabaos config vault store <NAME> [--bind <INTENTS>]
nabaos config vault list
```

| Argument | Required | Description |
|----------|----------|-------------|
| `NAME`   | yes      | Secret name (key) |
| `--bind` | no       | Pipe-separated intent binding, e.g. `"check\|analyze"` |

Requires `NABA_VAULT_PASSPHRASE` or prompts interactively.

**Example:**

```bash
echo "xoxb-my-slack-token" | nabaos config vault store SLACK_TOKEN --bind "notify|channel"
```

### config security

Configure security settings.

```
nabaos config security 2fa <METHOD>
```

| Argument | Required | Description |
|----------|----------|-------------|
| `METHOD` | yes      | `totp` or `password` |

### config agent

Manage installed agents.

```
nabaos config agent install <PACKAGE>
nabaos config agent list
nabaos config agent info <NAME>
nabaos config agent start <NAME>
nabaos config agent stop <NAME>
nabaos config agent disable <NAME>
nabaos config agent enable <NAME>
nabaos config agent uninstall <NAME>
nabaos config agent permissions <NAME>
nabaos config agent package <SOURCE> -o <OUTPUT>
```

| Subcommand    | Description |
|---------------|-------------|
| `install`     | Install an agent from a `.nap` package file |
| `list`        | List all installed agents with name, version, and state |
| `info`        | Show detailed information about an installed agent |
| `start/stop`  | Change the lifecycle state of an agent |
| `disable/enable` | Disable or enable an agent |
| `uninstall`   | Uninstall an agent and remove its data |
| `permissions` | Show all permissions granted to an agent |
| `package`     | Package a source directory into a `.nap` file |

---

## admin

Administration and diagnostic subcommands.

### admin classify

Classify a query into a W5H2 intent using the local models.

```
nabaos admin classify <QUERY>
```

**Example:**

```bash
nabaos admin classify "check my email for messages from Alice"
# Query:      check my email for messages from Alice
# Intent:     check|email
# Action:     check
# Target:     email
# Confidence: 94.2%
# Latency:    3.1ms
```

### admin cache

View cache statistics.

```
nabaos admin cache stats
```

**Example:**

```bash
nabaos admin cache stats
# === Cache Statistics ===
# Fingerprint Cache (Tier 0):
#   Entries: 142
#   Hits:    1038
# Intent Cache (Tier 2):
#   Total entries:   27
#   Enabled entries: 25
#   Total hits:      314
```

### admin scan

Scan text for credential leaks, PII, and prompt injection patterns. Runs
entirely locally.

```
nabaos admin scan <TEXT>
```

**Example:**

```bash
nabaos admin scan "my api key is sk-ant-abc123 and my SSN is 123-45-6789"
# === Security Scan ===
# Credentials: 1 found
# PII:         1 found
# Types:       ["api_key", "ssn"]
# === Redacted Output ===
# my api key is [REDACTED:api_key] and my SSN is [REDACTED:ssn]
```

### admin plugin

Manage plugins.

```
nabaos admin plugin install <MANIFEST>
nabaos admin plugin list
nabaos admin plugin remove <NAME>
nabaos admin plugin register-subprocess <CONFIG>
```

| Subcommand             | Description |
|------------------------|-------------|
| `install`              | Install a plugin from a manifest file |
| `list`                 | List installed plugins with trust levels |
| `remove`               | Remove an installed plugin by name |
| `register-subprocess`  | Register subprocess abilities from a YAML config |

### admin run

Execute a WASM agent module inside the sandboxed runtime.

```
nabaos admin run <WASM> --manifest <MANIFEST>
```

| Argument     | Required | Description |
|--------------|----------|-------------|
| `WASM`       | yes      | Path to the `.wasm` module |
| `--manifest` | yes      | Path to the agent manifest JSON |

**Example:**

```bash
nabaos admin run agents/weather.wasm --manifest agents/weather.json
```

### admin retrain

Export training data from the training queue for SetFit fine-tuning.

```
nabaos admin retrain
```

### admin deploy

Generate a Docker Compose file from the current module profile.

```
nabaos admin deploy [--output <PATH>]
```

| Argument       | Required | Description |
|----------------|----------|-------------|
| `-o, --output` | no       | Output path [default: `docker-compose.yml`] |

### admin latex

LaTeX document generation.

```
nabaos admin latex templates
nabaos admin latex generate <TEMPLATE> -o <OUTPUT>
```

Templates: `invoice`, `research_paper`, `report`, `letter`.

**Example:**

```bash
echo '{"company":"Acme","items":[...]}' | nabaos admin latex generate invoice -o invoice.pdf
```

### admin voice

Transcribe an audio file to text.

```
nabaos admin voice <FILE>
```

### admin oauth

Manage OAuth connectors.

```
nabaos admin oauth status
```

### admin browser

Manage browser sessions and extensions.

```
nabaos admin browser sessions
nabaos admin browser clear-sessions
nabaos admin browser captcha-status
nabaos admin browser extension-status
```

---

## memory

View and manage agent memory.

```
nabaos memory list
nabaos memory show
nabaos memory clear
```

---

## research

Run a research query through the deep research pipeline.

```
nabaos research <QUERY>
```

---

## init

Initialize a new NabaOS project directory with default configuration files.

```
nabaos init
```

---

## export

Export and hardware analysis.

```
nabaos export list
nabaos export analyze
nabaos export generate
nabaos export hardware
```

---

## pea

PEA (Persistent Execution Agent) management.

```
nabaos pea start <TASK>
nabaos pea list
nabaos pea status <ID>
nabaos pea tasks
nabaos pea pause <ID>
nabaos pea resume <ID>
nabaos pea cancel <ID>
```

| Subcommand | Description |
|------------|-------------|
| `start`    | Start a new PEA with a task description |
| `list`     | List all PEAs |
| `status`   | Show status of a specific PEA |
| `tasks`    | List all PEA tasks |
| `pause`    | Pause a running PEA |
| `resume`   | Resume a paused PEA |
| `cancel`   | Cancel a PEA |

---

## watcher

File watcher (requires the `watcher` feature flag at compile time).

```
nabaos watcher <SUBCOMMAND>
```

---

## check

Health check.

```
nabaos check [--health]
```

| Flag       | Description |
|------------|-------------|
| `--health` | Run a health check and exit |
