# Your First Agent

> **What you'll learn**
>
> - How to browse and search the agent catalog
> - How to install, run, and inspect an agent
> - How agent permissions and manifests work
> - How to uninstall an agent you no longer need

NabaOS ships with a catalog of pre-built agents that cover common
workflows: email triage, calendar management, news monitoring, document
generation, and more. In this guide you will install one, run it, and look at
its internals.

---

## Browse the Catalog

List every available agent:

```bash
nabaos config persona catalog list
```

**Expected output:**

```text
NAME                      CATEGORY        VERSION    DESCRIPTION
--------------------------------------------------------------------------------
morning-briefing          productivity    1.0.0      Daily summary of calendar, email, and news
email-triage              communication   1.0.0      Classify and prioritize incoming email
meeting-prep              productivity    1.0.0      Research attendees and prepare talking points
expense-tracker           finance         1.0.0      Extract amounts from receipts and log expenses
news-monitor              research        1.0.0      Track topics across RSS feeds and summarize
code-reviewer             development     1.0.0      Review pull requests for style and bugs
...
```

---

## Search by Keyword

Narrow down the list with a keyword search:

```bash
nabaos config persona catalog search "email"
```

**Expected output:**

```text
NAME                      CATEGORY        VERSION    DESCRIPTION
--------------------------------------------------------------------------------
email-triage              communication   1.0.0      Classify and prioritize incoming email
email-drafter             communication   1.0.0      Draft replies based on context and tone
email-digest              productivity    1.0.0      Daily digest of unread email by priority
```

---

## Inspect an Agent

Before installing, view the full details of an agent:

```bash
nabaos config persona catalog info morning-briefing
```

**Expected output:**

```text
Name:        morning-briefing
Version:     1.0.0
Category:    productivity
Author:      nabaos-contrib
Description: Daily summary of calendar, email, and news
Permissions: net:https, read:calendar, read:email
```

The **Permissions** field is important. This agent requests:

- `net:https` -- outbound HTTPS access (for fetching news).
- `read:calendar` -- read-only access to your calendar data.
- `read:email` -- read-only access to your email data.

It does **not** request `write:email` or `exec:shell`, so it cannot send
emails or run arbitrary commands. Permissions are enforced by the WASM sandbox
at runtime.

---

## Install an Agent

Install the agent package:

```bash
nabaos config agent install morning-briefing.nap
```

**Expected output:**

```text
Installed agent 'morning-briefing' v1.0.0
```

The `.nap` file (NabaOS Agent Package) is a signed archive containing the agent's
WASM module, manifest, and assets. The install command:

1. Verifies the package signature.
2. Extracts the WASM module and manifest.
3. Registers the agent in the local database.

Verify the agent is installed:

```bash
nabaos config agent list
```

**Expected output:**

```text
NAME                 VERSION    STATE
----------------------------------------
morning-briefing     1.0.0      stopped
```

---

## Examine the Manifest

Every agent has a manifest that declares its identity, permissions, and
resource limits. You can view the permissions that were granted:

```bash
nabaos config agent permissions morning-briefing
```

**Expected output (before first run):**

```text
No permissions granted to 'morning-briefing'.
```

Permissions are granted interactively on first run. When the agent tries to use
a capability it has declared in its manifest, NabaOS will prompt you to approve
or deny it.

View full agent details:

```bash
nabaos config agent info morning-briefing
```

**Expected output:**

```text
Name:         morning-briefing
Version:      1.0.0
State:        stopped
Installed at: 2026-02-24T10:30:00Z
Updated at:   2026-02-24T10:30:00Z
```

---

## Start the Agent

```bash
nabaos config agent start morning-briefing
```

**Expected output:**

```text
Agent 'morning-briefing' started.
```

The agent's state changes from `stopped` to `running`. When the server is active,
running agents are executed on their configured schedule (for `morning-briefing`,
that is typically once per day in the morning).

To manually trigger a one-off execution, use the `admin run` command with the agent's
WASM module:

```bash
nabaos admin run \
  agents/morning-briefing/agent.wasm \
  --manifest agents/morning-briefing/manifest.json
```

**Expected output:**

```text
=== WASM Sandbox Execution ===
Agent:       morning-briefing
Version:     1.0.0
Permissions: ["net:https", "read:calendar", "read:email"]
Fuel limit:  1000000
Memory cap:  64 MB

Success:       true
Fuel consumed: 234567
Logs:
  [morning-briefing] Fetching calendar events...
  [morning-briefing] Fetching unread email (3 messages)...
  [morning-briefing] Fetching news for topics: [rust, ai-agents]...
  [morning-briefing] Briefing ready.
```

The agent runs inside a WASM sandbox with a fuel limit (preventing infinite
loops) and a memory cap. It can only access the capabilities declared in its
manifest.

> **Note:** The `--manifest` flag accepts a JSON file, not YAML.

---

## Stop the Agent

```bash
nabaos config agent stop morning-briefing
```

**Expected output:**

```text
Agent 'morning-briefing' stopped.
```

---

## Uninstall the Agent

When you no longer need an agent:

```bash
nabaos config agent uninstall morning-briefing
```

**Expected output:**

```text
Agent 'morning-briefing' uninstalled.
```

This removes the agent's WASM module, manifest, and local data and deletes
its database entry.

Verify it is gone:

```bash
nabaos config agent list
```

**Expected output:**

```text
No agents installed.
```

---

## What to Do Next

| Goal | Next page |
|------|-----------|
| Configure LLM providers and budgets | [Configuration](configuration.md) |
| Build your own agent from scratch | [Building Agents](../guides/building-agents.md) |
| Write chain workflows for agents | [Writing Chains](../guides/writing-chains.md) |
| Understand agent permissions in depth | [Agent Packages](../concepts/agent-packages.md) |
