use std::fmt;
use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::core::error::Result;

/// The decision recorded for a permission grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    AllowOnce,
    AllowAlways,
    Deny,
}

impl fmt::Display for PermissionDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PermissionDecision::AllowOnce => write!(f, "allow_once"),
            PermissionDecision::AllowAlways => write!(f, "allow_always"),
            PermissionDecision::Deny => write!(f, "deny"),
        }
    }
}

impl PermissionDecision {
    fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "allow_once" => Some(Self::AllowOnce),
            "allow_always" => Some(Self::AllowAlways),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// A single persisted permission grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionGrant {
    pub agent_id: String,
    pub permission: String,
    pub decision: PermissionDecision,
    pub granted_at: u64,
}

/// Manages runtime permission grants backed by SQLite.
pub struct PermissionManager {
    db: Connection,
}

impl PermissionManager {
    /// Open (or create) the permission database at `db_path`.
    /// Pass `":memory:"` for an in-memory database (useful for tests).
    pub fn open(db_path: &Path) -> Result<Self> {
        let db = if db_path.to_str() == Some(":memory:") {
            Connection::open_in_memory()?
        } else {
            Connection::open(db_path)?
        };

        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS permissions (
                agent_id   TEXT NOT NULL,
                permission TEXT NOT NULL,
                decision   TEXT NOT NULL,
                granted_at INTEGER NOT NULL,
                PRIMARY KEY (agent_id, permission)
            );",
        )?;

