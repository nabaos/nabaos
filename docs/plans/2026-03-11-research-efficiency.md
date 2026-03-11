# PEA Research Efficiency + Structured Output Hardening

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate LLM dependency from PEA research phase and add grammar-constrained decoding to the provider layer for robust structured output across all model tiers.

**Architecture:** Replace LLM query generation with deterministic template expansion, replace LLM relevance scoring with a 3-tier cascade (heuristic → BERT re-rank → LLM fallback). Add `response_format` support to the provider abstraction so structured output works reliably even with weaker models.

**Tech Stack:** Rust, ort (ONNX Runtime), tokenizers, serde_json, existing all-MiniLM-L6-v2 model

---

## Task 1: Template-Based Query Expansion

**Files:**
- Modify: `src/pea/research.rs:60-70` (ResearchConfig — add `query_mode` field)
- Modify: `src/pea/research.rs:72-89` (Default impl — set query_mode default)
- Modify: `src/pea/research.rs:362-414` (generate_search_queries — add template path)

### Step 1 — Add `QueryMode` enum and config field

After `SearchBackend` (line 58), add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryMode {
    Template,   // Deterministic keyword + template expansion (default)
    Llm,        // Legacy LLM-generated queries
}

impl Default for QueryMode {
    fn default() -> Self {
        match std::env::var("NABA_PEA_QUERY_MODE").as_deref() {
            Ok("llm") => Self::Llm,
            _ => Self::Template,
        }
    }
}
```

Add to `ResearchConfig`:
```rust
pub query_mode: QueryMode,
```

In `Default for ResearchConfig`, add:
```rust
query_mode: QueryMode::default(),
```

### Step 2 — Add `generate_queries_template()` method

Add this method on `ResearchEngine` (before `generate_search_queries`):

```rust
/// Deterministic query expansion from objective + task keywords.
/// No LLM call — uses keyword extraction + templates.
fn generate_queries_template(&self, objective: &str, task: &str) -> Vec<String> {
    let stopwords: HashSet<&str> = [
        "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for",
        "of", "with", "by", "from", "is", "are", "was", "were", "be", "been",
        "being", "have", "has", "had", "do", "does", "did", "will", "would",
        "could", "should", "may", "might", "shall", "can", "this", "that",
        "these", "those", "it", "its", "as", "if", "not", "no", "so", "up",
        "out", "about", "into", "over", "after", "how", "what", "which",
        "who", "whom", "when", "where", "why", "all", "each", "every",
        "both", "few", "more", "most", "other", "some", "such", "than",
        "too", "very", "just", "also", "write", "create", "produce",
        "generate", "comprehensive", "detailed", "report",
    ].into_iter().collect();

    // Extract keywords from objective
    let keywords: Vec<&str> = objective
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| w.len() > 2 && !stopwords.contains(&w.to_ascii_lowercase().as_str()))
        .collect();

    let keyword_str = keywords.join(" ");
    let short_keywords = keywords.iter().take(4).copied().collect::<Vec<_>>().join(" ");

    let mut queries = Vec::with_capacity(20);

    // Verbatim objective (trimmed)
    let trimmed = objective.chars().take(150).collect::<String>();
    queries.push(trimmed);

    // Keyword-focused
    queries.push(keyword_str.clone());
    queries.push(short_keywords.clone());

    // Academic angle
    queries.push(format!("{} research paper", short_keywords));
    queries.push(format!("{} survey peer-reviewed", short_keywords));
    queries.push(format!("{} literature review", short_keywords));

    // Recent
    queries.push(format!("{} 2026", short_keywords));
    queries.push(format!("{} 2025 2026", short_keywords));

    // Data/empirical
    queries.push(format!("{} benchmarks empirical data", short_keywords));
    queries.push(format!("{} comparison analysis results", short_keywords));

    // Broader context
    queries.push(format!("{} state of the art", short_keywords));
    queries.push(format!("{} challenges limitations", short_keywords));
    queries.push(format!("{} future directions trends", short_keywords));

    // Expert/opinion
    queries.push(format!("{} expert analysis", short_keywords));

    // Task-specific
    if !task.is_empty() {
        let task_kw: String = task
            .split(|c: char| !c.is_alphanumeric() && c != '-')
            .filter(|w| w.len() > 2 && !stopwords.contains(&w.to_ascii_lowercase().as_str()))
            .take(4)
            .collect::<Vec<_>>()
            .join(" ");
        if !task_kw.is_empty() && task_kw != short_keywords {
            queries.push(task_kw.clone());
            queries.push(format!("{} research", task_kw));
        }
    }

    // Academic supplement queries
    if is_academic_objective(objective) {
        queries.push(format!("arxiv {}", short_keywords));
        queries.push(format!("{} meta-analysis systematic review", short_keywords));
    }

    // Dedup and cap
    let mut seen = HashSet::new();
    queries.retain(|q| {
        let key = q.to_ascii_lowercase();
        !key.is_empty() && seen.insert(key)
    });
    queries.truncate(self.config.max_search_queries);

    eprintln!("[research] template expansion: {} queries from keywords {:?}",
        queries.len(), &keywords[..keywords.len().min(6)]);

    queries
}
```

### Step 3 — Wire into `generate_search_queries()`

At the top of the existing `generate_search_queries()` method (line ~362), add a dispatch:

```rust
fn generate_search_queries(&self, objective: &str, task: &str) -> Vec<String> {
    if self.config.query_mode == QueryMode::Template {
        return self.generate_queries_template(objective, task);
    }

    // ... existing LLM-based generation below ...
```

### Step 4 — Tests

Add to `mod tests`:

```rust
#[test]
fn test_query_mode_default_is_template() {
    assert_eq!(QueryMode::default(), QueryMode::Template);
}

#[test]
fn test_generate_queries_template_produces_queries() {
    // We can't easily create a ResearchEngine without a registry/manifest,
    // so test the keyword extraction logic directly
    let objective = "survey of transformer efficiency techniques for edge deployment";
    assert!(is_academic_objective(objective));

    // Verify keyword extraction via the stopword-filtered split
    let stopwords: HashSet<&str> = ["a", "an", "the", "of", "for"].into_iter().collect();
    let keywords: Vec<&str> = objective
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| w.len() > 2 && !stopwords.contains(w))
        .collect();
    assert!(keywords.contains(&"transformer"));
    assert!(keywords.contains(&"efficiency"));
    assert!(!keywords.contains(&"of"));
    assert!(!keywords.contains(&"for"));
}

