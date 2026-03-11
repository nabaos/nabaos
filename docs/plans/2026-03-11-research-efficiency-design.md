# PEA Research Efficiency + Structured Output Hardening

> Date: 2026-03-11
> Status: Approved

## Problem

The PEA research phase uses 2 LLM calls that are expensive, slow, and unnecessary:
1. **Query generation** — LLM generates 15-20 search queries from objective (could be deterministic)
2. **Relevance scoring** — LLM scores ~200 candidates with fragile number-per-line parsing

Additionally, all structured LLM output across the pipeline uses optimistic text parsing (`extract_json()`, `line.parse::<f32>()`). Weaker/cheaper models that add any commentary break parsing silently.

## Scope

- **Part A:** Eliminate LLM from research phase (research.rs only)
- **Part B:** Add grammar-constrained decoding to provider infrastructure, apply to all composition phase structured outputs

## Design

### Part A: LLM-Free Research Phase

#### A1. Template-Based Query Expansion

Replace `generate_search_queries()` (line 362) with deterministic expansion:

```
Input: objective="survey of transformer efficiency techniques", task="..."

Step 1: Extract keywords via whitespace split + stopword removal
  → ["survey", "transformer", "efficiency", "techniques"]

Step 2: Apply templates:
  - Verbatim: "survey of transformer efficiency techniques"
  - Academic: "transformer efficiency research paper"
  - Academic2: "transformer efficiency survey peer-reviewed"
  - Recent: "transformer efficiency techniques 2026"
  - Data: "transformer efficiency benchmarks empirical data"
  - Review: "transformer efficiency literature review"
  - Specific: "transformer efficiency" (short, focused)
  - Broad: "efficient deep learning inference methods"
  - Expert: "transformer optimization expert analysis"
  ... up to 20 queries

Step 3: Domain-aware extras (if is_academic_objective):
  - Add "arxiv transformer efficiency"
  - Add "neural network efficiency meta-analysis"
```

No LLM, deterministic, repeatable, ~0ms.

Fallback: if `NABA_PEA_QUERY_MODE=llm`, use existing LLM generation (backward compat).

#### A2. Cascade Relevance Scoring

Replace `score_candidates()` (line 642) with configurable cascade:

**Tier 1 — Heuristic Score (always runs, ~1ms total):**
```
score = 0.0
+ 0.30 * keyword_overlap(objective_keywords, title + snippet)  // TF-IDF-lite
+ 0.25 * domain_authority(SourceTier::from_url)                 // Primary=1.0, Analytical=0.8, Reporting=0.5, Aggregator=0.2
+ 0.20 * recency_boost(url, snippet)                            // 2026=1.0, 2025=0.8, 2024=0.5, older=0.2
+ 0.15 * title_relevance(objective, title)                      // Jaccard similarity
+ 0.10 * source_diversity_bonus                                 // Bonus for underrepresented engines
```

For OpenAlex candidates, additionally:
```
+ citation_count_boost (log-scaled, capped at 0.15)
+ has_abstract_boost (0.05)
```

Eliminates bottom 50% of candidates.

**Tier 2 — BERT Re-ranking (if ort available):**
- Load sentence-transformer ONNX model (e.g., all-MiniLM-L6-v2, 80MB)
- Embed objective → 384-dim vector
- Embed each surviving candidate (title + first 100 chars snippet) → 384-dim
- Cosine similarity → re-rank
- ~5ms per candidate, ~500ms for 100 candidates

Uses existing `ort` + `tokenizers` dependencies (already default-enabled behind `bert` feature).

**Tier 3 — LLM Fallback:**
- If BERT model not found and `scoring_mode != heuristic_only`, fall through to existing LLM scoring
- Backward compatible

**Config:** `NABA_PEA_SCORING_MODE` = `heuristic_only` | `bert_rerank` | `cascade` (default) | `llm`

#### A3. Scoring Mode in ResearchConfig

```rust
pub enum ScoringMode {
    HeuristicOnly,   // Tier 1 only
    BertRerank,      // Tier 1 + Tier 2
    Cascade,         // Tier 1 + Tier 2 if available, else Tier 3
    Llm,             // Legacy LLM scoring
}

pub struct ResearchConfig {
    // ... existing fields ...
    pub scoring_mode: ScoringMode,
    pub bert_model_path: Option<PathBuf>,  // default: $NABA_DATA_DIR/models/minilm-l6.onnx
}
```

