# W5H2 Classification

> **What you'll learn**
>
> - The W5H2 framework and how it simplifies intent classification to Action + Target
> - All 11 actions and 30 targets with descriptions and examples
> - How classification maps to constitution rule matching
> - How the two-model system works (BERT for Tier 1, SetFit for Tier 2)
> - How fingerprinting provides fast exact-match before ML classification

---

## The W5H2 Framework

W5H2 stands for the seven question words: **Who, What, Where, When, Why, How, How-much**. These are the fundamental dimensions of any request. In NabaOS, we simplify this into two dimensions that capture the essential information needed for routing and policy enforcement:

```
W5H2 (full)                    NabaOS (simplified)
──────────────                  ──────────────────
Who   → implicit (the user)    Action  (What verb?)
What  → Action                 Target  (What noun?)
Where → Target context
When  → extracted parameter
Why   → not needed for routing
How   → determined by pipeline
How-much → extracted parameter
```

The key insight is that for routing queries to the right handler and enforcing constitution rules, you only need two things:

1. **Action** -- What does the user want to do? (check, send, create, delete, ...)
2. **Target** -- What does the user want to do it to? (email, calendar, price, code, ...)

Everything else (when, how much, specific parameters) is extracted as metadata after the core classification.

**Example classifications:**

```
"Check my email"                          → check + email
"Send a message to Alice about Friday"    → send + email
"What's the price of Bitcoin?"            → check + price
"Create a new invoice for $500"           → create + invoice
"Delete the old backup files"             → delete + document
"Schedule a meeting for tomorrow at 3pm"  → schedule + calendar
```

---

## Actions

The 11 actions represent the verbs of user intent. Each action maps to a distinct category of operation with different security implications:

| Action | Description | Example queries | Security profile |
|---|---|---|---|
| `check` | Read or inspect existing data | "Check my email," "What's the weather?" | Low risk (read-only) |
| `send` | Transmit data to an external recipient | "Send an email to Bob," "Message the team" | High risk (irreversible external effect) |
| `set` | Modify a configuration or state | "Set a reminder for 3pm," "Set my status to busy" | Medium risk (state change) |
| `control` | Operate a device or system | "Turn off the lights," "Start the server" | High risk (physical/system state change) |
| `add` | Append an item to a collection | "Add milk to the shopping list" | Low risk (additive) |
| `search` | Find information matching criteria | "Search for flights to Tokyo," "Find contracts from 2024" | Low risk (read-only) |
| `create` | Produce a new resource | "Create a new document," "Generate an invoice" | Medium risk (resource creation) |
| `delete` | Remove an existing resource | "Delete the old files," "Remove the event" | High risk (destructive, often irreversible) |
| `analyze` | Examine data and produce insights | "Analyze my portfolio performance," "Review this code" | Low risk (read-only + computation) |
| `schedule` | Set up a future action or event | "Schedule a meeting for Monday," "Set up daily reports" | Medium risk (commits future actions) |
| `generate` | Produce content from scratch | "Generate a summary," "Write a draft email" | Low risk (produces output, no side effects) |

---

## Targets

The 30 targets represent the nouns of user intent -- the objects being acted upon:

| Target | Description | Typical domains |
|---|---|---|
| `email` | Email messages and inbox | All |
| `weather` | Weather information and forecasts | General, agriculture |
| `calendar` | Calendar events and scheduling | All |
| `lights` | Smart lighting and IoT devices | Home automation |
| `shopping` | Shopping lists and product lookups | General, e-commerce |
| `reminder` | Reminders and time-based alerts | All |
| `price` | Financial prices (stocks, crypto, commodities) | Trading, finance, agriculture |
| `document` | Documents, files, and text content | All |
| `code` | Source code, scripts, and programming | Engineering, dev-assistant |
| `task` | Tasks, to-do items, and work items | All |
| `contact` | Contacts and address book entries | All |
| `invoice` | Invoices and billing documents | Freelancer, finance, e-commerce |
| `ticket` | Support tickets and issue reports | Customer support, engineering |
| `course` | Courses, learning materials, curriculum | Student, research |
| `property` | Real estate and physical properties | Real estate, legal |
| `health` | Health records, vitals, clinical data | Healthcare |
| `contract` | Legal contracts and agreements | Legal, consulting |
| `inventory` | Inventory counts and stock levels | E-commerce, logistics |
| `portfolio` | Investment portfolios and holdings | Trading, finance |
| `shipment` | Shipments, packages, and deliveries | Logistics, e-commerce |
| `compliance` | Regulatory compliance and audits | Legal, government, finance |
| `campaign` | Marketing campaigns and outreach | Digital marketing, sales |
| `media` | Media files, photos, videos, audio | Media, creative |
| `grant` | Grants, funding, and proposals | NGO, research |
| `asset` | Physical or digital assets | Engineering, finance |
| `vendor` | Vendors, suppliers, and partners | Logistics, e-commerce |
| `policy` | Policies, regulations, and guidelines | Government, legal, HR |
| `permit` | Permits, licenses, and certifications | Government, engineering |
| `budget` | Budgets, cost estimates, and allocations | Finance, NGO, consulting |
| `crop` | Agricultural crops and yield data | Agriculture |

---

## How Classification Maps to Constitution Rules

The W5H2 classification output directly feeds into the constitution enforcement engine. The mapping is straightforward:

```
W5H2 Classification          Constitution Rule Triggers
────────────────────          ──────────────────────────
action (e.g., "send")    →   trigger_actions: ["send"]
target (e.g., "email")   →   trigger_targets: ["email"]
```

