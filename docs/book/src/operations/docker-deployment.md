# Docker Deployment

> **What you'll learn**
>
> - How to run NabaOS in Docker with a single command
> - How to configure `docker-compose.yml` for persistent, production-ready deployments
> - How to set up volumes, environment variables, and health checks
> - How to run with the web dashboard
> - Where cloud deployment is headed (and what works today)

---

## Quick Start

Run the agent with a single `docker run` command:

```bash
docker run -d \
  --name nabaos \
  --restart unless-stopped \
  -e NABA_LLM_PROVIDER=anthropic \
  -e NABA_LLM_API_KEY="$NABA_LLM_API_KEY" \
  -e NABA_TELEGRAM_BOT_TOKEN="$NABA_TELEGRAM_BOT_TOKEN" \
  -e NABA_DAILY_BUDGET_USD=10.0 \
  -v nabaos-data:/data \
  -v nabaos-models:/models \
  ghcr.io/nabaos/nabaos:latest
```

Verify the container is running:

```bash
docker ps --filter name=nabaos
```

Check the logs to confirm startup:

```bash
docker logs nabaos
```

Expected output:

```
2026-02-24T10:00:01Z  INFO  NabaOS starting...
2026-02-24T10:00:01Z  INFO  Loading configuration from /data/config
2026-02-24T10:00:02Z  INFO  Security layer initialized
2026-02-24T10:00:02Z  INFO  Ready.
```

---

## docker-compose.yml Walkthrough

For production deployments, use `docker-compose.yml`. Here is the full file with annotations:

```yaml
version: '3.8'

services:
  nabaos:
    # Build from local source, or use the published image:
    #   image: ghcr.io/nabaos/nabaos:latest
    build: .

    environment:
      # --- LLM Provider ---
      - NABA_LLM_PROVIDER=${NABA_LLM_PROVIDER:-anthropic}

      # --- API Key ---
      - NABA_LLM_API_KEY=${NABA_LLM_API_KEY}

      # --- Telegram ---
      - NABA_TELEGRAM_BOT_TOKEN=${NABA_TELEGRAM_BOT_TOKEN}

      # --- Web Dashboard ---
      - NABA_WEB_PASSWORD=${NABA_WEB_PASSWORD}

      # --- Data paths ---
      - NABA_DATA_DIR=/data
      - NABA_MODEL_PATH=/models

      # --- Cost control ---
      - NABA_DAILY_BUDGET_USD=${NABA_DAILY_BUDGET_USD:-10.0}

      # --- Logging ---
      - RUST_LOG=${RUST_LOG:-info}

    volumes:
      # Persistent data: agents, plugins, catalog, cache DBs, config, logs
      - nabaos-data:/data
      # ML models: ONNX files for BERT classifier, embeddings
      - nabaos-models:/models

    # Expose the web dashboard port
    ports:
      - "8919:8919"

    # Restart automatically unless you explicitly stop the container
    restart: unless-stopped

volumes:
  nabaos-data:
  nabaos-models:
```

Start the stack:

```bash
docker compose up -d
```

Stop the stack:

```bash
docker compose down
```

---

## Volume Configuration

The container uses two volumes for persistent storage:

| Volume | Container path | Contents |
|--------|----------------|----------|
| `nabaos-data` | `/data` | Agents, plugins, catalog, SQLite databases (`nyaya.db`, `vault.db`, `cache.db`, `cost.db`), constitution files, logs |
| `nabaos-models` | `/models` | ONNX model files (BERT, SetFit, embedding models) |

### Bind mounts (alternative)

If you prefer host-directory bind mounts instead of named volumes, replace the volumes section:

```yaml
    volumes:
      - ./data:/data
      - ./models:/models
```

---

## Environment Variables

