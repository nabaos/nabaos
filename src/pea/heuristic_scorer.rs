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
        0.2
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
            openalex_meta: None,
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
            openalex_meta: None,
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
