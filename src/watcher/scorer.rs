//! Sliding window scorer — tracks per-component anomaly scores with linear decay.

use crate::watcher::events::WatchEvent;
use std::collections::{HashMap, VecDeque};

/// A scored event with its contribution to the component score.
#[derive(Debug, Clone)]
struct ScoredEvent {
    event: WatchEvent,
    score: f64,
}

/// Default sliding window duration in seconds (paper claims 60s).
pub const DEFAULT_WINDOW_SECS: u64 = 60;

/// Maintains a sliding window of events and per-component anomaly scores.
pub struct Scorer {
    window: VecDeque<ScoredEvent>,
    window_secs: u64,
}

impl Scorer {
    pub fn new(window_secs: u64) -> Self {
        Self {
            window: VecDeque::new(),
            window_secs,
        }
    }

    /// Add an event to the window.
    pub fn push(&mut self, event: WatchEvent) {
        let score = event.kind.base_score();
        self.window.push_back(ScoredEvent { event, score });
    }

    /// Evict events older than the window and compute per-component scores
    /// with linear decay (newest = full weight, oldest = 0).
    pub fn compute_scores(&mut self, now: u64) -> HashMap<String, f64> {
        let cutoff = now.saturating_sub(self.window_secs);

        // Evict old events
        while self
            .window
            .front()
            .is_some_and(|e| e.event.timestamp < cutoff)
        {
            self.window.pop_front();
        }

        let mut scores: HashMap<String, f64> = HashMap::new();

        for se in &self.window {
            let age = now.saturating_sub(se.event.timestamp) as f64;
            let decay = 1.0 - (age / self.window_secs as f64).min(1.0);
            let contribution = se.score * decay;
            let component = se.event.kind.component().to_string();
            *scores.entry(component).or_insert(0.0) += contribution;
        }

        // Cap all scores at 1.0
        for v in scores.values_mut() {
            *v = v.min(1.0);
        }

        scores
    }

    /// Return events for a specific component within the window.
    pub fn events_for_component(&self, component: &str, now: u64) -> Vec<&WatchEvent> {
        let cutoff = now.saturating_sub(self.window_secs);
        self.window
            .iter()
            .filter(|se| se.event.timestamp >= cutoff && se.event.kind.component() == component)
            .map(|se| &se.event)
            .collect()
    }

    /// Number of events currently in the window.
    pub fn len(&self) -> usize {
        self.window.len()
    }

    pub fn is_empty(&self) -> bool {
        self.window.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::events::{Severity, WatchEvent, WatchEventKind};

    fn make_event(kind: WatchEventKind, severity: Severity, timestamp: u64) -> WatchEvent {
        WatchEvent {
            timestamp,
            kind,
            severity,
        }
    }

    #[test]
    fn test_score_decays_over_time() {
        let mut scorer = Scorer::new(DEFAULT_WINDOW_SECS);
        scorer.push(make_event(
            WatchEventKind::Error {
                module: "test".into(),
                message: "oops".into(),
            },
            Severity::Warning,
            100,
        ));
        // At t=100, event is brand new → full score (0.1)
        let scores = scorer.compute_scores(100);
        let s1 = *scores.get("test").unwrap();

        // At t=130 (halfway through 60s window), score should be ~half
        let scores2 = scorer.compute_scores(130);
        let s2 = *scores2.get("test").unwrap();
        assert!(s2 < s1, "score should decay: {} < {}", s2, s1);
    }

    #[test]
    fn test_old_events_evicted() {
        let mut scorer = Scorer::new(DEFAULT_WINDOW_SECS);
        scorer.push(make_event(
            WatchEventKind::Error {
                module: "test".into(),
                message: "old".into(),
            },
            Severity::Warning,
            100,
        ));
        let scores = scorer.compute_scores(500); // 400s later, outside 60s window
        assert!(scores.get("test").is_none() || *scores.get("test").unwrap() == 0.0);
    }

    #[test]
    fn test_multiple_components_scored_independently() {
        let mut scorer = Scorer::new(DEFAULT_WINDOW_SECS);
        scorer.push(make_event(
            WatchEventKind::CredentialLeak {
                credential_type: "aws".into(),
                destination: "evil.com".into(),
            },
            Severity::Critical,
            100,
        ));
        scorer.push(make_event(
            WatchEventKind::Error {
                module: "orchestrator".into(),
                message: "fail".into(),
            },
            Severity::Warning,
            100,
        ));
        let scores = scorer.compute_scores(100);
        assert!(scores.get("security").unwrap() > scores.get("orchestrator").unwrap());
    }

    #[test]
    fn test_score_capped_at_one() {
        let mut scorer = Scorer::new(DEFAULT_WINDOW_SECS);
        // Push many high-score events for the same component
        for i in 0..20 {
            scorer.push(make_event(
                WatchEventKind::CredentialLeak {
                    credential_type: "key".into(),
                    destination: format!("d{}.com", i),
                },
                Severity::Critical,
                100,
            ));
        }
        let scores = scorer.compute_scores(100);
        assert!(*scores.get("security").unwrap() <= 1.0);
    }

    #[test]
    fn test_events_for_component_filters() {
        let mut scorer = Scorer::new(DEFAULT_WINDOW_SECS);
        scorer.push(make_event(
            WatchEventKind::Error {
                module: "pea".into(),
                message: "a".into(),
            },
            Severity::Warning,
            100,
        ));
        scorer.push(make_event(
            WatchEventKind::CredentialLeak {
                credential_type: "x".into(),
                destination: "y".into(),
            },
            Severity::Critical,
            100,
        ));
        let pea_events = scorer.events_for_component("pea", 100);
        assert_eq!(pea_events.len(), 1);
    }
}
