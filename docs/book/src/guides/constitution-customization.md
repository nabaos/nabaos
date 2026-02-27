# Constitution Customization

> **What you'll learn**
>
> - What constitutions are and how they enforce boundaries
> - How to start from a built-in template
> - How to write custom rules with actions, targets, and keywords
> - The four enforcement levels: allow, warn, confirm, block
> - How to test your constitution with the CLI
> - How to sign constitutions with Ed25519

---

## Prerequisites

- NabaOS installed (`nabaos --version`)
- A working data directory (default `~/.nabaos`)

---

## What is a constitution?

A constitution is a set of rules that gate every action before any LLM or tool execution. It defines what your agent is allowed to do, what requires confirmation, and what is permanently blocked.

Key design principles:

- **Deny by default**: Unmatched intents are blocked. You must explicitly allow actions.
- **Runs before everything**: Constitution checks happen before cache lookups, LLM calls, and tool execution.
- **Immutable at runtime**: The agent cannot modify its own constitution. Changes require the CLI with Ed25519 signing.
- **Per-agent isolation**: Each agent can have its own constitution.

---

## Step 1: List available templates

NabaOS ships with 21 constitution templates for different use cases:

```bash
nabaos constitution templates
```

Expected output:

```
Available constitution templates:
  default           Default safety constitution with common-sense boundaries
  solopreneur       Solopreneur assistant -- business planning, drafting, research
  freelancer        Freelancer assistant -- invoicing, client comms, time tracking
  digital-marketer  Digital marketing assistant -- analytics, content creation, SEO
  student           Student assistant -- research, study aids, assignment help
  sales             Sales assistant -- lead management, outreach, pipeline tracking
  customer-support  Support assistant -- ticket triage, KB search, response drafting
  legal             Legal assistant -- contract analysis, case research, document drafting
  ecommerce         E-commerce assistant -- inventory, orders, product listings
  hr                HR assistant -- recruitment, onboarding, employee engagement
  finance           Finance assistant -- accounting, tax, audit, budgeting
  healthcare        Healthcare assistant -- clinical summaries, triage, drug interactions
  engineering       Engineering assistant -- inspections, maintenance, project tracking
  media             Media assistant -- journalism, PR, content production
  government        Government assistant -- policy analysis, regulatory monitoring
  ngo               NGO assistant -- grant writing, donor reports, program monitoring
  logistics         Logistics assistant -- shipment tracking, route optimization
  research          Research assistant -- literature review, data analysis
  consulting        Consulting assistant -- competitive analysis, due diligence
  creative          Creative assistant -- design, trends, spec sheets
  agriculture       Agriculture assistant -- crop monitoring, market prices
```

## Step 2: Generate a constitution from a template

Start from a template and output it to a file:

```bash
nabaos constitution use-template finance -o my-constitution.yaml
```

Expected output:

```
Generated constitution from template: finance
Written to: my-constitution.yaml
```

The generated file looks like this:

```yaml
name: finance
version: 1.0.0
description: "Finance assistant -- accounting, tax, audit, budgeting"
default_enforcement: block
rules:
  - name: allow_finance_ops
    description: Allow standard finance operations
    trigger_actions:
      - check
      - search
      - create
      - analyze
      - generate
      - schedule
      - nlp
      - data
      - storage
      - docs
      - memory
      - flow
      - notify
      - calendar
      - files
      - trading
    trigger_targets: []
    trigger_keywords: []
    enforcement: allow
    reason: Standard finance operations are allowed

  - name: confirm_send
    description: Require confirmation before sending
    trigger_actions:
      - send
    trigger_targets: []
    trigger_keywords: []
    enforcement: confirm
    reason: Outbound communications need confirmation

  - name: confirm_delete
    description: Require confirmation before deleting
    trigger_actions:
      - delete
    trigger_targets: []
    trigger_keywords: []
    enforcement: confirm
    reason: Delete actions are destructive and need confirmation
```

## Step 3: Edit the rules

Open `my-constitution.yaml` in your editor and customize the rules.

### Adding a custom rule

Add a rule to block cryptocurrency-related queries:

```yaml
  - name: block_crypto
    description: Block all cryptocurrency operations
    trigger_actions:
      - "*"
    trigger_targets:
      - crypto
      - bitcoin
      - ethereum
    trigger_keywords:
      - crypto
      - bitcoin
      - ethereum
      - defi
      - nft
    enforcement: block
    reason: This agent does not handle cryptocurrency
```

### Adding a keyword-based safety rule

Block queries that contain dangerous phrases:

```yaml
  - name: block_destructive_keywords
    description: Block queries containing destructive keywords
    trigger_actions: []
    trigger_targets: []
    trigger_keywords:
      - delete all
      - rm -rf
      - drop table
      - format disk
      - wipe
      - destroy
    enforcement: block
    reason: Destructive operations require explicit confirmation
```

### Requiring confirmation for high-value actions

```yaml
  - name: confirm_large_transactions
    description: Require confirmation for transaction-related queries
    trigger_actions:
      - send
      - create
    trigger_targets:
      - invoice
      - payment
      - transfer
    trigger_keywords:
      - payment
      - transfer
      - wire
    enforcement: confirm
    reason: Financial transactions require explicit user confirmation
```

### Rule matching logic

Rules are evaluated in order, top to bottom. The first matching rule wins.

A rule matches if **either**:
- **Intent match**: The intent's action matches a `trigger_actions` entry AND the target matches a `trigger_targets` entry (if specified)
- **Keyword match**: The query text contains any of the `trigger_keywords`

