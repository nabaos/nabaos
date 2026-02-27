# Introduction

**NabaOS** is a self-hosted, privacy-first AI agent runtime built in Rust.
Think of it as **"The Android for AI Agents"** -- an open operating system where
you control which agents run, which LLM backends they use, and what data ever
leaves your machine.

## Why NabaOS Exists

Every mainstream AI agent today sends every request to a remote LLM, even when
you have asked the same question a hundred times before. That is slow, expensive,
and a privacy leak. NabaOS fixes this with a **five-tier semantic caching pipeline**
that resolves up to 90% of daily requests locally, cutting LLM costs by roughly
85% while keeping your data on your hardware.

```text
Request
  |
  v
+---------------------+   < 0.1 ms   Cost: $0.00
| Tier 1: Fingerprint |-- HIT ------> Done (exact-match hash)
+---------------------+
  | MISS
  v
+---------------------+   < 5 ms     Cost: $0.00
| Tier 2: SetFit ONNX |-- HIT ------> Done (local W5H2 classifier)
+---------------------+
  | MISS
  v
+---------------------+   < 20 ms    Cost: $0.00
| Tier 3: Intent Cache|-- HIT ------> Done (cached execution plan)
+---------------------+
  | MISS
  v
+---------------------+   ~ 1 s      Cost: ~$0.005
| Tier 4: Cheap LLM   |-- solved ---> Done + cache for next time
+---------------------+
  | too complex
  v
+---------------------+   5-120 s    Cost: $0.50-5.00
| Tier 5: Deep Agent   |-- solved ---> Done + decompose into cache
| (Manus/Claude/GPT)  |
+---------------------+
```

## Key Differentiators

- **Multi-backend.** Route tasks to Anthropic Claude, OpenAI, Google Gemini,
  Manus, DeepSeek, or a local model -- whichever is cheapest and best for the job.
  No single-vendor lock-in.

- **Constitutional governance.** Every agent operates under an Ed25519-signed YAML
  constitution that defines allowed domains, spending limits, and hard boundaries.
  The agent cannot modify its own constitution.

- **Privacy by default.** The five-tier pipeline means the vast majority of your
  requests never leave your machine. Credentials and PII are scanned and redacted
  before any external API call.

- **106 plugins, 130 pre-built agents.** Browse the catalog, install an agent with
  one command, and start it. Agents run in sandboxed WASM modules with
  permission-gated access to your data.

- **Channels everywhere.** Interact through Telegram, Discord, Slack, WhatsApp,
  a web dashboard, or the CLI. Same agent, same constitution, every channel.

## Who Is It For?

- **Privacy-conscious professionals** who want AI assistance without sending
  every message to a cloud provider.
- **Developers** who want to build, test, and deploy custom agents with a
  proper permission model and caching layer.
- **Regulated industries** (legal, healthcare, finance) that need auditable,
  constitution-enforced AI workflows where data residency matters.
- **Self-hosters** who run their own infrastructure and want an agent runtime
  that respects that philosophy.

## What's Next

Head to [Installation](getting-started/installation.md) to get NabaOS running in
under five minutes, or read about the [Architecture](core-concepts/architecture.md)
if you want to understand the system before you install it.
