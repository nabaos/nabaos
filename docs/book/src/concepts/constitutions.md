# Constitutions

> **What you'll learn**
>
> - What a constitution is and how it differs from a system prompt
> - The full YAML structure with annotated examples
> - How each enforcement type works: allow, block, confirm, warn
> - How trigger matching works with actions, targets, and keywords
> - The 8 built-in constitution templates
> - Where constitution checks happen in the pipeline

---

## What Is a Constitution?

A constitution is a **formal policy document** that defines what an agent is and is not allowed to do. It is not a system prompt, not a suggestion, and not something the LLM can override. It is a hard enforcement layer that runs before any LLM call.

Key differences from system prompts:

| Property | System prompt | Constitution |
|---|---|---|
| Enforced by | LLM (can be overridden) | Rust code (cannot be bypassed) |
| Format | Free-form text | Structured YAML |
| Modifiable at runtime | Often yes | Never (read-only mount) |
| Checked when | After LLM generates output | Before any LLM call |

The constitution is loaded at startup and treated as read-only. The agent cannot write to its own constitution. Modification requires editing the YAML file and restarting.

```bash
nabaos config rules show
```

---

## YAML Structure

A constitution is a YAML file with the following structure:

```yaml
# Constitution name — used for identification and template lookup
name: "my-trading-bot"

# Semantic version
version: "1.0.0"

# Human-readable description of this constitution's purpose (optional)
description: "Trading assistant — market monitoring, portfolio analysis, trade execution"

# Default enforcement for intents that don't match any rule.
# The shipped default.yaml uses "allow". The code default (when no
# constitution is loaded) is "block".
default_enforcement: allow

# Ordered list of rules. Rules are evaluated top-to-bottom;
# the FIRST matching rule wins.
rules:
  # Rule 1: Allow read-only price checks
  - name: allow_price_checks
    trigger_actions:
      - check          # Matches the "check" W5H2 action
    trigger_targets:
      - price          # Matches the "price" W5H2 target
    trigger_keywords: []
    enforcement: allow
    reason: "Price checks are read-only and safe"

  # Rule 2: Require confirmation for trade execution
  - name: confirm_trades
    trigger_actions:
      - send           # "send" covers trade execution
      - control        # "control" covers portfolio adjustments
    trigger_targets:
      - portfolio
      - price
    trigger_keywords: []
    enforcement: confirm
    reason: "Financial transactions require explicit user approval"

  # Rule 3: Block access to personal data
  - name: block_personal_data
    trigger_actions:
      - "*"            # Wildcard — matches ANY action
    trigger_targets:
      - email
      - calendar
      - contact
    trigger_keywords: []
    enforcement: block
    reason: "Trading bot has no business accessing personal data"

  # Rule 4: Block queries containing destructive keywords
  - name: block_destructive_keywords
    trigger_actions: []
    trigger_targets: []
    trigger_keywords:
      - "delete all"
      - "rm -rf"
      - "drop table"
      - "wipe"
    enforcement: block
    reason: "Destructive operations are never allowed"

  # Rule 5: Allow standard analysis operations
  - name: allow_analysis
    trigger_actions:
      - analyze
      - search
      - generate
    trigger_targets: []   # Empty = matches any target
    trigger_keywords: []
    enforcement: allow
    reason: "Analysis operations are the core function of this bot"

# Additional optional fields:
# channel_permissions:   Per-channel access control
# browser_stealth:       Browser automation stealth settings
# swarm_config:          Multi-agent swarm configuration
# ollama_config:         Local Ollama model configuration
# captcha_solver:        Captcha handling configuration
```

> **Note:** The `description` field on individual rules is optional.

---

## Enforcement Types

Each rule specifies one of four enforcement types that determine what happens when the rule matches:

### `allow`

The intent is permitted unconditionally. No user interaction required. The query proceeds through the pipeline.

```yaml
- name: allow_weather_checks
  trigger_actions: [check]
  trigger_targets: [weather]
  enforcement: allow
  reason: "Weather checks are harmless read-only operations"
```

**Use for:** Read-only operations, search queries, analysis, anything with no side effects.

### `block`

The intent is rejected immediately. The user receives an error message explaining why. No LLM is called.

```yaml
- name: block_email_access
  trigger_actions: ["*"]
  trigger_targets: [email]
  enforcement: block
  reason: "This agent is not authorized to access email"
```

**Use for:** Out-of-scope operations, dangerous actions, domain boundary enforcement.

### `confirm`

The intent is paused and the user is prompted for explicit approval. In Telegram, this appears as an inline keyboard with "Approve" and "Reject" buttons. The query only proceeds if the user approves.

```yaml
- name: confirm_send_email
  trigger_actions: [send]
  trigger_targets: [email]
  enforcement: confirm
  reason: "Sending emails has external effects and cannot be undone"
```

**Use for:** Irreversible actions (sending emails, executing trades, deleting data), expensive operations, anything with real-world consequences.

