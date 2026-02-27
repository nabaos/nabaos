# Docker Deployment

> **What you'll learn**
>
> - How to run NabaOS in Docker with a single command
> - How to configure `docker-compose.yml` for persistent, production-ready deployments
> - How to set up volumes, environment variables, and health checks
> - How to run a multi-container setup with the web dashboard
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
  -v nyaya-data:/data \
  -v nyaya-models:/models \
  ghcr.io/nabaos/nabaos:latest
```

Verify the container is running:

```bash
docker ps --filter name=nabaos
```

Expected output:

```
CONTAINER ID   IMAGE                                    STATUS         PORTS   NAMES
a1b2c3d4e5f6   ghcr.io/nabaos/nabaos:latest   Up 3 seconds           nabaos
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
2026-02-24T10:00:02Z  INFO  Daemon listening
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
      # Which provider to use: anthropic, openai, gemini, local
      - NABA_LLM_PROVIDER=${NABA_LLM_PROVIDER:-anthropic}

      # --- API Key ---
      # Your LLM provider's API key (required)
      - NABA_LLM_API_KEY=${NABA_LLM_API_KEY}

      # --- Telegram ---
      # Bot token for the Telegram messaging interface (optional)
      - NABA_TELEGRAM_BOT_TOKEN=${NABA_TELEGRAM_BOT_TOKEN}

      # --- Data paths ---
      # Inside the container, data lives at /data and models at /models.
      # These are mapped to named Docker volumes below.
      - NABA_DATA_DIR=/data
      - NABA_MODEL_PATH=/models

      # --- Cost control ---
      # Maximum daily spend on LLM API calls (USD). Default: $10.
      - NABA_DAILY_BUDGET_USD=${NABA_DAILY_BUDGET_USD:-10.0}

    volumes:
      # Persistent data: agents, plugins, catalog, cache DBs, config, logs
      - nyaya-data:/data
      # ML models: ONNX files for BERT classifier, embeddings
      - nyaya-models:/models

    # Restart automatically unless you explicitly stop the container
    restart: unless-stopped

volumes:
  nyaya-data:
  nyaya-models:
```

Start the stack:

```bash
docker compose up -d
```

Expected output:

```
[+] Running 2/2
 ✔ Volume "nabaos_nyaya-data"    Created
 ✔ Volume "nabaos_nyaya-models"  Created
 ✔ Container nabaos              Started
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
| `nyaya-data` | `/data` | Agents, plugins, catalog, SQLite databases (`nyaya.db`, `vault.db`), constitution files, logs |
| `nyaya-models` | `/models` | ONNX model files (`bert-security.onnx`, `gpt2-nyaya.onnx`, embedding models) |

### Bind mounts (alternative)

If you prefer host-directory bind mounts instead of named volumes, replace the volumes section:

```yaml
    volumes:
      - ./data:/data
      - ./models:/models
```

Verify volume contents:

```bash
docker exec nabaos ls /data
```

Expected output:

```
agents
catalog
config
logs
nyaya.db
plugins
vault.db
```

---

## Environment Variables

Pass these to the container via `-e` flags or the `environment:` block in Compose:

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `NABA_LLM_PROVIDER` | No | `anthropic` | LLM provider: `anthropic`, `openai`, `gemini`, `local` |
| `NABA_LLM_API_KEY` | **Yes** | -- | API key for your chosen LLM provider |
| `NABA_TELEGRAM_BOT_TOKEN` | No | -- | Telegram bot token for messaging interface |
| `NABA_DAILY_BUDGET_USD` | No | `10.0` | Daily spending cap for LLM API calls (USD) |
| `NABA_LOG_LEVEL` | No | `info` | Log verbosity: `debug`, `info`, `warn`, `error` |
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
NABA_DAILY_BUDGET_USD=10.0
NABA_LOG_LEVEL=info
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
      test: ["CMD", "nabaos", "cache", "stats"]
      interval: 30s
      timeout: 10s
      retries: 3
      start_period: 15s
```

Check health status:

```bash
docker inspect --format='{{.State.Health.Status}}' nabaos
```

Expected output:

```
healthy
```

If you are running the web dashboard (see below), you can also check the HTTP endpoint:

```bash
docker compose exec nabaos curl -sf http://localhost:3000/api/health || echo "unhealthy"
```

---

## Multi-Container Setup: Agent + Web Dashboard

Run the agent alongside the built-in web dashboard for a browser-based management interface:

```yaml
version: '3.8'

services:
  nabaos:
    image: ghcr.io/nabaos/nabaos:latest
    environment:
      - NABA_LLM_PROVIDER=${NABA_LLM_PROVIDER:-anthropic}
      - NABA_LLM_API_KEY=${NABA_LLM_API_KEY}
      - NABA_TELEGRAM_BOT_TOKEN=${NABA_TELEGRAM_BOT_TOKEN}
      - NABA_DATA_DIR=/data
      - NABA_MODEL_PATH=/models
      - NABA_DAILY_BUDGET_USD=${NABA_DAILY_BUDGET_USD:-10.0}
    volumes:
      - nyaya-data:/data
      - nyaya-models:/models
    restart: unless-stopped

  nyaya-web:
    image: ghcr.io/nabaos/nabaos:latest
    command: ["web", "--bind", "0.0.0.0:3000"]
    environment:
      - NABA_LLM_PROVIDER=${NABA_LLM_PROVIDER:-anthropic}
      - NABA_LLM_API_KEY=${NABA_LLM_API_KEY}
      - NABA_DATA_DIR=/data
      - NABA_MODEL_PATH=/models
    volumes:
      - nyaya-data:/data
      - nyaya-models:/models
    ports:
      - "3000:3000"
    depends_on:
      - nabaos
    restart: unless-stopped

volumes:
  nyaya-data:
  nyaya-models:
```

Start both services:

```bash
docker compose up -d
```

Open the dashboard at `http://localhost:3000`.

---

## Security Notes

The Docker image follows security best practices:

- **Non-root user**: The container runs as the `nyaya` user, not root.
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
  us-docker.pkg.dev/YOUR_PROJECT/nyaya/nabaos:latest
docker push us-docker.pkg.dev/YOUR_PROJECT/nyaya/nabaos:latest

# Deploy
gcloud run deploy nabaos \
  --image us-docker.pkg.dev/YOUR_PROJECT/nyaya/nabaos:latest \
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

### Azure Container Apps

```bash
# Push to Azure Container Registry
az acr login --name yourregistry
docker tag ghcr.io/nabaos/nabaos:latest yourregistry.azurecr.io/nabaos:latest
docker push yourregistry.azurecr.io/nabaos:latest

# Deploy as a container app
az containerapp create \
  --name nabaos \
  --resource-group YOUR_RG \
  --image yourregistry.azurecr.io/nabaos:latest \
  --env-vars "NABA_LLM_PROVIDER=anthropic" "NABA_LLM_API_KEY=$NABA_LLM_API_KEY"
```

All cloud platforms require you to handle persistent storage for `/data` separately, since the agent stores SQLite databases and configuration there.