Pass these to the container via `-e` flags or the `environment:` block in Compose:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `NABA_LLM_PROVIDER` | No | `anthropic` | LLM provider: `anthropic`, `openai`, `gemini` |
| `NABA_LLM_API_KEY` | **Yes** | -- | API key for your chosen LLM provider |
| `NABA_TELEGRAM_BOT_TOKEN` | No | -- | Telegram bot token for messaging interface |
| `NABA_WEB_PASSWORD` | No | -- | Password for the web dashboard |
| `NABA_DAILY_BUDGET_USD` | No | `10.0` | Daily spending cap for LLM API calls (USD) |
| `RUST_LOG` | No | `info` | Log verbosity: `debug`, `info`, `warn`, `error` |
| `NABA_DATA_DIR` | No | `/data` | Data directory inside the container |
| `NABA_MODEL_PATH` | No | `/models` | Model directory inside the container |
| `NABA_SECURITY_BOT_TOKEN` | No | -- | Separate Telegram bot for security alerts |
| `NABA_ALERT_CHAT_ID` | No | -- | Telegram chat ID for security alert delivery |

### Using a .env file

Create a `.env` file next to your `docker-compose.yml`:

```bash
NABA_LLM_PROVIDER=anthropic
NABA_LLM_API_KEY=sk-ant-api03-xxxxx
NABA_TELEGRAM_BOT_TOKEN=123456:ABC-DEF
NABA_WEB_PASSWORD=secure-dashboard-pw
NABA_DAILY_BUDGET_USD=10.0
RUST_LOG=info
```

Docker Compose reads `.env` automatically. Do not commit this file to version control.

---

## Health Checks

Add a health check to your `docker-compose.yml` so Docker monitors the process:

```yaml
services:
  nabaos:
    # ... (other configuration) ...
    healthcheck:
      test: ["CMD", "nabaos", "admin", "cache", "stats"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 15s
```

Check health status:

```bash
docker inspect --format='{{.State.Health.Status}}' nabaos
```

If you are running the web dashboard, you can also check the HTTP endpoint:

```bash
docker compose exec nabaos curl -sf http://localhost:8919/api/health || echo "unhealthy"
```

---

## Security Notes

The Docker image follows security best practices:

- **Non-root user**: The container runs as a non-root user.
- **Minimal base image**: `debian:bookworm-slim` with only `ca-certificates` installed.
- **Multi-stage build**: The Rust toolchain is not present in the final image.
- **Read-only filesystem** (optional): Add `read_only: true` and a tmpfs for `/tmp`:

```yaml
services:
  nabaos:
    # ... (other configuration) ...
    read_only: true
    tmpfs:
      - /tmp
```

---

## Cloud Deployment

> **Coming soon**: First-class deployment guides for GCP Cloud Run, AWS ECS, and Azure Container Apps are planned.

In the meantime, the standard Docker image works on any platform that runs containers. Here are the manual steps that work today:

### GCP Cloud Run

```bash
# Tag and push the image to Google Artifact Registry
docker tag ghcr.io/nabaos/nabaos:latest \
  us-docker.pkg.dev/YOUR_PROJECT/nabaos/nabaos:latest
docker push us-docker.pkg.dev/YOUR_PROJECT/nabaos/nabaos:latest

# Deploy
gcloud run deploy nabaos \
  --image us-docker.pkg.dev/YOUR_PROJECT/nabaos/nabaos:latest \
  --set-env-vars "NABA_LLM_PROVIDER=anthropic,NABA_LLM_API_KEY=$NABA_LLM_API_KEY" \
  --memory 1Gi \
  --cpu 1 \
  --no-allow-unauthenticated
```

**Caveat**: Cloud Run is stateless. You need to mount a persistent volume (e.g., GCS FUSE or Cloud SQL) for `/data`.

### AWS ECS

```bash
# Push to ECR
aws ecr get-login-password | docker login --username AWS --password-stdin YOUR_ACCOUNT.dkr.ecr.REGION.amazonaws.com
docker tag ghcr.io/nabaos/nabaos:latest YOUR_ACCOUNT.dkr.ecr.REGION.amazonaws.com/nabaos:latest
docker push YOUR_ACCOUNT.dkr.ecr.REGION.amazonaws.com/nabaos:latest

# Create task definition and service via the AWS Console or CLI
# Mount an EFS volume at /data for persistence
```

All cloud platforms require you to handle persistent storage for `/data` separately, since the agent stores SQLite databases and configuration there.
