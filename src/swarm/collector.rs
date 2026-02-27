use crate::swarm::worker::{WorkerOutcome, WorkerResult};
use std::collections::HashSet;

/// Collects, deduplicates, and ranks worker results.
pub struct ResultCollector {
    results: Vec<WorkerResult>,
    seen_hashes: HashSet<String>,
}

impl Default for ResultCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl ResultCollector {
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
            seen_hashes: HashSet::new(),
        }
    }

    /// Add a result, skipping duplicates by content hash.
    pub fn add(&mut self, result: WorkerResult) -> bool {
        if self.seen_hashes.contains(&result.content_hash) {
            return false; // duplicate
        }
        // Skip blocked/failed results with empty content
        if matches!(result.outcome, WorkerOutcome::Blocked(_)) && result.content.is_empty() {
            return false;
        }
        self.seen_hashes.insert(result.content_hash.clone());
        self.results.push(result);
        true
    }

    /// Get results sorted by priority (lowest number = highest priority).
    pub fn ranked_results(&self) -> Vec<&WorkerResult> {
        let mut sorted: Vec<&WorkerResult> = self
            .results
            .iter()
            .filter(|r| !r.content.is_empty())
            .collect();
        sorted.sort_by_key(|r| r.source_plan.priority);
        sorted
    }

    /// Truncate results to fit within a token budget (rough estimate: 4 chars per token).
    pub fn truncate_to_budget(&self, max_chars: usize) -> Vec<&WorkerResult> {
        let ranked = self.ranked_results();
        let mut total = 0;
        let mut selected = Vec::new();
        for result in ranked {
            if total + result.content.len() > max_chars {
                break;
            }
            total += result.content.len();
            selected.push(result);
        }
        selected
    }

    /// Total results collected (including duplicates filtered).
    pub fn total_collected(&self) -> usize {
        self.results.len()
    }

    /// Number of unique results.
    pub fn unique_count(&self) -> usize {
        self.seen_hashes.len()
    }
}

impl std::fmt::Display for ResultCollector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ResultCollector({} results, {} unique)",
            self.results.len(),
            self.seen_hashes.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::worker::*;

    fn make_result(content: &str, priority: u8) -> WorkerResult {
        WorkerResult {
            source_plan: SourcePlan {
                worker_type: "test".into(),
                target: SourceTarget::Url("http://example.com".into()),
                priority,
                needs_auth: false,
                extraction_focus: None,
            },
            outcome: WorkerOutcome::Success,
            content: content.to_string(),
            content_hash: WorkerResult::compute_hash(content),
            structured_data: None,
            citations: Vec::new(),
            elapsed_ms: 100,
        }
    }

    #[test]
    fn test_result_collector_dedup() {
        let mut collector = ResultCollector::new();
        let r1 = make_result("same content", 0);
        let r2 = make_result("same content", 1);
        assert!(collector.add(r1));
        assert!(!collector.add(r2)); // duplicate hash
        assert_eq!(collector.total_collected(), 1);
        assert_eq!(collector.unique_count(), 1);
    }

    #[test]
    fn test_result_collector_rank() {
        let mut collector = ResultCollector::new();
        collector.add(make_result("content c", 2));
        collector.add(make_result("content a", 0));
        collector.add(make_result("content b", 1));
        let ranked = collector.ranked_results();
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].source_plan.priority, 0);
        assert_eq!(ranked[1].source_plan.priority, 1);
        assert_eq!(ranked[2].source_plan.priority, 2);
    }
}