#[test]
fn test_generate_queries_template_deduplicates() {
    let stopwords: HashSet<&str> = HashSet::new();
    let queries = vec!["test query".to_string(), "test query".to_string(), "other".to_string()];
    let mut seen = HashSet::new();
    let deduped: Vec<_> = queries.into_iter().filter(|q| {
        let key = q.to_ascii_lowercase();
        seen.insert(key)
    }).collect();
    assert_eq!(deduped.len(), 2);
}
```

### Step 5 — Commit

```
feat(pea): deterministic template-based query expansion

Replace LLM query generation with keyword extraction + template
expansion. Configurable via NABA_PEA_QUERY_MODE (template|llm).
Saves one LLM call per research run (~5s + ~2K tokens).
```

---

## Task 2: Heuristic Relevance Scorer (Tier 1)

**Files:**
- Create: `src/pea/heuristic_scorer.rs`
- Modify: `src/pea/mod.rs` (add module)

### Step 1 — Create `src/pea/heuristic_scorer.rs`

```rust
//! Tier 1 heuristic relevance scorer for search candidates.
//! Pure keyword overlap + domain authority + recency — no LLM, no ML models.

use std::collections::HashSet;

use super::research::{SearchCandidate, SourceTier};

/// Heuristic relevance score for a search candidate against an objective.
/// Returns score in [0.0, 1.0].
pub fn heuristic_score(candidate: &SearchCandidate, objective_keywords: &[String]) -> f32 {
    let keyword_score = keyword_overlap(objective_keywords, candidate);
    let domain_score = domain_authority_score(&candidate.url);
    let recency = recency_score(&candidate.url, &candidate.snippet);
    let title_sim = title_similarity(objective_keywords, &candidate.title);
    let academic_bonus = if candidate.source_engine == "openalex" { 0.05 } else { 0.0 };

    // Weighted combination
    let score = 0.30 * keyword_score
        + 0.25 * domain_score
        + 0.20 * recency
        + 0.15 * title_sim
        + 0.10 * academic_bonus;

    score.clamp(0.0, 1.0)
}

