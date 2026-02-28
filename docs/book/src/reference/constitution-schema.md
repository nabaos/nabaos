# Constitution Schema

The constitution is a set of YAML rules that gate agent actions before any
LLM or tool execution.

The shipped `default.yaml` uses `default_enforcement: allow`. When no
constitution is loaded, the code default is `block` (deny-by-default).

## YAML Schema

```yaml
name: string                    # Constitution name (required)
version: string                 # Semantic version (required)
description: string             # Human-readable description (optional)
default_enforcement: string     # Enforcement for unmatched intents [default: block]

rules:
  - name: string                # Rule name (required)
    description: string         # Human-readable description (optional)
    enforcement: string         # Action when rule matches (required)

    # Trigger conditions (at least one category should be non-empty)
    trigger_actions:            # W5H2 actions that trigger this rule
      - string
    trigger_targets:            # W5H2 targets that trigger this rule
      - string
    trigger_keywords:           # Keywords in query text that trigger this rule
      - string

    reason: string              # Why this rule exists (optional)

# Additional fields (optional)
channel_permissions: object     # Per-channel permission overrides
browser_stealth: object         # Browser stealth configuration
swarm_config: object            # Swarm execution configuration
ollama_config: object           # Local Ollama model configuration
captcha_solver: object          # Captcha solving configuration
```

## Enforcement Levels

| Level | Behavior | Allowed |
|-------|----------|---------|
| `block` | Silently block the action. No LLM call, no tool execution. | No |
| `warn` | Allow the action but log a warning. | Yes |
| `confirm` | Require user confirmation before proceeding. In non-interactive contexts (chain execution, server), this blocks the action. | No |
| `allow` | Allow unconditionally. | Yes |

## Rule Matching

Rules are evaluated in order. The first matching rule determines the
enforcement. If no rule matches, `default_enforcement` applies.

### Action/Target Matching

A rule with `trigger_actions` fires when the classified W5H2 intent's
action matches any entry in the list. Matching is case-insensitive.

If `trigger_targets` is also specified, both the action and target must
match. If `trigger_targets` is empty, any target matches.

The wildcard `"*"` matches any action or target.

### Keyword Matching

A rule with `trigger_keywords` fires when the raw query text contains any
of the specified keywords (case-insensitive substring match). Keyword
matching is independent of action/target matching.

A rule can have both action/target triggers and keyword triggers. It fires
if either condition is met.

### Pre-Classification Check

The constitution also supports a pre-classification keyword check
(`check_query_text`) that runs before W5H2 classification. This ensures
cached results cannot bypass keyword-based safety rules. Only rules with
`trigger_keywords` participate in this check.

### Ability-Level Check

During chain execution, each step's ability name is checked against
constitution rules. The ability name is split at the first `.` to extract
the action part (e.g., `email.send` -> action `email`, target `send`).
Matching uses the same action/target logic.

## Default Constitution

The built-in default constitution (`name: default`) ships with these rules:

| Rule | Enforcement | Trigger |
|------|-------------|---------|
| `block_destructive_keywords` | block | Keywords: "delete all", "rm -rf", "drop table", "format disk", "wipe", "destroy" |
| `confirm_send_actions` | confirm | Action: send |
| `warn_control_actions` | warn | Action: control |
| `allow_check_actions` | allow | Action: check |
| `allow_add_actions` | allow | Action: add |
| `allow_set_reminders` | allow | Action: set; Target: reminder |

Default enforcement for unmatched intents: **allow** (in the shipped `default.yaml`).

## Templates

NabaOS ships 8 constitution templates for different use cases:

| Template | Description |
|----------|-------------|
| `default` | General-purpose safety defaults |
| `content-creator` | Content creation workflows |
| `dev-assistant` | Developer assistant (code/git/CI domain) |
| `full-autonomy` | Minimal restrictions for advanced users |
| `home-assistant` | Smart home (IoT/calendar domain) |
| `hr-assistant` | Human resources workflows |
| `research-assistant` | Research: papers, data analysis, experiments |
| `trading` | Financial markets monitoring and trading |

Generate a template with:

```bash
nabaos config rules use-template trading -o my-constitution.yaml
```

## Complete Example

```yaml
name: trading-bot
version: 1.0.0
description: Constitution for a financial trading assistant
default_enforcement: block

rules:
  # Allow price checks -- read-only, safe
  - name: allow_price_checks
    enforcement: allow
    trigger_actions: [check, search, get]
    trigger_targets: [price, portfolio, market]
    trigger_keywords: []
    reason: Read-only financial queries are safe

  # Allow analysis operations
  - name: allow_analysis
    enforcement: allow
    trigger_actions: [analyze, generate, nlp, data, docs]
    trigger_targets: []
    trigger_keywords: []
    reason: Analysis operations are read-only

  # Require confirmation before executing trades
  - name: confirm_trades
    enforcement: confirm
    trigger_actions: [trading]
    trigger_targets: []
    trigger_keywords: []
    reason: Trade execution has financial consequences

  # Block access to personal data
  - name: block_personal_data
    enforcement: block
    trigger_actions: ["*"]
    trigger_targets: [email, calendar, contacts]
    trigger_keywords: []
    reason: Trading bot cannot access personal data

  # Block destructive keywords regardless of intent
  - name: block_destructive
    enforcement: block
    trigger_actions: []
    trigger_targets: []
    trigger_keywords:
      - delete all
      - wipe
      - destroy
      - rm -rf
    reason: Destructive operations are never allowed

  # Block all delete and control actions
  - name: block_delete_control
    enforcement: block
    trigger_actions: [delete, control, send]
    trigger_targets: []
    trigger_keywords: []
    reason: Trading bot has no delete, control, or send permissions
```

## Loading

The constitution is loaded from one of three sources, in priority order:

1. **File**: `NABA_CONSTITUTION_PATH` environment variable points to a
   YAML file.
2. **Template**: `NABA_CONSTITUTION_TEMPLATE` environment variable
   selects a built-in template by name.
3. **Default**: If neither is set, the built-in default constitution
   is used.

The constitution is immutable at runtime -- the agent cannot modify its
own constitution.
