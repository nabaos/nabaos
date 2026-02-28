# Web API Endpoints

The NabaOS web dashboard exposes a REST API on the configured bind address
(default: `127.0.0.1:8919`). Start the server with:

```bash
nabaos start --web-only                        # default: 127.0.0.1:8919
nabaos start --web-only --bind 0.0.0.0:9000   # custom bind address
```

## Authentication

If `NABA_WEB_PASSWORD` is set, all API endpoints (except auth endpoints)
require a valid session token. Tokens are passed via the `Authorization`
header:

```
Authorization: Bearer <token>
```

If `NABA_WEB_PASSWORD` is not set, the web dashboard is disabled.

Sessions expire after 24 hours by default (configurable via
`NABA_WEB_SESSION_TTL`).

---

### `POST /api/auth/login`

Authenticate and obtain a session token.

**Request:**

```bash
curl -X POST http://localhost:8919/api/auth/login \
  -H "Content-Type: application/json" \
  -d '{"password": "my-password"}'
```

**Response (200):**

```json
{
  "token": "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
}
```

**Response (401):**

```json
{
  "error": "Invalid password"
}
```

---

### `POST /api/auth/logout`

Invalidate the current session.

**Request:**

```bash
curl -X POST http://localhost:8919/api/auth/logout \
  -H "Authorization: Bearer <token>"
```

**Response:** `204 No Content`

---

### `GET /api/auth/status`

Check whether authentication is required and whether the current token is
valid.

**Request:**

```bash
curl http://localhost:8919/api/auth/status \
  -H "Authorization: Bearer <token>"
```

**Response (200):**

```json
{
  "authenticated": true,
  "auth_required": true
}
```

---

## Dashboard

### `GET /api/dashboard`

Overview of the system: chain count, scheduled jobs, abilities, and cost
summary.

**Request:**

```bash
curl http://localhost:8919/api/dashboard \
  -H "Authorization: Bearer <token>"
```

**Response (200):**

```json
{
  "total_chains": 12,
  "total_scheduled_jobs": 3,
  "total_abilities": 47,
  "costs": {
    "total_spent_usd": 4.23,
    "total_saved_usd": 18.91,
    "savings_percent": 81.7,
    "total_llm_calls": 142,
    "total_cache_hits": 891,
    "total_input_tokens": 285400,
    "total_output_tokens": 98200
  }
}
```

---

## Query

### `POST /api/query`

Process a query through the full orchestrator pipeline.

**Request:**

```bash
curl -X POST http://localhost:8919/api/query \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"query": "check the price of NVDA"}'
```

**Response (200):**

```json
{
  "tier": "Tier1",
  "intent_key": "check|price",
  "confidence": 0.95,
  "allowed": true,
  "latency_ms": 0.42,
  "description": "Fingerprint cache hit",
  "response_text": "NVDA is currently trading at $142.50",
  "nyaya_mode": "MODE 1",
  "security": {
    "credentials_found": 0,
    "injection_detected": false,
    "injection_confidence": 0.0,
    "was_redacted": false
  }
}
```

---

## Chains

### `GET /api/chains`

List all stored chain definitions.

**Request:**

```bash
curl http://localhost:8919/api/chains \
  -H "Authorization: Bearer <token>"
```

**Response (200):**

```json
[
  {
    "chain_id": "check_weather",
    "name": "Check Weather",
    "description": "Fetch weather for a city and notify user",
    "trust_level": 3,
    "hit_count": 142,
    "success_count": 140,
    "created_at": "2026-01-15T10:30:00Z"
  }
]
```

---

## Scheduling

### `GET /api/chains/schedule`

List all scheduled jobs.

**Request:**

```bash
curl http://localhost:8919/api/chains/schedule \
  -H "Authorization: Bearer <token>"
```

**Response (200):**

```json
[
  {
    "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "chain_id": "check_email",
    "interval_secs": 600,
    "enabled": true,
    "last_run_at": 1708012345,
    "last_output": "No new messages",
    "run_count": 42,
    "created_at": 1707500000
  }
]
```

### `POST /api/chains/schedule`

Create a new scheduled job.

**Request:**

```bash
curl -X POST http://localhost:8919/api/chains/schedule \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{
    "chain_id": "check_email",
    "interval": "10m",
    "params": {"folder": "inbox"}
  }'
```

**Response (201):**

```json
{
  "job_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
}
```

The `interval` field accepts human-readable durations: `"30s"`, `"10m"`,
`"1h"`.

### `DELETE /api/chains/schedule/{id}`

Disable a scheduled job.

**Request:**

