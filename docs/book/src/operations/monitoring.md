# Monitoring

> **What you'll learn**
>
> - How to configure log levels with `NABA_LOG_LEVEL`
> - How to monitor LLM spending with `nabaos costs`
> - How to check cache hit rates with `nabaos cache stats`
> - How to set up security alerts via Telegram
> - How anomaly detection works and what triggers alerts
> - How to use the health check endpoint

---

## Log Levels

NabaOS uses `tracing-subscriber` for structured logging. Control verbosity with the `NABA_LOG_LEVEL` environment variable:

| Level | What it shows |
|-------|---------------|
| `error` | Only errors that require attention |
| `warn` | Warnings and errors |
| `info` | Normal operation messages, warnings, and errors (default) |
| `debug` | Detailed internal state, cache decisions, routing decisions |

### Set the log level

```bash
# Via environment variable
export NABA_LOG_LEVEL=debug
nabaos daemon
```

> **Note**: The underlying `tracing-subscriber` crate also respects `RUST_LOG`. If both are set, `RUST_LOG` takes precedence. For most users, `NABA_LOG_LEVEL` is the recommended variable.

Or in your `.env` / systemd environment file:

```
NABA_LOG_LEVEL=debug
```

Or in Docker:

```bash
docker run -e NABA_LOG_LEVEL=debug ghcr.io/nabaos/nabaos:latest
```

### Example log output at each level

**`info`** (default):

```
2026-02-24T10:00:01Z  INFO  NabaOS starting...
2026-02-24T10:00:02Z  INFO  Security layer initialized
2026-02-24T10:00:02Z  INFO  Daemon listening
2026-02-24T10:05:11Z  INFO  Cache hit: check_email (fingerprint match)
2026-02-24T10:05:11Z  INFO  Request completed in 12ms
```

**`debug`**:

```
2026-02-24T10:05:11Z  DEBUG  Fingerprint lookup: hash=a3f8c1 entries_checked=142
2026-02-24T10:05:11Z  DEBUG  Cache hit: similarity=0.97 threshold=0.92
2026-02-24T10:05:11Z  DEBUG  Skipping LLM call, executing cached tool sequence
2026-02-24T10:05:11Z  INFO   Request completed in 12ms
```

**`warn`**:

```
2026-02-24T10:15:00Z  WARN  Daily budget 82% consumed ($8.20 / $10.00)
2026-02-24T10:15:00Z  WARN  Anomaly score elevated: 0.73 (threshold: 0.80)
```

**`error`**:

```
2026-02-24T10:20:00Z  ERROR  LLM provider returned 429 Too Many Requests
2026-02-24T10:20:00Z  ERROR  Failed to write cache entry: database is locked
```

---

## Cost Monitoring

Track how much you are spending on LLM API calls:

```bash
nabaos costs
```

Expected output:

```
=== Cost Summary (All Time) ===
  Total LLM calls:     347
  Total cache hits:     2,841
  Cache hit rate:       89.1%
  Input tokens:         1,245,600
  Output tokens:        423,100
  Total spent:          $4.73
  Total saved:          $38.12
  Savings:              88.9%

=== Last 24 Hours ===
  Total LLM calls:     12
  Total cache hits:     94
  Cache hit rate:       88.7%
  Input tokens:         42,300
  Output tokens:        15,200
  Total spent:          $0.18
  Total saved:          $1.44
  Savings:              88.9%
```

### Key metrics

| Metric | What it means |
|--------|---------------|
| **Cache hit rate** | Percentage of requests served from cache without an LLM call. Target: >85% after the first week. |
| **Total spent** | Actual dollars spent on LLM API calls. |
| **Total saved** | Estimated dollars saved by cache hits (based on what those requests would have cost). |
| **Savings** | `total_saved / (total_spent + total_saved) * 100` |

### Programmatic access

If the web dashboard is running, query costs via the API:

```bash
curl -s http://localhost:3000/api/costs | python3 -m json.tool
```

Expected output:

```json
{
    "total_spent_usd": 4.73,
    "total_saved_usd": 38.12,
    "savings_percent": 88.9,
    "total_llm_calls": 347,
    "total_cache_hits": 2841,
    "total_input_tokens": 1245600,
    "total_output_tokens": 423100
}
```

---

## Cache Statistics

Monitor the cache tiers individually:

```bash
nabaos cache stats
```

Expected output:

```
=== Cache Statistics ===

Fingerprint Cache (Tier 1):
  Entries: 142
  Hits:    1,203

Intent Cache (Tier 2):
  Total entries:   89
  Enabled entries: 84
  Total hits:      1,638
```

### What the numbers mean

