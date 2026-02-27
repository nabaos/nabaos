# Chain DSL

Chains are parameterized sequences of ability calls. They represent compiled
execution plans -- what the LLM "compiles" from natural language into a
deterministic, replayable pipeline.

## Chain Definition

A chain is defined in YAML with the following top-level fields:

```yaml
id: string            # Unique chain identifier (required)
name: string          # Human-readable name (required)
description: string   # What this chain does (required)
params: [ParamDef]    # Parameter schema (required, may be empty)
steps: [ChainStep]    # Ordered list of steps (required, at least one)
```

## Parameters

Each parameter has a type, description, and optional default value:

```yaml
params:
  - name: city
    param_type: text
    description: City name to look up
    required: true
  - name: units
    param_type: text
    description: Temperature units
    required: false
    default: "celsius"
```

### Parameter Types

| Type | Description |
|------|-------------|
| `text` | Free-form string |
| `number` | Numeric value (integer or float) |
| `boolean` | `true` or `false` |
| `url` | URL string |
| `email` | Email address |
| `date_time` | Date/time string |

## Steps

Each step invokes one ability and optionally stores its output for use by
later steps:

```yaml
steps:
  - id: string              # Step identifier (required, must be unique)
    ability: string          # Ability name to invoke (required)
    args:                    # Arguments as key-value pairs
      key: "value"
    output_key: string       # Store result under this key (optional)
    condition: Condition     # Only run if condition is true (optional)
    on_failure: string       # Step ID to jump to on failure (optional)
```

### Template Variables

Step arguments support `{{variable}}` template syntax. Variables can
reference:

- **Chain parameters**: `{{city}}`, `{{units}}`
- **Previous step outputs**: `{{weather_data}}`, `{{summary}}`

Template values are sanitized before interpolation: `{{` and `}}`
markers are stripped from parameter values to prevent injection of
additional template references. Control characters (except newline and tab)
are also removed.

### Example

```yaml
id: check_weather
name: Check Weather
description: Fetch weather for a city and notify the user
params:
  - name: city
    param_type: text
    description: City name
    required: true
steps:
  - id: fetch
    ability: data.fetch_url
    args:
      url: "https://api.weather.com/v1/{{city}}"
    output_key: weather_data

  - id: notify
    ability: notify.user
    args:
      message: "Weather in {{city}}: {{weather_data}}"
```

## Conditional Steps

A step can be made conditional on the output of a previous step:

```yaml
steps:
  - id: check_status
    ability: data.fetch_url
    args:
      url: "https://api.example.com/status"
    output_key: status

  - id: alert_if_down
    ability: notify.user
    args:
      message: "Service is down!"
    condition:
      ref_key: status
      op: contains
      value: "error"
```

### Condition Operators

| Operator | Description |
|----------|-------------|
| `equals` | Exact string match |
| `not_equals` | String does not match |
| `contains` | Output contains the value as a substring |
| `greater_than` | Numeric comparison: output > value |
| `less_than` | Numeric comparison: output < value |
| `is_empty` | Output is empty or the key does not exist |
| `is_not_empty` | Output exists and is non-empty |

For `greater_than` and `less_than`, both the output and the value are
parsed as `f64`. If parsing fails, the condition evaluates to `false`.

For `is_empty` and `is_not_empty`, the `value` field is ignored.

## Error Handling with `on_failure`

When a step fails, you can redirect execution to a fallback step instead
of aborting the entire chain:

```yaml
steps:
  - id: primary_fetch
    ability: data.fetch_url
    args:
      url: "https://primary-api.com/data"
    output_key: result
    on_failure: fallback_fetch

  - id: fallback_fetch
    ability: data.fetch_url
    args:
      url: "https://backup-api.com/data"
    output_key: result

  - id: process
    ability: nlp.summarize
    args:
      text: "{{result}}"
```

When `primary_fetch` fails:
1. The error message is stored as `primary_fetch_error` in the outputs.
2. Execution jumps to `fallback_fetch`.
3. If `fallback_fetch` succeeds, execution continues normally from `process`.

The `on_failure` target must reference a valid step ID within the same
chain. Cycle detection prevents infinite loops -- if execution jumps to
a step that was already visited via an `on_failure` path, the chain aborts
with an error.

