# Architecture Overview

> **What you'll learn**
>
> - How NabaOS processes a query from arrival to response
> - The role of each major component: Orchestrator, Security, Cache, LLM Router, Deep Agents, Channels, and Agent OS
> - How the pipeline, constitution, and caching work together
> - Why caching reduces cost dramatically over time

---

## System Diagram

```
                          User
                           |
              +------------+------------+
              |            |            |
         Telegram      Discord        Web
              |            |            |
              +------+-----+------+----+
                     |            |
                  Gateway    WebSocket
                     |
          +----------v-----------+
          |     Orchestrator     |
          |  (Rust async runtime)|
          +----------+-----------+
                     |
       +-------------v--------------+
       |      Security Layer        |
       |                            |
       |  Credential Scanner        |
       |  Pattern Matcher           |
       |  Anomaly Detector          |
       |  Constitution Pre-check    |
       +-------------+--------------+
                     |
       +-------------v--------------+
       |     Pipeline (Tiers 0-4)   |
       |                            |
       |  Tier 0: Fingerprint       |  <1ms   $0.00
       |  Tier 1: BERT Classifier   |  5-10ms $0.00
       |    (8 classes, ~110M)      |
       |  Tier 2: SetFit Classifier |  ~10ms  $0.00
       |    (54 classes, ~22M)      |
       |  Tier 2.5: Semantic Cache  |  ~20ms  $0.00
       |  Tier 3: Cheap LLM        |  100ms+ $0.001-0.01
       |  Tier 4: Deep Agent        |  secs+  $0.50-5.00
       +-------------+--------------+
                     |
       +-------------v--------------+
       |      Response Layer        |
       |                            |
       |  Format + Channel Adapt    |
       |  Cost Tracking             |
       |  Cache Graduation          |
       +----------------------------+
```

---

## Component Overview

### Orchestrator

The orchestrator is the central event loop. Implemented as a Rust async runtime on top of tokio, it receives messages from all channels, routes them through security checks, dispatches them into the pipeline, and returns responses. It also manages the task queue, cost tracking, and agent lifecycle.

Key responsibilities:

- Receive and normalize messages from all channel adapters
- Enforce rate limiting and concurrent request caps
- Dispatch queries through the security layer and pipeline
- Track per-query cost and latency metrics
- Serialize memory writes through the task queue

### Security Layer

Security runs **before** any LLM call, making it a hard gate rather than a suggestion. The layer comprises four subsystems that execute in sequence:

| Subsystem | What it does | Latency |
|---|---|---|
| **Credential Scanner** | Regex-based detection of API keys, passwords, PII, and secrets in query text | <1ms |
| **Pattern Matcher** | Known prompt-injection and jailbreak pattern matching | <1ms |
| **Anomaly Detector** | Behavioral profiling that flags unusual request frequency, scope expansion, or pattern deviation | <1ms |
| **Constitution Pre-check** | Keyword-based constitution checks before classification | <1ms |

If any check fails, the query is rejected immediately. No LLM cost is incurred.

> **Note:** The BERT classifier is part of the pipeline (Tier 1), not the security layer. It performs W5H2 intent classification, not security classification.

### Two-Model Classification System

NabaOS uses two separate ML models for intent classification:

| Model | Tier | Architecture | Parameters | Classes | Threshold |
|---|---|---|---|---|---|
| **BERT** | Tier 1 | BERT-base-uncased (ONNX) | ~110M | 8 | 0.85 confidence |
| **SetFit** | Tier 2 | all-MiniLM-L6-v2 (ONNX) | ~22M | 54 | N/A |

The BERT classifier handles the 8 most common intent classes with high accuracy (97.3%). If confidence is below 0.85, the query cascades to SetFit which covers all 54 W5H2 classes.

> **`bert` feature gate:** The BERT model is optional. When built without `--features bert`, Tiers 1-2 degrade to `unknown_unknown` classification and queries fall through to the LLM tiers. This is useful for minimal deployments that don't need local classification.

### Cache (Fingerprint + Semantic Cache)

The cache system has multiple layers that work together:

**Fingerprint Cache (Tier 0):** An exact-match lookup on the normalized query string. If someone asks "check my email" for the 50th time, the fingerprint cache returns the cached response template in under 1ms. This is a simple hash-map lookup.

**Intent Cache (Tier 2):** After W5H2 classification determines the action-target pair (e.g., `check_email`), the intent cache looks for a cached execution plan for that intent class. This enables reuse even when the exact wording differs -- "show me my inbox" and "check my email" both map to `check_email` and hit the same cached plan.

**Semantic Cache (Tier 2.5):** Embedding-based similarity search that matches queries by semantic meaning rather than exact text or intent class.

Both caches store parameterized execution plans, not raw LLM outputs. The plan is a sequence of tool calls with parameter slots that get filled from the current query's extracted parameters.

### LLM Router

When the cache misses, the router decides which LLM provider to use based on task complexity and cost:

| Layer | Provider examples | Cost per request | When used |
|---|---|---|---|
| **Cheap LLM** | Claude Haiku, GPT-4o-mini, DeepSeek | $0.001 - $0.01 | Novel but simple tasks |
| **Medium LLM** | Claude Sonnet, GPT-4o | $0.01 - $0.05 | Multi-step reasoning |
| **Expensive LLM** | Claude Opus, GPT-4 | $0.05 - $0.50 | Complex analysis |

