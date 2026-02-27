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
- A working data directory (default `~/.nabaos`)
- For subprocess plugins: the command-line tool you want to wrap must be installed
- For cloud plugins: the API endpoint must be accessible

---

## What is a plugin?

A plugin is a `manifest.yaml` that registers one or more **abilities** with NabaOS. Abilities are the atomic operations that chain steps invoke. When you write `ability: weather.current` in a chain step, NabaOS looks up the `weather.current` ability from the weather plugin.

NabaOS ships with official plugins for common services:

| Plugin | Abilities | Type |
|--------|-----------|------|
| weather | `weather.current`, `weather.forecast` | cloud |
| gmail | `gmail.read`, `gmail.send`, `gmail.search`, `gmail.labels` | cloud |
| github | `github.issues`, `github.create_issue`, `github.prs`, `github.notifications` | cloud |
| search | `search.web`, `search.images` | cloud |
| browser | `browser.navigate`, `browser.screenshot`, `browser.extract` | cloud |
| calendar | `calendar.list` | cloud |
| slack | `slack.send`, `slack.channels` | cloud |
| notion | `notion.search`, `notion.create_page` | cloud |
| news | `news.headlines`, `news.search` | cloud |
| finance | `finance.quote`, `finance.chart` | cloud |

You can create your own plugins to add any ability.

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

  my-plugin.action_two:
    type: subprocess
    # ... type-specific fields
    description: "Another ability"
    receipt_fields: [output_length]
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

**plugins/my-weather/manifest.yaml:**

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

  my-weather.forecast:
    type: cloud
    endpoint: "https://api.open-meteo.com/v1/forecast"
    method: GET
    params:
      latitude: { type: string, required: true }
      longitude: { type: string, required: true }
      daily: { type: string, default: "temperature_2m_max,temperature_2m_min" }
      forecast_days: { type: int, default: 7 }
    description: "Get weather forecast for up to 7 days"
    receipt_fields: [forecast_days]
```

### Cloud ability fields

| Field | Required | Description |
|-------|----------|-------------|
| `type` | Yes | Must be `cloud` |
| `endpoint` | Yes | Full URL of the API endpoint |
| `method` | Yes | HTTP method: `GET`, `POST`, `PUT`, `DELETE` |
| `headers` | No | HTTP headers (key-value map) |
| `params` | Yes | Parameter definitions with type, required, and default |
| `description` | Yes | Human-readable description |
| `receipt_fields` | No | Which response fields to include in the execution receipt |

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
    params:
      query: { type: string, required: true }
    description: "Query the example API"
    receipt_fields: [result_count]
```

The `{{MY_API_TOKEN}}` value is resolved from the encrypted vault at runtime. Store it with:

```bash
echo "your-api-token" | nabaos secret store MY_API_TOKEN
```

### POST request example

```yaml
abilities:
  my-api.create:
    type: cloud
    endpoint: "https://api.example.com/v1/items"
    method: POST
    headers:
      Authorization: "Bearer {{MY_API_TOKEN}}"
      Content-Type: "application/json"
    params:
      title: { type: string, required: true }
      body: { type: string, required: false }
    description: "Create a new item"
    receipt_fields: [item_id]
```

---

## Creating a subprocess plugin

Subprocess abilities run local commands on the host machine. They are useful for wrapping CLI tools.

**plugins/my-tools/manifest.yaml:**

```yaml
name: my-tools
version: "1.0.0"
author: your-name
trust_level: COMMUNITY
description: "Local CLI tool wrappers"

abilities:
  my-tools.disk_usage:
    type: subprocess
    command: "df"
    args: ["-h", "--total"]
    description: "Check disk usage on the host"
    receipt_fields: [output_length]

  my-tools.ping:
    type: subprocess
    command: "ping"
    args: ["-c", "3", "{{host}}"]
    env:
      LC_ALL: "C"
    params:
      host: { type: string, required: true }
    description: "Ping a host 3 times"
    receipt_fields: [output_length]

  my-tools.git_status:
    type: subprocess
    command: "git"
    args: ["status", "--short"]
    working_dir: "{{repo_path}}"
    params:
      repo_path: { type: string, required: true }
    description: "Get git status of a repository"
    receipt_fields: [output_length]
```

### Subprocess ability fields

