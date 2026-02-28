# Plugin Development

> **What you'll learn**
>
> - The plugin manifest format and how plugins extend NabaOS
> - How to create subprocess abilities (run local commands)
> - How to create cloud abilities (call HTTP APIs)
> - Trust levels and what they mean
> - How to test and install your plugin

---

## Prerequisites

- NabaOS installed (`nabaos --version`)
- For subprocess plugins: the command-line tool you want to wrap must be installed
- For cloud plugins: the API endpoint must be accessible

---

## What is a plugin?

A plugin is a `manifest.yaml` that registers one or more **abilities** with NabaOS. Abilities are the atomic operations that chain steps invoke. When you write `ability: weather.current` in a chain step, NabaOS looks up the `weather.current` ability from the weather plugin.

---

## Plugin manifest format

Every plugin is defined by a `manifest.yaml` file:

```yaml
name: my-plugin
version: "1.0.0"
author: your-name
trust_level: COMMUNITY
description: "Description of what this plugin does"

abilities:
  my-plugin.action_one:
    type: cloud            # or "subprocess" or "wasm"
    # ... type-specific fields
    description: "What this ability does"
    receipt_fields: [field1, field2]
```

### Trust levels

| Level | Meaning |
|-------|---------|
| `OFFICIAL` | Maintained by the NabaOS team, shipped with the runtime |
| `VERIFIED` | Community plugin that has been reviewed and signed |
| `COMMUNITY` | Community plugin, not reviewed -- use at your own risk |

---

## Creating a cloud plugin

Cloud abilities call HTTP APIs. Here is a complete example that wraps the Open-Meteo weather API:

```yaml
name: my-weather
version: "1.0.0"
author: your-name
trust_level: COMMUNITY
description: "Weather data via Open-Meteo (free, no API key needed)"

abilities:
  my-weather.current:
    type: cloud
    endpoint: "https://api.open-meteo.com/v1/forecast"
    method: GET
    params:
      latitude: { type: string, required: true }
      longitude: { type: string, required: true }
      current_weather: { type: string, default: "true" }
    description: "Get current weather for a location"
    receipt_fields: [temperature, windspeed]
```

### Using secrets in headers

For APIs that require authentication, reference vault secrets with `{{VAR_NAME}}`:

```yaml
abilities:
  my-api.query:
    type: cloud
    endpoint: "https://api.example.com/v1/data"
    method: GET
    headers:
      Authorization: "Bearer {{MY_API_TOKEN}}"
```

Store the secret with:

```bash
echo "your-api-token" | nabaos config vault store MY_API_TOKEN
```

---

## Installing your plugin

```bash
nabaos admin plugin install plugins/my-weather/manifest.yaml
```

## Listing installed plugins

```bash
nabaos admin plugin list
```

## Removing a plugin

```bash
nabaos admin plugin remove my-weather
```

## Registering standalone subprocess abilities

```bash
nabaos admin plugin register-subprocess subprocess_config.yaml
```

---

## Next steps

- [Building Agents](./building-agents.md) -- Package plugins and chains into installable agents
- [Writing Chains](./writing-chains.md) -- Use your plugin abilities in chain steps
- [Secrets Management](./secrets-management.md) -- Store API keys that plugins need
