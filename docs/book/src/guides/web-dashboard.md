# Web Dashboard

> **What you'll learn**
>
> - How to start the web dashboard
> - How to set up password authentication
> - How to navigate the dashboard features
> - Available API endpoints for programmatic access

---

## Prerequisites

- NabaOS installed (`nabaos --version`)
- A configured data directory with at least one LLM provider

---

## Step 1: Set a dashboard password

The web dashboard requires a password. Set it via environment variable:

```bash
export NABA_WEB_PASSWORD="your-secure-password"
```

If `NABA_WEB_PASSWORD` is not set, the web dashboard will be disabled.

## Step 2: Start the web dashboard

### Standalone mode

Run the dashboard by itself:

```bash
nabaos start --web-only
```

Expected output:

```
Starting NabaOS web dashboard on http://127.0.0.1:8919...
```

### Custom bind address

To bind to a different address or port:

```bash
nabaos start --web-only --bind 0.0.0.0:9000
```

### As part of the full server

When running the full server, the web dashboard starts automatically if `NABA_WEB_PASSWORD` is set:

```bash
export NABA_WEB_PASSWORD="your-secure-password"
nabaos start
```

## Step 3: Access the dashboard

Open your browser and go to:

```
http://localhost:8919
```

You will be prompted to authenticate with the password you set in `NABA_WEB_PASSWORD`.

---

## Dashboard features

### Query interface

Submit queries directly from the browser. The query goes through the full NabaOS pipeline.

### Cache statistics

View the semantic work cache performance:

- **Hit rate**: Percentage of queries served from cache
- **Total entries**: Number of cached chain templates
- **Cost savings**: Estimated money saved by cache hits vs. LLM calls

### Cost tracking

Monitor LLM spending across providers.

### Agent management

- **List agents**: See all installed agents with status
- **Start/stop**: Control agents from the dashboard

### Constitution view

- **Active rules**: See all constitution rules and their enforcement levels
- **Recent checks**: History of constitution evaluations

---

## API endpoints

The web dashboard exposes a REST API. All endpoints require authentication.

### Query

```bash
curl -X POST http://localhost:8919/api/query \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $(echo -n 'your-secure-password' | base64)" \
  -d '{"query": "check NVDA price"}'
```

### Cache stats

```bash
curl http://localhost:8919/api/cache/stats \
  -H "Authorization: Bearer $(echo -n 'your-secure-password' | base64)"
```

### Cost summary

```bash
curl http://localhost:8919/api/costs \
  -H "Authorization: Bearer $(echo -n 'your-secure-password' | base64)"
```

### Agent list

```bash
curl http://localhost:8919/api/agents \
  -H "Authorization: Bearer $(echo -n 'your-secure-password' | base64)"
```

### API endpoint reference

| Method | Endpoint | Description |
|--------|----------|-------------|
| `POST` | `/api/query` | Submit a query through the pipeline |
| `GET` | `/api/cache/stats` | Cache hit rate and statistics |
| `GET` | `/api/costs` | Cost tracking summary |
| `GET` | `/api/agents` | List installed agents |
| `POST` | `/api/constitution/check` | Check a query against the constitution |
| `GET` | `/api/health` | Health check (no auth required) |

---

## Environment variable reference

| Variable | Required | Description |
|----------|----------|-------------|
| `NABA_WEB_PASSWORD` | Yes | Password for dashboard authentication |
| `NABA_WEB_BIND` | No | Bind address [default: `127.0.0.1:8919`] |
| `NABA_WEB_PORT` | No | Port [default: `8919`] |

---

## Security considerations

- The dashboard binds to `127.0.0.1` by default, accessible only from localhost.
- To expose it to a network, use `--bind 0.0.0.0:8919` -- but ensure you are behind a firewall or reverse proxy with TLS.
- For production, run behind a reverse proxy (nginx, Caddy) with HTTPS.

---

## Next steps

- [Telegram Setup](./telegram-setup.md) -- Set up the Telegram bot channel
- [Discord Setup](./discord-setup.md) -- Set up the Discord notification channel
- [Building Agents](./building-agents.md) -- Create agents to manage from the dashboard
