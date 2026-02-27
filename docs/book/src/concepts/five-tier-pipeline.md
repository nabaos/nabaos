# The 5-Tier Pipeline

> **What you'll learn**
>
> - How each of the 5 tiers works: Fingerprint, BERT Classifier, SetFit + Intent Cache, Cheap LLM, and Deep Agent
> - The latency and cost characteristics of each tier
> - How queries escalate from cheap/fast tiers to expensive/powerful ones
> - The math behind cache-driven cost savings
> - How entries graduate between tiers over time
> - How to configure the cache similarity threshold

---

## Overview

The 5-tier pipeline is the core routing mechanism of NabaOS. Every query enters at Tier 0 and escalates upward only if the current tier cannot handle it. The goal is to resolve as many queries as possible at the cheapest, fastest tier.

```
Query arrives
    |
    v
+------------------+
| Tier 0           |  <1ms, $0.00
| Fingerprint      |-----> HIT? --> Execute cached response
| (exact match)    |
+--------+---------+
         | MISS
         v
+------------------+
| Tier 1           |  5-10ms, $0.00
| BERT Classifier  |-----> Classify intent (action + target)
| (66M ONNX)       |
+--------+---------+
         |
         v
+------------------+
| Tier 2           |  ~10ms, $0.00
| SetFit + Intent  |-----> HIT? --> Execute cached plan
| Cache (ONNX)     |
+--------+---------+
         | MISS
         v
+------------------+
| Tier 3           |  100-500ms, $0.001-0.01
| Cheap LLM        |-----> Solve + metacognition
| (Haiku/GPT-mini) |       (may create cache entry)
+--------+---------+
         | TOO COMPLEX
         v
+------------------+
| Tier 4           |  seconds-minutes, $0.50-5.00
| Deep Agent       |-----> Manus / Claude / OpenAI
| (multi-backend)  |       (decomposes result for caching)
+------------------+
```

---

## Tier 0: Fingerprint Cache

**What it does:** Exact-match lookup on a normalized hash of the query string.

**When it activates:** Every query hits Tier 0 first. It activates (returns a hit) when the exact same query text has been seen before and a cached response exists.

**Latency:** <1ms (hash-map lookup)

**Cost:** $0.00 (no external calls)

**Example query that hits this tier:**

```
"What's the weather in Mumbai?"
```

If this exact query was asked yesterday, the fingerprint cache has the response template. The system fills in current weather data from the cached tool sequence and returns immediately.

**How it works:**

1. Normalize the query: lowercase, strip extra whitespace, remove punctuation
2. Compute a SHA-256 hash of the normalized string
3. Look up the hash in an in-memory hash map
4. If found, return the cached execution plan with parameter slots filled from the current context

Fingerprint caching is intentionally conservative. It only matches on exact (normalized) text. This prevents false cache hits at the cost of lower hit rates. The intent cache at Tier 2 handles fuzzy matching.

---

## Tier 1: BERT Classifier

**What it does:** Classifies the query into a W5H2 intent (action + target pair) using a 66M-parameter BERT model running locally via ONNX Runtime.

**When it activates:** Every query that misses Tier 0 passes through Tier 1. This tier does not return a response -- it produces a classification that Tier 2 uses.

**Latency:** 5-10ms (local ONNX inference)

**Cost:** $0.00 (runs entirely on-device)

**Example:**

```
Input:  "Can you check if I have any new emails?"
Output: Action=check, Target=email, Confidence=0.96
```

The BERT classifier maps natural language queries into the W5H2 intent space (11 actions x 30 targets). This classification serves two purposes:

1. **Constitution enforcement** -- the classified intent is checked against constitution rules before any further processing
2. **Intent cache lookup** -- the action-target pair becomes the cache key for Tier 2

The classifier runs a SetFit ONNX model that was trained on the 54 W5H2 classes defined in the system. At 66M parameters, it fits comfortably in memory on any modern machine and runs in single-digit milliseconds.

---

## Tier 2: SetFit + Intent Cache

**What it does:** Uses the W5H2 classification from Tier 1 to look up a cached execution plan. If the intent class (e.g., `check_email`) has been seen before and a plan was cached, it executes the plan directly without any LLM call.

