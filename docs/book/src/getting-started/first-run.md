# First Run

> **What you'll learn**
>
> - How to run the interactive setup wizard
> - What each wizard step configures
> - How to send your first classification query
> - How to start the daemon and access the web dashboard

## Step 1: Run the Setup Wizard

The setup wizard scans your hardware, suggests which modules to enable, and
writes a profile to `~/.nabaos/profile.toml`.

```bash
nyaya setup
```

**Expected output:**

```text
Scanning hardware...

=== Hardware Report ===
  CPU:    8 cores (x86_64)
  RAM:    15.6 GB total, 11.2 GB available
  Disk:   128 GB free
  GPU:    None detected
  Docker: Available (v24.0.7)

=== Suggested Modules ===
  [x] core
  [x] web
  [ ] voice (disabled)
  [ ] browser
  [x] telegram
  [ ] latex
  [ ] mobile
  [ ] oauth

Saving suggested profile (interactive selection coming soon).
Profile saved to /home/you/.nabaos/profile.toml
```

The wizard does four things:

1. **Scans hardware** -- detects CPU, RAM, disk, GPU, and whether Docker is
   available. This determines which modules your machine can comfortably run.

2. **Suggests modules** -- enables `core` (always on), `web` (dashboard), and
   `telegram` (if a bot token is present). Disables resource-heavy modules like
   `voice` and `browser` if your hardware is constrained.

3. **Writes the profile** -- saves the module configuration to
   `~/.nabaos/profile.toml`. You can edit this file later by hand or
   re-run `nyaya setup`.

4. **Downloads models** -- if the SetFit ONNX model files are not already
   present, they are fetched on first use (~25 MB).

If you want to skip prompts and accept the suggested profile automatically:

```bash
nyaya setup --non-interactive
```

---

## Step 2: Set Your LLM Provider

NabaOS needs at least one LLM API key for Tier 4 (cheap LLM) and Tier 5
(deep agent) requests. Cached requests (Tiers 1-3) never call an LLM.

Export your API key:

```bash
# Anthropic (default)
export NABA_LLM_PROVIDER=anthropic
export NABA_LLM_API_KEY=sk-ant-api03-your-key-here

# Or OpenAI
export NABA_LLM_PROVIDER=openai
export NABA_LLM_API_KEY=sk-your-key-here
```

Add these lines to your `~/.bashrc` or `~/.zshrc` so they persist across
terminal sessions.

---

## Step 3: Quick Test -- Classify a Query

Run the classifier to verify that the SetFit ONNX model is loaded and working:

```bash
nyaya classify "check my email"
```

**Expected output:**

```text
Query:      check my email
Intent:     check|email
Action:     check
Target:     email
Confidence: 94.2%
Latency:    4.7ms
```

The classifier maps natural language to a structured W5H2 intent (action + target)
in under 5 ms, entirely on your local machine with no API call.

Try a few more:

```bash
nyaya classify "summarize this PDF"
nyaya classify "what is the weather in Tokyo"
nyaya classify "schedule a meeting with Alice tomorrow"
```

---

## Step 4: Run the Full Pipeline

The `query` command runs a request through the complete five-tier pipeline:

```bash
nyaya query "check my email"
```

**Expected output (first run -- Tier 2 hit):**

```text
=== Tier 2: SetFit ONNX Classification ===
Intent:     check|email
Confidence: 94.2%
Latency:    4.7ms
(Stored in fingerprint cache for future instant lookup)

=== Constitution Check ===
Enforcement: Allow
Allowed:     YES

=== Intent Cache MISS ===
No cached execution plan for 'check|email'. Would route to LLM.
```

Run the same query again to see the fingerprint cache in action:

```bash
nyaya query "check my email"
```

**Expected output (second run -- Tier 1 hit):**

```text
=== Tier 1: Fingerprint Cache HIT ===
Intent:     check|email
Confidence: 94.2%
Latency:    0.031ms
```

The second run resolves in under 0.1 ms because the exact query was cached as
a fingerprint hash on the first run. No model inference, no API call.

---

## Step 5: Start the Daemon

The daemon runs the scheduler loop, the Telegram bot (if configured), and the
web dashboard (if configured) as background services.

Set a password for the web dashboard:

```bash
export NABA_WEB_PASSWORD=your-secure-password
```

Start the daemon:

```bash
nyaya daemon
```

**Expected output:**

```text
Starting NabaOS daemon...
[daemon] NABA_TELEGRAM_BOT_TOKEN not set -- Telegram bot disabled.
[daemon] Starting web dashboard on http://127.0.0.1:8919...
```

---

## Step 6: Access the Web Dashboard

Open your browser and navigate to:

```text
http://localhost:8919
```

Log in with the password you set in `NABA_WEB_PASSWORD`. The dashboard shows:

- **Pipeline status** -- cache hit rates, classification latency, active agents.
- **Cost tracker** -- daily and monthly LLM spend, savings from caching.
- **Query history** -- recent requests, which tier resolved them, and latency.
- **Constitution** -- active rules and enforcement decisions.

---

## What to Do Next

| Goal | Next page |
|------|-----------|
| Install and run a pre-built agent | [Your First Agent](your-first-agent.md) |
| Configure LLM providers and budgets | [Configuration](configuration.md) |
| Set up Telegram or Discord | [Telegram Setup](../guides/telegram-setup.md) |
| Understand the five-tier pipeline | [Five-Tier Pipeline](../core-concepts/five-tier-pipeline.md) |
| Write your own agent | [Building Agents](../guides/building-agents.md) |