        Ok(Self { db })
    }

    /// Look up the stored decision for `(agent_id, permission)`.
    /// Returns `None` if no grant exists (AllowOnce is never stored).
    pub fn check(&self, agent_id: &str, permission: &str) -> Result<Option<PermissionDecision>> {
        let mut stmt = self
            .db
            .prepare("SELECT decision FROM permissions WHERE agent_id = ?1 AND permission = ?2")?;
        let result = stmt.query_row(params![agent_id, permission], |row| {
            let val: String = row.get(0)?;
            Ok(val)
        });

        match result {
            Ok(val) => Ok(PermissionDecision::from_str_value(&val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Record a permission decision.
    /// `AllowOnce` is ephemeral and is **not** persisted — the method returns
    /// `Ok(())` immediately.
    pub fn grant(
        &self,
        agent_id: &str,
        permission: &str,
        decision: PermissionDecision,
    ) -> Result<()> {
        if decision == PermissionDecision::AllowOnce {
            return Ok(());
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.db.execute(
            "INSERT OR REPLACE INTO permissions (agent_id, permission, decision, granted_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![agent_id, permission, decision.to_string(), now],
        )?;
        Ok(())
    }

    /// Hierarchical permission check with scope fallback.
    ///
    /// Checks in order:
    ///   1. `"email.send:bob@example.com"` (exact scoped match)
    ///   2. `"email.send:*"` (wildcard scope)
    ///   3. `"email.send"` (base ability without scope)
    ///
    /// Returns `None` if no matching grant exists (needs user prompt).
    pub fn check_scoped(
        &self,
        agent_id: &str,
        ability: &str,
        target: &str,
    ) -> Result<Option<PermissionDecision>> {
        // 1. Exact scoped: "email.send:bob@example.com"
        let scoped = format!("{}:{}", ability, target);
        if let Some(decision) = self.check(agent_id, &scoped)? {
            return Ok(Some(decision));
        }
        // 2. Wildcard scope: "email.send:*"
        let wildcard = format!("{}:*", ability);
        if let Some(decision) = self.check(agent_id, &wildcard)? {
            return Ok(Some(decision));
        }
        // 3. Base ability: "email.send"
        self.check(agent_id, ability)
    }

    /// Revoke all permissions for a given agent. Returns the number of rows deleted.
    pub fn revoke_all(&self, agent_id: &str) -> Result<usize> {
        let count = self.db.execute(
            "DELETE FROM permissions WHERE agent_id = ?1",
            params![agent_id],
        )?;
        Ok(count)
    }

    /// List all permission grants for a given agent.
    pub fn list(&self, agent_id: &str) -> Result<Vec<PermissionGrant>> {
        let mut stmt = self.db.prepare(
            "SELECT agent_id, permission, decision, granted_at FROM permissions WHERE agent_id = ?1",
        )?;
        let rows = stmt.query_map(params![agent_id], |row| {
            Ok(PermissionGrant {
                agent_id: row.get(0)?,
                permission: row.get(1)?,
                decision: PermissionDecision::from_str_value(&row.get::<_, String>(2)?)
                    .unwrap_or(PermissionDecision::Deny),
                granted_at: row.get(3)?,
            })
        })?;

        let mut grants = Vec::new();
        for row in rows {
            grants.push(row?);
        }
        Ok(grants)
    }

    /// Return distinct agent ids that have at least one permission grant.
    pub fn list_agents(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .db
            .prepare("SELECT DISTINCT agent_id FROM permissions")?;
        let rows = stmt.query_map([], |row| row.get(0))?;

        let mut agents = Vec::new();
        for row in rows {
            agents.push(row?);
        }
        Ok(agents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn memory_db() -> PermissionManager {
        PermissionManager::open(Path::new(":memory:")).unwrap()
    }

    #[test]
    fn test_grant_and_check() {
        let pm = memory_db();
        pm.grant("a1", "network", PermissionDecision::AllowAlways)
            .unwrap();
        let decision = pm.check("a1", "network").unwrap();
        assert_eq!(decision, Some(PermissionDecision::AllowAlways));
    }

    #[test]
    fn test_deny_persisted() {
        let pm = memory_db();
        pm.grant("a1", "fs_write", PermissionDecision::Deny)
            .unwrap();
        let decision = pm.check("a1", "fs_write").unwrap();
        assert_eq!(decision, Some(PermissionDecision::Deny));
    }

    #[test]
    fn test_allow_once_not_persisted() {
        let pm = memory_db();
        pm.grant("a1", "camera", PermissionDecision::AllowOnce)
            .unwrap();
        let decision = pm.check("a1", "camera").unwrap();
        assert_eq!(decision, None);
    }

    #[test]
    fn test_unknown_permission_returns_none() {
        let pm = memory_db();
        let decision = pm.check("a1", "nonexistent").unwrap();
        assert_eq!(decision, None);
    }

    #[test]
    fn test_revoke_all() {
        let pm = memory_db();
        pm.grant("a1", "network", PermissionDecision::AllowAlways)
            .unwrap();
        pm.grant("a1", "fs_write", PermissionDecision::Deny)
            .unwrap();
        let removed = pm.revoke_all("a1").unwrap();
        assert_eq!(removed, 2);
        assert_eq!(pm.check("a1", "network").unwrap(), None);
        assert_eq!(pm.check("a1", "fs_write").unwrap(), None);
    }

    #[test]
    fn test_list_permissions() {
        let pm = memory_db();
        pm.grant("a1", "network", PermissionDecision::AllowAlways)
            .unwrap();
        pm.grant("a1", "fs_write", PermissionDecision::Deny)
            .unwrap();
        let grants = pm.list("a1").unwrap();
        assert_eq!(grants.len(), 2);
    }

    #[test]
    fn test_check_scoped_exact() {
        let pm = memory_db();
        pm.grant("a1", "email.send:bob@x.com", PermissionDecision::AllowAlways)
            .unwrap();
        let decision = pm.check_scoped("a1", "email.send", "bob@x.com").unwrap();
        assert_eq!(decision, Some(PermissionDecision::AllowAlways));
    }

    #[test]
    fn test_check_scoped_wildcard() {
        let pm = memory_db();
        pm.grant("a1", "email.send:*", PermissionDecision::AllowAlways)
            .unwrap();
        let decision = pm.check_scoped("a1", "email.send", "anyone@x.com").unwrap();
        assert_eq!(decision, Some(PermissionDecision::AllowAlways));
    }

    #[test]
    fn test_check_scoped_fallback_to_base() {
        let pm = memory_db();
        pm.grant("a1", "email.send", PermissionDecision::Deny)
            .unwrap();
        let decision = pm.check_scoped("a1", "email.send", "bob@x.com").unwrap();
        assert_eq!(decision, Some(PermissionDecision::Deny));
    }

    #[test]
    fn test_check_scoped_no_match() {
        let pm = memory_db();
        let decision = pm.check_scoped("a1", "email.send", "bob@x.com").unwrap();
        assert_eq!(decision, None);
    }

    #[test]
    fn test_list_agents() {
        let pm = memory_db();
        pm.grant("alpha", "network", PermissionDecision::AllowAlways)
            .unwrap();
        pm.grant("beta", "fs_write", PermissionDecision::Deny)
            .unwrap();
        let agents = pm.list_agents().unwrap();
        assert_eq!(agents.len(), 2);
    }
}