**When it activates:** After Tier 1 classifies the intent, Tier 2 checks the intent cache. It activates (returns a hit) when a cached plan exists for the classified action-target pair.

**Latency:** ~10ms (cache lookup + parameter extraction)

**Cost:** $0.00 (no external calls)

**Example query that hits this tier:**

```
"Show me my inbox"
```

Even though the exact text differs from "check my email," both queries classify as `check_email`. If a cached plan for `check_email` exists, Tier 2 executes it directly.

**Cached plan structure:**

```yaml
intent: check_email
plan:
  - tool: email.list
    args:
      folder: "{{folder|inbox}}"
      limit: "{{limit|10}}"
  - tool: format.email_summary
    args:
      emails: "{{prev.result}}"
parameters:
  - name: folder
    type: text
    default: inbox
  - name: limit
    type: number
    default: 10
hit_count: 347
success_rate: 0.98
```

Parameters enclosed in `{{...}}` are extracted from the current query by a lightweight parameter extractor. Default values are used when a parameter is not found in the query.

---

## Tier 3: Cheap LLM

**What it does:** Sends the query to an inexpensive LLM (Claude Haiku, GPT-4o-mini, or DeepSeek) for resolution. The LLM also receives a metacognition prompt that asks it to evaluate whether the solution should be cached.

**When it activates:** When both the fingerprint cache (Tier 0) and intent cache (Tier 2) miss.

**Latency:** 100-500ms (API round-trip)

**Cost:** $0.001 - $0.01 per request

**Example query that hits this tier:**

```
"What caused the NVDA stock dip today?"
```

This is a novel query -- the wording is unique and the intent (analyze + price) does not have a cached plan yet. The cheap LLM handles it and, via metacognition, decides whether to cache the pattern.

**Metacognition response example:**

```json
{
  "cacheable": true,
  "function_name": "analyze_stock_movement",
  "description": "Analyze why a given stock moved significantly",
  "parameters": [
    {"name": "ticker", "type": "text", "description": "Stock ticker symbol"},
    {"name": "direction", "type": "text", "description": "up or down"}
  ],
  "tool_sequence": [
    {"tool": "market.get_price", "args": {"ticker": "{{ticker}}"}},
    {"tool": "news.search", "args": {"query": "{{ticker}} stock {{direction}}"}},
    {"tool": "llm.summarize", "args": {"context": "{{prev.result}}"}}
  ],
  "confidence": 0.85
}
```

This cached plan means the next time someone asks "Why did AAPL drop?" or "Explain the Tesla rally," it hits the intent cache at Tier 2 instead of calling the LLM again.

---

## Tier 4: Deep Agent

**What it does:** Delegates genuinely complex, multi-step, or autonomous tasks to specialized deep agent backends. These are external services that can browse the web, execute code, manage files, and perform long-running workflows.

**When it activates:** When the cheap LLM at Tier 3 determines the task is too complex for a single LLM call -- typically tasks requiring multi-step research, browser automation, or sustained autonomous operation.

**Latency:** Seconds to minutes (depends on task complexity)

**Cost:** $0.50 - $5.00 per task

**Example query that hits this tier:**

```
"Research the top 5 semiconductor ETFs, compare their expense ratios
and top holdings, and recommend one based on my risk profile."
```

This requires multiple web searches, data extraction, comparison analysis, and personalized recommendation -- well beyond what a single LLM call can handle.

**Backend selection:**

The system routes to the best backend based on task type and cost:

```
Task type         Preferred backend     Reason
─────────         ─────────────────     ──────
Web research      Manus API             Best at multi-step browsing
Code analysis     Claude computer-use   Best at code comprehension
Structured data   OpenAI agents         Best at function-calling
Custom tasks      User-defined          Configurable
```

**Constitution gates for Tier 4:**

Before dispatching to a deep agent, the system checks:

1. Is this task type allowed by the constitution's `[deep_agent]` section?
2. Is the estimated cost within per-task, daily, and monthly spending limits?
3. Does the cost exceed the approval threshold? If so, prompt the user for confirmation via Telegram inline keyboard.

