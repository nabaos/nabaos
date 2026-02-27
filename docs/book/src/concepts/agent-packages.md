# Agent Packages

> **What you'll learn**
>
> - What a `.nap` file is and what it contains
> - The full manifest.yaml schema with annotated examples
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
    +-- manifest.yaml        # Agent identity, permissions, and configuration
    +-- agent.wasm           # Compiled WebAssembly module (the agent logic)
    +-- chains/              # Chain definition files (optional)
    |   +-- check_weather.yaml
    |   +-- daily_forecast.yaml
    +-- assets/              # Static assets (optional)
    |   +-- templates/
    |       +-- forecast.hbs
    +-- README.md            # Human-readable documentation (optional)
```

The `.nap` extension is a convention. Under the hood, it is a standard gzip-compressed tar archive:

```bash
# Create a .nap file
tar czf weather-agent-0.1.0.nap manifest.yaml agent.wasm chains/ assets/

# Inspect a .nap file
tar tzf weather-agent-0.1.0.nap

# Extract a .nap file
tar xzf weather-agent-0.1.0.nap -C /tmp/inspect/
```

---

## manifest.yaml Schema

The manifest is the heart of a `.nap` package. It declares the agent's identity, permissions, resource requirements, and behavior.

```yaml
# === IDENTITY ===

# Human-readable agent name (required)
# Must be non-empty. Used as the default data namespace.
name: "weather-agent"

# Semantic version (required)
# Must be non-empty. Used for upgrade/downgrade decisions.
version: "0.1.0"

# Short description of what this agent does (required)
description: "Fetches weather data and provides forecasts for any city"

# Author name or organization (optional)
author: "NabaOS Community"

# === PERMISSIONS ===

# List of permissions this agent requests (required, can be empty)
# The runtime will grant or deny each permission based on user approval.
# See "Known Permissions" section below for the full list.
permissions:
  - kv.read             # Read from key-value store
  - kv.write            # Write to key-value store
  - http.fetch          # Make outbound HTTP requests
  - log.info            # Write info-level log entries
  - notify.user         # Send notifications to the user

# === RESOURCE LIMITS ===

# Maximum memory in MB the WASM module may use (default: 64)
# Must be between 1 and 512.
memory_limit_mb: 32

# Fuel limit for execution (default: 1,000,000)
# Fuel is consumed per WASM instruction. Prevents infinite loops.
# Must be > 0.
fuel_limit: 500000

# Extended resource limits for Agent OS sandbox (optional)
resources:
  max_memory_mb: 64           # Memory cap for the agent sandbox
  max_fuel: 1000000           # Fuel cap per invocation
  max_api_calls_per_hour: 50  # Rate limit on API calls

# === INTENT ROUTING ===

# Intent filters determine which queries get routed to this agent.
# If a user query classifies to a matching action+target pair,
# this agent is a candidate to handle it.
intent_filters:
  - actions: [check, search]     # Match these W5H2 actions
    targets: [weather]           # Match these W5H2 targets
    priority: 10                 # Higher priority = preferred over other agents

# === DATA ISOLATION ===

# Namespace for the agent's scoped key-value store (optional)
# Defaults to the agent name if not specified.
# Two agents with different namespaces cannot read each other's data.
kv_namespace: "weather-agent"

# Data namespace override for Agent OS (optional)
# Use this to share data between related agents.
data_namespace: "weather-data"

# === LIFECYCLE ===

# Whether this agent runs as a background service (default: false)
# Background agents stay running and process events continuously.
# Non-background agents are invoked on-demand and shut down after.
background: false

# Event subscriptions for background wake (default: empty)
# Only relevant if background: true.
# The agent wakes up when any subscribed event fires.
subscriptions:
  - "weather.alert"
  - "schedule.daily"

# === SECURITY ===

# Cryptographic signature for verification (optional)
# Set by the signing tool; do not edit manually.
signature: "base64-encoded-ed25519-signature..."
```

---

## Permissions Model

### Declaration

Agents declare their required permissions in `manifest.yaml`. This is a declaration of intent, not a grant. The runtime decides whether to grant each permission based on the user's approval and the agent's trust level.

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

By default, the namespace is the agent's name. You can override it with `kv_namespace` or `data_namespace` in the manifest:

```yaml
# Two agents that need shared access to the same data
# Agent 1: weather-collector
kv_namespace: "weather-data"

# Agent 2: weather-reporter
kv_namespace: "weather-data"
```

Both agents access the same `weather-data` namespace. Use this carefully -- shared namespaces require coordination between agents.

### Storage location

Data is stored on the host filesystem under the agent's data directory:

```
~/.nabaos/
  agents/
    weather-agent/
      data/           # Agent-specific data directory
      logs/           # Agent log files
      state.json      # Runtime state (running/stopped/etc.)
    email-agent/
      data/
      logs/
      state.json