The router also appends a metacognition prompt to every LLM call. The LLM evaluates its own solution and decides whether the result should be cached for future reuse. This feedback loop is how the system gets cheaper over time.

### Deep Agents (Tier 4)

For genuinely complex, multi-step, autonomous tasks, the system delegates to specialized deep agent backends:

| Backend | Best for | Typical cost |
|---|---|---|
| **Manus API** | Web research, multi-step browsing, data gathering | $1 - $5 |
| **Claude computer-use** | Code analysis, document processing, structured output | $0.50 - $3 |
| **OpenAI agents** | Structured data tasks, function-calling workflows | $0.50 - $2 |
| **Custom backends** | User-defined integrations | Varies |

The backend selector picks the cheapest provider that meets the task's quality requirements. Constitution spending limits and user approval flows gate expensive operations.

### Channels

Channels are the user-facing interfaces. Each channel adapter normalizes messages into a common internal format and adapts responses back to the channel's native format:

- **Telegram** -- Primary channel. Supports inline keyboards for confirmation flows.
- **Discord** -- Outbound message delivery (no inbound slash commands).
- **Web** -- Dashboard and WebSocket-based interface for browser clients.

All channels share the same orchestrator, security layer, and pipeline. A query from Telegram is processed identically to one from the web dashboard.

### Agent OS (Packages and Permissions)

Agent OS is the runtime environment for third-party agent packages (`.nap` files). It provides:

- **Package management** -- Install, start, stop, and uninstall agent packages
- **Permission enforcement** -- Each agent declares required permissions in its manifest; the runtime grants or denies them
- **Data isolation** -- Each agent gets a namespaced key-value store; agents cannot read each other's data
- **Resource limits** -- Memory, CPU, and fuel (execution step) limits prevent runaway agents
- **Intent routing** -- Agents declare intent filters; incoming intents are routed to the best-matching agent

> **Note:** Intent routing depends on the BERT/SetFit classification models. Without the `bert` feature gate, intent routing degrades and all queries are classified as `unknown_unknown`.

---

## Data Flow Walkthrough

Here is the complete path of a query through the system, using "Send an email to Alice about the project update" as an example:

```
1. ARRIVAL
   User sends message via Telegram
   Gateway normalizes to internal Message struct

2. SECURITY GATE
   Credential Scanner: no secrets detected          PASS
   Pattern Matcher: no injection patterns            PASS
   Anomaly Detector: within normal parameters        PASS

3. KEYWORD CONSTITUTION CHECK
   Constitution checks query text for keyword triggers
   "send" + "email" does not match any keyword-only block rules   PASS

4. TIER 0 — FINGERPRINT
   Hash lookup on normalized query: MISS
   (exact wording not seen before)

5. TIER 1 — BERT CLASSIFIER
   Classify intent: send_email (confidence 0.94)

6. TIER 2 — SETFIT CLASSIFIER
   Refine classification: send_email (54-class space)

7. INTENT CONSTITUTION CHECK
   Action: send, Target: email
   Rule "confirm_send_actions" matches
   Enforcement: Confirm
   → Prompt user for confirmation via Telegram inline keyboard

8. USER CONFIRMS
   User taps "Approve" on Telegram

9. TIER 2.5 — SEMANTIC CACHE
   Lookup cached plan for send_email: HIT
   Cached plan: call email API with (to: <recipient>, subject: <subject>, body: <body>)
   Extract params from query: to=Alice, subject="project update"

10. EXECUTE
    Run the cached tool sequence
    Email sent successfully

11. RESPONSE
    Format response: "Email sent to Alice: 'project update'"
    Deliver via Telegram
    Log: latency=250ms, cost=$0.00, tier=2.5
```

If this had been a novel query with no cached plan, it would have escalated to Tier 3 (cheap LLM) or Tier 4 (deep agent). After the LLM solves it, the metacognition step would evaluate whether to cache the solution for next time.

---

## How Caching Reduces Cost Over Time

The key insight is that most daily interactions are repeated patterns. After a one-week learning period, the distribution typically looks like this:

```
Week 1 (learning):
  Cache hits:     20%  x $0.00  = $0.00
  Cheap LLM:      60%  x $0.005 = $0.003
  Deep Agent:      20%  x $2.00  = $0.40
  Average cost per query: ~$0.40

Week 4 (steady state):
  Cache hits:     90%  x $0.00  = $0.00
  Cheap LLM:       8%  x $0.005 = $0.0004
  Deep Agent:       2%  x $2.00  = $0.04
  Average cost per query: ~$0.04
```

For a user making 100 queries per day:

| Period | Without cache | With cache | Monthly savings |
|---|---|---|---|
| Day 1 | $10.00 | $8.00 | -- |
| Week 2 | $10.00/day | $5.00/day | $150 |
| Month 2 | $10.00/day | $4.00/day | $180 |

The cache gets better over time because:

1. **More patterns are learned** -- each novel query that gets cached is one fewer future LLM call
2. **Similarity thresholds adapt** -- cache entries with high success rates relax their match threshold, catching more variations
3. **Metacognition improves routing** -- the LLM's delegation assessments train the complexity classifier, so fewer queries get routed to expensive tiers
4. **Deep agent results decompose** -- when a $3 deep agent task completes, the result decomposer breaks it into reusable subtask patterns that become cache entries

The system is designed so that the user's **first month is the most expensive month they will ever have**. Every subsequent month costs less as the cache grows.
