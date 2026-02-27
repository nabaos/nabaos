//! Anomaly detector — behavioral profiling and deviation scoring.
//!
//! Ported from nyaya-guard TypeScript implementation.
//! Tracks per-agent behavioral baselines using rolling windows (1h, 24h, 7d).
//! Detects frequency anomalies (tool call spikes, message bursts) and
//! scope anomalies (new paths, new domains, new tools).
//!
//! Learning mode: no alerts during the first N hours (configurable).
//!
//! SECURITY: Never stores or logs raw paths/domains — hashes only.

use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static RE_EXTRACT_PATHS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"(?:/[\w.-]+)+").unwrap());
static RE_EXTRACT_DOMAINS: LazyLock<regex::Regex> =
    LazyLock::new(|| regex::Regex::new(r"https?://([^/\s]+)").unwrap());

const ONE_HOUR_MS: i64 = 60 * 60 * 1000;
const ONE_DAY_MS: i64 = 24 * ONE_HOUR_MS;
const ONE_WEEK_MS: i64 = 7 * ONE_DAY_MS;

/// Alert severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "LOW"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::High => write!(f, "HIGH"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Category of anomaly detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyCategory {
    /// Tool call rate spike, message burst
    Frequency,
    /// New path, new domain, new tool (after learning period)
    Scope,
}

impl std::fmt::Display for AnomalyCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frequency => write!(f, "frequency"),
            Self::Scope => write!(f, "scope"),
        }
    }
}

/// A detected behavioral anomaly.
#[derive(Debug, Clone)]
pub struct Anomaly {
    /// Unique ID
    pub id: String,
    /// When detected (ms since epoch)
    pub timestamp: i64,
    /// Category of anomaly
    pub category: AnomalyCategory,
    /// How severe
    pub severity: AlertSeverity,
    /// Human-readable description (safe to log — no raw data)
    pub description: String,
    /// SHA-256 hash of description (for dedup)
    pub trigger_hash: String,
    /// Session/channel key
    pub session_key: String,
}

/// Rolling frequency counters.
#[derive(Debug, Clone, Default)]
pub struct FrequencyCounters {
    pub last_hour: u32,
    pub last_24h: u32,
    pub last_7d: u32,
    pub avg_hourly: f64,
}

/// Behavioral profile for a single agent.
#[derive(Debug, Clone)]
pub struct BehaviorProfile {
    /// Agent identifier
    pub agent_id: String,
    /// When profile was created (ms since epoch)
    pub created_at: i64,
    /// Whether in learning mode (no alerts)
    pub learning_mode: bool,
    /// Tool call frequency counters
    pub tool_calls: FrequencyCounters,
    /// Hashed known file paths (never store raw)
    pub known_paths: HashSet<String>,
    /// Known network domains
    pub known_domains: HashSet<String>,
    /// Known tool names
    pub known_tools: HashSet<String>,
    /// Per-channel message counts
    pub channel_frequency: HashMap<String, u32>,
    /// Recent tool call timestamps (for windowed counts)
    pub recent_tool_calls: Vec<i64>,
    /// Recent message timestamps (for burst detection)
    pub recent_messages: Vec<i64>,
}

