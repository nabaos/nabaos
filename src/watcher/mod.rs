//! Runtime watcher — optional event bus monitoring for NabaOS.
//!
//! Feature-gated under `watcher`. When disabled, this module does not exist.

pub mod actions;
pub mod alerts;
pub mod analyzer;
pub mod config;
pub mod events;
pub mod scorer;

use events::WatchEvent;
use tokio::sync::broadcast;

const BUS_CAPACITY: usize = 1024;

/// The event bus. Components hold a clone of the sender.
pub struct WatchBus {
    tx: broadcast::Sender<WatchEvent>,
}

impl WatchBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(BUS_CAPACITY);
        Self { tx }
    }

    /// Emit an event onto the bus. Silently drops if no receivers.
    #[inline]
    pub fn emit(&self, event: WatchEvent) {
        let _ = self.tx.send(event);
    }

    /// Subscribe to the bus (used by RuntimeWatcher).
    pub fn subscribe(&self) -> broadcast::Receiver<WatchEvent> {
        self.tx.subscribe()
    }

    /// Clone the sender for passing to components.
    pub fn sender(&self) -> broadcast::Sender<WatchEvent> {
        self.tx.clone()
    }
}

impl Default for WatchBus {
    fn default() -> Self {
        Self::new()
    }
}

use crate::core::error::Result;
use actions::PauseRegistry;
use alerts::{Alert, AlertStore};
use analyzer::Analyzer;
use config::WatcherConfig;
use scorer::Scorer;

/// The main watcher engine — drains events, scores, triggers analysis, emits alerts.
pub struct RuntimeWatcher {
    rx: broadcast::Receiver<WatchEvent>,
    scorer: Scorer,
    analyzer: Analyzer,
    pub pause_registry: PauseRegistry,
    alert_store: AlertStore,
    config: WatcherConfig,
    // TODO: periodic review not yet implemented
    last_prune: u64,
}

impl RuntimeWatcher {
    pub fn open(bus: &WatchBus, data_dir: &std::path::Path, config: WatcherConfig) -> Result<Self> {
        let db_path = data_dir.join("watcher.db");
        let alert_store = AlertStore::open(&db_path)?;

        // Load persisted pause state from DB
        let pause_registry = PauseRegistry::new();
        if let Ok(paused) = alert_store.list_paused() {
            for (component, reason, _ts) in &paused {
                pause_registry.pause(component, reason);
            }
        }

        Ok(Self {
            rx: bus.subscribe(),
            scorer: Scorer::new(config.window_secs),
            analyzer: Analyzer::new(config.llm_cooldown_secs),
            pause_registry,
            alert_store,
            // TODO: periodic review not yet implemented
            last_prune: 0,
            config,
        })
    }

    /// Main tick — drain events, score, check thresholds, emit alerts.
    /// Called from daemon loop. Returns list of actions taken this tick.
    pub fn tick(&mut self) -> Result<Vec<String>> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        // 1. Drain all pending events from the broadcast channel,
        //    skipping events whose monitor category is disabled.
        loop {
            match self.rx.try_recv() {
                Ok(event) => {
                    let category = event.kind.monitor_category();
                    if self.config.enabled_monitors.is_enabled(category) {
                        self.scorer.push(event);
                    }
                }
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    tracing::warn!("Watcher lagged, missed {} events", n);
                    break;
                }
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }

        // 2. Compute scores
        let scores = self.scorer.compute_scores(now);
        let mut actions_taken = Vec::new();

        for (component, score) in &scores {
            if *score < self.config.llm_threshold {
                continue;
            }

            // Score crossed LLM threshold
            if *score >= self.config.pause_threshold {
                // Auto-pause
                if self
                    .pause_registry
                    .pause(component, "anomaly score exceeded pause threshold")
                {
                    // Persist pause to DB so CLI can see it
                    let _ = self.alert_store.save_pause(
                        component,
                        "anomaly score exceeded pause threshold",
                        now,
                    );
                    let alert = self.create_alert(component, *score, "pause", now);
                    self.alert_store.save_alert(&alert)?;
                    actions_taken.push(format!("PAUSED {} (score={:.2})", component, score));
                }
            } else if !self.analyzer.in_cooldown(component, now) {
                // LLM threshold crossed but below pause — would call LLM here
                // For now, emit alert without LLM (LLM integration is wired when
                // RuntimeWatcher gets access to LlmProvider)
                self.analyzer.record_call(component, now);
                let alert = self.create_alert(component, *score, "alert", now);
                self.alert_store.save_alert(&alert)?;
                actions_taken.push(format!("ALERT {} (score={:.2})", component, score));
            }
        }

        // 3. Periodic pruning — only once per hour (3600 seconds)
        if now.saturating_sub(self.last_prune) >= 3600 {
            let _ = self.alert_store.prune_old(self.config.alert_retention_days);
            self.last_prune = now;
        }

