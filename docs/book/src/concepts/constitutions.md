# Constitutions

> **What you'll learn**
>
> - What a constitution is and how it differs from a system prompt
> - The full YAML structure with annotated examples
> - How each enforcement type works: allow, block, confirm, warn
> - How trigger matching works with actions, targets, and keywords
> - How Ed25519 signing protects constitutions from tampering
> - The 21 built-in constitution templates
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
| Signed | No | Ed25519 signature |
| Checked when | After LLM generates output | Before any LLM call |

The constitution is mounted read-only into the runtime container. The agent cannot write to its own constitution. Modification requires the local CLI tool with Ed25519 signing:

```bash
nabaos constitution edit
```

---

## YAML Structure

A constitution is a YAML file with the following structure:

```yaml
# Constitution name — used for identification and template lookup
name: "my-trading-bot"

# Semantic version
version: "1.0.0"

# Human-readable description of this constitution's purpose
description: "Trading assistant — market monitoring, portfolio analysis, trade execution"

# Default enforcement for intents that don't match any rule.
# IMPORTANT: This should almost always be "block" (deny-by-default).
# Using "allow" or "warn" here means ANY unmatched intent is permitted,
# which defeats the purpose of the constitution.
default_enforcement: block

# Ordered list of rules. Rules are evaluated top-to-bottom;
# the FIRST matching rule wins.
rules:
  # Rule 1: Allow read-only price checks
  - name: allow_price_checks
    description: "Allow checking stock/crypto prices"
    trigger_actions:
      - check          # Matches the "check" W5H2 action
    trigger_targets:
      - price          # Matches the "price" W5H2 target
    trigger_keywords: []
    enforcement: allow
    reason: "Price checks are read-only and safe"

  # Rule 2: Require confirmation for trade execution
  - name: confirm_trades
    description: "Require user approval before executing any trade"
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
    description: "Trading bot cannot access personal communications"
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
    description: "Block queries with dangerous keywords regardless of intent"
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
    description: "Allow market analysis and research"
    trigger_actions:
      - analyze
      - search
      - generate
    trigger_targets: []   # Empty = matches any target
    trigger_keywords: []
    enforcement: allow
    reason: "Analysis operations are the core function of this bot"
```

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

**Important security note:** The default enforcement for unmatched intents is `block` (deny-by-default). If you set `default_enforcement: warn` or `default_enforcement: allow`, any intent that does not match a rule will be permitted. This is almost always a mistake.

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

## Ed25519 Signing

Constitutions are signed with Ed25519 to prevent tampering. The signing workflow:

```
1. User edits constitution YAML on their local machine
2. CLI computes Ed25519 signature of the file contents
3. Signature is stored alongside the constitution
4. At load time, the runtime verifies the signature
5. If the signature is invalid, the constitution is rejected
   and the agent refuses to start
```

**Generate a signing key:**

```bash
nabaos constitution keygen
# Writes keypair to ~/.nabaos/constitution.key
```

**Sign a constitution:**

```bash
nabaos constitution sign config/constitutions/trading.yaml
# Appends signature to the file or writes a .sig sidecar
```

**Verify a constitution:**

```bash
nabaos constitution verify config/constitutions/trading.yaml
# Output: Constitution signature valid: trading v1.0.0
```

The signing key is stored on the user's machine and never transmitted to the agent runtime. This ensures that even if the agent runtime is compromised, the constitution cannot be modified.

---

## Built-in Templates

NabaOS ships with 21 constitution templates covering common use cases. Each template follows deny-by-default policy and includes sensible rules for its domain.

| Template name | Description | Key rules |
|---|---|---|
| `default` | General-purpose safety defaults | Block destructive keywords, confirm sends/deletes, allow reads |
| `solopreneur` | Business planning, drafting, research | Allow business ops, confirm sends/deletes |
| `freelancer` | Invoicing, client comms, time tracking | Allow freelance ops, confirm sends/deletes |
| `digital-marketer` | Analytics, content creation, SEO | Allow marketing, block financial access |
| `student` | Research, study aids, assignments | Allow learning ops, block financial access |
| `sales` | Lead management, outreach, pipeline | Allow sales ops, confirm outreach sends |
| `customer-support` | Ticket triage, KB search, response drafting | Allow support ops, block delete/control |
| `legal` | Contract analysis, case research, drafting | Allow legal ops, block delete/control |
| `ecommerce` | Inventory, orders, product listings | Allow e-commerce ops, confirm sends/deletes |
| `hr` | Recruitment, onboarding, engagement | Allow HR ops, block financial access |
| `finance` | Accounting, tax, audit, budgeting | Allow finance + trading ops, confirm sends |
| `healthcare` | Clinical summaries, triage, drug interactions | Allow healthcare ops, block delete/control |
| `engineering` | Inspections, maintenance, project tracking | Allow engineering ops, confirm sends/deletes |
| `media` | Journalism, PR, content production | Allow media ops, block financial access |
| `government` | Policy analysis, regulatory, compliance | Allow government ops, block destructive actions |
| `ngo` | Grant writing, donor reports, monitoring | Allow NGO ops, confirm sends/deletes |
| `logistics` | Shipment tracking, route optimization | Allow logistics ops, confirm sends/deletes |
| `research` | Literature review, data analysis, papers | Allow research ops, block financial + destructive |
| `consulting` | Competitive analysis, due diligence | Allow consulting ops, confirm sends/deletes |
| `creative` | Design, trends, spec sheets, content | Allow creative ops, block financial access |
| `agriculture` | Crop monitoring, market prices, weather | Allow agriculture + trading, confirm sends/deletes |

**Using a template:**

```bash
# Initialize with a template
nabaos init --constitution solopreneur

# Or switch templates later
nabaos constitution use legal
```

Templates can be customized after initialization. The template provides a starting point; you add or modify rules for your specific needs.

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

After Tier 1 classifies the intent into an action-target pair, the constitution enforcer checks all action-trigger and target-trigger rules against the classified intent.

```
    ...
Tier 1: BERT classifies intent
    |
    v
Constitution intent check   <--- CHECK 2
    |
    v
Tier 2: Intent cache lookup
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