### `warn`

The intent is permitted but a warning is logged. The user sees a notice that the action was flagged. This is useful for monitoring without blocking.

```yaml
- name: warn_device_control
  trigger_actions: [control]
  trigger_targets: [lights]
  enforcement: warn
  reason: "Device control changes physical state — logging for audit"
```

**Use for:** Actions you want to audit but not block, operations in a testing phase, low-risk but notable actions.

---

## Trigger Matching

Rules match incoming queries through three independent trigger mechanisms. A rule matches if **either** its intent triggers or its keyword triggers fire.

### Action triggers (`trigger_actions`)

Match against the W5H2 action component of the classified intent. The 11 possible actions are: `check`, `send`, `set`, `control`, `add`, `search`, `create`, `delete`, `analyze`, `schedule`, `generate`.

- An empty list means the rule does not use action-based matching.
- The wildcard `"*"` matches any action.
- Matching is case-insensitive.

### Target triggers (`trigger_targets`)

Match against the W5H2 target component of the classified intent. The 30 possible targets include: `email`, `weather`, `calendar`, `price`, `code`, `document`, `portfolio`, and more (see the [W5H2 Classification](./w5h2-classification.md) page for the full list).

- An empty list means the rule matches any target (when the action matches).
- The wildcard `"*"` matches any target.
- Matching is case-insensitive.

### Keyword triggers (`trigger_keywords`)

Match against the raw query text using substring search. Keywords are checked **before** W5H2 classification (at Tier 0/1), ensuring that cached results cannot bypass keyword-based blocks.

- An empty list means the rule does not use keyword-based matching.
- Matching is case-insensitive substring search.
- Keywords are checked independently of action/target triggers.

### Matching logic

```
Rule matches if:
  (has_action_triggers AND action_matches AND target_matches)
  OR
  (has_keyword_triggers AND any_keyword_found_in_query)
```

A rule with no triggers at all (empty actions, empty targets, empty keywords) matches nothing and is skipped.

### Rule evaluation order

Rules are evaluated **top to bottom**. The **first matching rule wins**. This means you should place more specific rules before more general ones:

```yaml
rules:
  # Specific: block delete on portfolio (evaluated first)
  - name: block_portfolio_delete
    trigger_actions: [delete]
    trigger_targets: [portfolio]
    enforcement: block

  # General: allow all delete actions (evaluated second)
  - name: allow_delete
    trigger_actions: [delete]
    trigger_targets: []
    enforcement: allow
```

If no rule matches, the `default_enforcement` is applied.

---

## Built-in Templates

NabaOS ships with 8 constitution templates covering common use cases:

| Template name | Description |
|---|---|
| `default` | General-purpose safety defaults — block destructive keywords, confirm sends, allow reads |
| `content-creator` | Content creation workflows |
| `dev-assistant` | Developer assistant (code/git/CI domain) |
| `full-autonomy` | Minimal restrictions for advanced users |
| `home-assistant` | Smart home (IoT/calendar domain) |
| `hr-assistant` | Human resources workflows |
| `research-assistant` | Research: papers, data analysis, experiments |
| `trading` | Financial markets monitoring and trading |

**Using a template:**

```bash
# List available templates
nabaos config rules templates

# Generate a constitution file from a template
nabaos config rules use-template trading --output constitution.yaml

# View the active constitution
nabaos config rules show
```

Templates can be customized after generation. The template provides a starting point; you add or modify rules for your specific needs.

---

## Constitution Check in the Pipeline

The constitution is checked at two points in the pipeline:

### Check 1: Keyword check (before classification)

Immediately after security scanning and before any cache or classification lookup, the constitution enforcer scans the raw query text against keyword-trigger rules. This ensures that keyword-based blocks cannot be bypassed by cache hits.

```
Query arrives
    |
    v
Security scan (credential scanner, pattern matcher)
    |
    v
Constitution keyword check  <--- CHECK 1
    |
    v
Tier 0: Fingerprint cache
    ...
```

### Check 2: Intent check (after classification)

After Tiers 1-2 classify the intent into an action-target pair, the constitution enforcer checks all action-trigger and target-trigger rules against the classified intent.

```
    ...
Tier 1-2: BERT/SetFit classifies intent
    |
    v
Constitution intent check   <--- CHECK 2
    |
    v
Tier 2.5: Semantic cache lookup
    ...
```

If either check returns `block`, the query is rejected. If either check returns `confirm`, the user is prompted for approval. Processing continues only after both checks pass.

### Check 3: Ability check (before chain execution)

When executing agent chains (multi-step workflows), each step's declared ability is checked against the constitution before execution. This prevents chains from performing actions that the user's constitution does not permit.

```
Chain execution starts
    |
    v
For each step:
    Constitution ability check  <--- CHECK 3
    |
    v
    Execute step (if allowed)
```

This three-layer enforcement ensures that the constitution is respected at every stage of query processing, from raw text input through classification to execution.
