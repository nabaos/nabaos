# FAQ

> **What you'll learn**
>
> - Answers to the most common questions about NabaOS
> - Cost expectations, privacy model, and platform support
> - How to extend, reset, and troubleshoot the system

---

## Cost and Pricing

### How much does NabaOS cost to run?

NabaOS itself is free and open source. Your only cost is LLM API usage. For a
typical user making ~100 queries per day:

| Period | Estimated monthly cost |
|---|---|
| Month 1 (cache learning) | $15-25 |
| Month 2+ (steady state) | $8-15 |

The cost drops over time because the six-tier caching pipeline resolves an
increasing percentage of queries locally. In steady state, roughly 90% of
queries hit Tiers 0-2 (fingerprint, BERT classifier, SetFit intent
classification), which cost $0.00 and never leave your machine.

### What drives the cost?

- **Tier 3 (Cheap LLM):** ~8% of queries at $0.001-0.01 each. These are novel
  but simple tasks routed to Claude Haiku, GPT-4o-mini, or DeepSeek.
- **Tier 4 (Deep Agent):** ~2% of queries at $0.50-5.00 each. These are complex
  multi-step tasks delegated to Manus, Claude computer-use, or OpenAI agents.

Cached queries (Tiers 0-2.5) are free. The system gets cheaper every month as
more query patterns are cached.

### Can I set spending limits?

Yes. The constitution's `deep_agent` section defines per-task, daily, and
monthly spending caps:

```yaml
deep_agent:
  max_per_task_usd: 5.00
  max_daily_usd: 20.00
  max_monthly_usd: 200.00
  approval_threshold_usd: 2.00   # Tasks above this require confirmation
```

You can also view spending in real time:

```bash
nabaos status
```

---

## Privacy and Data

### Is my data private?

Yes. NabaOS is self-hosted. Your data stays on your machine unless a query
explicitly requires an external API call (Tiers 3-4). Specifics:

- **Tiers 0-2.5 (90% of queries):** Processed entirely locally. No data leaves
  your machine. No network call is made.
- **Tier 3 (Cheap LLM):** The query text is sent to your configured LLM
  provider (Anthropic, OpenAI, etc.). Credential scanning redacts any secrets
  before the API call.
- **Tier 4 (Deep Agent):** The task description is sent to the selected backend
  (Manus, Claude, etc.). Constitution spending limits and approval flows gate
  these calls.

There is no telemetry, no analytics, and no phone-home behavior. NabaOS never
sends data to its developers or any third party.

### Where is my data stored?

All data is stored locally in `~/.nabaos/` (or the path set by
`NABA_DATA_DIR`):

```text
~/.nabaos/
  cache.db          SQLite database for fingerprint and intent caches
  cost.db           LLM cost tracking history
  profiles.db       Behavioral profiles for anomaly detection
  models/           ONNX model files for local classification
  constitution.yaml Active constitution
```

### Can I export my data?

Yes:

```bash
nabaos export
```

---

## LLM Providers

### Which LLM providers work with NabaOS?

NabaOS supports three categories of LLM backend:

| Category | Providers | Use case |
|---|---|---|
| **Cloud LLMs** | Anthropic (Claude), OpenAI (GPT), Google (Gemini), DeepSeek | Tier 3 (cheap) and Tier 4 (deep) |
| **Deep Agents** | Manus API, Claude computer-use, OpenAI agents | Tier 4 (complex multi-step tasks) |
| **Local models** | Ollama, llama.cpp, any OpenAI-compatible local server | Tier 3 (offline, free) |

Set the provider and model:

```bash
export NABA_LLM_PROVIDER=anthropic
export NABA_LLM_API_KEY=sk-ant-api03-...
export NABA_CHEAP_MODEL=claude-haiku-4-5
export NABA_EXPENSIVE_MODEL=claude-opus-4-6
```

### How do I add a new LLM provider?

If the provider exposes an OpenAI-compatible API (most local servers do), point
NabaOS at it:

```bash
export NABA_LLM_PROVIDER=openai
export NABA_LLM_API_KEY=not-needed
export NABA_LLM_BASE_URL=http://localhost:11434/v1   # Ollama example
export NABA_CHEAP_MODEL=llama3.2
```

For providers with a proprietary API, you would need to implement the provider
trait in `src/llm_router/provider.rs`. See the existing Anthropic and OpenAI
implementations as reference.

### Can I run completely offline?

Partially. When all LLM providers are unavailable:

- **Tiers 0-2.5 work fully offline.** Fingerprint cache, BERT classifier,
  SetFit intent classification, and semantic cache all run locally with no
  network dependency.
- **Tier 3-4 fail gracefully.** Novel queries that miss the cache will return
  a "no LLM provider available" error instead of hanging.

If you use a local LLM (Ollama, llama.cpp), Tier 3 also works offline. The
only tier that always requires an external network is Tier 4 (deep agents).

---

## Platform Support

### Does NabaOS run on Windows?

Not natively. NabaOS is a Linux/macOS application. On Windows, use one of:

- **WSL2 (recommended):** Install WSL2 with Ubuntu, then follow the standard
  Linux installation instructions.
- **Docker:** Run NabaOS as a Docker container on Docker Desktop for Windows.

```bash
# WSL2
wsl --install
# Then inside WSL2:
bash <(curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh)
```