### Part B: Structured Output Hardening

#### B1. Provider Capability: `supports_structured_output`

Add to `ProviderDef`:
```rust
pub struct ProviderDef {
    // ... existing fields ...
    pub supports_structured_output: bool,
}
```

Set `true` for: OpenAI, Anthropic, Google Gemini, DeepSeek, Mistral, Groq.
Set `false` for: Ollama (varies by model), local providers.

#### B2. `response_format` Parameter in LLM Calls

Extend `complete()` and `llm.chat` ability to accept optional schema:

```rust
// In provider.rs
pub fn complete_structured(
    &self,
    system_prompt: &str,
    prompt: &str,
    schema: &serde_json::Value,  // JSON Schema
    max_tokens: Option<u32>,
) -> Result<String>;
```

Implementation per API format:
- **OpenAI-compatible:** Add `response_format: { "type": "json_schema", "json_schema": { "name": "...", "schema": ... } }` to request body
- **Anthropic:** Use single-tool pattern — define tool with input_schema matching desired output, force tool_choice
- **Unsupported providers:** Append schema description to system prompt, parse with `extract_json()` + serde validation

#### B3. `llm.chat` Ability Extension

Add optional `response_schema` parameter:
```json
{
    "prompt": "...",
    "system": "...",
    "response_schema": {
        "type": "object",
        "properties": {
            "scores": { "type": "array", "items": { "type": "number" } }
        },
        "required": ["scores"]
    }
}
```

When present and provider supports it → grammar-constrained. When present but unsupported → prompt augmentation + validation. When absent → current behavior.

#### B4. Apply Schemas to Composition Phase

Define schemas for each structured output call in composer.rs:

| Call | Schema |
|------|--------|
| `plan_structure()` | DocumentOutline with sections array |
| `verify_numerical_claims()` | Array of {section_id, claim, contradiction} |
| `reconcile_taxonomies()` | Array of {sections, conflict, resolution} |
| `enforce_evidence_gate()` | {title: string, content: string} |
| `nyaya_trim()` | {merges: [{absorb_id, into_id, reason, unique_claims}]} |
| `generate_charts()` | Array of {caption, python_script, data_type} |
| `review_document()` | Array of {section_id, issue, severity, fix} |

Each gets a `const SCHEMA_*: &str` in composer.rs and passes it through `response_schema`.

#### B5. Retry with Correction

When serde validation fails after extraction:
1. If provider supports structured output → this shouldn't happen, log error, return default
2. If prompt-based → retry once with: `"Your previous response was not valid JSON. Here is what you returned: {truncated}. Please output ONLY valid JSON matching this schema: {schema}"`
3. If retry also fails → return sensible default (existing fallback behavior)

## File Changes

| File | Changes |
|------|---------|
| `src/pea/research.rs` | A1: template query gen, A2: cascade scoring, A3: ScoringMode config |
| `src/providers/registry.rs` | B1: `supports_structured_output` field |
| `src/providers/catalog.rs` | B1: set flag per provider |
| `src/llm_router/provider.rs` | B2: `complete_structured()` method |
| `src/runtime/host_functions.rs` | B3: `response_schema` param in llm.chat |
| `src/pea/composer.rs` | B4: schema constants, B5: retry logic |
| New: `src/pea/heuristic_scorer.rs` | A2 Tier 1: keyword overlap, domain authority, recency |
| New: `src/pea/bert_reranker.rs` | A2 Tier 2: ONNX sentence-transformer re-ranking |

## Model File

BERT re-ranker needs `all-MiniLM-L6-v2.onnx` (~80MB). Download on first use:
```
$NABA_DATA_DIR/models/minilm-l6-v2.onnx
$NABA_DATA_DIR/models/minilm-l6-v2-tokenizer.json
```

Auto-download from HuggingFace Hub if not present (same pattern as existing BERT classifier model download in security module).

## Performance Impact

| Metric | Before | After (cascade) |
|--------|--------|-----------------|
| LLM calls (research) | 2 | 0 (cascade mode) |
| Research phase latency | ~15-30s | ~2-3s |
| Token cost (research) | ~5K tokens | 0 |
| Scoring quality | Model-dependent | Consistent across runs |
| Structured output failures | Silent fallback to 0.5 | Schema-enforced or validated retry |
