# Agent Packages

> **What you'll learn**
>
> - What a `.nap` file is and what it contains
> - The full manifest schema with annotated examples
> - How the permissions model works: declaration, enforcement, and known permissions
> - Data namespaces and how agents are isolated from each other
> - Resource limits: memory, CPU, fuel, and API rate limiting
> - Trigger types: scheduled, event-based, and webhook
> - The full package lifecycle from creation to uninstallation
> - How to browse and search the agent catalog

---

## What Is a .nap File?

A `.nap` file (NabaOS Agent Package) is a tar.gz archive that contains everything needed to install and run an agent on NabaOS. Think of it as an APK for Android or a `.app` bundle for macOS, but for AI agents.

```
weather-agent-0.1.0.nap
    |
    +-- manifest.json       # Agent identity, permissions, and configuration
    +-- agent.wasm          # Compiled WebAssembly module (the agent logic)
    +-- chains/             # Chain definition files (optional)
    |   +-- check_weather.yaml
    |   +-- daily_forecast.yaml
    +-- assets/             # Static assets (optional)
    |   +-- templates/
    |       +-- forecast.hbs
    +-- README.md           # Human-readable documentation (optional)
```

The `.nap` extension is a convention. Under the hood, it is a standard gzip-compressed tar archive:

```bash
# Create a .nap file
tar czf weather-agent-0.1.0.nap manifest.json agent.wasm chains/ assets/

# Inspect a .nap file
tar tzf weather-agent-0.1.0.nap

# Extract a .nap file
tar xzf weather-agent-0.1.0.nap -C /tmp/inspect/
```

---

## Manifest Schema

The manifest is the heart of a `.nap` package. It declares the agent's identity, permissions, resource requirements, and behavior.

```json
{
  "name": "weather-agent",
  "version": "0.1.0",
  "description": "Fetches weather data and provides forecasts for any city",
  "author": "NabaOS Community",
  "permissions": [
    "kv.read",
    "kv.write",
    "http.fetch",
    "log.info",
    "notify.user"
  ],
  "memory_limit_mb": 32,
  "fuel_limit": 500000,
  "resources": {
    "max_memory_mb": 64,
    "max_fuel": 1000000,
    "max_api_calls_per_hour": 50
  },
  "intent_filters": [
    {
      "actions": ["check", "search"],
      "targets": ["weather"],
      "priority": 10
    }
  ],
  "kv_namespace": "weather-agent",
  "data_namespace": "weather-data",
  "background": false,
  "subscriptions": ["weather.alert", "schedule.daily"]
}
```

> **Note:** The `--manifest` flag in `nabaos admin run` expects a JSON file.

> **`bert` feature gate caveat:** Intent routing depends on the BERT/SetFit classification models. Without the `bert` feature, all queries classify as `unknown_unknown` and intent filters in agent manifests will not match. Agents will only be invocable by direct name.

---

## Permissions Model

### Declaration

Agents declare their required permissions in the manifest. This is a declaration of intent, not a grant. The runtime decides whether to grant each permission based on the user's approval and the agent's trust level.

### Known permissions

The following permissions are recognized by the runtime:

| Permission | Description | Risk level |
|---|---|---|
| `kv.read` | Read from the agent's namespaced key-value store | Low |
| `kv.write` | Write to the agent's namespaced key-value store | Low |
| `http.fetch` | Make outbound HTTP requests | Medium |
| `log.info` | Write info-level log entries | Low |
| `log.error` | Write error-level log entries | Low |
| `notify.user` | Send notifications to the user | Low |
| `data.fetch_url` | Fetch data from a URL | Medium |
| `nlp.sentiment` | Run sentiment analysis | Low |
| `nlp.summarize` | Run text summarization | Low |
| `storage.get` | Read from persistent storage | Low |
| `storage.set` | Write to persistent storage | Medium |
| `flow.branch` | Branch execution flow | Low |
| `flow.stop` | Stop execution flow | Low |
| `schedule.delay` | Schedule delayed execution | Medium |
| `email.send` | Send email | High |
| `trading.get_price` | Fetch financial price data | Low |

### Runtime enforcement

Permission checks happen at the WASM runtime boundary. When an agent's code calls a host function (e.g., `http_fetch`), the runtime checks:

1. Does the agent's manifest declare `http.fetch`?
2. Was this permission approved during installation?
3. Is the agent within its resource limits?

If any check fails, the call is denied and the agent receives an error. The agent cannot bypass permission checks because it runs inside a sandboxed WASM environment -- it has no direct access to the host system.

