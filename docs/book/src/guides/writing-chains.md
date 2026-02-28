# Writing Chains

> **What you'll learn**
>
> - The Chain DSL YAML structure and how chains are parsed
> - All step types: tool calls, LLM delegation, conditionals, branching
> - Variable interpolation with `{{var}}` syntax
> - Error handling with `on_failure` handlers
> - Circuit breaker configuration for resilience
> - How to test and debug chains

---

## Prerequisites

- NabaOS installed (`nabaos --version`)
- Familiarity with the [Building Agents](./building-agents.md) guide

---

## Chain YAML structure

A chain is a YAML document with four top-level fields:

```yaml
id: weather_check            # unique identifier (snake_case)
name: Weather Check          # human-readable name
description: Fetch weather   # what this chain does
params:                      # input parameters
  - name: city
    param_type: text
    description: City name
    required: true
steps:                       # ordered list of steps to execute
  - id: fetch
    ability: data.fetch_url
    args:
      url: "https://api.weather.com/v1/{{city}}"
    output_key: weather_data
```

### Top-level fields

| Field | Required | Description |
|-------|----------|-------------|
| `id` | Yes | Unique chain identifier, used for lookups and scheduling |
| `name` | Yes | Display name |
| `description` | Yes | What the chain does |
| `params` | Yes | Input parameters the chain accepts |
| `steps` | Yes | Ordered sequence of steps |

### Parameter types

Each parameter in `params` has a type that guides validation:

| `param_type` | Description | Example values |
|--------------|-------------|----------------|
| `text` | Free-form string | `"NYC"`, `"hello world"` |
| `number` | Numeric value | `42`, `3.14`, `"800"` |
| `url` | A URL | `"https://example.com"` |
| `bool` | Boolean | `"true"`, `"false"` |

---

## Step types

Every step invokes an **ability** -- a registered function from a plugin or built-in capability.

### Basic tool step

The most common step type calls an ability with arguments and stores the result:

```yaml
- id: fetch_price
  ability: trading.get_price
  args:
    symbol: "{{ticker}}"
  output_key: current_price
```

### LLM delegation step

Delegate complex reasoning to an LLM or deep agent backend:

```yaml
- id: review
  ability: deep.delegate
  args:
    task: "Review this code for bugs and improvements"
    content: "{{source_code}}"
    type: "code"
  output_key: review_result
```

### Conditional step

A step can include a `condition` that must be met for it to execute:

```yaml
- id: notify_alert
  ability: notify.user
  args:
    message: "ALERT: {{ticker}} is at {{current_price}}"
  condition:
    ref_key: threshold_exceeded
    op: equals
    value: "true"
```

### Branching step

Use `flow.branch` to evaluate a condition and store a boolean result:

```yaml
- id: check_threshold
  ability: flow.branch
  args:
    ref_key: "current_price"
    op: "greater_than"
    value: "{{threshold}}"
  output_key: threshold_exceeded
```

---

## Variable interpolation

Use `{{variable_name}}` to reference:

1. **Chain parameters** -- values passed when the chain is invoked
2. **Step outputs** -- values stored by previous steps via `output_key`
3. **Environment secrets** -- values like `{{GMAIL_ACCESS_TOKEN}}` from the vault

Variables are resolved at execution time. If a variable is not found, the raw `{{name}}` string is preserved (no error), which helps with debugging.

---

## Error handling with on_failure

Each step can define an `on_failure` handler that runs if the step fails:

```yaml
- id: fetch_data
  ability: data.fetch_url
  args:
    url: "{{data_url}}"
  output_key: raw_data
  on_failure:
    action: skip          # skip this step and continue
    message: "Data fetch failed, continuing without data"
```

### on_failure actions

| Action | Behavior |
|--------|----------|
| `skip` | Skip this step, continue to the next |
| `abort` | Stop the entire chain, report failure |
| `default` | Use `default_value` as the output and continue |
| `retry` | Retry the step (up to `max_retries` times) |

---

## Circuit breaker configuration

For chains that run on a schedule and call external services, circuit breakers prevent cascading failures:

```yaml
circuit_breaker:
  failure_threshold: 5      # open circuit after 5 consecutive failures
  reset_timeout_secs: 300   # try again after 5 minutes
  half_open_max: 2          # allow 2 test requests in half-open state
```

---

## Testing chains

Test chains via the `ask` command:

```bash
nabaos ask "research https://example.com about web standards"
```

---

## How chains are compiled from LLM responses

When the NabaOS orchestrator processes a novel request, the LLM can emit a chain definition in a compact `<nyaya>` block format:

```
<nyaya>
NEW:weather_check
P:city:str:NYC
S:data.fetch_url:https://api.weather.com/$city>weather_data
S:notify.user:Weather: $weather_data
L:weather_query
R:weather in {city}|forecast for {city}
</nyaya>
```

This is automatically compiled into the full YAML chain format, stored in the chain store, and reused for future matching requests.

---

## Next steps

- [Building Agents](./building-agents.md) -- Package chains into installable agents
- [Plugin Development](./plugin-development.md) -- Create abilities for your chains to call
- [Constitution Customization](./constitution-customization.md) -- Control which abilities chains can use
