# Building Agents

> **What you'll learn**
>
> - How to create a custom NabaOS agent from scratch
> - The structure of manifest and chain files
> - How to declare permissions and triggers
> - How to package, install, test, and publish your agent

---

## Prerequisites

- NabaOS installed and on your `PATH` (`nabaos --version` prints a version)
- A working data directory (default `~/.nabaos`, or set via `NABA_DATA_DIR`)
- At least one LLM provider configured (`NABA_LLM_API_KEY` set)
- ONNX models available (set via `NABA_MODEL_PATH`)

---

## Step 1: Create the agent directory

Every agent lives in its own directory with at minimum a manifest file:

```bash
mkdir -p ~/my-agents/stock-watcher
cd ~/my-agents/stock-watcher
```

## Step 2: Write the manifest

The manifest declares your agent's identity, permissions, and triggers. Create `manifest.yaml`:

```yaml
name: stock-watcher
version: 1.0.0
description: "Monitor stock prices and alert on threshold crossings"
category: finance
author: your-name
permissions:
  - trading.get_price
  - notify.user
  - flow.branch
  - llm.query
triggers:
  scheduled:
    - chain: price_alert
      interval: 15m
```

### Manifest fields reference

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Unique agent identifier (lowercase, hyphens allowed) |
| `version` | Yes | Semantic version (e.g., `1.0.0`) |
| `description` | Yes | One-line description of what the agent does |
| `category` | No | Category for catalog grouping |
| `author` | No | Author name or organization |
| `permissions` | Yes | List of abilities the agent can use |
| `triggers` | No | When the agent runs (scheduled, on-demand, or event-driven) |

### Permissions

Permissions map directly to plugin abilities. An agent can only invoke abilities it has declared in `permissions`. Examples from official plugins:

- `weather.current`, `weather.forecast` -- Weather plugin
- `gmail.read`, `gmail.send`, `gmail.search` -- Gmail plugin
- `github.issues`, `github.create_issue`, `github.prs` -- GitHub plugin
- `trading.get_price` -- Trading abilities
- `notify.user` -- Send notifications to the user
- `llm.query` -- Query an LLM for reasoning

### Triggers

Scheduled triggers run the named chain at the given interval:

```yaml
triggers:
  scheduled:
    - chain: price_alert
      interval: 15m
    - chain: daily_summary
      interval: 6h
      at: "07:00"
```

The `at` field is optional and specifies a preferred time of day (24-hour format).

## Step 3: Write a chain file

Chains define the step-by-step logic your agent executes. Create `chains/price_alert.yaml`:

```yaml
id: price_alert
name: Price Alert
description: Get a trading price and notify the user if it crosses a threshold
params:
  - name: ticker
    param_type: text
    description: Stock or crypto ticker symbol
    required: true
  - name: threshold
    param_type: number
    description: Price threshold for the alert
    required: true
steps:
  - id: fetch_price
    ability: trading.get_price
    args:
      symbol: "{{ticker}}"
    output_key: current_price

  - id: check_threshold
    ability: flow.branch
    args:
      ref_key: "current_price"
      op: "greater_than"
      value: "{{threshold}}"
    output_key: threshold_exceeded

  - id: notify_alert
    ability: notify.user
    args:
      message: "ALERT: {{ticker}} is at {{current_price}} (threshold: {{threshold}})"
    condition:
      ref_key: threshold_exceeded
      op: equals
      value: "true"
```

Every step in the chain references an ability that must be listed in your manifest's `permissions`.

See the [Writing Chains](./writing-chains.md) guide for the full Chain DSL reference.

## Step 4: Verify your directory structure

Your agent directory should look like this:

```
stock-watcher/
  manifest.yaml
  constitution.yaml          # optional
  chains/
    price_alert.yaml
```

## Step 5: Package the agent

Package your agent directory into a `.nap` (NabaOS Agent Package) file:

```bash
nabaos config agent package ~/my-agents/stock-watcher --output stock-watcher.nap
```

Expected output:

```
Packaging agent from ~/my-agents/stock-watcher...
  manifest.yaml ............ OK
  chains/price_alert.yaml .. OK
Agent packaged: stock-watcher.nap (2.1 KB)
```

## Step 6: Install the agent

```bash
nabaos config agent install stock-watcher.nap
```

Expected output:

```
Installing agent: stock-watcher v1.0.0
  Validating manifest ......... OK
  Checking permissions ........ OK (4 abilities)
  Registering chains .......... OK (1 chain)
Agent installed: stock-watcher
```

## Step 7: Verify the installation

List installed agents:

```bash
nabaos config agent list
```

Check the agent's permissions:

```bash
nabaos config agent permissions stock-watcher
```

## Step 8: Start the agent

```bash
nabaos config agent start stock-watcher
```

## Step 9: Test the agent

You can test your agent's chains directly using the `ask` command:

```bash
nabaos ask "check NVDA price"
```

## Step 10: Stop or manage the agent

```bash
# Stop a running agent
nabaos config agent stop stock-watcher

# Disable (prevents starting)
nabaos config agent disable stock-watcher

# Re-enable
nabaos config agent enable stock-watcher

# Uninstall completely
nabaos config agent uninstall stock-watcher
```

## Complete working example

Here is a full `morning-briefing` agent modeled after the official catalog entry:

**manifest.yaml:**

```yaml
name: morning-briefing
version: 1.0.0
description: "Daily summary: weather, calendar, unread emails, news"
category: daily-productivity
author: your-name
permissions:
  - weather.current
  - calendar.list
  - gmail.read
  - news.headlines
  - llm.query
  - notify.user
triggers:
  scheduled:
    - chain: morning_briefing
      interval: 6h
      at: "07:00"
```

**chains/morning_briefing.yaml:**

```yaml
id: morning_briefing
name: Morning Briefing
description: Multi-step morning briefing with weather, calendar, and email
params:
  - name: city
    param_type: text
    description: City for weather forecast
    required: true
  - name: email_account
    param_type: text
    description: Email account identifier
    required: true
steps:
  - id: fetch_weather
    ability: weather.current
    args:
      latitude: "28.6139"
      longitude: "77.2090"
    output_key: weather_data

  - id: check_calendar
    ability: calendar.list
    args:
      range: "today"
    output_key: calendar_events

  - id: check_email
    ability: gmail.read
    args:
      max_results: 5
    output_key: email_count

  - id: summarize
    ability: notify.user
    args:
      message: "Good morning! Weather: {{weather_data}}. Calendar: {{calendar_events}}. Unread: {{email_count}}."
```

---

## Next steps

- [Writing Chains](./writing-chains.md) -- Learn the full Chain DSL with advanced features like conditionals and circuit breakers
- [Plugin Development](./plugin-development.md) -- Create custom plugins to extend your agent's abilities
- [Constitution Customization](./constitution-customization.md) -- Fine-tune what your agent is allowed to do
- [Secrets Management](./secrets-management.md) -- Store API keys your agent needs