The wildcard `"*"` in `trigger_actions` or `trigger_targets` matches anything.

### Enforcement levels

| Level | Behavior |
|-------|----------|
| `allow` | Permit the action unconditionally |
| `warn` | Allow the action but log a warning |
| `confirm` | Require the user to confirm before proceeding |
| `block` | Silently reject the action |

## Step 4: Test your constitution

Test individual queries against your constitution:

```bash
nabaos constitution check "delete all files"
```

Expected output:

```
Query: "delete all files"
Result: BLOCKED
Matched rule: block_destructive_keywords
Reason: Destructive operations require explicit confirmation
```

Test an allowed query:

```bash
nabaos constitution check "check my account balance"
```

Expected output:

```
Query: "check my account balance"
Result: ALLOWED
Matched rule: allow_finance_ops
Reason: Standard finance operations are allowed
```

Test a query that requires confirmation:

```bash
nabaos constitution check "send payment to vendor"
```

Expected output:

```
Query: "send payment to vendor"
Result: CONFIRM REQUIRED
Matched rule: confirm_send
Reason: Outbound communications need confirmation
```

## Step 5: View the active constitution

Show all rules in the currently loaded constitution:

```bash
nabaos constitution show
```

Expected output:

```
Constitution: finance (v1.0.0)
  Finance assistant -- accounting, tax, audit, budgeting
  Default enforcement: block

Rules:
  1. allow_finance_ops          [allow]     16 actions
  2. confirm_send               [confirm]   1 action: send
  3. confirm_delete             [confirm]   1 action: delete
  4. block_crypto               [block]     5 keywords, 3 targets
  5. block_destructive_keywords [block]     6 keywords
```

---

## Ed25519 constitution signing

For production deployments, constitutions should be signed to prevent tampering. The agent verifies the signature before loading a constitution.

### Generate a signing key pair

```bash
# Generate a private key
openssl genpkey -algorithm Ed25519 -out constitution_key.pem

# Extract the public key
openssl pkey -in constitution_key.pem -pubout -out constitution_key.pub
```

### Sign the constitution

```bash
openssl pkeyutl -sign \
  -inkey constitution_key.pem \
  -rawin \
  -in my-constitution.yaml \
  -out my-constitution.yaml.sig
```

### Configure NabaOS to verify signatures

Set the `require_signature` option and provide the public key:

```bash
export NABA_CONSTITUTION_PATH="my-constitution.yaml"
export NABA_CONSTITUTION_SIGNATURE="my-constitution.yaml.sig"
export NABA_CONSTITUTION_PUBKEY="constitution_key.pub"
```

When the agent loads, it verifies the signature before accepting the constitution. If the signature does not match, the agent refuses to start.

### Why signing matters

The constitution is mounted read-only into the orchestrator container. Even if an attacker gains access to the container, they cannot modify the constitution. Signing adds a second layer: even if the file is modified on disk, the invalid signature prevents the agent from using the tampered constitution.

---

## Complete working example

Here is a full constitution for a trading bot with strict domain boundaries:

**trading-constitution.yaml:**

```yaml
name: trading-bot
version: 1.0.0
description: "Trading bot -- monitor markets, execute pre-approved strategies"
default_enforcement: block
rules:
  # Allow read-only market operations
  - name: allow_market_checks
    description: Allow checking prices and market data
    trigger_actions:
      - check
      - search
      - analyze
      - data
      - trading
    trigger_targets: []
    trigger_keywords: []
    enforcement: allow
    reason: Read-only market operations are safe

  # Allow standard utility operations
  - name: allow_utility_ops
    description: Allow notifications, memory, and flow control
    trigger_actions:
      - notify
      - memory
      - flow
      - nlp
      - docs
      - generate
      - files
      - calendar
      - schedule
      - storage
    trigger_targets: []
    trigger_keywords: []
    enforcement: allow
    reason: Utility operations are allowed

  # Block access to personal data
  - name: block_personal_data
    description: Block access to email, contacts, and personal files
    trigger_actions:
      - "*"
    trigger_targets:
      - email
      - contacts
      - personal
    trigger_keywords:
      - personal email
      - my contacts
      - private messages
    enforcement: block
    reason: Trading bot cannot access personal data

  # Block destructive operations
  - name: block_destructive
    description: Block all destructive keywords
    trigger_actions: []
    trigger_targets: []
    trigger_keywords:
      - delete all
      - rm -rf
      - drop table
      - destroy
    enforcement: block
    reason: Destructive operations are always blocked

  # Require confirmation for sending
  - name: confirm_outbound
    description: Require confirmation for any outbound communication
    trigger_actions:
      - send
    trigger_targets: []
    trigger_keywords: []
    enforcement: confirm
    reason: Outbound messages need user confirmation
```

Test it:

```bash
nabaos constitution check "check NVDA price"
# Result: ALLOWED (allow_market_checks)

nabaos constitution check "read my personal email"
# Result: BLOCKED (block_personal_data)

nabaos constitution check "send alert to team"
# Result: CONFIRM REQUIRED (confirm_outbound)

nabaos constitution check "delete all positions"
# Result: BLOCKED (block_destructive)
```

---

## Next steps

- [Building Agents](./building-agents.md) -- Add a constitution to your agent package
- [Secrets Management](./secrets-management.md) -- Store the signing key securely
- [Telegram Setup](./telegram-setup.md) -- See constitution enforcement in action via Telegram