If a step fails and has no `on_failure` handler, the chain aborts
immediately.

A chain where any step triggered an `on_failure` handler is marked as
`success: false` in the execution result, even if all subsequent steps
succeed. This allows callers to distinguish clean runs from recovered runs.

## Circuit Breakers

Circuit breakers are safety rules that can halt chain execution before a
step runs. They are evaluated before every step and can abort the chain,
require confirmation, or throttle execution.

### Breaker Specification Format

Circuit breakers are specified in `B:` lines within `<nyaya>` blocks
returned by the LLM:

```
B:condition|action|"reason"
```

### Condition Types

#### Threshold

Fires when a numeric output exceeds a value:

```
B:amount>1000|abort|"Transaction exceeds $1000 limit"
```

The `amount` key is looked up in the chain's step outputs. If the value
parses as a number greater than 1000, the breaker fires.

If the key is missing from outputs, the threshold condition evaluates to
`true` (fires) -- this is a safety-first default that prevents missing
data from bypassing spending limits.

#### Frequency

Fires when chain execution frequency exceeds a rate within a sliding
time window:

```
B:frequency>10/1h|throttle|"Too many requests per hour"
```

Format: `frequency>COUNT/WINDOW` where WINDOW uses duration suffixes:
`s` (seconds), `m` (minutes), `h` (hours), `d` (days).

The breaker registry tracks execution timestamps per chain and evaluates
them against a sliding window.

#### Ability

Fires when a specific ability is about to be called:

```
B:ability:email.send|confirm|"Email send requires confirmation"
```

This allows gating sensitive operations regardless of the chain's logic.

#### Output Pattern

Fires when a step's output contains a specific string:

```
B:output:error_msg|contains:fail|abort|"Step produced failure indicator"
```

### Breaker Actions

| Action | Behavior |
|--------|----------|
| `abort` | Stop the chain immediately. The chain fails with an error. |
| `confirm` | Requires user confirmation. Since no interactive confirmation channel is available at the chain executor level, `confirm` is treated as `abort` (blocks execution) to prevent silent bypass of safety checks. |
| `throttle` | Rate-limit the execution. The chain is allowed to proceed but may be delayed. |

### Scope

Circuit breakers can be scoped to a specific chain or applied globally:

- **Chain-specific**: Registered with a chain ID, only evaluated for that chain.
- **Global**: Registered with `"*"` as the chain ID, evaluated for every chain.

### Example: Trading Chain with Safety Breakers

```yaml
id: execute_trade
name: Execute Trade
description: Place a stock trade with safety limits
params:
  - name: ticker
    param_type: text
    description: Stock ticker
    required: true
  - name: amount_usd
    param_type: number
    description: Trade amount in USD
    required: true
steps:
  - id: get_price
    ability: trading.get_price
    args:
      ticker: "{{ticker}}"
    output_key: current_price

  - id: execute
    ability: trading.execute
    args:
      ticker: "{{ticker}}"
      amount: "{{amount_usd}}"
    output_key: trade_result
```

With circuit breakers registered:

```
B:amount_usd>5000|abort|"Trade exceeds $5000 single-trade limit"
B:ability:trading.execute|confirm|"Trade execution requires confirmation"
B:frequency>20/1h|throttle|"Max 20 trades per hour"
```

## Execution Flow

The chain executor processes steps sequentially:

1. **Parameter validation**: All required parameters must be provided
   or have defaults.
2. **Frequency recording**: The execution event is recorded for frequency
   breaker tracking.
3. For each step:
   a. **Condition check**: If a condition is present and evaluates to false,
      skip the step.
   b. **Constitution check**: If a constitution enforcer is attached, verify
      the step's ability is allowed. Blocked abilities cause the chain to
      abort with a `PermissionDenied` error.
   c. **Circuit breaker check**: Evaluate all applicable breakers against
      current outputs and the ability about to be called.
   d. **Template resolution**: Resolve `{{variables}}` in arguments using
      chain parameters and previous step outputs.
   e. **Ability execution**: Call the ability through the ability registry,
      which checks manifest permissions.
   f. **Output storage**: If `output_key` is set, store the result.
   g. **Receipt collection**: A `ToolReceipt` is generated for each executed
      step.
4. Return the `ChainExecutionResult` with receipts, outputs, skipped steps,
   and timing.
