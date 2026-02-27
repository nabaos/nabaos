# W5H2 Classification

> **What you'll learn**
>
> - The W5H2 framework and how it simplifies intent classification to Action + Target
> - All 11 actions and 30 targets with descriptions and examples
> - How classification maps to constitution rule matching
> - How the SetFit ONNX model works
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

1. BERT classifies: action=send, target=email

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

## SetFit ONNX Model

### What is SetFit?

SetFit (Sentence Transformer Fine-Tuning) is a few-shot learning framework that produces high-accuracy text classifiers from small amounts of training data. Unlike traditional fine-tuning that requires thousands of examples per class, SetFit can achieve strong performance with as few as 8 examples per class.

### How the model is trained

The NabaOS W5H2 classifier is trained on the 54 intent classes defined in `W5H2_CLASSES`:

```
Training data per class: 8-16 example queries
Total classes: 54 (subset of 11 actions x 30 targets)
Model architecture: Sentence transformer (all-MiniLM-L6-v2 base)
Export format: ONNX (for fast inference without Python)
Model size: ~23MB
```

Example training data for the `check_email` class:

```
"Check my email"
"Do I have any new messages?"
"Show me my inbox"
"Any emails from Alice?"
"What's in my email?"
"Read my latest emails"
"Open my mail"
"Check for new email notifications"
```

### Inference pipeline

```
Query text
    |
    v
Tokenizer (WordPiece)
    |
    v
Sentence Transformer (ONNX)
    |
    v
Embedding vector (384 dimensions)
    |
    v
Classification head
    |
    v
Predicted class + confidence score
    |
    v
Parse into (Action, Target) pair

Example:
  Input:  "Can you check if I got any new emails?"
  Output: check_email (confidence: 0.96)
  Parsed: Action::Check, Target::Email
```

### Accuracy

Based on validation benchmarks:

| Metric | Score |
|---|---|
| Top-1 accuracy | 92-95% |
| Top-3 accuracy | 98%+ |
| Average confidence (correct) | 0.94 |
| Average confidence (incorrect) | 0.61 |

The confidence gap between correct and incorrect predictions makes it possible to set a threshold (default: 0.70) below which the system falls back to LLM-based classification at Tier 3.

### Why ONNX?

The model is exported to ONNX format for several reasons:

- **No Python dependency:** The Rust runtime uses the `ort` crate (ONNX Runtime bindings) for inference. No Python interpreter needed.
- **Fast inference:** ONNX Runtime is optimized for inference with hardware acceleration support.
- **Small footprint:** The 23MB model fits comfortably on any machine.
- **Deterministic:** Same input always produces same output, unlike LLM-based classification.

---

## Fingerprinting: Fast Exact-Match Before ML

Before the BERT/SetFit classifier runs, the fingerprint cache performs an exact-match lookup. This is the fastest possible classification path.

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
| Requires ML | No | Yes (Tier 1 classification) |
| Hit rate | 20-40% (habitual users) | 60-80% (after learning) |
| Combined hit rate | 80-95% of all queries |

The two caches are complementary. Fingerprinting handles the easy cases instantly, and the intent cache handles the variations that exact matching misses.

### Populating the fingerprint cache

Fingerprint entries are created automatically:

1. A query goes through the full pipeline (Tiers 1-3)
2. If the response is successful and the query resolves to a cacheable plan
3. The normalized query hash is stored with its classification result
4. Next time the exact same query arrives, it resolves at Tier 0

The fingerprint cache is bounded in size (default: 10,000 entries) with LRU eviction. Frequently-used entries stay cached; rarely-used ones are evicted and re-classified on next use.