impl BehaviorProfile {
    /// Create a fresh profile for a new agent.
    pub fn new(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            created_at: now_ms(),
            learning_mode: true,
            tool_calls: FrequencyCounters::default(),
            known_paths: HashSet::new(),
            known_domains: HashSet::new(),
            known_tools: HashSet::new(),
            channel_frequency: HashMap::new(),
            recent_tool_calls: Vec::new(),
            recent_messages: Vec::new(),
        }
    }

    /// Maximum number of known paths/domains/tools to track (prevent unbounded growth).
    const MAX_KNOWN_ITEMS: usize = 10_000;
    /// Maximum recent timestamps to retain.
    const MAX_RECENT_TIMESTAMPS: usize = 50_000;

    /// Record a tool call event.
    pub fn record_tool_call(&mut self, tool_name: &str, args: &HashMap<String, String>) {
        let now = now_ms();

        // Prune old timestamps (keep 7d window)
        self.recent_tool_calls.retain(|&t| t > now - ONE_WEEK_MS);
        // H9: Cap timestamp list to prevent unbounded growth
        if self.recent_tool_calls.len() >= Self::MAX_RECENT_TIMESTAMPS {
            let drain_count = self.recent_tool_calls.len() - Self::MAX_RECENT_TIMESTAMPS + 1;
            self.recent_tool_calls.drain(..drain_count);
        }
        self.recent_tool_calls.push(now);

        // Update frequency windows
        self.tool_calls.last_hour = self
            .recent_tool_calls
            .iter()
            .filter(|&&t| t > now - ONE_HOUR_MS)
            .count() as u32;
        self.tool_calls.last_24h = self
            .recent_tool_calls
            .iter()
            .filter(|&&t| t > now - ONE_DAY_MS)
            .count() as u32;
        self.tool_calls.last_7d = self.recent_tool_calls.len() as u32;

        // Update rolling average
        let hours_active = ((now - self.created_at) as f64 / ONE_HOUR_MS as f64).max(1.0);
        self.tool_calls.avg_hourly = self.tool_calls.last_7d as f64 / hours_active.min(168.0);

        // Track tool name (bounded)
        if self.known_tools.len() < Self::MAX_KNOWN_ITEMS {
            self.known_tools.insert(tool_name.to_string());
        }

        // Track hashed paths from args (bounded)
        for path in extract_paths(args) {
            if self.known_paths.len() >= Self::MAX_KNOWN_ITEMS {
                break;
            }
            self.known_paths.insert(hash_path(&path));
        }

        // Track domains from args (bounded)
        for domain in extract_domains(args) {
            if self.known_domains.len() >= Self::MAX_KNOWN_ITEMS {
                break;
            }
            self.known_domains.insert(domain);
        }
    }

    /// Record an incoming message.
    pub fn record_message(&mut self, channel: &str) {
        let now = now_ms();
        self.recent_messages.retain(|&t| t > now - ONE_HOUR_MS);
        // H9: Cap message timestamps to prevent unbounded growth
        if self.recent_messages.len() >= Self::MAX_RECENT_TIMESTAMPS {
            let drain_count = self.recent_messages.len() - Self::MAX_RECENT_TIMESTAMPS + 1;
            self.recent_messages.drain(..drain_count);
        }
        self.recent_messages.push(now);

        *self
            .channel_frequency
            .entry(channel.to_string())
            .or_insert(0) += 1;
    }

    /// Check if learning mode should end.
    pub fn check_learning_mode(&mut self, learning_hours: f64) {
        if self.learning_mode {
            let hours_active = (now_ms() - self.created_at) as f64 / ONE_HOUR_MS as f64;
            if hours_active >= learning_hours {
                self.learning_mode = false;
            }
        }
    }
}

/// An event to assess for anomalies.
#[derive(Debug, Default)]
pub struct SecurityEvent {
    pub tool_name: Option<String>,
    pub args: HashMap<String, String>,
    pub channel: Option<String>,
}

