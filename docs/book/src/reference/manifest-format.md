# Agent Manifest Format

Every agent in NabaOS is declared by a manifest file (JSON or YAML).
The manifest specifies the agent's identity, permissions, resource
limits, intent routing filters, background behavior, and triggers.

## Schema

```yaml
# Required fields
name: string              # Human-readable agent name (must be non-empty)
version: string           # Semantic version (must be non-empty)
description: string       # Short description of what this agent does

# Permissions
permissions:              # List of ability names this agent is allowed to call
  - string

# Optional identity
author: string            # Author name or organization
signature: string         # Signature for package verification

# Resource limits (WASM sandbox)
memory_limit_mb: u32      # Max memory in MB [default: 64, max: 512]
fuel_limit: u64           # Fuel limit for execution [default: 1_000_000]

# Key-value store
kv_namespace: string      # Namespace for scoped KV store [default: agent name]
data_namespace: string    # Data namespace override

# Agent OS integration
background: bool          # Whether this agent runs as a background service [default: false]

subscriptions:            # Event subscriptions for background wake
  - string

intent_filters:           # Intent filters for Agent OS routing
  - actions:              # W5H2 actions to match (empty = match all)
      - string
    targets:              # W5H2 targets to match (empty = match all)
      - string
    priority: i32         # Routing priority [default: 0, higher = preferred]

resources:                # Resource limits for the Agent OS sandbox
  max_memory_mb: u32      # Memory limit [default: 128]
  max_fuel: u64           # Fuel limit [default: 1_000_000]
  max_api_calls_per_hour: u32  # API call rate limit [default: 100]

triggers:                 # Automated wake-up triggers
  scheduled:              # Time-based triggers
    - chain: string       # Chain ID to execute
      interval: string    # Interval string (e.g., "10m", "1h")
      at: string          # Optional: specific time (e.g., "09:00")
      params:             # Parameters to pass to the chain
        key: value

  events:                 # Event-based triggers (MessageBus)
    - on: string          # Event name to listen for
      chain: string       # Chain ID to execute
      filter:             # Optional: event field filters
        key: value
      params:
        key: value

  webhooks:               # HTTP webhook triggers
    - path: string        # URL path (e.g., "/hook/my-agent")
      chain: string       # Chain ID to execute
      secret: string      # Optional: HMAC secret for verification
      params:
        key: value
```

## Known Permissions

The following ability names can be granted to agents:

| Permission | Description |
|------------|-------------|
| `kv.read` | Read from the agent's scoped key-value store |
| `kv.write` | Write to the agent's scoped key-value store |
| `http.fetch` | Make outbound HTTP requests |
| `log.info` | Write info-level log messages |
| `log.error` | Write error-level log messages |
| `notify.user` | Send notifications to the user |
| `data.fetch_url` | Fetch data from a URL |
| `nlp.sentiment` | Run sentiment analysis |
| `nlp.summarize` | Summarize text |
| `storage.get` | Read from persistent storage |
| `storage.set` | Write to persistent storage |
| `flow.branch` | Conditional branching in chain execution |
| `flow.stop` | Stop chain execution |
| `schedule.delay` | Delay execution |
| `email.send` | Send email |
| `trading.get_price` | Fetch financial price data |

Plugin and subprocess abilities extend this list dynamically.

## Annotated Example

```json
{
  "name": "weather-monitor",
  "version": "1.2.0",
  "description": "Monitors weather conditions and sends alerts for severe events",
  "author": "nabaos-community",
  "permissions": [
    "data.fetch_url",
    "notify.user",
    "kv.read",
    "kv.write",
    "schedule.delay"
  ],
  "memory_limit_mb": 32,
  "fuel_limit": 500000,
  "kv_namespace": "weather",
  "background": true,
  "subscriptions": [
    "weather.alert",
    "location.changed"
  ],
  "intent_filters": [
    {
      "actions": ["check", "search"],
      "targets": ["weather", "forecast"],
      "priority": 10
    },
    {
      "actions": ["notify"],
      "targets": ["weather"],
      "priority": 5
    }
  ],
  "resources": {
    "max_memory_mb": 64,
    "max_fuel": 500000,
    "max_api_calls_per_hour": 50
  }
}
```

> **Note:** Without the `bert` feature enabled at compile time, intent-based
> routing degrades to `unknown_unknown`. Agents that rely on specific intent
> filters should document this dependency.

## Validation Rules

The manifest is validated on load with the following constraints:

- `name` must be non-empty.
- `version` must be non-empty.
- `memory_limit_mb` must be between 1 and 512.
- `fuel_limit` must be greater than 0.
- Intent filter matching is case-insensitive.
- An empty `actions` or `targets` list in an intent filter matches all
  values (wildcard behavior).

## Packaging

Agent source directories are packaged into `.nap` files using:

```bash
nabaos config agent package ./my-agent/ -o my-agent.nap
```

The source directory must contain a manifest file at its root. The
resulting `.nap` file can be installed with `nabaos config agent install`.
