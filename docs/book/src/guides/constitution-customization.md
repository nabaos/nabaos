# Constitution Customization

> **What you'll learn**
>
> - What constitutions are and how they enforce boundaries
> - How to start from a built-in template
> - How to write custom rules with actions, targets, and keywords
> - The four enforcement levels: allow, warn, confirm, block
> - How to test your constitution with the CLI

---

## Prerequisites

- NabaOS installed (`nabaos --version`)

---

## What is a constitution?

A constitution is a set of rules that gate every action before any LLM or tool execution. It defines what your agent is allowed to do, what requires confirmation, and what is permanently blocked.

Key design principles:

- **Configurable default**: The shipped `default.yaml` uses `allow` as the default enforcement. The code default (when no constitution is loaded) is `block`.
- **Runs before everything**: Constitution checks happen before cache lookups, LLM calls, and tool execution.
- **Immutable at runtime**: The agent cannot modify its own constitution.
- **Per-agent isolation**: Each agent can have its own constitution.

---

## Step 1: List available templates

NabaOS ships with 8 constitution templates:

```bash
nabaos config rules templates
```

Expected output:

```
Available constitution templates:
  default              General-purpose safety defaults
  content-creator      Content creation workflows
  dev-assistant        Developer assistant (code/git/CI domain)
  full-autonomy        Minimal restrictions for advanced users
  home-assistant       Smart home (IoT/calendar domain)
  hr-assistant         Human resources workflows
  research-assistant   Research: papers, data analysis, experiments
  trading              Financial markets monitoring and trading
```

## Step 2: Generate a constitution from a template

Start from a template and output it to a file:

```bash
nabaos config rules use-template trading --output my-constitution.yaml
```

## Step 3: Edit the rules

Open `my-constitution.yaml` in your editor and customize the rules.

### Enforcement levels

| Level | Behavior |
|-------|----------|
| `allow` | Permit the action unconditionally |
| `warn` | Allow the action but log a warning |
| `confirm` | Require the user to confirm before proceeding |
| `block` | Reject the action |

### Rule matching logic

Rules are evaluated in order, top to bottom. The first matching rule wins.

A rule matches if **either**:
- **Intent match**: The intent's action matches a `trigger_actions` entry AND the target matches a `trigger_targets` entry (if specified)
- **Keyword match**: The query text contains any of the `trigger_keywords`

## Step 4: Test your constitution

Test individual queries against your constitution:

```bash
nabaos config rules check "delete all files"
```

View the active constitution:

```bash
nabaos config rules show
```

---

## Next steps

- [Building Agents](./building-agents.md) -- Add a constitution to your agent package
- [Secrets Management](./secrets-management.md) -- Store the signing key securely
- [Telegram Setup](./telegram-setup.md) -- See constitution enforcement in action via Telegram