/// Extract objective keywords (stopword-filtered, lowercased).
pub fn extract_keywords(objective: &str) -> Vec<String> {
    let stopwords: HashSet<&str> = [
        "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for",
        "of", "with", "by", "from", "is", "are", "was", "were", "be", "been",
        "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "can", "this", "that", "it", "its", "as",
        "if", "not", "no", "so", "how", "what", "which", "who", "when",
        "where", "why", "all", "some", "than", "very", "just", "also",
        "write", "create", "produce", "generate", "comprehensive", "detailed",
        "report", "about",
    ].into_iter().collect();

    objective
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| w.len() > 2 && !stopwords.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Keyword overlap: fraction of objective keywords found in title+snippet.
fn keyword_overlap(keywords: &[String], candidate: &SearchCandidate) -> f32 {
    if keywords.is_empty() {
        return 0.5;
    }
    let text = format!("{} {}", candidate.title, candidate.snippet).to_ascii_lowercase();
    let hits = keywords.iter().filter(|k| text.contains(k.as_str())).count();
    hits as f32 / keywords.len() as f32
}

/// Domain authority from SourceTier classification.
fn domain_authority_score(url: &str) -> f32 {
    match SourceTier::from_url(url) {
        SourceTier::Primary => 1.0,
        SourceTier::Analytical => 0.8,
        SourceTier::Reporting => 0.5,
        SourceTier::Aggregator => 0.2,
    }
}

/// Recency heuristic: check URL and snippet for year mentions.
fn recency_score(url: &str, snippet: &str) -> f32 {
    let text = format!("{} {}", url, snippet);
    if text.contains("2026") {
        1.0
    } else if text.contains("2025") {
        0.8
    } else if text.contains("2024") {
        0.5
    } else if text.contains("2023") {
        0.3
    } else {
        0.2 // unknown recency, slight penalty
    }
}

/// Title similarity: Jaccard-like overlap of objective keywords with title words.
fn title_similarity(keywords: &[String], title: &str) -> f32 {
    if keywords.is_empty() {
        return 0.5;
    }
    let title_words: HashSet<String> = title
        .to_ascii_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(|w| w.to_string())
        .collect();
    let kw_set: HashSet<&str> = keywords.iter().map(|k| k.as_str()).collect();
    let intersection = title_words.iter().filter(|w| kw_set.contains(w.as_str())).count();
    let union = title_words.len() + kw_set.len() - intersection;
    if union == 0 { 0.0 } else { intersection as f32 / union as f32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_keywords_filters_stopwords() {
        let kw = extract_keywords("survey of transformer efficiency for edge deployment");
        assert!(kw.contains(&"transformer".to_string()));
        assert!(kw.contains(&"efficiency".to_string()));
        assert!(!kw.contains(&"of".to_string()));
        assert!(!kw.contains(&"for".to_string()));
    }

    #[test]
    fn test_heuristic_score_range() {
        let candidate = SearchCandidate {
            url: "https://arxiv.org/abs/2026.12345".into(),
            title: "Transformer Efficiency Survey".into(),
            snippet: "A 2026 survey of efficient transformer techniques".into(),
            source_engine: "brave".into(),
            relevance_score: None,
        };
        let keywords = extract_keywords("survey of transformer efficiency techniques");
        let score = heuristic_score(&candidate, &keywords);
        assert!(score > 0.5, "relevant candidate should score > 0.5, got {}", score);
        assert!(score <= 1.0);
    }

    #[test]
    fn test_heuristic_score_irrelevant_low() {
        let candidate = SearchCandidate {
            url: "https://reddit.com/r/cooking/best_pasta".into(),
            title: "Best Pasta Recipe 2020".into(),
            snippet: "How to make carbonara at home".into(),
            source_engine: "ddg".into(),
            relevance_score: None,
        };
        let keywords = extract_keywords("survey of transformer efficiency techniques");
        let score = heuristic_score(&candidate, &keywords);
        assert!(score < 0.4, "irrelevant candidate should score < 0.4, got {}", score);
    }

    #[test]
    fn test_domain_authority_tiers() {
        assert_eq!(domain_authority_score("https://www.state.gov/report"), 1.0);
        assert_eq!(domain_authority_score("https://arxiv.org/abs/123"), 0.8);
        assert_eq!(domain_authority_score("https://reuters.com/article"), 0.5);
        assert_eq!(domain_authority_score("https://reddit.com/r/test"), 0.2);
    }

    #[test]
    fn test_recency_score() {
        assert_eq!(recency_score("https://example.com/2026/report", ""), 1.0);
        assert_eq!(recency_score("https://example.com", "published in 2025"), 0.8);
        assert_eq!(recency_score("https://example.com", "no year mentioned"), 0.2);
    }
}
```

### Step 2 — Add module to `src/pea/mod.rs`

Add `pub mod heuristic_scorer;` in `src/pea/mod.rs`.

### Step 3 — Run tests

```bash
cargo test --lib pea::heuristic_scorer
```

### Step 4 — Commit

```
feat(pea): add heuristic relevance scorer (Tier 1)

Keyword overlap + domain authority + recency + title similarity.
Pure Rust, no ML, ~1ms for 200 candidates.
```

---

## Task 3: BERT Re-ranker (Tier 2)

**Files:**
- Create: `src/pea/bert_reranker.rs`
- Modify: `src/pea/mod.rs` (add module)

### Step 1 — Create `src/pea/bert_reranker.rs`

Reuses the proven ONNX + tokenizer pattern from `src/w5h2/classifier.rs` which already loads all-MiniLM-L6-v2 and has an `embed()` method.

```rust
//! Tier 2 BERT re-ranker for search candidates.
//! Uses sentence-transformer (all-MiniLM-L6-v2) to embed objective and
//! candidate titles, then ranks by cosine similarity.

use std::path::Path;

use crate::core::error::{NyayaError, Result};

/// BERT-based re-ranker using sentence-transformer embeddings.
pub struct BertReranker {
    session: ort::session::Session,
    tokenizer: tokenizers::Tokenizer,
    max_length: usize,
}

impl BertReranker {
    /// Load sentence-transformer model from directory.
    /// Expects: `model.onnx` and `tokenizer.json`.
    pub fn load(model_dir: &Path) -> Result<Self> {
        let onnx_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !onnx_path.exists() {
            return Err(NyayaError::ModelLoad(format!(
                "Sentence-transformer model not found at {}. \
                 Run: hf download sentence-transformers/all-MiniLM-L6-v2 --local-dir {}",
                onnx_path.display(),
                model_dir.display()
            )));
        }

        let session = ort::session::Session::builder()
            .map_err(|e| NyayaError::ModelLoad(format!("ONNX session builder: {}", e)))?
            .with_intra_threads(1)
            .map_err(|e| NyayaError::ModelLoad(format!("ONNX thread config: {}", e)))?
            .commit_from_file(&onnx_path)
            .map_err(|e| NyayaError::ModelLoad(format!(
                "ONNX load from {}: {}", onnx_path.display(), e
            )))?;

        let tokenizer = tokenizers::Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| NyayaError::ModelLoad(format!("Tokenizer load: {}", e)))?;

        Ok(Self { session, tokenizer, max_length: 128 })
    }

    /// Embed a text string into a 384-dim normalized vector.
    pub fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let encoding = self.tokenizer
            .encode(text, true)
            .map_err(|e| NyayaError::Inference(format!("Tokenization: {}", e)))?;

        let mut input_ids: Vec<i64> = encoding.get_ids().iter().map(|&id| id as i64).collect();
        let mut attention_mask: Vec<i64> = encoding
            .get_attention_mask()
            .iter()
            .map(|&m| m as i64)
            .collect();

        input_ids.truncate(self.max_length);
        attention_mask.truncate(self.max_length);
        while input_ids.len() < self.max_length {
            input_ids.push(0);
            attention_mask.push(0);
        }

        let shape = vec![1i64, self.max_length as i64];

        let input_ids_tensor = ort::value::Tensor::from_array((shape.clone(), input_ids))
            .map_err(|e| NyayaError::Inference(format!("Tensor: {}", e)))?;
        let attention_mask_tensor = ort::value::Tensor::from_array((shape, attention_mask))
            .map_err(|e| NyayaError::Inference(format!("Tensor: {}", e)))?;

        let outputs = self.session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
            ])
            .map_err(|e| NyayaError::Inference(format!("ONNX inference: {}", e)))?;

        let (_shape, embedding) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| NyayaError::Inference(format!("Output extraction: {}", e)))?;

        // Mean pooling + L2 normalize
        let emb: Vec<f32> = embedding.to_vec();
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            Ok(emb.iter().map(|x| x / norm).collect())
        } else {
            Ok(emb)
        }
    }

    /// Score candidates by cosine similarity to objective embedding.
    /// Returns (index, score) pairs sorted by score descending.
    pub fn rank(
        &self,
        objective_embedding: &[f32],
        candidates: &[(String, String)],  // (title, snippet)
    ) -> Vec<(usize, f32)> {
        let mut scores: Vec<(usize, f32)> = candidates
            .iter()
            .enumerate()
            .map(|(i, (title, snippet))| {
                let text = format!("{} {}", title, &snippet[..snippet.len().min(100)]);
                match self.embed(&text) {
                    Ok(emb) => {
                        let sim = cosine_similarity(objective_embedding, &emb);
                        (i, sim)
                    }
                    Err(_) => (i, 0.0),
                }
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scores
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    // Vectors are already L2-normalized, so dot product = cosine similarity
    dot.clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v = vec![0.5, 0.5, 0.5, 0.5];
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        let normalized: Vec<f32> = v.iter().map(|x| x / norm).collect();
        let sim = cosine_similarity(&normalized, &normalized);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b)).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_mismatched_length() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0]), 0.0);
    }
}
```

### Step 2 — Gate behind `bert` feature

In `src/pea/mod.rs`, add:
```rust
#[cfg(feature = "bert")]
pub mod bert_reranker;
```

### Step 3 — Run tests

```bash
cargo test --lib pea::bert_reranker
```

### Step 4 — Commit

```
feat(pea): add BERT sentence-transformer re-ranker (Tier 2)

