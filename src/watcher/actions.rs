//! Pause/resume actions — reversible component control.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Shared registry tracking which components are paused by the watcher.
#[derive(Debug, Clone)]
pub struct PauseRegistry {
    inner: Arc<RwLock<HashMap<String, PauseRecord>>>,
}

#[derive(Debug, Clone)]
pub struct PauseRecord {
    pub component: String,
    pub reason: String,
    pub paused_at: u64,
}

impl PauseRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Pause a component. Returns true if newly paused, false if already paused.
    pub fn pause(&self, component: &str, reason: &str) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut map = self.inner.write().unwrap();
        if map.contains_key(component) {
            return false;
        }
        map.insert(
            component.to_string(),
            PauseRecord {
                component: component.to_string(),
                reason: reason.to_string(),
                paused_at: now,
            },
        );
        true
    }

    /// Resume a component. Returns true if it was paused, false if not.
    pub fn resume(&self, component: &str) -> bool {
        let mut map = self.inner.write().unwrap();
        map.remove(component).is_some()
    }

    /// Check if a component is paused.
    pub fn is_paused(&self, component: &str) -> bool {
        let map = self.inner.read().unwrap();
        map.contains_key(component)
    }

    /// List all currently paused components.
    pub fn list_paused(&self) -> Vec<PauseRecord> {
        let map = self.inner.read().unwrap();
        map.values().cloned().collect()
    }
}

impl Default for PauseRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pause_and_resume() {
        let reg = PauseRegistry::new();
        assert!(reg.pause("pea", "budget anomaly"));
        assert!(reg.is_paused("pea"));
        assert!(!reg.is_paused("cache"));
        assert!(reg.resume("pea"));
        assert!(!reg.is_paused("pea"));
    }

    #[test]
    fn test_double_pause_returns_false() {
        let reg = PauseRegistry::new();
        assert!(reg.pause("x", "reason1"));
        assert!(!reg.pause("x", "reason2")); // already paused
    }

    #[test]
    fn test_resume_nonexistent_returns_false() {
        let reg = PauseRegistry::new();
        assert!(!reg.resume("nonexistent"));
    }

    #[test]
    fn test_list_paused() {
        let reg = PauseRegistry::new();
        reg.pause("pea", "r1");
        reg.pause("cache", "r2");
        let paused = reg.list_paused();
        assert_eq!(paused.len(), 2);
    }
}