```

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

### Resource monitoring

The runtime tracks actual resource consumption per agent:

```
ResourceUsage {
  fuel_consumed: 234567,        # Total fuel used in current invocation
  api_calls_this_hour: 12,      # API calls in the current hour window
  peak_memory_bytes: 8388608,   # Peak memory usage (8 MB)
}
```

Administrators can view resource usage with:

```bash
nabaos agent stats weather-agent

# Output:
# Agent: weather-agent v0.1.0
# State: running
# Memory: 8 MB / 64 MB (12.5%)
# Fuel: 234K / 1M per invocation
# API calls: 12 / 100 this hour
# Uptime: 4h 23m
```

---

## Triggers

Triggers define when an agent wakes up and runs. There are three trigger types:

### Scheduled triggers

Fire on a time interval, similar to cron jobs:

```yaml
triggers:
  scheduled:
    - chain: daily_forecast       # Which chain to run
      interval: "24h"             # Run every 24 hours
      at: "07:00"                 # Specifically at 7 AM
      params:
        city: "Mumbai"            # Parameters passed to the chain

    - chain: price_check
      interval: "5m"              # Run every 5 minutes
      params:
        ticker: "BTC"
```

### Event triggers

Fire when a matching event appears on the internal message bus:

```yaml
triggers:
  events:
    - on: "email.received"        # Event name to listen for
      filter:
        from: "boss@example.com"  # Only fire for emails from this sender
      chain: urgent_email_handler # Which chain to run
      params:
        priority: "high"

    - on: "price.alert"
      filter:
        ticker: "ETH"
        direction: "down"
      chain: price_drop_handler
```

### Webhook triggers

Fire when an external HTTP POST arrives at a specific path:

```yaml
triggers:
  webhooks:
    - path: "/hooks/github"       # URL path to listen on
      chain: github_event_handler # Which chain to run
      secret: "webhook-secret"    # HMAC validation secret
      params:
        repo: "nabaos"
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
nabaos package create \
  --manifest manifest.yaml \
  --wasm target/wasm32-wasi/release/weather_agent.wasm \
  --output weather-agent-0.1.0.nap
```

### Install

Install the `.nap` package into the local Agent OS:

```bash
nabaos agent install weather-agent-0.1.0.nap

# Output:
# Installing weather-agent v0.1.0...
# Permissions requested:
#   - kv.read       (low risk)
#   - kv.write      (low risk)
#   - http.fetch    (medium risk)
#   - log.info      (low risk)
#   - notify.user   (low risk)
# Approve all permissions? [y/N]: y
# Agent installed: weather-agent v0.1.0
```

During installation:

1. The `.nap` archive is extracted and validated
2. The manifest is parsed and checked for required fields
3. The signature is verified (if present)
4. The user is prompted to approve permissions
5. A data directory is created under the agent's namespace
6. The agent is registered in the Agent OS database

### Start

Start the agent:

```bash
nabaos agent start weather-agent

# Output:
# Starting weather-agent v0.1.0...
# Agent running (PID: agent-weather-agent-001)
# Scheduled triggers active: daily_forecast (every 24h at 07:00)
```

### Stop

Stop the agent gracefully:

```bash
nabaos agent stop weather-agent

# Output:
# Stopping weather-agent...
# Agent stopped. Data preserved.
```

### Uninstall

Remove the agent and optionally its data:

```bash
# Uninstall but keep data
nabaos agent uninstall weather-agent

# Uninstall and delete all data
nabaos agent uninstall weather-agent --purge

# Output:
# Uninstalling weather-agent v0.1.0...
# Remove agent data? [y/N]: y
# Agent uninstalled. Data deleted.
```

---

## Catalog

The agent catalog is a registry of available agent packages that can be browsed and installed.

### Browsing the catalog

```bash
# List all available agents
nabaos catalog list

# Output:
# Name                Version  Author           Description
# weather-agent       0.1.0    NabaOS Community  Weather data and forecasts
# email-summarizer    1.2.0    NabaOS Community  Daily email digest
# price-tracker       0.3.1    TradeCo          Real-time price monitoring
# code-reviewer       0.5.0    DevTools Inc     Automated code review
# ...

# Search by keyword
nabaos catalog search "weather"

# Filter by category
nabaos catalog list --category trading
```

### Viewing agent details

```bash
nabaos catalog info price-tracker

# Output:
# Name:        price-tracker
# Version:     0.3.1
# Author:      TradeCo
# Description: Real-time price monitoring for stocks and crypto
# Permissions: kv.read, kv.write, http.fetch, notify.user
# Resources:   32 MB memory, 500K fuel
# Triggers:    scheduled (every 5m)
# Rating:      4.7/5 (23 reviews)
# Downloads:   1,247
```

### Installing from catalog

```bash
# Install the latest version
nabaos catalog install price-tracker

# Install a specific version
nabaos catalog install price-tracker@0.3.1
```

### Updating installed agents

```bash
# Check for updates
nabaos agent update --check

# Output:
# weather-agent: 0.1.0 → 0.2.0 available
# price-tracker: 0.3.1 (up to date)

# Update a specific agent
nabaos agent update weather-agent

# Update all agents
nabaos agent update --all
```