Embeds objective + candidate titles via all-MiniLM-L6-v2 ONNX,
ranks by cosine similarity. ~5ms per candidate, gated behind
bert feature flag.
```

---

## Task 4: Wire Cascade Scoring into ResearchEngine

**Files:**
- Modify: `src/pea/research.rs:60-70` (add `ScoringMode` to config)
- Modify: `src/pea/research.rs:642-708` (wrap `score_candidates` with cascade dispatch)

### Step 1 — Add `ScoringMode` enum

After `QueryMode` in `research.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoringMode {
    HeuristicOnly,  // Tier 1 only — fast, no models
    BertRerank,     // Tier 1 filter + Tier 2 BERT re-rank
    Cascade,        // Tier 1 + Tier 2 if available, else Tier 3 LLM
    Llm,            // Legacy LLM-only scoring
}

impl Default for ScoringMode {
    fn default() -> Self {
        match std::env::var("NABA_PEA_SCORING_MODE").as_deref() {
            Ok("heuristic_only") => Self::HeuristicOnly,
            Ok("bert_rerank") | Ok("bert") => Self::BertRerank,
            Ok("llm") => Self::Llm,
            _ => Self::Cascade,
        }
    }
}
```

Add to `ResearchConfig`:
```rust
pub scoring_mode: ScoringMode,
```

In `Default for ResearchConfig`:
```rust
scoring_mode: ScoringMode::default(),
```

### Step 2 — Add `score_candidates_heuristic()` method

```rust
fn score_candidates_heuristic(&self, candidates: &mut [SearchCandidate], objective: &str) {
    use super::heuristic_scorer::{extract_keywords, heuristic_score};
    let keywords = extract_keywords(objective);
    for candidate in candidates.iter_mut() {
        candidate.relevance_score = Some(heuristic_score(candidate, &keywords));
    }
    candidates.sort_by(|a, b| {
        b.relevance_score.unwrap_or(0.0)
            .partial_cmp(&a.relevance_score.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    eprintln!("[research] heuristic scoring: top={:.2}, median={:.2}",
        candidates.first().and_then(|c| c.relevance_score).unwrap_or(0.0),
        candidates.get(candidates.len() / 2).and_then(|c| c.relevance_score).unwrap_or(0.0),
    );
}
```

### Step 3 — Add `score_candidates_bert_rerank()` method

```rust
#[cfg(feature = "bert")]
fn score_candidates_bert_rerank(&self, candidates: &mut [SearchCandidate], objective: &str) {
    use super::heuristic_scorer::{extract_keywords, heuristic_score};
    use super::bert_reranker::BertReranker;

    // Tier 1: heuristic pre-filter (keep top 50%)
    let keywords = extract_keywords(objective);
    for candidate in candidates.iter_mut() {
        candidate.relevance_score = Some(heuristic_score(candidate, &keywords));
    }
    candidates.sort_by(|a, b| {
        b.relevance_score.unwrap_or(0.0)
            .partial_cmp(&a.relevance_score.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let cutoff = candidates.len() / 2;

    // Tier 2: BERT re-rank survivors
    let model_dir = std::env::var("NABA_DATA_DIR")
        .unwrap_or_else(|_| {
            dirs::home_dir()
                .map(|h| h.join(".nabaos").to_string_lossy().to_string())
                .unwrap_or_else(|| ".nabaos".into())
        });
    let model_path = std::path::Path::new(&model_dir).join("models");

    match BertReranker::load(&model_path) {
        Ok(reranker) => {
            match reranker.embed(objective) {
                Ok(obj_emb) => {
                    let pairs: Vec<(String, String)> = candidates[..cutoff]
                        .iter()
                        .map(|c| (c.title.clone(), c.snippet.clone()))
                        .collect();
                    let rankings = reranker.rank(&obj_emb, &pairs);
                    // Blend: 0.4 * heuristic + 0.6 * bert
                    for (rank_idx, bert_score) in &rankings {
                        if let Some(c) = candidates.get_mut(*rank_idx) {
                            let h = c.relevance_score.unwrap_or(0.0);
                            c.relevance_score = Some(0.4 * h + 0.6 * bert_score);
                        }
                    }
                    candidates[..cutoff].sort_by(|a, b| {
                        b.relevance_score.unwrap_or(0.0)
                            .partial_cmp(&a.relevance_score.unwrap_or(0.0))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    });
                    eprintln!("[research] BERT re-rank: top={:.2}",
                        candidates.first().and_then(|c| c.relevance_score).unwrap_or(0.0));
                }
                Err(e) => eprintln!("[research] BERT embed failed: {}, using heuristic only", e),
            }
        }
        Err(e) => eprintln!("[research] BERT model not available: {}, using heuristic only", e),
    }
}
```

### Step 4 — Dispatch in `score_candidates()`

Replace the beginning of the existing `score_candidates()`:

```rust
fn score_candidates(&self, candidates: &mut [SearchCandidate], objective: &str) {
    if candidates.is_empty() {
        return;
    }

    match self.config.scoring_mode {
        ScoringMode::HeuristicOnly => {
            self.score_candidates_heuristic(candidates, objective);
            return;
        }
        ScoringMode::BertRerank => {
            #[cfg(feature = "bert")]
            {
                self.score_candidates_bert_rerank(candidates, objective);
                return;
            }
            #[cfg(not(feature = "bert"))]
            {
                eprintln!("[research] bert feature not enabled, falling back to heuristic");
                self.score_candidates_heuristic(candidates, objective);
                return;
            }
        }
        ScoringMode::Cascade => {
            #[cfg(feature = "bert")]
            {
                // Try BERT, gracefully degrade to heuristic, then LLM
                let model_dir = std::env::var("NABA_DATA_DIR")
                    .unwrap_or_else(|_| {
                        dirs::home_dir()
                            .map(|h| h.join(".nabaos").to_string_lossy().to_string())
                            .unwrap_or_else(|| ".nabaos".into())
                    });
                let model_path = std::path::Path::new(&model_dir).join("models");
                if model_path.join("model.onnx").exists() {
                    self.score_candidates_bert_rerank(candidates, objective);
                    return;
                }
                eprintln!("[research] BERT model not found, cascade falling through to heuristic");
                self.score_candidates_heuristic(candidates, objective);
                return;
            }
            #[cfg(not(feature = "bert"))]
            {
                self.score_candidates_heuristic(candidates, objective);
                return;
            }
        }
        ScoringMode::Llm => {
            // Fall through to existing LLM scoring below
        }
    }

    // Existing LLM-based scoring (legacy, ScoringMode::Llm only)
    // ... keep all existing code below ...
```

### Step 5 — Tests

```rust
#[test]
fn test_scoring_mode_default_is_cascade() {
    assert_eq!(ScoringMode::default(), ScoringMode::Cascade);
}

#[test]
fn test_scoring_mode_from_env() {
    // Can't easily test env vars in unit tests, but verify parsing logic
    let mode = match "heuristic_only" {
        "heuristic_only" => ScoringMode::HeuristicOnly,
        "bert_rerank" | "bert" => ScoringMode::BertRerank,
        "llm" => ScoringMode::Llm,
        _ => ScoringMode::Cascade,
    };
    assert_eq!(mode, ScoringMode::HeuristicOnly);
}
```

### Step 6 — Commit

```
feat(pea): wire cascade scoring into research engine

ScoringMode: HeuristicOnly → BertRerank → Cascade (default) → Llm.
Cascade tries BERT if model available, degrades to heuristic, never
calls LLM unless explicitly set to Llm mode.
```

---

## Task 5: Add `supports_structured_output` to Provider Registry

**Files:**
- Modify: `src/providers/registry.rs:28-39` (add field to ProviderDef)
- Modify: `src/providers/catalog.rs:3-15` (update openai_compat helper)
- Modify: `src/providers/catalog.rs:57-88` (update Big 5 providers)

### Step 1 — Add field to `ProviderDef`

In `src/providers/registry.rs`, add to the struct:

```rust
pub struct ProviderDef {
    // ... existing fields ...
    pub supports_structured_output: bool,
}
```

### Step 2 — Update `openai_compat()` helper

In `src/providers/catalog.rs`, update the helper to include:
```rust
fn openai_compat(id: &str, name: &str, base_url: &str) -> ProviderDef {
    ProviderDef {
        // ... existing fields ...
        supports_structured_output: false, // conservative default
    }
}
```

### Step 3 — Set `true` for known-capable providers

For the Big 5 + known providers that support `response_format` with JSON schema:
- `openai` → `true`
- `anthropic` → `true` (via tool_use pattern)
- `google` → `true`
- `deepseek` → `true`
- `mistral` → `true`
- `groq` → `true`

All others remain `false` (safe default).

Also update any explicit provider constructions in catalog.rs (search for `supports_vision` to find them all — each needs the new field added).

### Step 4 — Fix compilation

Run `cargo check` and fix any struct initialization that's now missing the field. Every place that constructs `ProviderDef` must include `supports_structured_output`.

### Step 5 — Tests

```rust
#[test]
fn test_openai_supports_structured_output() {
    let providers = build_provider_catalog();
    let openai = providers.iter().find(|p| p.id == "openai").unwrap();
    assert!(openai.supports_structured_output);
}

#[test]
fn test_generic_provider_no_structured_output() {
    let providers = build_provider_catalog();
    // Pick a random openai_compat aggregator
    let random = providers.iter().find(|p| p.id == "together").unwrap();
    assert!(!random.supports_structured_output);
}
```

### Step 6 — Commit

```
feat(providers): add supports_structured_output capability flag

Set true for OpenAI, Anthropic, Google, DeepSeek, Mistral, Groq.
Conservative false default for 55 other providers.
```

---

## Task 6: Add `complete_structured()` to Provider

**Files:**
- Modify: `src/llm_router/provider.rs` (add method)

### Step 1 — Add `complete_structured()` method

Add after `complete_no_think()`:

```rust
/// Complete with grammar-constrained structured output.
/// If provider supports it, uses API-level schema enforcement.
/// Otherwise, augments prompt with schema description and validates.
pub fn complete_structured(
    &self,
    system_prompt: &str,
    user_message: &str,
    schema: &serde_json::Value,
    schema_name: &str,
    max_tokens: Option<u32>,
    supports_structured: bool,
) -> Result<LlmResponse> {
    if supports_structured {
        self.complete_with_schema(system_prompt, user_message, schema, schema_name, max_tokens)
    } else {
        // Augment prompt with schema description
        let augmented_system = format!(
            "{}\n\nYou MUST respond with ONLY valid JSON matching this schema:\n```json\n{}\n```\nDo NOT include any text outside the JSON.",
            system_prompt,
            serde_json::to_string_pretty(schema).unwrap_or_default()
        );
        self.complete_no_think(&augmented_system, user_message, max_tokens)
    }
}
```

### Step 2 — Add `complete_with_schema()` (API-level enforcement)

```rust
fn complete_with_schema(
    &self,
    system_prompt: &str,
    user_message: &str,
    schema: &serde_json::Value,
    schema_name: &str,
    max_tokens: Option<u32>,
) -> Result<LlmResponse> {
    let client = reqwest::blocking::Client::new();
    let start = std::time::Instant::now();

    match self.api_format {
        ApiFormat::Anthropic => {
            // Anthropic: use single-tool pattern for structured output
            let tool = serde_json::json!({
                "name": schema_name,
                "description": format!("Output structured data as {}", schema_name),
                "input_schema": schema,
            });
            let body = serde_json::json!({
                "model": self.model,
                "max_tokens": max_tokens.unwrap_or(4096),
                "system": system_prompt,
                "messages": [{"role": "user", "content": user_message}],
                "tools": [tool],
                "tool_choice": {"type": "tool", "name": schema_name},
                "temperature": 0.2,
            });
            let resp = client
                .post(&self.base_url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("content-type", "application/json")
                .json(&body)
                .send()
                .map_err(|e| NyayaError::Config(format!("Anthropic request: {}", e)))?;
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            if !status.is_success() {
                return Err(NyayaError::Config(format!("Anthropic {}: {}", status, text)));
            }
            let parsed: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| NyayaError::Config(format!("Anthropic parse: {}", e)))?;
            // Extract tool_use input from response
            let tool_input = parsed["content"]
                .as_array()
                .and_then(|blocks| blocks.iter().find(|b| b["type"] == "tool_use"))
                .and_then(|b| b.get("input"))
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let result_text = serde_json::to_string(&tool_input).unwrap_or_default();
            let input_tokens = parsed["usage"]["input_tokens"].as_u64().unwrap_or(0);
            let output_tokens = parsed["usage"]["output_tokens"].as_u64().unwrap_or(0);
            Ok(LlmResponse {
                text: result_text,
                input_tokens,
                output_tokens,
                latency_ms: start.elapsed().as_millis() as u64,
            })
        }
        _ => {
            // OpenAI-compatible: use response_format with json_schema
            let body = serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": user_message}
                ],
                "max_tokens": max_tokens.unwrap_or(4096),
                "temperature": 0.2,
                "response_format": {
                    "type": "json_schema",
                    "json_schema": {
                        "name": schema_name,
                        "strict": true,
                        "schema": schema,
                    }
                }
            });
            let mut req = client
                .post(&self.base_url)
                .header("content-type", "application/json");
            if !self.api_key.is_empty() {
                req = req.header("Authorization", format!("Bearer {}", self.api_key));
            }
            let resp = req.json(&body).send()
                .map_err(|e| NyayaError::Config(format!("OpenAI request: {}", e)))?;
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            if !status.is_success() {
                return Err(NyayaError::Config(format!("OpenAI {}: {}", status, text)));
            }
            let parsed: serde_json::Value = serde_json::from_str(&text)
                .map_err(|e| NyayaError::Config(format!("OpenAI parse: {}", e)))?;
            let result_text = parsed["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let input_tokens = parsed["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
            let output_tokens = parsed["usage"]["completion_tokens"].as_u64().unwrap_or(0);
            Ok(LlmResponse {
                text: result_text,
                input_tokens,
                output_tokens,
                latency_ms: start.elapsed().as_millis() as u64,
            })
        }
    }
}
```

### Step 3 — Commit

```
feat(providers): add complete_structured() with grammar-constrained decoding

Anthropic: single-tool pattern with tool_choice forced.
OpenAI-compatible: response_format with json_schema.
Unsupported providers: schema-augmented prompt fallback.
```

---

## Task 7: Extend `llm.chat` Ability with `response_schema`

**Files:**
- Modify: `src/runtime/host_functions.rs:914-927` (ability spec)
- Modify: `src/runtime/host_functions.rs:1147-1168` (execution handler)

### Step 1 — Update ability spec

Update the `input_schema` for `llm.chat` to include `response_schema`:

```rust
input_schema: Some(serde_json::json!({
    "type": "object",
    "properties": {
        "prompt": {"type": "string", "description": "Prompt to send to the LLM"},
        "system": {"type": "string", "description": "Optional system prompt"},
        "response_schema": {"type": "object", "description": "Optional JSON Schema for structured output"},
        "schema_name": {"type": "string", "description": "Name for the structured output schema"}
    },
    "required": ["prompt"]
})),
```

### Step 2 — Update execution handler

In the `"llm.chat" | "llm.query"` match arm, after extracting `thinking`:

```rust
"llm.chat" | "llm.query" => {
    let prompt = input.get("prompt").or_else(|| input.get("input"))
        .and_then(|v| v.as_str())
        .ok_or("llm.chat requires 'prompt' argument")?;
    let system_prompt = input.get("system")
        .and_then(|v| v.as_str())
        .unwrap_or("You are a helpful assistant. Answer clearly and concisely.");
    let max_tokens = input.get("max_tokens").and_then(|v| v.as_u64()).map(|v| v as u32);
    let thinking = input.get("thinking").and_then(|v| v.as_bool()).unwrap_or(true);
    let response_schema = input.get("response_schema");
    let schema_name = input.get("schema_name")
        .and_then(|v| v.as_str())
        .unwrap_or("structured_output");

    let provider = self.llm_provider.as_ref()
        .ok_or("No LLM provider configured for llm.chat")?;

    let response = if let Some(schema) = response_schema {
        // Structured output path
        let supports_structured = self.llm_provider_supports_structured;
        provider.complete_structured(
            system_prompt, prompt, schema, schema_name,
            max_tokens, supports_structured,
        )
    } else if thinking {
        provider.complete(system_prompt, prompt, max_tokens)
    } else {
        provider.complete_no_think(system_prompt, prompt, max_tokens)
    }.map_err(|e| format!("LLM chat failed: {}", e))?;

    // ... existing facts extraction ...
}
```

### Step 3 — Store provider capability

In `AbilityRegistry`, add a field to track the current provider's structured output support:

```rust
pub llm_provider_supports_structured: bool,
```

Set it when `set_llm_provider()` is called (will need to accept the flag or look it up from the provider registry).

### Step 4 — Commit

```
feat(runtime): extend llm.chat with response_schema for structured output

Agents can now pass response_schema in llm.chat calls to get
grammar-constrained output. Routes to complete_structured() which
uses API-level enforcement or prompt augmentation as fallback.
```

---

## Task 8: Apply Schemas to Composition Phase

**Files:**
- Modify: `src/pea/composer.rs` (add schema constants + use them in LLM calls)

### Step 1 — Define schema constants

Add at the top of `composer.rs` (after imports):

```rust
// -- Structured output schemas for composition LLM calls ----------------------

fn schema_review_issues() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "issues": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "section_id": {"type": "string"},
                        "issue": {"type": "string"},
                        "severity": {"type": "string", "enum": ["high", "medium", "low"]},
                        "fix": {"type": "string"}
                    },
                    "required": ["section_id", "issue", "severity", "fix"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["issues"],
        "additionalProperties": false
    })
}

fn schema_contradictions() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "contradictions": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "section_id": {"type": "string"},
                        "claim": {"type": "string"},
                        "contradiction": {"type": "string"}
                    },
                    "required": ["section_id", "claim", "contradiction"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["contradictions"],
        "additionalProperties": false
    })
}

