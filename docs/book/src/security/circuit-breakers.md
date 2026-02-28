# Circuit Breakers

> **What you'll learn**
>
> - What circuit breakers are and why chains need them
> - The 4 condition types: threshold, frequency, ability, output
> - The 3 actions: abort, confirm, throttle
> - How to configure breakers in chain YAML and `<nyaya>` blocks
> - A complete working example with multiple breakers

---

## What Are Circuit Breakers?

A circuit breaker is a safety rule that monitors chain execution and can halt,
pause, or rate-limit a chain when a condition is met. They exist because chains
execute multi-step plans autonomously -- and autonomous systems need guardrails.

Without circuit breakers, a chain that fetches a stock price and executes a trade
could spend unlimited money. A chain that sends emails could fire off hundreds of
messages in a loop. A chain that polls an API could exceed rate limits and get
your account banned.

Circuit breakers are the "stop" button that fires automatically.

---

## How They Work

Circuit breakers are evaluated **before each step** in a chain. The breaker
registry checks the current chain ID, the step outputs so far, and the ability
about to be called. If any breaker's condition matches, the breaker fires and
its action is applied.

```text
Chain step 1 executes -> outputs: { amount: "750" }
                                      |
                        +-------------v--------------+
                        | Circuit Breaker Registry   |
                        |                            |
                        | Rule: amount > 500 -> abort|
                        | Result: FIRED              |
                        +----------------------------+
                                      |
                            Chain execution stops.
                            Reason: "amount 750 exceeds
                            threshold 500"
```

---

## 4 Condition Types

### 1. Threshold (`key>value`)

Fires when a numeric output from a previous step exceeds a specified value.

**Use case:** Prevent a trading chain from executing orders above a dollar limit.

**Syntax:**

```text
amount>1000
```

This checks whether the step output key `amount` contains a number greater than
`1000`. If the key is missing or not a valid number, the breaker fires as a
safety default (fail-closed).

### 2. Frequency (`frequency>count/window`)

Fires when the chain has been executed more than `count` times within a sliding
time window.

**Use case:** Prevent a polling chain from running too often and exceeding API
rate limits.

**Syntax:**

```text
frequency>10/1h
```

The window supports these duration units:

| Unit | Meaning | Example |
|---|---|---|
| `s` | Seconds | `30s` |
| `m` | Minutes | `15m` |
| `h` | Hours | `1h` |
| `d` | Days | `1d` |

The registry tracks execution timestamps in a sliding window. The history
is capped at 10,000 entries per chain to prevent unbounded memory growth.

### 3. Ability (`ability:name`)

Fires when a specific ability (tool) is about to be called.

**Use case:** Require user confirmation before sending an email, or block shell
execution entirely.

**Syntax:**

```text
ability:email.send
```

### 4. Output (`output:key|contains:pattern`)

Fires when a step output contains a specific string pattern.

**Use case:** Halt a chain if an intermediate step produced an error or
unexpected result.

**Syntax (in B: line format):**

```text
output:error_msg|contains:fail
```

---

## 3 Actions

When a circuit breaker fires, it executes one of three actions:

### `abort`

**Immediately stops the chain.** No further steps execute. The chain returns
an error with the breaker's reason message.

Use `abort` for hard safety limits: spending caps, forbidden operations,
error conditions that cannot be recovered from.

```text
amount>5000|abort|"Spending limit exceeded"
```

### `confirm`

**Pauses the chain and asks the user for confirmation.** If the user approves,
the chain continues. If the user declines (or does not respond within the
timeout), the chain is aborted.

> **Note:** In the current implementation, `confirm` breakers are treated as
> `abort` when no interactive confirmation channel is available. This is a
> deliberate security choice -- the system fails closed rather than silently
> bypassing a safety check.

```text
ability:email.send|confirm|"Confirm before sending email"
```

### `throttle`

**Rate-limits the chain.** The current execution may be delayed or skipped, but
the chain is not permanently halted.

```text
frequency>10/1h|throttle|"Rate limited to 10 executions per hour"
```

---

## Configuring Circuit Breakers

### Method 1: B: lines in `<nyaya>` blocks

When an LLM generates a chain plan, it can include circuit breakers in the
`<nyaya>` block using `B:` lines:

```text
<nyaya>
D:trading|medium
G:ability:trading.execute,ability:trading.get_price
B:amount>1000|abort|"Transaction exceeds $1000 limit"
B:frequency>5/1h|throttle|"Max 5 trades per hour"
B:ability:trading.execute|confirm|"Confirm before executing trade"
</nyaya>
```

Each `B:` line follows the format:

```text
B:condition|action|"reason"
```

### Method 2: Chain YAML files

For pre-built agents, circuit breakers are defined in the chain YAML file:

```yaml
id: price_alert_trade
name: Price Alert with Auto-Trade
description: Monitor price and execute trade if conditions met

params:
  - name: ticker
    param_type: text
    description: Stock ticker symbol
    required: true
  - name: max_spend
    param_type: number
    description: Maximum dollar amount per trade
    required: true

circuit_breakers:
  - condition: "amount>{{max_spend}}"
    action: abort
    reason: "Trade amount exceeds maximum spend of ${{max_spend}}"
  - condition: "frequency>3/1d"
    action: throttle
    reason: "Maximum 3 auto-trades per day"
  - condition: "ability:trading.execute"
    action: confirm
    reason: "Confirm before executing trade"

steps:
  - id: fetch_price
    ability: trading.get_price
    args:
      symbol: "{{ticker}}"
    output_key: current_price

  - id: execute_trade
    ability: trading.execute
    args:
      symbol: "{{ticker}}"
      amount: "{{max_spend}}"
    output_key: trade_result
    condition:
      ref_key: current_price
      op: less_than
      value: "{{buy_threshold}}"
```

### Method 3: Global breakers

Register a breaker with chain ID `*` to apply it to every chain in the system:

```text
B:ability:shell.execute|abort|"Shell execution is forbidden"
```

This is useful for system-wide policies that should apply regardless of which
agent or chain is running.

---

## Next Steps

- [Threat Model](threat-model.md) -- understand why circuit breakers exist in the security model
- [Writing Chains](../guides/writing-chains.md) -- full chain DSL reference
- [Anomaly Detection](anomaly-detection.md) -- behavioral monitoring that complements circuit breakers
