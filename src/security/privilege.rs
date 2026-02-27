//! Tiered privilege system with session-based 2FA enforcement.
//!
//! Four privilege levels requiring increasing authentication:
//! - Open (0): No auth needed — read-only and safe operations
//! - Elevated (1): TOTP required — write operations and external actions
//! - Admin (2): TOTP + password — meta-operations like deploy, constitution edits
//! - Critical (3): TOTP + password + weblink — single-use, irreversible operations

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Privilege levels requiring increasing authentication.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum PrivilegeLevel {
    Open = 0,
    Elevated = 1,
    Admin = 2,
    Critical = 3,
}

impl std::fmt::Display for PrivilegeLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrivilegeLevel::Open => write!(f, "Open"),
            PrivilegeLevel::Elevated => write!(f, "Elevated"),
            PrivilegeLevel::Admin => write!(f, "Admin"),
            PrivilegeLevel::Critical => write!(f, "Critical"),
        }
    }
}

/// Proof of authentication at various levels.
#[derive(Debug, Clone)]
pub enum AuthProof {
    Totp(String),
    TotpAndPassword(String, String),
    TotpPasswordWeblink(String, String, String),
}

/// Session state tracking privilege level and expiry.
#[derive(Debug, Clone)]
pub struct PrivilegeSession {
    pub level: PrivilegeLevel,
    pub granted_at: u64,
    pub ttl_secs: u64,
}

impl PrivilegeSession {
    /// Returns true if the session has expired.
    /// A TTL of 0 means the session was consumed (single-use) and is expired.
    pub fn is_expired(&self) -> bool {
        if self.ttl_secs == 0 {
            return true; // single-use consumed
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now > self.granted_at + self.ttl_secs
    }
}

/// Challenge returned when a session has insufficient privilege for an ability.
#[derive(Debug, Clone)]
pub struct PrivilegeChallenge {
    pub required_level: PrivilegeLevel,
    pub current_level: PrivilegeLevel,
    pub message: String,
}

impl std::fmt::Display for PrivilegeChallenge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

/// Default TTL for Level 1 (Elevated) sessions: 1 hour.
const DEFAULT_LEVEL_1_TTL: u64 = 3600;

/// Default TTL for Level 2 (Admin) sessions: 15 minutes.
const DEFAULT_LEVEL_2_TTL: u64 = 900;

/// Guard that enforces tiered privilege requirements for abilities.
pub struct PrivilegeGuard {
    privilege_map: HashMap<String, PrivilegeLevel>,
    sessions: Mutex<HashMap<String, PrivilegeSession>>,
    level_1_ttl: u64,
    level_2_ttl: u64,
}

impl std::fmt::Debug for PrivilegeGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrivilegeGuard")
            .field("privilege_map_size", &self.privilege_map.len())
            .field("level_1_ttl", &self.level_1_ttl)
            .field("level_2_ttl", &self.level_2_ttl)
            .finish()
    }
}

fn lock_mutex<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

impl PrivilegeGuard {
    /// Create a new PrivilegeGuard with the default privilege map and TTLs.
    pub fn new() -> Self {
        Self {
            privilege_map: default_privilege_map(),
            sessions: Mutex::new(HashMap::new()),
            level_1_ttl: DEFAULT_LEVEL_1_TTL,
            level_2_ttl: DEFAULT_LEVEL_2_TTL,
        }
    }

    /// Create a PrivilegeGuard from a custom configuration.
    ///
    /// `config` maps level names ("open", "elevated", "admin", "critical") to lists
    /// of ability strings assigned to that level.
    pub fn with_config(config: &HashMap<String, Vec<String>>, l1_ttl: u64, l2_ttl: u64) -> Self {
        let mut privilege_map = HashMap::new();

        for (level_name, abilities) in config {
            let level = match level_name.to_lowercase().as_str() {
                "open" => PrivilegeLevel::Open,
                "elevated" => PrivilegeLevel::Elevated,
                "admin" => PrivilegeLevel::Admin,
                "critical" => PrivilegeLevel::Critical,
                _ => continue,
            };
            for ability in abilities {
                privilege_map.insert(ability.clone(), level);
            }
        }

        Self {
            privilege_map,
            sessions: Mutex::new(HashMap::new()),
            level_1_ttl: l1_ttl,
            level_2_ttl: l2_ttl,
        }
    }