### What about macOS?

Fully supported on Apple Silicon (aarch64). The one-line installer detects
your architecture automatically.

### What are the minimum system requirements?

| Requirement | Minimum |
|---|---|
| RAM | 512 MB free |
| Disk | 200 MB |
| CPU | Any 64-bit (x86_64 or aarch64) |
| Network | Outbound HTTPS (only for Tier 3-4) |

---

## Multi-User and Scaling

### Does NabaOS support multiple users?

Not yet. NabaOS is currently designed as a single-user, self-hosted system.
Each instance serves one user. If you need multiple users, run separate
instances with separate data directories and constitutions.

Multi-user support with per-user authentication and data isolation is on the
roadmap.

### Can agents communicate with each other?

No, and this is by design. Each agent operates within its own constitution
boundary. Agent A cannot read Agent B's data, invoke Agent B's tools, or
modify Agent B's constitution. Cross-agent communication would create a
privilege escalation path that violates the isolation model.

If you need coordinated behavior, create a single agent with a chain that
calls multiple tools in sequence.

---

## Performance

### Why is classification slow on first run?

The first classification after startup takes 200-500ms because the ONNX
models must be loaded into memory. Subsequent classifications run in
under 5ms because the models stay loaded.

```text
First run:   nabaos admin classify "test" → 4.7ms (but 350ms total including model load)
Second run:  nabaos admin classify "test" → 0.031ms (fingerprint cache hit)
Third query: nabaos admin classify "new query" → 4.2ms (model already loaded)
```

If you run NabaOS as a service (`nabaos start`), the models are loaded once at
startup and stay in memory. There is no slow first-query penalty.

### Why is my query hitting Tier 4 instead of the cache?

A query hits Tier 4 (deep agent) only when:

1. It missed Tiers 0-2.5 (no fingerprint match, no classification match, no
   intent cache hit, no semantic cache hit), AND
2. The Tier 3 cheap LLM determined it was too complex to handle.

Check which tier resolved your query:

```bash
RUST_LOG=debug nabaos ask "your query here"
```

Common reasons for cache misses:

- **New phrasing:** The query wording is different enough from cached entries.
  The cache will learn this phrasing after the first resolution.
- **Low similarity:** The semantic similarity to cached entries is below the
  threshold. The system is conservative by design.
- **Cache cold start:** During the first week, the cache has few entries.
  Hit rates improve as patterns accumulate.

---

## Configuration and Maintenance

### How do I reset everything?

```bash
# Nuclear option: delete all data and start fresh
rm -rf ~/.nabaos/
nabaos setup
```

This deletes:
- All cached queries (fingerprint, intent, semantic cache)
- Behavioral profiles (anomaly detection baselines)
- Cost history
- Constitution (will be recreated by setup wizard)
- Vault (all stored secrets are lost)

### How do I update NabaOS?

```bash
# If installed via the one-line installer
bash <(curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh)

# If installed via Cargo
cargo install --git https://github.com/nabaos/nabaos.git --force

# If using Docker
docker pull ghcr.io/nabaos/nabaos:latest
docker restart nabaos
```

---

## Comparison

### What is the difference from LangChain / AutoGen / CrewAI?

| Feature | LangChain | AutoGen | NabaOS |
|---|---|---|---|
| **Language** | Python | Python | Rust |
| **Hosting** | Library (you host) | Library (you host) | Standalone runtime (you host) |
| **Caching** | Optional, basic | None built-in | 6-tier semantic cache (core feature) |
| **Security** | None built-in | None built-in | Multi-module security layer, constitution |
| **Cost model** | Every call hits LLM | Every call hits LLM | 90% cached after learning period |
| **Multi-backend** | Yes (many) | Yes (OpenAI focus) | Yes (route to cheapest/best) |
| **Agent isolation** | None | None | Per-agent constitution, permission manifest |

LangChain and AutoGen are Python libraries for building LLM applications.
NabaOS is a runtime that runs agents with built-in security, caching, and
cost optimization. They solve different problems at different layers.

### Is there a hosted/cloud version?

Not yet. NabaOS is self-hosted only. A managed cloud version may be offered in
the future, but the self-hosted version will always be available and fully
featured. The project's core philosophy is that your data stays on your machine.

---

## Contributing and Security

### How do I contribute?

```bash
# Clone the repo
git clone https://github.com/nabaos/nabaos.git
cd nabaos

# Build and run tests
cargo build
cargo test

# See open issues
gh issue list --repo nabaos/nabaos
```

Contributions are welcome in all areas: code, documentation, agent packages,
plugins, and security research.

### How do I report a security vulnerability?

**Do NOT open a public GitHub issue for security vulnerabilities.**

Email security reports to: `security@nabaos.dev`

Include:
- Description of the vulnerability
- Steps to reproduce
- Impact assessment
- Suggested fix (if you have one)

We follow a 90-day responsible disclosure policy.

---

## Miscellaneous

### What does "NabaOS" mean?

NabaOS is an AI agent operating system. The name reflects the project's
philosophy of structured, evidence-based decision-making -- classifying intent,
checking trust boundaries, and making routing decisions rather than blindly
forwarding everything to an LLM.

### What license is NabaOS under?

NabaOS is open source. Check the `LICENSE` file in the repository root for the
specific license terms.