| Cache tier | Description |
|------------|-------------|
| **Fingerprint Cache (Tier 1)** | Exact-match lookup by query hash. Sub-millisecond. Zero cost. |
| **Intent Cache (Tier 2)** | Semantic similarity match using embeddings. Handles paraphrased queries. |
| **Enabled vs. total entries** | Entries with low success rates are automatically disabled (not deleted). |

A healthy system shows the fingerprint cache growing over time as repeated queries are recognized, and the intent cache accumulating entries for paraphrased patterns.

---

## Security Alerts

NabaOS can send real-time security alerts to a dedicated Telegram bot. This keeps security notifications separate from the main agent conversation.

### Setup

1. Create a second Telegram bot via [@BotFather](https://t.me/BotFather) for security alerts.
2. Get the chat ID where alerts should go (send a message to the bot, then check `https://api.telegram.org/bot<TOKEN>/getUpdates`).
3. Set the environment variables:

```bash
export NABA_SECURITY_BOT_TOKEN="987654:XYZ-security-bot-token"
export NABA_ALERT_CHAT_ID="123456789"
```

Or in `/etc/nabaos/env`:

```
NABA_SECURITY_BOT_TOKEN=987654:XYZ-security-bot-token
NABA_ALERT_CHAT_ID=123456789
```

### What triggers alerts

| Alert type | Trigger |
|------------|---------|
| **Credential detected** | API keys, passwords, tokens, or PII found in a user query |
| **Injection attempt** | Prompt injection or jailbreak patterns detected by the security layer |
| **Out-of-domain request** | A query falls outside the constitution's allowed domains |
| **Anomaly detected** | Behavioral deviation exceeds the anomaly threshold |
| **Budget exceeded** | Daily LLM spending exceeds `NABA_DAILY_BUDGET_USD` |

### Example alert message

```
[SECURITY ALERT] Credential Detected

Type:      API key pattern
Source:     Telegram / user:42
Timestamp: 2026-02-24T10:30:15Z
Action:    Blocked — credential stripped before processing

The query contained what appears to be an AWS access key.
The credential was NOT forwarded to any LLM provider.
```

---

## Anomaly Detection

The agent builds a behavioral profile of normal usage patterns during a learning period (default: 24 hours, configurable via `NABA_LEARNING_HOURS`). After the learning period, deviations trigger alerts.

Anomaly detection monitors:

| Signal | Normal | Anomalous |
|--------|--------|-----------|
| **Request frequency** | 5-20 requests/hour | 200+ requests/hour (possible automation abuse) |
| **Query length** | 10-500 characters | 5000+ characters (possible injection payload) |
| **Domain distribution** | Consistent with constitution | Sudden shift to out-of-domain topics |
| **Time-of-day patterns** | Active 9am-11pm | Burst at 3am (possible compromised token) |
| **Cost per request** | $0.00-0.01 avg | $5+ per request (possible exploitation) |

When the anomaly score crosses the threshold (default: 0.80), the agent:
1. Sends a Telegram alert (if security bot is configured).
2. Logs the event at `WARN` level.
3. Continues processing (alerts are informational, not blocking by default).

---

## Health Check Endpoint

When the web dashboard is running (`nabaos web`), a health endpoint is available:

```bash
curl -s http://localhost:3000/api/health
```

Expected response (HTTP 200):

```json
{
    "status": "ok",
    "version": "0.1.0"
}
```

Use this endpoint for:

- **Docker health checks**: `test: ["CMD", "curl", "-sf", "http://localhost:3000/api/health"]`
- **Load balancer probes**: Point your ALB/Cloud Run health check at `/api/health`
- **Uptime monitoring**: Ping from an external service (UptimeRobot, Pingdom, etc.)

### Dashboard endpoint

For richer status information, use the dashboard API:

```bash
curl -s http://localhost:3000/api/dashboard | python3 -m json.tool
```

Expected response:

```json
{
    "total_chains": 5,
    "total_scheduled_jobs": 2,
    "total_abilities": 12,
    "costs": {
        "total_spent_usd": 4.73,
        "total_saved_usd": 38.12,
        "savings_percent": 88.9,
        "total_llm_calls": 347,
        "total_cache_hits": 2841,
        "total_input_tokens": 1245600,
        "total_output_tokens": 423100
    }
}
```

---

## Summary of Monitoring Commands

| Command | What it shows |
|---------|---------------|
| `nabaos costs` | LLM spending, cache savings, token usage |
| `nabaos cache stats` | Cache entries and hit counts per tier |
| `journalctl -u nabaos -f` | Live log stream (systemd) |
| `docker logs -f nabaos` | Live log stream (Docker) |
| `curl localhost:3000/api/health` | Health check (web dashboard) |
| `curl localhost:3000/api/dashboard` | Full status with costs (web dashboard) |
