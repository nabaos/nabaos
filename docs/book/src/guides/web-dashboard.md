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

Add it to your shell profile for persistence:

```bash
# ~/.bashrc or ~/.zshrc
export NABA_WEB_PASSWORD="your-secure-password"
```

If `NABA_WEB_PASSWORD` is not set, the web dashboard will be disabled.

## Step 2: Start the web dashboard

### Standalone mode

Run the dashboard by itself:

```bash
nabaos web
```

Expected output:

```
Starting NabaOS web dashboard on http://127.0.0.1:8919...
  2026-02-24T10:00:00  INFO  NabaOS web dashboard listening on http://127.0.0.1:8919
```

### Custom bind address

To bind to a different address or port:

```bash
nabaos web --bind 0.0.0.0:9000
```

This makes the dashboard accessible from other machines on the network.

### As part of the daemon

When running the full daemon, the web dashboard starts automatically if `NABA_WEB_PASSWORD` is set:

```bash
export NABA_WEB_PASSWORD="your-secure-password"
nabaos daemon
```

Expected output:

```
[daemon] Starting Telegram bot...
[daemon] Starting web dashboard on http://127.0.0.1:8919...
[daemon] Scheduler running
[daemon] Ready.
```

If `NABA_WEB_PASSWORD` is not set, the daemon logs:

```
[daemon] NABA_WEB_PASSWORD not set -- web dashboard disabled.
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

Submit queries directly from the browser. The query goes through the full NabaOS pipeline:

1. Constitution check (keyword scan)
2. Fingerprint and intent classification
3. Cache lookup
4. LLM routing (if cache miss)
5. Result display with execution receipts

### Cache statistics

View the semantic work cache performance:

- **Hit rate**: Percentage of queries served from cache
- **Total entries**: Number of cached chain templates
- **Top entries**: Most frequently used cache entries with hit counts and success rates
- **Cost savings**: Estimated money saved by cache hits vs. LLM calls

### Cost tracking

Monitor LLM spending across providers:

- **Daily spend**: Breakdown by provider (Anthropic, OpenAI, etc.)
- **Monthly totals**: Running cost by category
- **Per-query costs**: Average cost per cache miss

### Agent management

- **List agents**: See all installed agents with status (running/stopped/disabled)
- **Agent details**: View permissions, chains, triggers, and constitution
- **Start/stop**: Control agents from the dashboard

### Constitution view

- **Active rules**: See all constitution rules and their enforcement levels
- **Recent checks**: History of constitution evaluations with outcomes (allowed/blocked/confirmed)

---

## API endpoints

The web dashboard exposes a REST API that you can use programmatically. All endpoints require authentication via the `Authorization` header or session cookie.

### Query

```bash
curl -X POST http://localhost:8919/api/query \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $(echo -n 'your-secure-password' | base64)" \
  -d '{"query": "check NVDA price"}'
```

Response:

```json
{
  "result": "NVDA is currently at $847.50",
  "cache_hit": true,
  "latency_ms": 42,
  "cost_usd": 0.0
}
```

### Cache stats

```bash
curl http://localhost:8919/api/cache/stats \
  -H "Authorization: Bearer $(echo -n 'your-secure-password' | base64)"
```

Response:

```json
{
  "total_entries": 47,
  "hit_rate": 0.89,
  "total_hits": 1523,
  "total_misses": 187,
  "estimated_savings_usd": 12.40
}
```

### Cost summary

```bash
curl http://localhost:8919/api/costs \
  -H "Authorization: Bearer $(echo -n 'your-secure-password' | base64)"
```

Response:

```json
{
  "daily_usd": 0.42,
  "monthly_usd": 8.75,
  "by_provider": {
    "anthropic": 6.20,
    "openai": 2.55
  }
}
```

### Agent list

```bash
curl http://localhost:8919/api/agents \
  -H "Authorization: Bearer $(echo -n 'your-secure-password' | base64)"
```

Response:

```json
{
  "agents": [
    {
      "name": "morning-briefing",
      "version": "1.0.0",
      "status": "running",
      "description": "Daily summary: weather, calendar, unread emails, news"
    },
    {
      "name": "stock-watcher",
      "version": "1.0.0",
      "status": "stopped",
      "description": "Monitor stock prices and alert on threshold crossings"
    }
  ]
}
```

### Constitution check

```bash
curl -X POST http://localhost:8919/api/constitution/check \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $(echo -n 'your-secure-password' | base64)" \
  -d '{"query": "delete all files"}'
```

Response:

```json
{
  "allowed": false,
  "enforcement": "block",
  "matched_rule": "block_destructive_keywords",
  "reason": "Destructive operations require explicit confirmation"
}
```

---

## API endpoint reference

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

---

## Security considerations

- The dashboard binds to `127.0.0.1` by default, accessible only from localhost.
- To expose it to a network, use `--bind 0.0.0.0:8919` -- but ensure you are behind a firewall or reverse proxy with TLS.
- The password is hashed server-side; it is never stored in plaintext.
- For production, run behind a reverse proxy (nginx, Caddy) with HTTPS.

Example nginx reverse proxy configuration:

```nginx
server {
    listen 443 ssl;
    server_name nyaya.yourdomain.com;

    ssl_certificate /etc/letsencrypt/live/nyaya.yourdomain.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/nyaya.yourdomain.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8919;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
    }
}
```

---

## Troubleshooting

**Dashboard shows "NABA_WEB_PASSWORD not set":**
- Export the `NABA_WEB_PASSWORD` variable before starting the daemon or web server.

**Cannot connect to http://localhost:8919:**
- Verify the web server is running (check terminal output).
- If you changed the bind address, use the correct address.
- Check if another process is using port 8919: `lsof -i :8919`.

**Authentication fails:**
- Double-check the password matches `NABA_WEB_PASSWORD`.
- Clear browser cookies and try again.

---

## Next steps

- [Telegram Setup](./telegram-setup.md) -- Set up the Telegram bot channel
- [Discord Setup](./discord-setup.md) -- Set up the Discord bot channel
- [Building Agents](./building-agents.md) -- Create agents to manage from the dashboard