/// Detect anomalies in current behavior against the profile.
/// Returns a list of anomalies (possibly empty).
pub fn detect_anomalies(
    profile: &BehaviorProfile,
    event: &SecurityEvent,
    threshold: f64,
) -> Vec<Anomaly> {
    // Don't alert during learning mode
    if profile.learning_mode {
        return vec![];
    }

    let mut anomalies = Vec::new();
    let now = now_ms();
    let channel = event.channel.as_deref().unwrap_or("unknown");

    // === FREQUENCY ANOMALIES ===

    // Tool call spike: current hour > threshold * average
    if profile.tool_calls.avg_hourly > 0.0
        && (profile.tool_calls.last_hour as f64) > profile.tool_calls.avg_hourly * threshold
    {
        let ratio = profile.tool_calls.last_hour as f64 / profile.tool_calls.avg_hourly;
        anomalies.push(make_anomaly(
            AnomalyCategory::Frequency,
            determine_severity(ratio, threshold),
            &format!(
                "Tool call rate {}/hr is {:.1}x above average {:.1}/hr",
                profile.tool_calls.last_hour, ratio, profile.tool_calls.avg_hourly
            ),
            channel,
        ));
    }

    // Message burst: >10 messages in 1 minute = possible automated probing
    let last_minute_messages = profile
        .recent_messages
        .iter()
        .filter(|&&t| t > now - 60_000)
        .count();
    if last_minute_messages > 10 {
        anomalies.push(make_anomaly(
            AnomalyCategory::Frequency,
            AlertSeverity::Medium,
            &format!(
                "{} messages in last minute — possible automated probing",
                last_minute_messages
            ),
            channel,
        ));
    }

    // === SCOPE ANOMALIES ===

    // New file path never seen before
    for path in extract_paths(&event.args) {
        if !profile.known_paths.contains(&hash_path(&path)) {
            let severity = if is_sensitive_path(&path) {
                AlertSeverity::High
            } else {
                AlertSeverity::Low
            };
            anomalies.push(make_anomaly(
                AnomalyCategory::Scope,
                severity,
                &format!(
                    "First-ever access to path category: {}",
                    categorize_path(&path)
                ),
                channel,
            ));
        }
    }

    // New domain never contacted before
    for domain in extract_domains(&event.args) {
        if !profile.known_domains.contains(&domain) {
            anomalies.push(make_anomaly(
                AnomalyCategory::Scope,
                AlertSeverity::Medium,
                "First-ever network contact to new domain category",
                channel,
            ));
        }
    }

    // New tool never used before (after learning period)
    if let Some(ref tool_name) = event.tool_name {
        if !profile.known_tools.contains(tool_name.as_str()) {
            anomalies.push(make_anomaly(
                AnomalyCategory::Scope,
                AlertSeverity::Low,
                &format!("First-ever use of tool: {}", tool_name),
                channel,
            ));
        }
    }

    anomalies
}

/// Security assessment from anomaly detection — summary safe to log/include in pipeline.
#[derive(Debug)]
pub struct AnomalyAssessment {
    /// Number of anomalies detected
    pub anomaly_count: usize,
    /// Highest severity
    pub max_severity: Option<AlertSeverity>,
    /// Whether any HIGH or CRITICAL anomaly was found
    pub has_critical: bool,
    /// Category descriptions (safe to log)
    pub categories: Vec<String>,
}

/// Assess current event against profile — returns summary.
pub fn assess(
    profile: &BehaviorProfile,
    event: &SecurityEvent,
    threshold: f64,
) -> AnomalyAssessment {
    let anomalies = detect_anomalies(profile, event, threshold);
    let max_severity = anomalies.iter().map(|a| a.severity).max();
    let has_critical = anomalies.iter().any(|a| a.severity >= AlertSeverity::High);
    let categories: Vec<String> = anomalies
        .iter()
        .map(|a| format!("{}: {}", a.category, a.description))
        .collect();

    AnomalyAssessment {
        anomaly_count: anomalies.len(),
        max_severity,
        has_critical,
        categories,
    }
}

// === Helpers ===

fn make_anomaly(
    category: AnomalyCategory,
    severity: AlertSeverity,
    description: &str,
    session_key: &str,
) -> Anomaly {
    let now = now_ms();
    let mut hasher = Sha256::new();
    hasher.update(description.as_bytes());
    let trigger_hash = format!("{:x}", hasher.finalize())[..16].to_string();

    Anomaly {
        id: format!("{}-{:08x}", now, rand_u32()),
        timestamp: now,
        category,
        severity,
        description: description.to_string(),
        trigger_hash,
        session_key: session_key.to_string(),
    }
}

fn determine_severity(ratio: f64, threshold: f64) -> AlertSeverity {
    if ratio > threshold * 3.0 {
        AlertSeverity::Critical
    } else if ratio > threshold * 2.0 {
        AlertSeverity::High
    } else if ratio > threshold {
        AlertSeverity::Medium
    } else {
        AlertSeverity::Low
    }
}