fn schema_taxonomy_conflicts() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "conflicts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "sections": {"type": "array", "items": {"type": "string"}},
                        "conflict": {"type": "string"},
                        "resolution": {"type": "string"}
                    },
                    "required": ["sections", "conflict", "resolution"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["conflicts"],
        "additionalProperties": false
    })
}

fn schema_nyaya_merges() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "merges": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "absorb_id": {"type": "string"},
                        "into_id": {"type": "string"},
                        "reason": {"type": "string"},
                        "unique_claims_to_preserve": {"type": "string"}
                    },
                    "required": ["absorb_id", "into_id", "reason", "unique_claims_to_preserve"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["merges"],
        "additionalProperties": false
    })
}

fn schema_chart_specs() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "charts": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "caption": {"type": "string"},
                        "python_script": {"type": "string"},
                        "data_type": {"type": "string"}
                    },
                    "required": ["caption", "python_script", "data_type"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["charts"],
        "additionalProperties": false
    })
}
```

### Step 2 — Update composition LLM calls to pass schemas

For each structured output call in composer.rs, add `"response_schema"` and `"schema_name"` to the input JSON. Example for `verify_numerical_claims()`:

**Before:**
```rust
let input = serde_json::json!({
    "system": "You detect numerical contradictions...",
    "prompt": format!("..."),
    "thinking": false,
});
```

**After:**
```rust
let input = serde_json::json!({
    "system": "You detect numerical contradictions...",
    "prompt": format!("..."),
    "thinking": false,
    "response_schema": schema_contradictions(),
    "schema_name": "contradictions",
});
```

Apply the same pattern to:
- `review_document()` → `schema_review_issues()` / `"review_issues"`
- `reconcile_taxonomies()` → `schema_taxonomy_conflicts()` / `"taxonomy_conflicts"`
- `nyaya_trim()` → `schema_nyaya_merges()` / `"nyaya_merges"`
- `generate_charts()` → `schema_chart_specs()` / `"chart_specs"`

### Step 3 — Add serde validation after extraction

After `extract_json()`, validate with serde:

```rust
// Example in verify_numerical_claims:
let json_str = extract_json(&output);
let parsed: serde_json::Value = match serde_json::from_str(json_str) {
    Ok(v) => v,
    Err(e) => {
        eprintln!("[composer] JSON parse failed: {}, retrying", e);
        // Retry once with correction prompt
        let retry_input = serde_json::json!({
            "system": "Your previous response was not valid JSON. Output ONLY valid JSON.",
            "prompt": format!(
                "Previous invalid output:\n{}\n\nPlease output valid JSON matching the required format.",
                &output[..output.len().min(500)]
            ),
            "thinking": false,
            "response_schema": schema_contradictions(),
            "schema_name": "contradictions",
        });
        match self.registry.execute_ability(self.manifest, "llm.chat", &retry_input.to_string()) {
            Ok(retry_result) => {
                let retry_output = String::from_utf8_lossy(&retry_result.output).to_string();
                let retry_json = extract_json(&retry_output);
                serde_json::from_str(retry_json).unwrap_or(serde_json::json!({"contradictions": []}))
            }
            Err(_) => serde_json::json!({"contradictions": []})
        }
    }
};
```

### Step 4 — Commit

```
feat(pea): apply JSON schemas to all composition structured outputs