```
Agent code                    WASM Runtime                   Host System
──────────                    ────────────                   ───────────
call http_fetch("...") ──→   Check permissions:
                              ✓ http.fetch declared
                              ✓ http.fetch approved
                              ✓ API rate limit not exceeded
                                        ──────────────────→  Execute HTTP request
                                        ←──────────────────  Response
                        ←──  Return response to agent
```

---

## Data Namespaces

Each agent gets an isolated data namespace. This prevents agents from reading or modifying each other's data.

### How namespaces work

```
Agent: weather-agent     namespace: "weather-agent"
  kv.get("last_city")   → reads from weather-agent/last_city
  kv.set("cache", data) → writes to weather-agent/cache

Agent: email-agent       namespace: "email-agent"
  kv.get("last_city")   → reads from email-agent/last_city (DIFFERENT key)
  kv.get("cache")       → reads from email-agent/cache (DIFFERENT data)
```

The weather agent cannot access the email agent's data, and vice versa. The namespace is enforced at the runtime level, not by convention.

### Custom namespaces

By default, the namespace is the agent's name. You can override it with `kv_namespace` or `data_namespace` in the manifest to share data between related agents.

---

## Resource Limits

Every agent runs within enforced resource limits. This prevents runaway agents from consuming all system resources.

### Memory limit (`memory_limit_mb`)

The maximum memory the agent's WASM module can allocate. Default: 64 MB. Range: 1 - 512 MB.

If the agent tries to allocate beyond this limit, the WASM runtime traps with an out-of-memory error.

### Fuel limit (`fuel_limit`)

Fuel is a counter that decrements with each WASM instruction executed. When fuel reaches zero, execution is terminated. Default: 1,000,000. This prevents infinite loops and unbounded computation.

```
Rough equivalents:
  100,000 fuel    ≈ simple data transformation
  500,000 fuel    ≈ moderate computation (JSON parsing, filtering)
  1,000,000 fuel  ≈ complex computation (data analysis, formatting)
  10,000,000 fuel ≈ heavy computation (only for trusted agents)
```

### API rate limit (`max_api_calls_per_hour`)

Limits how many external API calls the agent can make per hour. Default: 100. The counter resets every 3600 seconds.

---

## Triggers

Triggers define when an agent wakes up and runs. There are three trigger types:

### Scheduled triggers

Fire on a time interval, similar to cron jobs:

```yaml
triggers:
  scheduled:
    - chain: daily_forecast
      interval: "24h"
      at: "07:00"
      params:
        city: "Mumbai"

    - chain: price_check
      interval: "5m"
      params:
        ticker: "BTC"
```

### Event triggers

Fire when a matching event appears on the internal message bus:

```yaml
triggers:
  events:
    - on: "email.received"
      filter:
        from: "boss@example.com"
      chain: urgent_email_handler
      params:
        priority: "high"
```

### Webhook triggers

Fire when an external HTTP POST arrives at a specific path:

```yaml
triggers:
  webhooks:
    - path: "/hooks/github"
      chain: github_event_handler
      secret: "webhook-secret"
```

---

## Package Lifecycle

Agent packages follow a defined lifecycle:

```
create → install → start → (running) → stop → uninstall
                     ↑         |
                     +--- restart/update
```

### Create

Build the agent code, write the manifest, and package everything into a `.nap` file:

```bash
# Build the WASM module
cargo build --target wasm32-wasi --release

# Package into a .nap file
nabaos config agent package \
  source-dir/ \
  --output weather-agent-0.1.0.nap
```

### Install

Install the `.nap` package:

```bash
nabaos config agent install weather-agent-0.1.0.nap
```

During installation:

1. The `.nap` archive is extracted and validated
2. The manifest is parsed and checked for required fields
3. The user is prompted to approve permissions
4. A data directory is created under the agent's namespace
5. The agent is registered in the database

### Start / Stop

```bash
nabaos config agent start weather-agent
nabaos config agent stop weather-agent
```

### Uninstall

```bash
nabaos config agent uninstall weather-agent
```

---

## Catalog

The agent catalog is a registry of available agent packages that can be browsed and installed.

### Browsing the catalog

```bash
# List all available agents
nabaos config persona catalog list

# Search by keyword
nabaos config persona catalog search "weather"

# View agent details
nabaos config persona catalog info price-tracker

# Install from catalog
nabaos config persona catalog install price-tracker
```