/// Hash a path for storage — never store raw paths.
fn hash_path(path: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

/// Extract file paths from tool arguments.
fn extract_paths(args: &HashMap<String, String>) -> Vec<String> {
    let path_keys = [
        "path",
        "file",
        "filepath",
        "filename",
        "directory",
        "dir",
        "target",
    ];
    let cmd_keys = ["command", "cmd", "script"];
    let mut paths = Vec::new();

    for (key, value) in args {
        let key_lower = key.to_lowercase();
        if path_keys.contains(&key_lower.as_str()) || value.starts_with('/') {
            paths.push(value.clone());
        }
        if cmd_keys.contains(&key_lower.as_str()) {
            // Extract paths from commands (rough heuristic)
            let re = &*RE_EXTRACT_PATHS;
            for m in re.find_iter(value) {
                paths.push(m.as_str().to_string());
            }
        }
    }

    paths
}

/// Extract domain names from tool arguments.
fn extract_domains(args: &HashMap<String, String>) -> Vec<String> {
    let re = &*RE_EXTRACT_DOMAINS;
    let mut domains = Vec::new();

    for value in args.values() {
        for cap in re.captures_iter(value) {
            if let Some(host) = cap.get(1) {
                domains.push(host.as_str().to_string());
            }
        }
    }

    domains
}

/// Check if a path is sensitive (credentials, keys, etc.).
fn is_sensitive_path(path: &str) -> bool {
    let sensitive = [
        ".ssh/",
        ".gnupg/",
        ".aws/",
        ".env",
        "/etc/passwd",
        "/etc/shadow",
        "credentials",
        "id_rsa",
        "id_ed25519",
        ".pem",
        ".key",
    ];
    let path_lower = path.to_lowercase();
    sensitive.iter().any(|s| path_lower.contains(s))
        || path_lower.contains("keychain")
        || path_lower.contains("wallet")
}

/// Categorize a path without revealing it (for safe logging).
fn categorize_path(path: &str) -> &'static str {
    if is_sensitive_path(path) {
        "SENSITIVE_CREDENTIALS"
    } else if path.contains(".ssh") {
        "SSH_CONFIG"
    } else if path.starts_with("/etc") {
        "SYSTEM_CONFIG"
    } else if path.starts_with("/tmp") {
        "TEMP"
    } else if path.to_lowercase().contains("documents") {
        "USER_DOCUMENTS"
    } else if path.to_lowercase().contains("downloads") {
        "USER_DOWNLOADS"
    } else {
        "OTHER"
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn rand_u32() -> u32 {
    // Simple non-crypto random for anomaly IDs
    let t = now_ms() as u32;
    t.wrapping_mul(2654435761) // Knuth's multiplicative hash
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile() -> BehaviorProfile {
        let mut p = BehaviorProfile::new("test-agent");
        p.learning_mode = false; // Skip learning for tests
        p.created_at = now_ms() - ONE_DAY_MS; // Created 24h ago
        p
    }

    #[test]
    fn test_new_profile_in_learning_mode() {
        let p = BehaviorProfile::new("agent-1");
        assert!(p.learning_mode);
        assert!(p.known_tools.is_empty());
        assert!(p.known_paths.is_empty());
    }

    #[test]
    fn test_learning_mode_blocks_alerts() {
        let p = BehaviorProfile::new("agent-1"); // learning_mode = true
        let event = SecurityEvent {
            tool_name: Some("new_tool".into()),
            ..Default::default()
        };
        let anomalies = detect_anomalies(&p, &event, 3.0);
        assert!(
            anomalies.is_empty(),
            "Should not alert during learning mode"
        );
    }

    #[test]
    fn test_check_learning_mode_exits() {
        let mut p = BehaviorProfile::new("agent-1");
        p.created_at = now_ms() - (25 * ONE_HOUR_MS); // 25 hours ago
        assert!(p.learning_mode);
        p.check_learning_mode(24.0);
        assert!(!p.learning_mode);
    }

    #[test]
    fn test_record_tool_call_updates_counters() {
        let mut p = test_profile();
        let args = HashMap::new();
        p.record_tool_call("data.fetch_url", &args);
        p.record_tool_call("data.fetch_url", &args);

        assert!(p.known_tools.contains("data.fetch_url"));
        assert_eq!(p.tool_calls.last_hour, 2);
    }

    #[test]
    fn test_record_message_tracks_channel() {
        let mut p = test_profile();
        p.record_message("telegram");
        p.record_message("telegram");
        p.record_message("discord");

        assert_eq!(p.channel_frequency["telegram"], 2);
        assert_eq!(p.channel_frequency["discord"], 1);
    }

    #[test]
    fn test_new_tool_anomaly() {
        let mut p = test_profile();
        // Build baseline
        p.known_tools.insert("data.fetch_url".into());
        p.known_tools.insert("storage.get".into());

        let event = SecurityEvent {
            tool_name: Some("shell.execute".into()),
            ..Default::default()
        };
        let anomalies = detect_anomalies(&p, &event, 3.0);
        assert!(anomalies
            .iter()
            .any(|a| a.description.contains("shell.execute")));
    }

    #[test]
    fn test_sensitive_path_anomaly() {
        let p = test_profile();

        let mut args = HashMap::new();
        args.insert("path".into(), "/home/user/.ssh/id_rsa".into());

        let event = SecurityEvent {
            tool_name: Some("file.read".into()),
            args,
            ..Default::default()
        };
        let anomalies = detect_anomalies(&p, &event, 3.0);
        assert!(anomalies.iter().any(|a| a.severity == AlertSeverity::High));
    }

    #[test]
    fn test_new_domain_anomaly() {
        let p = test_profile();

        let mut args = HashMap::new();
        args.insert("url".into(), "https://evil.com/collect".into());

        let event = SecurityEvent {
            args,
            ..Default::default()
        };
        let anomalies = detect_anomalies(&p, &event, 3.0);
        assert!(anomalies
            .iter()
            .any(|a| a.category == AnomalyCategory::Scope));
    }

    #[test]
    fn test_message_burst_anomaly() {
        let mut p = test_profile();
        // Simulate >10 messages in 1 minute
        let now = now_ms();
        for i in 0..15 {
            p.recent_messages.push(now - (i * 1000)); // 1 msg per second
        }

        let event = SecurityEvent::default();
        let anomalies = detect_anomalies(&p, &event, 3.0);
        assert!(anomalies
            .iter()
            .any(|a| a.description.contains("messages in last minute")));
    }

    #[test]
    fn test_tool_call_spike_anomaly() {
        let mut p = test_profile();
        // Set a low baseline
        p.tool_calls.avg_hourly = 2.0;
        // Simulate high current hour
        let now = now_ms();
        for i in 0..10 {
            p.recent_tool_calls.push(now - (i * 1000));
        }
        p.tool_calls.last_hour = 10;

        let event = SecurityEvent::default();
        let anomalies = detect_anomalies(&p, &event, 3.0);
        assert!(anomalies
            .iter()
            .any(|a| a.category == AnomalyCategory::Frequency));
    }

    #[test]
    fn test_known_tool_no_anomaly() {
        let mut p = test_profile();
        p.known_tools.insert("data.fetch_url".into());

        let event = SecurityEvent {
            tool_name: Some("data.fetch_url".into()),
            ..Default::default()
        };
        let anomalies = detect_anomalies(&p, &event, 3.0);
        // Known tool should NOT trigger scope anomaly
        assert!(!anomalies
            .iter()
            .any(|a| a.description.contains("data.fetch_url")));
    }

    #[test]
    fn test_assess_summary() {
        let p = test_profile();
        let event = SecurityEvent {
            tool_name: Some("new_tool".into()),
            ..Default::default()
        };
        let assessment = assess(&p, &event, 3.0);
        assert!(assessment.anomaly_count >= 1);
        assert!(!assessment.has_critical); // Just a new tool, not critical
    }

    #[test]
    fn test_severity_ordering() {
        assert!(AlertSeverity::Critical > AlertSeverity::High);
        assert!(AlertSeverity::High > AlertSeverity::Medium);
        assert!(AlertSeverity::Medium > AlertSeverity::Low);
    }

    #[test]
    fn test_hash_path_consistency() {
        let h1 = hash_path("/home/user/.ssh/id_rsa");
        let h2 = hash_path("/home/user/.ssh/id_rsa");
        assert_eq!(h1, h2);
        let h3 = hash_path("/home/user/documents/file.txt");
        assert_ne!(h1, h3);
    }
}
