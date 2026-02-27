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
- A data directory configured (default `~/.nabaos`)

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

| Field | Required | Description |
|-------|----------|-------------|
| `id` | Yes | Unique step identifier within this chain |
| `ability` | Yes | The ability to invoke (must be in agent's permissions) |
| `args` | Yes | Key-value arguments passed to the ability |
| `output_key` | No | Variable name to store the result for later steps |

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

The `deep.delegate` ability routes the task to the best available backend (Manus, Claude computer-use, or OpenAI agents) based on the task `type` and cost settings.

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

Supported condition operators:

| Operator | Description |
|----------|-------------|
| `equals` | Exact string match |
| `not_equals` | Inverse of equals |
| `contains` | Substring check |
| `greater_than` | Numeric comparison |
| `less_than` | Numeric comparison |

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

This stores `"true"` or `"false"` in `threshold_exceeded`, which subsequent steps can use in their `condition` blocks.

---

## Variable interpolation

Use `{{variable_name}}` to reference:

1. **Chain parameters** -- values passed when the chain is invoked
2. **Step outputs** -- values stored by previous steps via `output_key`
3. **Environment secrets** -- values like `{{GMAIL_ACCESS_TOKEN}}` from the vault

### How interpolation works

```yaml
params:
  - name: city
    param_type: text
    required: true

steps:
  - id: fetch
    ability: data.fetch_url
    args:
      url: "https://api.weather.com/v1/{{city}}"   # from params
    output_key: weather_data

  - id: notify
    ability: notify.user
    args:
      message: "Weather in {{city}}: {{weather_data}}"  # city from params,
                                                         # weather_data from step output
```

Variables are resolved at execution time. If a variable is not found, the raw `{{name}}` string is preserved (no error), which helps with debugging.

### Nested variable references

You can compose variables freely within strings:

```yaml
args:
  message: "Report for {{client_name}}: {{analysis}} (generated at {{timestamp}})"
```

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

- id: call_api
  ability: cloud.request
  args:
    endpoint: "{{api_url}}"
  output_key: api_result
  on_failure:
    action: abort         # stop the entire chain
    message: "Critical API call failed"

- id: optional_enrichment
  ability: nlp.sentiment
  args:
    text: "{{raw_data}}"
  output_key: sentiment
  on_failure:
    action: default       # use a default value
    default_value: "neutral"
    message: "Sentiment analysis unavailable, using default"
```

### on_failure actions

| Action | Behavior |
|--------|----------|
| `skip` | Skip this step, continue to the next |
| `abort` | Stop the entire chain, report failure |
| `default` | Use `default_value` as the output and continue |
| `retry` | Retry the step (up to `max_retries` times) |

### Retry configuration

```yaml
- id: flaky_api
  ability: cloud.request
  args:
    endpoint: "{{url}}"
  output_key: result
  on_failure:
    action: retry
    max_retries: 3
    retry_delay_ms: 1000
    message: "Retrying API call..."
```

---

## Circuit breaker configuration

For chains that run on a schedule and call external services, circuit breakers prevent cascading failures:

```yaml
id: market_monitor
name: Market Monitor
description: Periodically check market data
params:
  - name: sector
    param_type: text
    required: true
circuit_breaker:
  failure_threshold: 5      # open circuit after 5 consecutive failures
  reset_timeout_secs: 300   # try again after 5 minutes
  half_open_max: 2          # allow 2 test requests in half-open state
steps:
  - id: fetch_data
    ability: data.fetch_url
    args:
      url: "https://market.api/{{sector}}"
    output_key: market_data
  - id: notify
    ability: notify.user
    args:
      message: "Market update ({{sector}}): {{market_data}}"
```

### Circuit breaker states

```
CLOSED  ---[failure_threshold reached]--->  OPEN
  ^                                           |
  |                                           |
  +---[success in half-open]---  HALF-OPEN  <-+-- [reset_timeout expires]
```

- **Closed**: Normal operation. Failures are counted.
- **Open**: All calls fail immediately without executing. Protects downstream services.
- **Half-open**: After `reset_timeout_secs`, allows `half_open_max` test calls. If they succeed, the circuit closes. If they fail, it reopens.

---

## Complete working example

Here is a full research chain that fetches a web page, summarizes it with an LLM, stores the result in memory, and notifies the user:

```yaml
id: research_topic
name: Research Topic
description: Fetch a web page, summarize its content, and store the summary
params:
  - name: url
    param_type: url
    description: URL to research
    required: true
  - name: topic
    param_type: text
    description: Topic label for memory storage
    required: true
circuit_breaker:
  failure_threshold: 3
  reset_timeout_secs: 120
steps:
  - id: fetch_page
    ability: browser.fetch
    args:
      url: "{{url}}"
    output_key: page_content
    on_failure:
      action: abort
      message: "Could not fetch URL"

  - id: summarize
    ability: deep.delegate
    args:
      task: "Summarize this content concisely: {{page_content}}"
      type: "analysis"
    output_key: summary
    on_failure:
      action: default
      default_value: "Summary unavailable"

  - id: store_result
    ability: memory.store
    args:
      key: "research_{{topic}}"
      value: "{{summary}}"
    on_failure:
      action: skip
      message: "Could not store to memory"

  - id: notify
    ability: notify.user
    args:
      message: "Research on {{topic}}: {{summary}}"
```

### Required permissions for this chain

Your agent's `manifest.yaml` must include:

```yaml
permissions:
  - browser.fetch
  - deep.delegate
  - memory.store
  - notify.user
```

### Testing the chain

Load and test the chain via the orchestrator:

```bash
nabaos orchestrate "research https://example.com about web standards"
```

Or if the chain is part of an installed agent:

```bash
nabaos query "research https://arxiv.org/abs/2301.00001 about transformers"
```

---

## Chain design patterns

### Pattern 1: Fetch-Analyze-Notify

The most common pattern. Fetch data, process it, tell the user:

```yaml
steps:
  - id: fetch
    ability: data.fetch_url
    args: { url: "{{source}}" }
    output_key: raw_data
  - id: analyze
    ability: nlp.sentiment
    args: { text: "{{raw_data}}" }
    output_key: analysis
  - id: notify
    ability: notify.user
    args: { message: "{{analysis}}" }
```

### Pattern 2: Conditional Branching

Check a value and act differently based on the result:

```yaml
steps:
  - id: fetch_price
    ability: trading.get_price
    args: { symbol: "{{ticker}}" }
    output_key: price
  - id: check
    ability: flow.branch
    args: { ref_key: "price", op: "greater_than", value: "{{threshold}}" }
    output_key: is_above
  - id: alert_high
    ability: notify.user
    args: { message: "{{ticker}} is ABOVE {{threshold}}: {{price}}" }
    condition: { ref_key: is_above, op: equals, value: "true" }
  - id: alert_low
    ability: notify.user
    args: { message: "{{ticker}} is below {{threshold}}: {{price}}" }
    condition: { ref_key: is_above, op: equals, value: "false" }
```

### Pattern 3: Multi-Source Aggregation

Pull from multiple sources and combine results:

```yaml
steps:
  - id: fetch_weather
    ability: weather.current
    args: { latitude: "{{lat}}", longitude: "{{lon}}" }
    output_key: weather
  - id: check_calendar
    ability: calendar.list
    args: { range: "today" }
    output_key: events
  - id: check_email
    ability: gmail.read
    args: { max_results: 5 }
    output_key: emails
  - id: summarize
    ability: notify.user
    args:
      message: "Weather: {{weather}}. Events: {{events}}. Emails: {{emails}}."
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

This is automatically compiled into the full YAML chain format, stored in the chain store, and reused for future matching requests. On subsequent calls, the LLM emits a lightweight template reference instead:

```
<nyaya>C:weather_check|Delhi</nyaya>
```

This hits the chain cache directly -- no LLM call needed.

---

## Next steps

- [Building Agents](./building-agents.md) -- Package chains into installable agents
- [Plugin Development](./plugin-development.md) -- Create abilities for your chains to call
- [Constitution Customization](./constitution-customization.md) -- Control which abilities chains can use
