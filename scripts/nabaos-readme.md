# NabaOS

A self-hosted personal agent runtime with a cache-first architecture. Most daily requests hit the semantic work cache and execute locally in milliseconds at zero cost. Complex tasks route to the best available LLM or deep agent backend.

## Architecture

```
User Message
  |
  +-- Tier 0: Fingerprint Cache (exact match, <1ms, free)
  +-- Tier 1: BERT Classifier (security check, <10ms, free)
  +-- Tier 2: Intent Cache (pattern match, <5ms, free)
  +-- Tier 3: Semantic Cache (embedding similarity, <20ms, free)
  +-- Tier 4: Cheap LLM (Haiku/GPT-4o-mini, ~$0.005)
  +-- Tier 5: Deep Agent (Claude/OpenAI, $0.50-5.00)
```

After learning your patterns, ~90% of requests resolve from cache without any LLM call.

## Quick Start

### Option 1: Install Script

```bash
curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh | bash
nabaos init
nabaos daemon
```

### Option 2: From Source

```bash
git clone https://github.com/nabaos/nabaos.git
cd nabaos
cargo build --release
./target/release/nabaos init
./target/release/nabaos daemon
```

### Option 3: Docker

```bash
cp .env.example .env
# Edit .env with your API keys
docker compose up -d
```

## Configuration

### API Keys

Set in your environment or `.env` file:

```bash
NABA_LLM_API_KEY=sk-...          # Required: Anthropic, OpenAI, or DeepSeek key
NABA_LLM_PROVIDER=anthropic       # anthropic | openai | deepseek
NABA_TELEGRAM_BOT_TOKEN=...       # Optional: Telegram bot
NABA_WEB_PASSWORD=...             # Optional: Web dashboard password
```

### Constitution

NabaOS uses a constitution file to define what your agent can and cannot do. Choose a template during `nabaos init`:

- `default` — General-purpose with safety boundaries
- `trading` — Financial markets domain
- `dev-assistant` — Developer tools domain
- `research-assistant` — Academic research domain
- `content-creator` — Creative content domain
- `home-assistant` — Smart home / IoT domain
- `full-autonomy` — Minimal restrictions

Edit your constitution: `nabaos config edit-constitution`

## CLI Reference

```bash
nabaos init                  # First-time setup wizard
nabaos daemon                # Start the agent daemon
nabaos ask "your question"   # One-shot query
nabaos status                # Show agent status
nabaos check                 # Validate configuration
nabaos check --health        # Check running daemon health

# Cache management
nabaos status --query "text" # Check cache for a query
nabaos export list           # List cached work entries
nabaos export analyze        # Analyze cache for export

# Research
nabaos research "topic"      # Run parallel research swarm

# Autonomous objectives
nabaos pea start "objective" --budget 10.0
nabaos pea list              # List active objectives
nabaos pea status <id>       # Check objective progress
```

## Channels

NabaOS connects to users through multiple channels:

| Channel | Setup |
|---------|-------|
| Telegram | Set `NABA_TELEGRAM_BOT_TOKEN` |
| Web Dashboard | Set `NABA_WEB_PASSWORD`, visit `http://localhost:8919` |
| WhatsApp | Set `NABA_WHATSAPP_TOKEN` + `NABA_WHATSAPP_PHONE_ID` |
| Discord | Set `NABA_DISCORD_BOT_TOKEN` |
| Slack | Set `NABA_SLACK_BOT_TOKEN` + `NABA_SLACK_SIGNING_SECRET` |
| Email (IMAP) | Set `NABA_IMAP_HOST` + credentials |

## Deployment

### systemd (Linux)

```bash
sudo cp deploy/nabaos.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now nabaos
```

### Docker

```bash
docker compose up -d
docker compose logs -f nabaos
```

### Oracle Cloud Free Tier (ARM)

Cross-compile for ARM:

```bash
make build-arm
scp target/aarch64-unknown-linux-musl/release/nabaos user@oracle-vm:/opt/nabaos/bin/
```

## Backup & Restore

```bash
./scripts/backup.sh ~/backups/
./scripts/restore.sh ~/backups/nabaos-backup-20260227-120000.tar.gz
```

## Philosophy

NabaOS uses concepts from Nyaya Indian philosophy for decision validation:
- **Pramana** (valid knowledge) — 4 epistemological methods validate autonomous decisions
- **Hetvabhasa** (fallacy detection) — identifies flawed reasoning in inference chains
- The autonomous execution engine (PEA) combines BDI architecture with Nyaya epistemology

## License

MIT