```bash
curl -X DELETE http://localhost:8919/api/chains/schedule/a1b2c3d4-e5f6-7890 \
  -H "Authorization: Bearer <token>"
```

**Response:** `204 No Content`

---

## Costs

### `GET /api/costs`

Retrieve cost tracking data. Optionally filter by time range.

**Request:**

```bash
# All-time costs
curl http://localhost:8919/api/costs \
  -H "Authorization: Bearer <token>"

# Costs since a specific Unix timestamp (milliseconds)
curl "http://localhost:8919/api/costs?since=1708012345000" \
  -H "Authorization: Bearer <token>"
```

**Response (200):**

```json
{
  "total_spent_usd": 4.23,
  "total_saved_usd": 18.91,
  "savings_percent": 81.7,
  "total_llm_calls": 142,
  "total_cache_hits": 891,
  "total_input_tokens": 285400,
  "total_output_tokens": 98200
}
```

---

## Security

### `POST /api/security/scan`

Scan text for credentials, PII, and prompt injection patterns.

**Request:**

```bash
curl -X POST http://localhost:8919/api/security/scan \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"text": "my api key is sk-ant-abc123 and SSN 123-45-6789"}'
```

**Response (200):**

```json
{
  "credential_count": 1,
  "pii_count": 1,
  "types_found": ["api_key", "ssn"],
  "injection_detected": false,
  "injection_match_count": 0,
  "injection_max_confidence": 0.0,
  "injection_category": null,
  "redacted": "my api key is [REDACTED:api_key] and SSN [REDACTED:ssn]"
}
```

---

## Abilities

### `GET /api/abilities`

List all available abilities (built-in, plugin, subprocess, cloud).

**Request:**

```bash
curl http://localhost:8919/api/abilities \
  -H "Authorization: Bearer <token>"
```

**Response (200):**

```json
[
  {
    "name": "data.fetch_url",
    "description": "Fetch content from a URL",
    "source": "built-in"
  },
  {
    "name": "files.read_psd",
    "description": "Read Adobe PSD files, extract layers and metadata",
    "source": "plugin"
  }
]
```

---

## Constitution

### `GET /api/constitution`

Retrieve the active constitution rules and available templates.

**Request:**

```bash
curl http://localhost:8919/api/constitution \
  -H "Authorization: Bearer <token>"
```

**Response (200):**

```json
{
  "name": "default",
  "rules": [
    {
      "name": "block_destructive_keywords",
      "enforcement": "Block",
      "trigger_actions": [],
      "trigger_targets": [],
      "trigger_keywords": ["delete all", "rm -rf", "drop table", "format disk", "wipe", "destroy"],
      "reason": "Destructive operations require explicit confirmation"
    },
    {
      "name": "allow_check_actions",
      "enforcement": "Allow",
      "trigger_actions": ["check"],
      "trigger_targets": [],
      "trigger_keywords": [],
      "reason": "Read-only operations are safe"
    }
  ],
  "templates": [
    {
      "name": "default",
      "description": "General-purpose safety defaults",
      "rules_count": 6
    },
    {
      "name": "trading",
      "description": "Financial markets monitoring and trading",
      "rules_count": 3
    }
  ]
}
```

---

### `POST /api/constitution/check`

Check a query against the active constitution.

**Request:**

```bash
curl -X POST http://localhost:8919/api/constitution/check \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"query": "delete all files"}'
```

---

## Confirmation

### `POST /api/auth/confirm/{token}`

Confirm a pending weblink confirmation token (used by the 2FA system for
Telegram bot confirmations via web link).

**Request:**

```bash
curl -X POST http://localhost:8919/api/auth/confirm/abc123-token
```

**Response (200):**

```json
{
  "confirmed": true
}
```

---

## Health

### `GET /api/health`

Health check endpoint. No authentication required.

**Request:**

```bash
curl http://localhost:8919/api/health
```

**Response (200):**

```json
{
  "status": "ok"
}
```

---

## Static Files

If the `nyaya-web/dist/` directory exists (built SPA frontend), it is served
as static files at the root path. All non-API routes fall back to
`index.html` for client-side routing.

If the frontend is not built, a fallback HTML page is shown with
instructions to build it.

## Error Responses

All error responses follow the same format:

```json
{
  "error": "Description of the error"
}
```

Common HTTP status codes:

| Code | Meaning |
|------|---------|
| 200 | Success |
| 201 | Created (new resource) |
| 204 | No Content (success, no body) |
| 400 | Bad Request (invalid input) |
| 401 | Unauthorized (missing or invalid token) |
| 500 | Internal Server Error |
