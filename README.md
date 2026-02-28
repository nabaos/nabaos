<div align="center">

# NabaOS

### An Operating System for Autonomous AI Agents

**Cache-first. Constitution-enforced. Provider-independent.**

[![CI](https://img.shields.io/github/actions/workflow/status/nabaos/nabaos/ci.yml?branch=main&style=flat-square&label=build)](https://github.com/nabaos/nabaos/actions)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.80+-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Tests](https://img.shields.io/badge/tests-1%2C500+-brightgreen.svg?style=flat-square)]()

[Quick Start](#quick-start) · [Why an OS?](#why-an-operating-system) · [Architecture](#architecture) · [Security](#security-model) · [Docs](https://nabaos.github.io/nabaos/) · [Contributing](#contributing)

</div>

---

## What is NabaOS?

NabaOS is a **self-hosted runtime for AI agents** — written in Rust, designed to run on your hardware, and built so that no single AI provider controls your data or your costs.

It provides the same abstractions an operating system provides to programs — process isolation, a permission model, a filesystem, inter-process communication, hardware drivers, a package manager — but adapted for autonomous AI agents that act in the real world.

After a one-week learning period, **~90% of daily requests resolve from a local cache in under 10ms at zero cost.** The remaining requests route to whichever AI backend offers the best cost/quality tradeoff. Agents can pursue multi-week objectives autonomously, generate media, browse the web, send messages across six channels, and integrate with 2,800+ APIs — all governed by cryptographically signed constitutions that the agent cannot modify.

---

## Quick Start

```bash
# Install (requires bash, not sh)
bash <(curl -fsSL https://raw.githubusercontent.com/nabaos/nabaos/main/scripts/install.sh)

# Configure
nabaos setup --interactive    # pick constitution, add API keys, choose channels

# Run
nabaos start                  # start the agent runtime
```

<details>
<summary><b>Docker</b></summary>

```bash
docker run -d --name nabaos \
  -e NABA_LLM_API_KEY=sk-... \
  -e NABA_TELEGRAM_BOT_TOKEN=... \
  -v nabaos-data:/data \
  ghcr.io/nabaos/nabaos:latest
```

</details>

<details>
<summary><b>Build from source</b></summary>

```bash
git clone https://github.com/nabaos/nabaos.git && cd nabaos
cargo build --release
./target/release/nabaos setup --interactive
./target/release/nabaos start
```

</details>

<details>
<summary><b>ARM (Oracle Cloud Free Tier / Raspberry Pi)</b></summary>

```bash
make build-arm    # cross-compile for aarch64
scp target/aarch64-unknown-linux-musl/release/nabaos user@host:/opt/nabaos/bin/
```

</details>

---

## Why an Operating System?

Traditional agent frameworks give you a loop: receive input → call LLM → return output. That's a script, not a system. The moment you need two agents that shouldn't read each other's data, or a workflow that survives a reboot, or a budget that actually stops spending — the framework falls apart.

NabaOS treats agents the way an OS treats processes:

```
┌─────────────────────────────────────────────────────────────────┐
│                        Traditional Agents                       │
│                                                                 │
│   User ──→ LLM ──→ Tools ──→ Response                          │
│                                                                 │
│   No isolation. No permissions. No persistence.                 │
│   Every request costs money. One bad prompt breaks everything.  │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                            NabaOS                               │
│                                                                 │
│                    ┌─────────────────────┐                      │
│                    │   Constitution       │  ← Signed policy    │
│                    │   (Ed25519, r/o)     │    agent can't edit  │
│                    └────────┬────────────┘                      │
│                             │                                   │
│   User ──→ Security ──→ Router ──→ Cache/LLM/Agent ──→ Response│
│              │              │            │                       │
│              │         ┌────┴────┐  ┌────┴────┐                 │
│              │         │Scheduler│  │  WASM   │                 │
│              │         │Workflows│  │ Sandbox │                 │
│              │         └─────────┘  └─────────┘                 │
│              │                                                  │
│         ┌────┴─────────────────────────────────┐                │
│         │ Vault │ 2FA │ Anomaly │ Permissions  │                │
│         └──────────────────────────────────────┘                │
│                                                                 │
│   Isolation. Permissions. Persistence. Metered execution.       │
│   90% of requests are free. Constitution prevents harm.         │
└─────────────────────────────────────────────────────────────────┘
```

| OS Concept | NabaOS Equivalent |
|---|---|
| **Processes** | Agents — isolated, each with its own constitution, persona, and resource quotas |
| **Kernel** | Orchestrator — Rust/Tokio async runtime, routes all requests through security |
| **Filesystem** | Encrypted SQLite stores + sandboxed file I/O with path allowlisting |
| **Permissions** | Three-level resolution: constitution (ceiling) → agent (narrows) → workflow (widens within ceiling) |
| **IPC / Message Bus** | Tokio broadcast channel — 17 typed event kinds, pub/sub across all subsystems |
| **Package Manager** | `.nap` agent packages — 130 in the catalog, installable with manifests and dependency tracking |
| **Drivers** | Plugin system — 106 service integrations (GPIO, HTTP, Cloud, Subprocess, WASM) |
| **Scheduler** | Cron + interval scheduling, channel-triggered workflows, background objectives |
| **Resource Limits** | Per-agent CPU fuel, memory caps, API call quotas, daily/monthly spending budgets |
| **Syscalls** | Ability registry — typed, metered, constitution-checked function dispatch |

---

## Architecture

NabaOS is organized into six layers. Each layer builds on the one below it.

```
Figure 1. Layered Architecture

┌─────────────────────────────────────────────────────────────────────┐
│ Layer 6: Deployment & Export                                        │
│                                                                     │
│  ARM static binary   Docker    systemd    Cloud Run    ROS 2        │
│  (Oracle Free Tier)  Compose   service    container    package      │
│                                                                     │
│  Cache export: turn learned behaviors into deployable artifacts     │
│  for Raspberry Pi, ESP32, Cloud Run, or ROS 2 robots               │
├─────────────────────────────────────────────────────────────────────┤
│ Layer 5: Channels & Integration                                     │
│                                                                     │
│  Telegram  Discord  Slack  WhatsApp  Email  Web Dashboard           │
│                                                                     │
│  106 provider plugins (GitHub, Stripe, Notion, Home Assistant...)   │
│  2,800+ APIs discoverable via OpenAPI auto-config (APIs.guru)       │
│  MCP server integration    Chrome extension bridge                  │
├─────────────────────────────────────────────────────────────────────┤
│ Layer 4: Creative & Research                                        │
│                                                                     │
│  Studio: image (DALL-E, fal.ai, ComfyUI) │ video (Runway, Kling)  │
│          audio (ElevenLabs, OpenAI TTS)   │ slides (reveal.js)     │
│  Swarm: parallel multi-source research with LLM synthesis           │
│  Browser: 4-layer NavCascade (DOM → YOLO → WebBERT → LLM)         │
│  Charts: line, bar, scatter, candlestick SVG via plotters           │
├─────────────────────────────────────────────────────────────────────┤
│ Layer 3: Autonomy & Planning (PEA Engine)                           │
│                                                                     │
│  BDI deliberation ── intention stability, prevents goal thrashing   │
│  HTN decomposition ── hierarchical task networks with backtracking  │
│  Pramana validation ── 4 epistemological methods for every decision │
│  Cybernetic budget ── 4-mode adaptive cost control                  │
│  Episodic memory ── learns from past executions                     │
│  Hegelian review ── thesis-antithesis-synthesis when stuck          │
│                                                                     │
│  Chain DSL: declarative workflows with circuit breakers,            │
│  progressive trust graduation, and transactional compensation       │
├─────────────────────────────────────────────────────────────────────┤
│ Layer 2: Security & Governance                                      │
│                                                                     │
│  Constitution ── Ed25519-signed YAML, tamper-proof, 8 templates     │
│  BERT classifier ── ONNX threat detection in <10ms                  │
│  Credential scanner ── 16 secret patterns + 4 PII patterns          │
│  Injection detector ── 6 categories incl. multilingual + Unicode    │
│  Anomaly detector ── behavioral profiling with deviation scoring    │
│  Vault ── AES-256-GCM, zeroize-on-drop                             │
│  Privilege guard ── 4-level (Open → Elevated → Admin → Critical)   │
│  Channel permissions ── per-contact, per-group, per-domain          │
│  Runtime Watcher ── event bus monitoring with auto-pause             │
├─────────────────────────────────────────────────────────────────────┤
│ Layer 1: Intelligence & Routing                                     │
│                                                                     │
│  5-Tier Cascade:                                                    │
│    T0 Fingerprint  (<1ms, $0.00)  ── exact SHA-256 match            │
│    T1 BERT         (5ms,  $0.00)  ── ONNX action classifier        │
│    T2 SetFit+Cache (10ms, $0.00)  ── 384-dim semantic match         │
│    T3 Cheap LLM    (200ms,$0.005) ── Haiku/GPT-4o-mini + metacog   │
│    T4 Deep Agent   (secs, $0.50+) ── Claude / OpenAI / Ollama      │
│                                                                     │
│  W5H2 intent decomposition (Who/What/When/Where/Why/How/HowMuch)   │
│  Semantic work cache with self-improving metacognition loop          │
│  Multi-provider LLM router (Anthropic, OpenAI, Gemini, DeepSeek,   │
│  Ollama) with failover, streaming, vision, and function calling     │
├─────────────────────────────────────────────────────────────────────┤
│ Layer 0: Runtime                                                    │
│                                                                     │
│  Rust + Tokio async    SQLite persistence    WASM sandbox (fuel)    │
│  Docker executor       Encrypted storage     Signed receipts        │
└─────────────────────────────────────────────────────────────────────┘
```

---

## The Five-Tier Cascade

Every query enters at the top and falls through tiers until one can handle it. After one week of learning, ~90% of daily queries resolve at Tiers 0–2 — locally, instantly, for free.

```
Figure 2. Query Routing Through the Five-Tier Cascade

                          ┌──────────┐
                    Query │  User    │
                          └────┬─────┘
                               │
                     ┌─────────▼──────────┐
                     │  Security Layer     │
                     │  Constitution check │
                     │  BERT classifier    │
                     │  Credential scan    │
                     └─────────┬──────────┘
                               │
              ┌────────────────▼────────────────┐
              │  Tier 0: Fingerprint Cache       │  <1ms   $0.00
              │  SHA-256 exact match             │
              │  ┌─ HIT ──→ execute cached fn ──→ done
              │  └─ MISS ─┐                      │
              └───────────┼──────────────────────┘
                          │
              ┌───────────▼──────────────────────┐
              │  Tier 1: BERT Classifier          │  5ms    $0.00
              │  66M-param ONNX action routing    │
              │  ┌─ confident (≥0.85) ──→ route ──→ done
              │  └─ uncertain ─┐                  │
              └────────────────┼──────────────────┘
                               │
              ┌────────────────▼──────────────────┐
              │  Tier 2: SetFit + Intent Cache     │  ~10ms  $0.00
              │  384-dim semantic similarity match  │
              │  ┌─ HIT (≥0.92) ──→ execute ──→ done
              │  └─ MISS ─┐                        │
              └───────────┼────────────────────────┘
          ────────────────┼──── local / free boundary ──────
                          │
              ┌───────────▼────────────────────────┐
              │  Tier 3: Cheap LLM                  │  ~200ms $0.005
              │  Haiku / GPT-4o-mini / Ollama       │
              │  + metacognition prompt:             │
              │    "Should we cache this?"           │
              │  ┌─ solved ──→ cache + respond ──→ done
              │  └─ too complex ─┐                  │
              └─────────────────┼──────────────────┘
                                │
              ┌─────────────────▼──────────────────┐
              │  Tier 4: Deep Agent                 │  secs   $0.50+
              │  Claude / OpenAI / Ollama (local)   │
              │  Full autonomy with budget guard     │
              │  Constitution spending check         │
              │  Result decomposed → cache subtasks  │
              └────────────────────────────────────┘
```

The **metacognition loop** is what makes the cache self-improving: every Tier 3–4 response includes the LLM's self-assessment of whether and how to cache the solution. Over time, more queries migrate down to Tiers 0–2.

---

## PEA: Plan and Execute Autonomously

Most agents are reactive — they answer one question, then forget. NabaOS includes **PEA**, a persistent autonomous execution engine that can pursue complex objectives over days or weeks.

PEA is built on the **Nyaya Triad** — three complementary frameworks:

```
Figure 3. The Nyaya Triad — PEA's Decision Architecture

                    ┌──────────────────────┐
                    │      Objective        │
                    │  "Write an Indian     │
                    │   cuisine cookbook     │
                    │   and market it"      │
                    └──────────┬───────────┘
                               │
              ┌────────────────▼────────────────┐
              │          BDI Engine              │
              │  (Belief-Desire-Intention)       │
              │                                  │
              │  Beliefs: what the agent knows   │
              │  Desires: what it wants to achieve│
              │  Intentions: what it commits to   │
              │                                  │
              │  Key property: intention          │
              │  stability — once committed,      │
              │  doesn't thrash between goals     │
              │  (fixes AutoGPT's critical flaw)  │
              └────────────────┬────────────────┘
                               │
              ┌────────────────▼────────────────┐
              │      HTN Decomposer             │
              │  (Hierarchical Task Networks)    │
              │                                  │
              │  "write cookbook" decomposes to:  │
              │   ├─ research_and_compile        │
              │   │   ├─ search recipes          │
              │   │   ├─ organize by region      │
              │   │   └─ synthesize              │
              │   ├─ write_document              │
              │   │   ├─ draft chapters          │
              │   │   └─ edit and format         │
              │   ├─ generate_media              │
              │   │   └─ food photography        │
              │   └─ publish_content             │
              │       ├─ format for platforms    │
              │       └─ social_media_campaign   │
              └────────────────┬────────────────┘
                               │
              ┌────────────────▼────────────────┐
              │     Pramana Validator            │
              │  (Epistemological Verification)  │
              │                                  │
              │  Every decision validated by:    │
              │                                  │
              │  Pratyaksha — direct observation │
              │    "Did the API return data?"    │
              │                                  │
              │  Anumana — inference             │
              │    "If X then Y" with fallacy    │
              │    detection (hetvabhasa)        │
              │                                  │
              │  Upamana — analogy               │
              │    "A similar task succeeded     │
              │     this way last week"          │
              │                                  │
              │  Shabda — testimony              │
              │    "Ask the human when           │
              │     confidence < 0.7"            │
              └────────────────┬────────────────┘
                               │
              ┌────────────────▼────────────────┐
              │     Budget Controller            │
              │  (Cybernetic Feedback Loop)      │
              │                                  │
              │  4 modes:                        │
              │  Aggressive ──→ Conservative     │
              │   (< 80% burn)    (80-95%)       │
              │                                  │
              │  Minimal ──→ Exhausted           │
              │   (> 95%)     (budget depleted)  │
              │                                  │
              │  Auto-switches based on burn     │
              │  rate — never exceeds budget     │
              └─────────────────────────────────┘
```

When PEA gets stuck for more than an hour, it triggers a **Hegelian dialectic review** — the LLM generates a thesis (current approach), antithesis (why it's failing), and synthesis (a new strategy). This replaces the infinite retry loops that plague other autonomous agents.

**Example:** `nabaos pea start "Write an Indian cuisine cookbook and make it popular on social media" --budget 50.0` triggers weeks of autonomous research, writing, illustration, publishing, and social media engagement — with every decision epistemologically validated and every dollar tracked.

---

## Security Model

NabaOS was built security-first. Every query passes through multiple security layers before any action is taken.

```
Figure 4. Security Pipeline — 8 Layers, Every Query

  Query
    │
    ▼
  ┌──────────────────────────┐
  │ 1. Constitution Check     │  Ed25519-signed YAML
  │    Typed rules: deny /    │  Agent cannot modify
  │    warn / confirm / allow │  8 templates included
  └────────────┬─────────────┘
               │
  ┌────────────▼─────────────┐
  │ 2. BERT Security          │  66M-param ONNX model
  │    Threat classification  │  <10ms latency
  │    in <10ms               │  Catches prompt injection
  └────────────┬─────────────┘
               │
  ┌────────────▼─────────────┐
  │ 3. Credential Scanner     │  16 secret patterns
  │    AWS, GCP, OpenAI,      │  4 PII patterns
  │    GitHub, Stripe, PEM,   │  Auto-redaction
  │    Telegram, HuggingFace  │
  └────────────┬─────────────┘
               │
  ┌────────────▼─────────────┐
  │ 4. Injection Detector     │  6 categories
  │    Direct, identity,      │  Multilingual (7 langs)
  │    authority, exfil,      │  Unicode normalization
  │    encoded, multilingual  │  (catches homoglyphs)
  └────────────┬─────────────┘
               │
  ┌────────────▼─────────────┐
  │ 5. Channel Permissions    │  Per-contact, per-group
  │    Android-like granular  │  per-domain control
  │    Three-level resolution │  with "-" exclusion
  └────────────┬─────────────┘
               │
  ┌────────────▼─────────────┐
  │ 6. Privilege Guard        │  4 levels:
  │    Open → Elevated →      │  Open (32 abilities)
  │    Admin → Critical       │  Elevated (TOTP, 1h TTL)
  │                           │  Admin (TOTP+pass, 15min)
  │                           │  Critical (single-use)
  └────────────┬─────────────┘
               │
  ┌────────────▼─────────────┐
  │ 7. Anomaly Detector       │  Behavioral profiling
  │    Rolling windows        │  Deviation scoring
  │    (1h / 24h / 7d)        │  New tool/domain alerts
  └────────────┬─────────────┘
               │
  ┌────────────▼─────────────┐
  │ 8. Vault & Encryption     │  AES-256-GCM storage
  │    Secrets zeroized on    │  PBKDF2 key derivation
  │    drop, intent-bound     │  100K iterations
  └────────────┬─────────────┘
               │
               ▼
           Approved → Execute
```

The **Runtime Watcher** (optional, zero-cost when disabled) monitors all subsystems via a typed event bus. When anomaly scores cross thresholds, it triggers LLM analysis and can auto-pause components — but it **never** kills the daemon, deletes data, or modifies the constitution. All pause actions are reversible.

---

## Constitution System

Constitutions are not system prompts — they are **Ed25519-signed, read-only YAML policies** mounted into the runtime. The agent cannot modify, bypass, or reason its way around them.

```yaml
# config/constitutions/trading.yaml
identity:
  name: "TradeWatch"
  purpose: "Monitor markets, execute pre-approved trading strategies"

domain:
  allowed: [market_data, trading, financial_news, portfolio]
  out_of_domain_action: block_and_alert    # not just "warn"

boundaries:
  never_access: ["~/.ssh", "~/.aws", "personal email"]
  approved_tools: [market_data_fetch, portfolio_read, trade_execute]

deep_agent:
  max_per_task_usd: 5.00
  max_daily_usd: 20.00
  approval_threshold_usd: 2.00             # ask user above this
```

**8 templates included:** Default, Developer, Trading, Research, Content Creator, Home Assistant, HR, Full Autonomy.

---

## Progressive Trust

Agents earn autonomy through demonstrated reliability — not through prompting.

```
Figure 5. Trust Graduation

Level 0: Supervised         Level 1: Graduated         Level 2: Autonomous
─────────────────           ──────────────────         ────────────────────
Every step verified         Only unproven steps        Full self-direction
by cheap LLM                verified                   for graduated abilities

             50 runs, >95% success              50 runs, >95% success
  Level 0 ──────────────────────→ Level 1 ──────────────────────→ Level 2

  PINNED abilities NEVER graduate:
  trading.execute, trading.buy, trading.sell,
  payment.send, payment.transfer, email.send
```

---

## Agent Catalog & Plugin System

NabaOS ships with **130 pre-built agent packages** and **106 provider plugins**, installable via `.nap` manifests.

<details>
<summary><b>Sample agents from the catalog (130 total)</b></summary>

| Category | Agents |
|----------|--------|
| **Productivity** | todo-manager, habit-tracker, weekly-review, daily-journal, study-planner |
| **Finance** | crypto-tracker, portfolio-monitor, budget-tracker, bill-reminder, tax-organizer |
| **DevOps** | ci-monitor, dependency-scanner, deploy-assistant, log-analyzer, pr-reviewer |
| **Content** | blog-writer, copywriter, video-scripter, social-scheduler, seo-optimizer |
| **Research** | web-researcher, competitive-intel, patent-monitor, academic-reviewer |
| **Health** | medication-reminder, sleep-analyzer, workout-planner, wellness-checker |
| **Communication** | email-digest, newsletter-curator, welcome-sequence, template-responder |
| **Business** | lead-scorer, contract-reviewer, win-loss-analyzer, satisfaction-monitor |
| **Travel** | trip-planner, visa-checker, flight-monitor |

</details>

<details>
<summary><b>Provider plugins (106 integrations)</b></summary>

GitHub, GitLab, Slack, Discord, Telegram, Gmail, Outlook, Google Drive, Google Sheets, Google Docs, Notion, Airtable, Jira, Linear, Confluence, Salesforce, HubSpot, Stripe, PayPal, Shopify, QuickBooks, Twilio, SendGrid, Mailgun, Home Assistant, SmartThings, Hue, MQTT, S3, Firebase, Supabase, Docker Hub, Sentry, PagerDuty, Datadog, CloudWatch, Amplitude, Mixpanel, Reddit, Twitter, LinkedIn, Instagram, TikTok, YouTube, Spotify, Wikipedia, ArXiv, PubMed, HuggingFace, Weather, News, Flights, Hotels, and 50+ more.

</details>

Agents declare their required abilities and triggers in a manifest:

```yaml
# catalog/email-digest/manifest.yaml
name: email-digest
version: 1.0.0
description: Daily email digest summary
permissions: [gmail.read, llm.query]
triggers:
  scheduled:
    - chain: daily-digest
      interval: 24h
      at: "08:00"
```

The **Skill Forge** can also create new skills at runtime — Chain-level skills need no auth, WASM skills require TOTP, and Shell skills require TOTP + password.

---

## Creative Engine (Studio)

NabaOS includes a unified multimodal generation pipeline with 5 provider backends and cost-aware routing.

```
Figure 6. Studio — Multimodal Creative Pipeline

  ┌───────────────────────────────────────────────────────────┐
  │  "Create a 2-minute video about Indian street food"       │
  └───────────────────────────┬───────────────────────────────┘
                              │
              ┌───────────────▼───────────────┐
              │       Shot Planner            │
              │   LLM generates shot list:    │
              │   Shot 1: bustling market (7s) │
              │   Shot 2: vendor cooking (8s)  │
              │   Shot 3: close-up food (5s)   │
              │   + narration script           │
              └───────────────┬───────────────┘
                              │
      ┌───────────────────────┼───────────────────────┐
      │                       │                       │
      ▼                       ▼                       ▼
  ┌─────────┐          ┌──────────┐           ┌──────────┐
  │ Image   │          │  Video   │           │  Audio   │
  │ ComfyUI │ (free)   │  fal.ai  │ (cheap)   │ Eleven   │
  │ fal.ai  │ (cheap)  │  Runway  │ (quality) │ Labs     │
  │ DALL-E  │ (quality)│          │           │ OpenAI   │
  └────┬────┘          └────┬─────┘           └────┬─────┘
       │                    │                      │
       │              ┌─────▼──────┐               │
       │              │ VideoLooper│               │
       │              │ generate → │               │
       │              │ extract    │               │
       │              │ last frame │               │
       │              │ → describe │               │
       │              │ → continue │               │
       │              └─────┬──────┘               │
       │                    │                      │
       └────────────────────┼──────────────────────┘
                            │
                   ┌────────▼─────────┐
                   │  ffmpeg concat   │
                   │  + audio merge   │
                   └────────┬─────────┘
                            │
                   ┌────────▼─────────┐
                   │  Final video     │
                   │  with narration  │
                   └──────────────────┘

  Provider priority: Local (free) → Cheap → Quality
  Cost estimate shown before generation starts
```

Also generates: **slides** (reveal.js + export to PPTX/ODP/PDF via pandoc), **charts** (line/bar/scatter/candlestick SVG), and **images** with 3 backend options.

---

## Research Swarm

Parallel multi-source research with LLM-powered synthesis and academic citation tracking.

```
  "semiconductor ETF landscape"
           │
     ┌─────┼──────────────────┐
     ▼     ▼                  ▼
  ┌──────┐ ┌──────────┐ ┌────────┐
  │Search│ │ Academic  │ │  PDF   │
  │Worker│ │ (OpenAlex)│ │ Worker │
  └──┬───┘ └────┬─────┘ └───┬────┘
     │          │            │
     │   citations + DOIs    │
     │          │            │
     └──────────┼────────────┘
                │
     ┌──────────▼──────────┐
     │ Dedup by content    │
     │ hash (SHA-256)      │
     └──────────┬──────────┘
                │
     ┌──────────▼──────────┐
     │ LLM Synthesis       │
     │ Executive summary   │
     │ Thematic sections   │
     │ [1][2] citations    │
     │ Research gaps        │
     └─────────────────────┘
```

---

## Browser Automation

4-layer navigation cascade — each layer is cheaper and faster than the next, falling through only when uncertain.

| Layer | Method | Latency | Cost |
|-------|--------|---------|------|
| 0 | DOM Heuristics — rule-based element matching | <1ms | $0.00 |
| 1 | YOLO Detector — vision-based element detection | ~50ms | $0.00 |
| 2 | WebBERT — 15-class ONNX action classifier | ~5ms | $0.00 |
| 3 | LLM Fallback — full page understanding | ~2s | ~$0.01 |

Includes: ChromePool (tab management), stealth mode (anti-detection), CAPTCHA solver (VLM + CapSolver tiers), session persistence (cookies + localStorage), and Chrome extension bridge (WebSocket + HMAC).

---

## Workflow Engine

Declarative multi-step workflows with compensation, circuit breakers, and progressive trust.

```yaml
# A transactional workflow with rollback
nodes:
  - id: charge
    type: action
    ability: stripe.charge
    params: { amount: "{{amount}}", customer: "{{customer_id}}" }

  - id: fulfill
    type: action
    ability: warehouse.ship
    params: { order_id: "{{order_id}}" }

  - id: rollback
    type: compensate
    compensates: charge
    ability: stripe.refund
```

**7 node types:** Action, WaitEvent, Delay, WaitPoll, Parallel (All/Any/N join), Branch, Compensate.

**Circuit breakers** can abort workflows when spending exceeds thresholds, frequency limits are hit, or dangerous patterns are detected. The `Confirm` action is deliberately treated as `Abort` — a security decision.

---

## Cache Export

Cached agent behaviors compile to deployable artifacts for 4 platforms:

| Target | Output | Use Case |
|--------|--------|----------|
| **Cloud Run** | Dockerfile + Rust HTTP service | Serverless deployment |
| **Raspberry Pi** | ARM cross-compiled binary | Edge computing |
| **ESP32** | WASM module for microcontroller | IoT devices |
| **ROS 2** | `ament_cargo` package with auto-mapped topics/services | Robotics |

The ROS 2 export automatically maps hardware abilities to DDS topics (reads → `r2r::Publisher`) and services (writes → `r2r::ServiceServer`).

---

## CLI Reference

```bash
nabaos setup --interactive    # First-time setup wizard
nabaos start                  # Start the agent runtime
nabaos ask "your question"    # One-shot query
nabaos status                 # Agent status, costs + cache stats

# Autonomous objectives
nabaos pea start "goal" --budget 10.0
nabaos pea list               # Active objectives
nabaos pea status <id>        # Progress + spend

# Research
nabaos research "topic"       # Parallel multi-source research

# Cache management
nabaos admin retrain          # Retrain local classifiers
nabaos export list            # List cached behaviors
nabaos export generate <id>   # Export to Cloud Run / RPi / ESP32 / ROS 2

# API discovery
nabaos config resource discover stripe     # Search 2,800+ APIs
nabaos config resource auto-add stripe     # Auto-register from OpenAPI spec

# Security & monitoring
nabaos admin scan "text"      # Check for threats
nabaos check                  # Validate config
nabaos check --health         # HTTP health check
nabaos watcher status         # Anomaly scores
nabaos watcher resume <comp>  # Resume paused component

# Plugins
nabaos admin plugin install <url>   # Install from manifest
nabaos admin plugin list            # Installed plugins
```

---

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `NABA_LLM_API_KEY` | API key for your chosen provider | *(required)* |
| `NABA_LLM_PROVIDER` | `anthropic` / `openai` / `deepseek` / `local` | `anthropic` |
| `NABA_DAILY_BUDGET_USD` | Daily spending cap | `10.0` |
| `NABA_CACHE_SIMILARITY` | Cache hit threshold (0.0–1.0) | `0.92` |
| `NABA_TELEGRAM_BOT_TOKEN` | Telegram bot | *(optional)* |
| `NABA_DISCORD_BOT_TOKEN` | Discord bot | *(optional)* |
| `NABA_SLACK_BOT_TOKEN` | Slack bot | *(optional)* |
| `NABA_WEB_PASSWORD` | Web dashboard auth | *(optional)* |
| `NABA_DATA_DIR` | Data directory | `./data` |

[Full reference →](https://nabaos.github.io/nabaos/reference/environment-variables/)

---

## What's Inside

| Metric | Value |
|--------|-------|
| Language | Rust (82,000+ lines across 229 source files) |
| Tests | 1,500+ passing |
| Source modules | 30 subsystems |
| Agent catalog | 130 pre-built agents |
| Provider plugins | 106 service integrations |
| Constitution templates | 8 (Default, Dev, Trading, Research, Content, Home, HR, Full Autonomy) |
| Persona templates | 5 (each binds voice/tone/quirks to an LLM provider preference) |
| Communication channels | 6 (Telegram, Discord, Slack, WhatsApp, Email, Web) |
| LLM providers | 5+ (Anthropic, OpenAI, Gemini, DeepSeek, Ollama/local) |
| Deep agent backends | 2 (Claude, OpenAI) + custom backend support |
| API discovery | 2,800+ APIs via OpenAPI auto-config (APIs.guru) |
| Export targets | 4 (Cloud Run, Raspberry Pi, ESP32, ROS 2) |

---

## Philosophy

NabaOS draws from **Nyaya** — the Indian philosophical tradition of logic, epistemology, and structured reasoning:

- **Pramana** (valid knowledge) — Four epistemological methods validate every autonomous decision: direct observation, inference with fallacy detection, analogy from past experience, and testimony from humans.
- **Hetvabhasa** (fallacy detection) — Three categories of logical fallacy (Asiddha, Viruddha, Savyabhichara) are checked before any inference is accepted.
- The autonomous execution engine (PEA) combines BDI architecture with Nyaya epistemology for decisions that are not just capable, but *justified*.

The name **NabaOS** comes from *Naba* (নব, Bengali: "new") — a new kind of operating system for a new kind of software.

---

## Contributing

```bash
cargo test --all-features          # 1,500+ tests
cargo clippy -- -D warnings        # zero warnings policy
```

Conventions:
- `#![deny(unsafe_code)]` — no unsafe Rust, anywhere
- All crypto through `ring` — no hand-rolled cryptography
- Never log message content — metadata only
- All SQL parameterized — no string interpolation

See [CONTRIBUTING.md](CONTRIBUTING.md) for details.

---

## License

MIT — see [LICENSE](LICENSE).

---

<div align="center">

**Your agents. Your data. Your rules.**

</div>