**Example flow:**

```
User query: "Forward this email to the marketing team"

1. BERT classifies: action=send, target=email (confidence 0.94)

2. Constitution rule evaluation:
   Rule "confirm_send_actions":
     trigger_actions: ["send"]      ← matches "send"
     trigger_targets: []            ← empty = match all
     enforcement: confirm
     → MATCH: require user confirmation

3. User is prompted: "Approve sending email? [Approve] [Reject]"
```

The wildcard `"*"` in trigger_actions or trigger_targets matches any value:

```yaml
# Block ALL operations on portfolio data
- name: block_portfolio_access
  trigger_actions: ["*"]      # Any action...
  trigger_targets: [portfolio] # ...on portfolio target
  enforcement: block
```

This means the W5H2 classification serves double duty:

1. **Routing:** Determines which cached plan or tool sequence to execute
2. **Policy enforcement:** Determines which constitution rules apply

---

## Two-Model Classification System

NabaOS uses two separate ML models for intent classification, running as Tier 1 and Tier 2 of the pipeline:

### Tier 1: BERT Classifier (8 classes)

The BERT classifier is a fine-tuned BERT-base-uncased model (~110M parameters) exported to ONNX format. It handles the 8 most common intent classes with high accuracy:

```
Trained classes:
  add_shopping, check_calendar, check_email, check_price,
  check_weather, control_lights, send_email, set_reminder
```

| Metric | Score |
|---|---|
| Accuracy (8-class) | 97.3% |
| Pass-through rate (>0.85 confidence) | 97.9% |
| Accuracy at >0.85 confidence | 98.2% |
| Inference latency | 5-10ms |
| Max sequence length | 128 tokens |

**Cascade threshold:** If BERT's confidence is below **0.85**, the query cascades to the SetFit classifier at Tier 2.

### Tier 2: SetFit Classifier (54 classes)

The SetFit classifier uses an all-MiniLM-L6-v2 sentence transformer backbone (~22M parameters) with a logistic regression classification head. It covers all 54 W5H2 intent classes:

| Metric | Score |
|---|---|
| Embedding dimension | 384 |
| Inference | Two-step: ONNX embedding → logistic regression |
| Max sequence length | 128 tokens |
| Classification head | Logistic regression (weights from sklearn) |

**Inference pipeline:**

```
Query text
    |
    v
Tokenizer
    |
    v
Sentence Transformer (ONNX)
    |
    v
384-dim normalized embedding
    |
    v
Logistic regression head (embedding @ weights^T + bias)
    |
    v
Predicted class + confidence score
    |
    v
Parse into (Action, Target) pair
```

### `bert` Feature Gate

Both models are gated behind the `bert` compile-time feature flag. When built without `--features bert`:

- Tiers 1 and 2 are skipped
- All queries are classified as `unknown_unknown`
- Queries fall through directly to the semantic cache (Tier 2.5) and LLM tiers
- Intent routing for agents degrades (all queries match the default handler)

This is useful for minimal deployments where local classification is not needed.

### Why ONNX?

Both models are exported to ONNX format for several reasons:

- **No Python dependency:** The Rust runtime uses the `ort` crate (ONNX Runtime bindings) for inference. No Python interpreter needed.
- **Fast inference:** ONNX Runtime is optimized for inference with hardware acceleration support.
- **Small footprint:** Both models fit comfortably on any machine.
- **Deterministic:** Same input always produces same output, unlike LLM-based classification.

---

## Fingerprinting: Fast Exact-Match Before ML

Before the BERT/SetFit classifiers run, the fingerprint cache performs an exact-match lookup. This is the fastest possible classification path.

### How fingerprinting works

```
1. Normalize the query:
   - Lowercase
   - Strip leading/trailing whitespace
   - Collapse multiple spaces to single space
   - Remove trailing punctuation

2. Compute SHA-256 hash of normalized text

3. Look up hash in fingerprint table:
   Hash → (Action, Target, cached_plan_id)

4. If found: skip ML classification entirely
   If not found: proceed to BERT/SetFit (Tier 1)
```

### When fingerprinting helps

Fingerprinting is most effective for:

- **Habitual queries:** Users who type the same thing every day ("check my email," "what's the weather")
- **Bot-generated queries:** Automated triggers that produce identical query text
- **Template responses:** Queries from pre-built buttons or quick-reply options

### Fingerprint vs. intent cache

| Feature | Fingerprint (Tier 0) | Intent cache (Tier 2) |
|---|---|---|
| Match type | Exact text match | Action-target class match |
| Latency | <1ms | ~10ms |
| Handles variations | No | Yes |
| Requires ML | No | Yes (Tier 1-2 classification) |
| Hit rate | 20-40% (habitual users) | 60-80% (after learning) |
| Combined hit rate | 80-95% of all queries |

The two caches are complementary. Fingerprinting handles the easy cases instantly, and the intent cache handles the variations that exact matching misses.

### Populating the fingerprint cache

Fingerprint entries are created automatically:

1. A query goes through the full pipeline (Tiers 1-4)
2. If the response is successful and the query resolves to a cacheable plan
3. The normalized query hash is stored with its classification result
4. Next time the exact same query arrives, it resolves at Tier 0

The fingerprint cache is bounded in size (default: 10,000 entries) with LRU eviction. Frequently-used entries stay cached; rarely-used ones are evicted and re-classified on next use.