        Ok(actions_taken)
    }

    fn create_alert(&self, component: &str, score: f64, action: &str, now: u64) -> Alert {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let events = self.scorer.events_for_component(component, now);
        let summary = if events.is_empty() {
            format!("Anomaly score {:.2} with no recent events", score)
        } else {
            let last = events.last().unwrap();
            format!(
                "Score {:.2} — latest: {} ({})",
                score,
                last.severity,
                last.kind.component()
            )
        };
        Alert {
            id: format!("wa_{:x}_{:x}_{:x}", now, pid, seq),
            timestamp: now,
            component: component.to_string(),
            severity: if score >= self.config.pause_threshold {
                events::Severity::Critical
            } else {
                events::Severity::Suspicious
            },
            event_summary: summary,
            llm_verdict_json: None,
            action_taken: action.to_string(),
            resolved_at: None,
        }
    }

    /// Get recent alerts for CLI display.
    pub fn recent_alerts(&self, since_secs: u64) -> Result<Vec<Alert>> {
        self.alert_store.list_recent(since_secs)
    }

    /// Resume a paused component and resolve its alert.
    pub fn resume_component(&self, component: &str) -> Result<bool> {
        let was_paused = self.pause_registry.resume(component);
        if was_paused {
            let _ = self.alert_store.remove_pause(component);
        }
        Ok(was_paused)
    }

    /// Get current component scores (for status display).
    pub fn component_scores(&mut self) -> std::collections::HashMap<String, f64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.scorer.compute_scores(now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use events::{Severity, WatchEventKind};

    #[test]
    fn test_watcher_tick_no_events_no_actions() {
        let bus = WatchBus::new();
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = RuntimeWatcher::open(&bus, dir.path(), WatcherConfig::default()).unwrap();
        let actions = watcher.tick().unwrap();
        assert!(actions.is_empty());
    }

    #[test]
    fn test_watcher_tick_high_score_triggers_alert() {
        let bus = WatchBus::new();
        let dir = tempfile::tempdir().unwrap();
        let config = WatcherConfig {
            llm_threshold: 0.3,
            pause_threshold: 0.9,
            ..WatcherConfig::default()
        };
        let mut watcher = RuntimeWatcher::open(&bus, dir.path(), config).unwrap();

        // Emit a high-score event
        bus.emit(WatchEvent::new(
            WatchEventKind::CredentialLeak {
                credential_type: "aws".into(),
                destination: "evil.com".into(),
            },
            Severity::Critical,
        ));

        let actions = watcher.tick().unwrap();
        assert!(!actions.is_empty());
        assert!(actions[0].contains("ALERT") || actions[0].contains("PAUSED"));
    }

    #[test]
    fn test_watcher_tick_very_high_score_pauses() {
        let bus = WatchBus::new();
        let dir = tempfile::tempdir().unwrap();
        let config = WatcherConfig {
            llm_threshold: 0.1,
            pause_threshold: 0.5,
            ..WatcherConfig::default()
        };
        let mut watcher = RuntimeWatcher::open(&bus, dir.path(), config).unwrap();

        // Emit multiple high-score events to push above pause threshold
        bus.emit(WatchEvent::new(
            WatchEventKind::CredentialLeak {
                credential_type: "key".into(),
                destination: "x.com".into(),
            },
            Severity::Critical,
        ));

        let actions = watcher.tick().unwrap();
        assert!(actions.iter().any(|a| a.contains("PAUSED")));
        assert!(watcher.pause_registry.is_paused("security"));
    }

    #[test]
    fn test_watcher_resume_component() {
        let bus = WatchBus::new();
        let dir = tempfile::tempdir().unwrap();
        let watcher = RuntimeWatcher::open(&bus, dir.path(), WatcherConfig::default()).unwrap();
        watcher.pause_registry.pause("test", "manual");
        assert!(watcher.resume_component("test").unwrap());
        assert!(!watcher.pause_registry.is_paused("test"));
    }

    #[test]
    fn test_emission_functions_compile_and_send() {
        let bus = WatchBus::new();
        let tx = bus.sender();
        let mut rx = bus.subscribe();

        crate::security::pattern_matcher::emit_injection_event(
            &tx,
            "test_pattern",
            0.9,
            "test_source",
        );
        let event = rx.try_recv().unwrap();
        assert_eq!(event.kind.component(), "security");

        crate::security::credential_scanner::emit_credential_event(&tx, "api_key", "evil.com");
        let event = rx.try_recv().unwrap();
        assert_eq!(event.severity, Severity::Critical);

        crate::pea::engine::emit_budget_event(&tx, "obj-1", 2.5, 1.5);
        let event = rx.try_recv().unwrap();
        assert_eq!(event.kind.component(), "pea");
    }

    #[test]
    fn test_watcher_component_scores_empty() {
        let bus = WatchBus::new();
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = RuntimeWatcher::open(&bus, dir.path(), WatcherConfig::default()).unwrap();
        let scores = watcher.component_scores();
        assert!(scores.is_empty());
    }
}