| Field | Required | Description |
|-------|----------|-------------|
| `type` | Yes | Must be `subprocess` |
| `command` | Yes | The executable to run |
| `args` | No | List of command-line arguments (supports `{{var}}` interpolation) |
| `env` | No | Environment variables to set |
| `working_dir` | No | Working directory for the command |
| `params` | No | Parameters that can be interpolated into args |
| `description` | Yes | Human-readable description |
| `receipt_fields` | No | Fields for the execution receipt |

### Security considerations for subprocess plugins

Subprocess plugins run commands directly on your machine. Be cautious:

- Only install subprocess plugins from trusted sources
- Review the `command` and `args` fields before installing
- Use the constitution to block dangerous commands
- Parameters are interpolated into args -- ensure they cannot inject shell commands

---

## Creating a WASM plugin

WASM plugins run compiled WebAssembly modules in a sandboxed environment:

```yaml
abilities:
  my-wasm.process:
    type: wasm
    module: "process.wasm"
    description: "Process data in a sandboxed WASM module"
    receipt_fields: [output_length]
```

WASM plugins execute with fuel metering (via wasmtime), so they cannot run indefinitely. They have no filesystem or network access unless explicitly granted.

---

## Installing your plugin

Install a plugin by pointing to its manifest file:

```bash
nabaos plugin install plugins/my-weather/manifest.yaml
```

Expected output:

```
Installing plugin: my-weather v1.0.0
  Trust level: COMMUNITY
  Abilities registered:
    my-weather.current (cloud GET)
    my-weather.forecast (cloud GET)
Plugin installed: my-weather
```

## Listing installed plugins

```bash
nabaos plugin list
```

Expected output:

```
Installed plugins:
  weather      v1.0.0  OFFICIAL    2 abilities
  gmail        v1.0.0  OFFICIAL    4 abilities
  github       v1.0.0  OFFICIAL    4 abilities
  my-weather   v1.0.0  COMMUNITY   2 abilities
```

## Removing a plugin

```bash
nabaos plugin remove my-weather
```

Expected output:

```
Removed plugin: my-weather
  Unregistered abilities: my-weather.current, my-weather.forecast
```

---

## Registering standalone subprocess abilities

For quick one-off subprocess abilities, you can register them without a full plugin manifest:

```bash
nabaos plugin register-subprocess subprocess_config.yaml
```

Where `subprocess_config.yaml` defines the abilities directly.

---

## Complete working example

Here is a GitHub plugin that wraps the GitHub REST API:

**plugins/my-github/manifest.yaml:**

```yaml
name: my-github
version: "1.0.0"
author: your-name
trust_level: COMMUNITY
description: "GitHub API integration"

abilities:
  my-github.issues:
    type: cloud
    endpoint: "https://api.github.com/repos/{{owner}}/{{repo}}/issues"
    method: GET
    headers:
      Authorization: "Bearer {{GITHUB_TOKEN}}"
      Accept: "application/vnd.github+json"
    params:
      owner: { type: string, required: true }
      repo: { type: string, required: true }
      state: { type: string, default: "open" }
    description: "List GitHub issues"
    receipt_fields: [issue_count]

  my-github.create_issue:
    type: cloud
    endpoint: "https://api.github.com/repos/{{owner}}/{{repo}}/issues"
    method: POST
    headers:
      Authorization: "Bearer {{GITHUB_TOKEN}}"
      Accept: "application/vnd.github+json"
      Content-Type: "application/json"
    params:
      owner: { type: string, required: true }
      repo: { type: string, required: true }
      title: { type: string, required: true }
      body: { type: string, required: false }
    description: "Create a GitHub issue"
    receipt_fields: [issue_number]
```

Store the GitHub token in the vault:

```bash
echo "ghp_your_token_here" | nabaos secret store GITHUB_TOKEN --bind "check|create"
```

Install the plugin:

```bash
nabaos plugin install plugins/my-github/manifest.yaml
```

Use it in a chain:

```yaml
steps:
  - id: list_issues
    ability: my-github.issues
    args:
      owner: "your-org"
      repo: "your-repo"
      state: "open"
    output_key: issues
  - id: notify
    ability: notify.user
    args:
      message: "Open issues: {{issues}}"
```

---

## Next steps

- [Building Agents](./building-agents.md) -- Package plugins and chains into installable agents
- [Writing Chains](./writing-chains.md) -- Use your plugin abilities in chain steps
- [Secrets Management](./secrets-management.md) -- Store API keys that plugins need