**Result decomposition:**

After a deep agent completes a task, the result decomposer analyzes the execution trace and extracts reusable subtask patterns. A $3 research task might produce 4-5 cached plans that prevent future deep agent calls for similar subtasks.

---

## Cost Savings Math

Here is the cost model for a steady-state system (after the initial learning period):

```
Tier distribution      Cost per query     Weighted cost
──────────────────     ──────────────     ─────────────
90% Tier 0-2 (cache)   $0.00              $0.000
 8% Tier 3 (cheap LLM) $0.005             $0.0004
 2% Tier 4 (deep agent) $2.00             $0.040
                                          ──────
                        Average per query: $0.04
```

Without the caching pipeline, every query would hit at least Tier 3:

```
 0% cache               $0.00              $0.000
80% cheap LLM           $0.005             $0.004
20% deep agent           $2.00             $0.400
                                          ──────
                        Average per query: $0.40
```

**That is a 10x cost reduction.** For a user making 100 queries per day:

| Metric | Without cache | With cache (steady state) |
|---|---|---|
| Daily cost | $40.00 | $4.00 |
| Monthly cost | $1,200.00 | $120.00 |
| Annual cost | $14,400.00 | $1,440.00 |
| **Annual savings** | -- | **$12,960** |

---

## How Entries Graduate Between Tiers

Cache entries are not static. They improve and graduate over time through a feedback loop:

### Tier 3 to Tier 2 graduation

When a Tier 3 (cheap LLM) response includes a positive metacognition assessment (`"cacheable": true`), the solution is compiled into a parameterized plan and stored in the intent cache. Future queries matching the same intent class resolve at Tier 2 instead of Tier 3.

### Tier 2 similarity threshold relaxation

New cache entries start with a strict similarity threshold of 0.95. After 5 successful uses, the threshold relaxes to 0.92, allowing more query variations to hit the cache.

```
Cache entry lifecycle:

  Created          5 hits           Low success
  threshold=0.95 → threshold=0.92 → threshold=0.95 (tightened)
                                     or disabled if <60% success
```

### Tier 4 result decomposition

When a deep agent completes a complex task, the result decomposer breaks the execution trace into subtasks. Each subtask that appears reusable becomes a new Tier 2 cache entry. This is how a single $3 deep agent call can prevent dozens of future expensive calls.

### Cache entry retirement

If a cache entry's success rate drops below 0.80, its similarity threshold tightens back to 0.95. If success rate drops below 0.60, the entry is disabled and flagged for re-evaluation by the expensive LLM on the next matching query.

---

## Configuration

### NABA_CACHE_SIMILARITY

The `NABA_CACHE_SIMILARITY` environment variable controls the cosine similarity threshold for semantic cache matching. This determines how closely a new query must match an existing cache entry to be considered a hit.

```bash
# Default: 0.92 (recommended for most users)
export NABA_CACHE_SIMILARITY=0.92

# Conservative: fewer false cache hits, more LLM calls
export NABA_CACHE_SIMILARITY=0.95

# Aggressive: more cache hits, higher risk of mismatches
export NABA_CACHE_SIMILARITY=0.88
```

**Guidelines:**

- **0.95** -- Use during the first week while the cache is learning. Prevents bad cache entries from forming.
- **0.92** -- Default for steady state. Good balance between hit rate and accuracy.
- **0.88** -- Only for domains with highly repetitive queries (e.g., a trading bot that always asks similar price-check questions).

### Other pipeline settings

```bash
# Hours before anomaly detection kicks in (learning period)
export NABA_LEARNING_HOURS=24

# LLM provider for Tier 3
export NABA_CHEAP_LLM_PROVIDER=anthropic
export NABA_CHEAP_LLM_MODEL=claude-haiku-4-5

# Expensive model for metacognition evaluation
export NABA_EXPENSIVE_LLM_MODEL=claude-opus-4-6
```

### CLI cache management

```bash
# View cache statistics
nabaos cache stats

# List all cached entries
nabaos cache list

# Invalidate a specific cache entry
nabaos cache invalidate <entry-id>
```