    /// Check if the given session has sufficient privilege for the ability.
    ///
    /// Returns `Ok(())` if the session meets or exceeds the required level,
    /// or `Err(PrivilegeChallenge)` describing what authentication is needed.
    pub fn check(&self, ability: &str, session_id: &str) -> Result<(), PrivilegeChallenge> {
        let required = self.required_level(ability);

        if required == PrivilegeLevel::Open {
            return Ok(());
        }

        let sessions = lock_mutex(&self.sessions);
        let current_level = match sessions.get(session_id) {
            Some(session) if !session.is_expired() => session.level,
            _ => PrivilegeLevel::Open,
        };

        if current_level >= required {
            Ok(())
        } else {
            let message = match required {
                PrivilegeLevel::Open => unreachable!(),
                PrivilegeLevel::Elevated => {
                    format!(
                        "Ability '{}' requires Elevated privilege (TOTP authentication needed)",
                        ability
                    )
                }
                PrivilegeLevel::Admin => {
                    format!(
                        "Ability '{}' requires Admin privilege (TOTP + password authentication needed)",
                        ability
                    )
                }
                PrivilegeLevel::Critical => {
                    format!(
                        "Ability '{}' requires Critical privilege (TOTP + password + weblink authentication needed)",
                        ability
                    )
                }
            };

            Err(PrivilegeChallenge {
                required_level: required,
                current_level,
                message,
            })
        }
    }