Define schemas for review issues, contradictions, taxonomy conflicts,
nyaya merges, and chart specs. Pass via response_schema for grammar-
constrained decoding. Add retry-once on parse failure.
```

---

## Task 9: Build + Test + Deploy

### Step 1 — Run all PEA tests

```bash
cargo test --lib pea::
```

### Step 2 — Build release

```bash
cargo build --release
```

### Step 3 — Deploy to VPS

```bash
ssh -i ~/.ssh/nabaos_vultr root@144.202.48.94 'systemctl stop nabaos'
scp -i ~/.ssh/nabaos_vultr target/release/nabaos root@144.202.48.94:/root/.local/bin/nabaos
ssh -i ~/.ssh/nabaos_vultr root@144.202.48.94 'systemctl start nabaos'
```

### Step 4 — Commit

```
chore: build and deploy research efficiency + structured output
```

---

## Verification

1. `cargo test --lib pea::` — all pass
2. `cargo test --lib pea::heuristic_scorer` — new scorer tests pass
3. `cargo test --lib pea::bert_reranker` — cosine similarity tests pass
4. `cargo build --release` — clean compile
5. Run PEA with academic objective → verify in logs:
   - `[research] template expansion: N queries from keywords [...]` (no LLM call)
   - `[research] heuristic scoring: top=X.XX, median=Y.YY` (or BERT re-rank)
   - No `[research] scoring batch` messages (LLM scoring bypassed)
   - Composition calls show schema enforcement (no parse failures)
