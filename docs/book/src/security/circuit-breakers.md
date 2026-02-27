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
Chain step 1 executes → outputs: { amount: "750" }
                                      |
                        +-------------v--------------+
                        | Circuit Breaker Registry   |
                        |                            |
                        | Rule: amount > 500 → abort |
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

**Example:**

```text
Step outputs: { "amount": "1500", "ticker": "NVDA" }
Breaker:      amount>1000|abort|"Transaction exceeds $1000 limit"
Result:       FIRED (1500 > 1000) → chain aborted
```

### 2. Frequency (`frequency>count/window`)

Fires when the chain has been executed more than `count` times within a sliding
time window.

**Use case:** Prevent a polling chain from running too often and exceeding API
rate limits.

**Syntax:**

```text
frequency>10/1h
```

This means "fire if this chain has run more than 10 times in the last 1 hour."
The window supports these duration units:

| Unit | Meaning | Example |
|---|---|---|
| `s` | Seconds | `30s` |
| `m` | Minutes | `15m` |
| `h` | Hours | `1h` |
| `d` | Days | `1d` |

The registry tracks execution timestamps in a sliding window. When the count of
timestamps within the window exceeds `max_count`, the breaker fires. The history
is capped at 10,000 entries per chain to prevent unbounded memory growth.

**Example:**

```text
Chain "price_poll" has run 11 times in the last hour.
Breaker:  frequency>10/1h|throttle|"Too many polls per hour"
Result:   FIRED (11 > 10 in 1h window) → chain throttled
```

### 3. Ability (`ability:name`)

Fires when a specific ability (tool) is about to be called.

**Use case:** Require user confirmation before sending an email, or block shell
execution entirely.

**Syntax:**

```text
ability:email.send
```

This fires whenever the chain is about to call the `email.send` ability,
regardless of the step outputs or execution frequency.

**Example:**

```text
Next step calls: email.send
Breaker:  ability:email.send|confirm|"Email send requires confirmation"
Result:   FIRED → chain paused, waiting for user confirmation
```

### 4. Output (`output:key|contains:pattern`)

Fires when a step output contains a specific string pattern.

**Use case:** Halt a chain if an intermediate step produced an error or
unexpected result.

**Syntax (in B: line format):**

```text
output:error_msg|contains:fail
```

This checks whether the step output key `error_msg` contains the string `fail`.

**Example:**

```text
Step outputs: { "error_msg": "API call failed: timeout" }
Breaker condition: output key "error_msg" contains "fail"
Result:   FIRED → chain aborted with reason
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

Use `confirm` for operations that are allowed but risky: sending emails,
making purchases, modifying external systems.

```text
ability:email.send|confirm|"Confirm before sending email"
```

> **Note:** In the current implementation, `confirm` breakers are treated as
> `abort` when no interactive confirmation channel is available. This is a
> deliberate security choice -- the system fails closed rather than silently
> bypassing a safety check. When Telegram inline keyboards or web dashboard
> confirmation dialogs are connected, `confirm` will prompt the user
> interactively.

### `throttle`

**Rate-limits the chain.** The current execution may be delayed or skipped, but
the chain is not permanently halted. Future executions will proceed normally
once the rate drops below the threshold.

Use `throttle` for frequency control: polling intervals, API rate limits,
message sending cadence.

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
  - name: buy_threshold
    param_type: number
    description: Price below which to buy
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

  - id: calculate_shares
    ability: math.divide
    args:
      numerator: "{{max_spend}}"
      denominator: "{{current_price}}"
    output_key: share_count

  - id: execute_trade
    ability: trading.execute
    args:
      symbol: "{{ticker}}"
      shares: "{{share_count}}"
      action: buy
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

## Complete Example: Trading Agent with Safety Rails

Here is a full agent configuration with multiple layers of circuit breakers:

**manifest.yaml:**

```yaml
name: safe-trader
version: 1.0.0
description: "Stock trading agent with spending limits and confirmation flows"
category: finance
permissions:
  - trading.get_price
  - trading.execute
  - notify.user
  - flow.branch
  - math.divide
```

**chains/auto_trade.yaml:**

```yaml
id: auto_trade
name: Auto Trade
description: Fetch price, evaluate condition, execute trade with safety checks

params:
  - name: ticker
    param_type: text
    required: true
  - name: target_price
    param_type: number
    required: true
  - name: budget
    param_type: number
    required: true

circuit_breakers:
  # Hard spending cap
  - condition: "budget>5000"
    action: abort
    reason: "Budget exceeds $5,000 single-trade limit"

  # Rate limit trading frequency
  - condition: "frequency>5/1d"
    action: throttle
    reason: "Maximum 5 trades per day"

  # Require confirmation for every trade execution
  - condition: "ability:trading.execute"
    action: confirm
    reason: "Approve trade: {{ticker}} at {{current_price}}?"

steps:
  - id: get_price
    ability: trading.get_price
    args:
      symbol: "{{ticker}}"
    output_key: current_price

  - id: check_condition
    ability: flow.branch
    args:
      ref_key: current_price
      op: less_than
      value: "{{target_price}}"
    output_key: should_buy

  - id: calculate_shares
    ability: math.divide
    args:
      numerator: "{{budget}}"
      denominator: "{{current_price}}"
    output_key: share_count
    condition:
      ref_key: should_buy
      op: equals
      value: "true"

  - id: execute_buy
    ability: trading.execute
    args:
      symbol: "{{ticker}}"
      shares: "{{share_count}}"
      action: buy
    output_key: trade_result
    condition:
      ref_key: should_buy
      op: equals
      value: "true"

  - id: notify
    ability: notify.user
    args:
      message: "Trade result for {{ticker}}: {{trade_result}}"
```

**What the breakers do at runtime:**

1. Before the chain starts: the `budget>5000` breaker checks the input parameter.
   If someone passes `budget: 10000`, the chain is aborted immediately.

2. The `frequency>5/1d` breaker checks the execution history. If this chain has
   already run 5 times today, it is throttled (skipped or delayed).

3. When step `execute_buy` is about to call `trading.execute`, the ability
   breaker fires and requests user confirmation via Telegram inline keyboard.

---

## Inspecting Breakers

List all registered circuit breakers:

```bash
nyaya chain breakers auto_trade
```

**Expected output:**

```text
Circuit breakers for chain "auto_trade":
  [1] budget > 5000       → abort     "Budget exceeds $5,000 single-trade limit"
  [2] frequency > 5/1d    → throttle  "Maximum 5 trades per day"
  [3] ability:trading.execute → confirm "Approve trade?"
```

Check breaker evaluation against test data:

```bash
nyaya chain test-breakers auto_trade --outputs '{"budget":"3000","current_price":"150"}' --ability trading.execute
```

**Expected output:**

```text
Evaluating breakers for "auto_trade":
  [1] budget > 5000:           PASS (3000 <= 5000)
  [2] frequency > 5/1d:        PASS (2 executions in window)
  [3] ability:trading.execute:  FIRED → confirm "Approve trade?"

Result: BLOCKED (confirm action, no interactive channel)
```

---

## Next Steps

- [Threat Model](threat-model.md) -- understand why circuit breakers exist in the security model
- [Writing Chains](../guides/writing-chains.md) -- full chain DSL reference
- [Anomaly Detection](anomaly-detection.md) -- behavioral monitoring that complements circuit breakers