    /// Grant a privilege level to a session with the appropriate TTL.
    ///
    /// - Open: no session needed (no-op)
    /// - Elevated: TTL = level_1_ttl (default 1 hour)
    /// - Admin: TTL = level_2_ttl (default 15 minutes)
    /// - Critical: TTL = 1 second (effectively single-use, consumed via `consume_critical`)
    pub fn elevate(&self, session_id: &str, level: PrivilegeLevel) {
        if level == PrivilegeLevel::Open {
            return;
        }

        let ttl = match level {
            PrivilegeLevel::Open => return,
            PrivilegeLevel::Elevated => self.level_1_ttl,
            PrivilegeLevel::Admin => self.level_2_ttl,
            PrivilegeLevel::Critical => 86400, // 24h, but consumed after single use
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let session = PrivilegeSession {
            level,
            granted_at: now,
            ttl_secs: ttl,
        };

        let mut sessions = lock_mutex(&self.sessions);
        sessions.insert(session_id.to_string(), session);
    }

    /// Consume a Critical-level session after single use.
    ///
    /// Sets the TTL to 0, marking the session as expired.
    pub fn consume_critical(&self, session_id: &str) {
        let mut sessions = lock_mutex(&self.sessions);
        if let Some(session) = sessions.get_mut(session_id) {
            if session.level == PrivilegeLevel::Critical {
                session.ttl_secs = 0;
            }
        }
    }

    /// Get the required privilege level for an ability.
    ///
    /// Unknown abilities default to Open (Level 0).
    pub fn required_level(&self, ability: &str) -> PrivilegeLevel {
        self.privilege_map
            .get(ability)
            .copied()
            .unwrap_or(PrivilegeLevel::Open)
    }
}

/// Build the default privilege map covering all 59 abilities.
fn default_privilege_map() -> HashMap<String, PrivilegeLevel> {
    let mut map = HashMap::new();

    // Level 0 — Open (32 abilities): no auth needed
    let open_abilities = [
        "storage.get",
        "storage.set",
        "memory.search",
        "memory.store",
        "nlp.sentiment",
        "nlp.summarize",
        "data.analyze",
        "data.extract_json",
        "data.template",
        "data.transform",
        "files.read",
        "files.list",
        "flow.branch",
        "flow.stop",
        "schedule.delay",
        "calendar.list",
        "browser.fetch",
        "browser.screenshot",
        "trading.get_price",
        "docs.read_pdf",
        "git.status",
        "git.diff",
        "tracking.check",
        "coupon.validate",
        "research.wide",
        "notify.user",
        "data.fetch_url",
        "home.list_entities",
        "home.get_state",
        "db.list_tables",
        "email.list",
        "email.read",
    ];
    for ability in &open_abilities {
        map.insert(ability.to_string(), PrivilegeLevel::Open);
    }

    // Level 1 — Elevated (27 abilities): TOTP required
    let elevated_abilities = [
        "shell.exec",
        "git.push",
        "git.commit",
        "git.clone",
        "db.query",
        "email.send",
        "email.reply",
        "sms.send",
        "api.call",
        "api.webhook_listen",
        "api.webhook_get",
        "home.set_state",
        "autonomous.execute",
        "deep.delegate",
        "channel.send",
        "calendar.add",
        "voice.speak",
        "data.download",
        "files.write",
        "docs.generate",
        "docs.create_spreadsheet",
        "docs.create_csv",
        "browser.set_cookies",
        "browser.fill_form",
        "browser.click",
        "coupon.generate",
        "tracking.subscribe",
    ];
    for ability in &elevated_abilities {
        map.insert(ability.to_string(), PrivilegeLevel::Elevated);
    }

    // Level 2 — Admin: TOTP + password required
    map.insert("deploy".to_string(), PrivilegeLevel::Admin);

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_abilities_pass_without_auth() {
        let guard = PrivilegeGuard::new();
        assert!(guard.check("storage.get", "session-1").is_ok());
        assert!(guard.check("nlp.sentiment", "session-1").is_ok());
        assert!(guard.check("files.read", "session-1").is_ok());
        assert!(guard.check("email.read", "session-1").is_ok());
    }

    #[test]
    fn test_elevated_abilities_fail_without_auth() {
        let guard = PrivilegeGuard::new();
        let result = guard.check("email.send", "session-1");
        assert!(result.is_err());
        let challenge = result.unwrap_err();
        assert_eq!(challenge.required_level, PrivilegeLevel::Elevated);
        assert_eq!(challenge.current_level, PrivilegeLevel::Open);
        assert!(challenge.message.contains("TOTP"));
    }

    #[test]
    fn test_elevate_then_check() {
        let guard = PrivilegeGuard::new();

        // Before elevation, email.send fails
        assert!(guard.check("email.send", "session-1").is_err());

        // Elevate to Elevated
        guard.elevate("session-1", PrivilegeLevel::Elevated);

        // Now email.send passes
        assert!(guard.check("email.send", "session-1").is_ok());
    }

    #[test]
    fn test_critical_single_use() {
        let guard = PrivilegeGuard::new();

        // Elevate to Critical
        guard.elevate("session-1", PrivilegeLevel::Critical);

        // Should pass before consumption
        assert!(guard.check("deploy", "session-1").is_ok());

        // Consume the critical session
        guard.consume_critical("session-1");

        // Should fail after consumption (TTL set to 0)
        assert!(guard.check("deploy", "session-1").is_err());
    }

    #[test]
    fn test_unknown_ability_defaults_to_open() {
        let guard = PrivilegeGuard::new();
        assert!(guard.check("totally.unknown.ability", "session-1").is_ok());
        assert_eq!(
            guard.required_level("totally.unknown.ability"),
            PrivilegeLevel::Open
        );
    }

    #[test]
    fn test_custom_config() {
        let mut config = HashMap::new();
        config.insert(
            "elevated".to_string(),
            vec!["custom.action".to_string(), "custom.write".to_string()],
        );
        config.insert("admin".to_string(), vec!["custom.deploy".to_string()]);

        let guard = PrivilegeGuard::with_config(&config, 7200, 1800);

        // custom.action requires Elevated
        assert_eq!(
            guard.required_level("custom.action"),
            PrivilegeLevel::Elevated
        );
        assert!(guard.check("custom.action", "s1").is_err());

        guard.elevate("s1", PrivilegeLevel::Elevated);
        assert!(guard.check("custom.action", "s1").is_ok());

        // custom.deploy requires Admin
        assert_eq!(guard.required_level("custom.deploy"), PrivilegeLevel::Admin);
        assert!(guard.check("custom.deploy", "s1").is_err());
    }

    #[test]
    fn test_session_expiry() {
        let guard = PrivilegeGuard::new();

        // Manually insert an already-expired session
        {
            let mut sessions = lock_mutex(&guard.sessions);
            sessions.insert(
                "expired-session".to_string(),
                PrivilegeSession {
                    level: PrivilegeLevel::Elevated,
                    granted_at: 0, // epoch — definitely expired
                    ttl_secs: 1,
                },
            );
        }

        // Elevated ability should fail because session is expired
        assert!(guard.check("email.send", "expired-session").is_err());
    }

    #[test]
    fn test_required_level() {
        let guard = PrivilegeGuard::new();
        assert_eq!(guard.required_level("storage.get"), PrivilegeLevel::Open);
        assert_eq!(guard.required_level("email.send"), PrivilegeLevel::Elevated);
        assert_eq!(guard.required_level("shell.exec"), PrivilegeLevel::Elevated);
        assert_eq!(guard.required_level("deploy"), PrivilegeLevel::Admin);
        // Unknown defaults to Open
        assert_eq!(guard.required_level("unknown"), PrivilegeLevel::Open);
    }

    #[test]
    fn test_higher_level_covers_lower() {
        let guard = PrivilegeGuard::new();

        // Admin session should pass Elevated checks
        guard.elevate("admin-session", PrivilegeLevel::Admin);
        assert!(guard.check("email.send", "admin-session").is_ok());
        assert!(guard.check("shell.exec", "admin-session").is_ok());
        assert!(guard.check("storage.get", "admin-session").is_ok());
        assert!(guard.check("deploy", "admin-session").is_ok());
    }

    #[test]
    fn test_display_impl() {
        assert_eq!(format!("{}", PrivilegeLevel::Open), "Open");
        assert_eq!(format!("{}", PrivilegeLevel::Elevated), "Elevated");
        assert_eq!(format!("{}", PrivilegeLevel::Admin), "Admin");
        assert_eq!(format!("{}", PrivilegeLevel::Critical), "Critical");
    }
}
