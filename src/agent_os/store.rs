use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection};

use super::package::extract_package;
use super::types::{AgentState, InstalledAgent};
use crate::core::error::{NyayaError, Result};

/// Agent store backed by SQLite — manages installed agents.
pub struct AgentStore {
    db: Connection,
    agents_dir: PathBuf,
}

impl AgentStore {
    /// Open (or create) the agent store at `db_path` with agents installed under `agents_dir`.
    /// Pass `":memory:"` for an in-memory database (useful for tests).
    pub fn open(db_path: &Path, agents_dir: &Path) -> Result<Self> {
        let db = if db_path.to_str() == Some(":memory:") {
            Connection::open_in_memory()?
        } else {
            if let Some(parent) = db_path.parent() {
                fs::create_dir_all(parent)?;
            }
            Connection::open(db_path)?
        };

        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS agents (
                id          TEXT PRIMARY KEY,
                version     TEXT NOT NULL,
                state       TEXT NOT NULL DEFAULT 'stopped',
                data_dir    TEXT NOT NULL,
                installed_at INTEGER NOT NULL,
                updated_at  INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS agent_versions (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                agent_id    TEXT NOT NULL,
                version     TEXT NOT NULL,
                installed_at INTEGER NOT NULL,
                FOREIGN KEY (agent_id) REFERENCES agents(id)
            );",
        )?;

        fs::create_dir_all(agents_dir)?;

        Ok(Self {
            db,
            agents_dir: agents_dir.to_path_buf(),
        })
    }

    /// Install a .nap package. Extracts, copies to agents_dir, registers in DB.
    /// Rejects duplicates (same agent id already installed).
    pub fn install(&self, nap_path: &Path) -> Result<InstalledAgent> {
        // Create a temporary directory for extraction
        let tmp_base = std::env::temp_dir().join(format!(
            "nyaya-install-{}",
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&tmp_base)?;
        let metadata = extract_package(nap_path, &tmp_base)?;

        // Check for duplicate
        if self.get(&metadata.name)?.is_some() {
            return Err(NyayaError::Config(format!(
                "Agent '{}' is already installed",
                metadata.name
            )));
        }

        let agent_dir = self.agents_dir.join(&metadata.name);
        fs::create_dir_all(&agent_dir)?;
        copy_dir_contents(&tmp_base, &agent_dir)?;

        // Clean up temp directory
        let _ = fs::remove_dir_all(&tmp_base);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.db.execute(
            "INSERT INTO agents (id, version, state, data_dir, installed_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                metadata.name,
                metadata.version,
                "stopped",
                agent_dir.to_string_lossy().to_string(),
                now,
                now,
            ],
        )?;

        self.db.execute(
            "INSERT INTO agent_versions (agent_id, version, installed_at)
             VALUES (?1, ?2, ?3)",
            params![metadata.name, metadata.version, now],
        )?;

        Ok(InstalledAgent {
            id: metadata.name,
            version: metadata.version,
            state: AgentState::Stopped,
            data_dir: agent_dir,
            installed_at: now,
            updated_at: now,
        })
    }

    /// Look up an installed agent by ID.
    pub fn get(&self, agent_id: &str) -> Result<Option<InstalledAgent>> {
        let mut stmt = self.db.prepare(
            "SELECT id, version, state, data_dir, installed_at, updated_at
             FROM agents WHERE id = ?1",
        )?;

        let result = stmt.query_row(params![agent_id], |row| {
            let state_str: String = row.get(2)?;
            let state = match state_str.as_str() {
                "running" => AgentState::Running,
                "paused" => AgentState::Paused,
                "disabled" => AgentState::Disabled,
                _ => AgentState::Stopped,
            };
            Ok(InstalledAgent {
                id: row.get(0)?,
                version: row.get(1)?,
                state,
                data_dir: PathBuf::from(row.get::<_, String>(3)?),
                installed_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        });

        match result {
            Ok(agent) => Ok(Some(agent)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// List all installed agents.
    pub fn list(&self) -> Result<Vec<InstalledAgent>> {
        let mut stmt = self
            .db
            .prepare("SELECT id, version, state, data_dir, installed_at, updated_at FROM agents")?;

        let rows = stmt.query_map([], |row| {
            let state_str: String = row.get(2)?;
            let state = match state_str.as_str() {
                "running" => AgentState::Running,
                "paused" => AgentState::Paused,
                "disabled" => AgentState::Disabled,
                _ => AgentState::Stopped,
            };
            Ok(InstalledAgent {
                id: row.get(0)?,
                version: row.get(1)?,
                state,
                data_dir: PathBuf::from(row.get::<_, String>(3)?),
                installed_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;

        let mut agents = Vec::new();
        for row in rows {
            agents.push(row?);
        }
        Ok(agents)
    }

    /// Update the state of an installed agent.
    pub fn set_state(&self, agent_id: &str, state: AgentState) -> Result<()> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let affected = self.db.execute(
            "UPDATE agents SET state = ?1, updated_at = ?2 WHERE id = ?3",
            params![state.to_string(), now, agent_id],
        )?;

        if affected == 0 {
            return Err(NyayaError::Config(format!(
                "Agent '{}' not found",
                agent_id
            )));
        }
        Ok(())
    }

    /// Disable an agent (convenience wrapper).
    pub fn disable(&self, agent_id: &str) -> Result<()> {
        self.set_state(agent_id, AgentState::Disabled)
    }

    /// Enable an agent (set to Stopped so it can be started).
    pub fn enable(&self, agent_id: &str) -> Result<()> {
        self.set_state(agent_id, AgentState::Stopped)
    }

    /// Uninstall an agent — removes data directory and DB entries.
    pub fn uninstall(&self, agent_id: &str) -> Result<()> {
        // Get agent to find data dir
        let agent = self
            .get(agent_id)?
            .ok_or_else(|| NyayaError::Config(format!("Agent '{}' not found", agent_id)))?;

        // Remove data directory
        if agent.data_dir.exists() {
            fs::remove_dir_all(&agent.data_dir)?;
        }

        // Remove from DB
        self.db.execute(
            "DELETE FROM agent_versions WHERE agent_id = ?1",
            params![agent_id],
        )?;
        self.db
            .execute("DELETE FROM agents WHERE id = ?1", params![agent_id])?;

        Ok(())
    }

    /// Install an agent directly from an unpacked directory (not a .nap archive).
    pub fn install_from_dir(&self, agent_dir: &std::path::Path) -> Result<InstalledAgent> {
        let manifest_path = agent_dir.join("manifest.yaml");
        let yaml = std::fs::read_to_string(&manifest_path)
            .map_err(|e| NyayaError::Config(format!("Failed to read manifest: {}", e)))?;
        let metadata: super::package::PackageMetadata = serde_yaml::from_str(&yaml)?;
        metadata.validate()?;

        if self.get(&metadata.name)?.is_some() {
            return Err(NyayaError::Config(format!(
                "Agent '{}' is already installed. Uninstall first.",
                metadata.name
            )));
        }

        let dest = self.agents_dir.join(&metadata.name);
        copy_dir_recursive(agent_dir, &dest)?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.db.execute(
            "INSERT INTO agents (id, version, state, data_dir, installed_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                metadata.name,
                metadata.version,
                "stopped",
                dest.to_string_lossy().to_string(),
                now,
                now,
            ],
        )?;

        self.db.execute(
            "INSERT INTO agent_versions (agent_id, version, installed_at) \
             VALUES (?1, ?2, ?3)",
            params![metadata.name, metadata.version, now],
        )?;

        Ok(InstalledAgent {
            id: metadata.name,
            version: metadata.version,
            state: AgentState::Stopped,
            data_dir: dest,
            installed_at: now,
            updated_at: now,
        })
    }

    /// List version history for an agent.
    pub fn version_history(&self, agent_id: &str) -> Result<Vec<(String, u64)>> {
        let mut stmt = self.db.prepare(
            "SELECT version, installed_at FROM agent_versions
             WHERE agent_id = ?1 ORDER BY installed_at",
        )?;

        let rows = stmt.query_map(params![agent_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })?;

        let mut history = Vec::new();
        for row in rows {
            history.push(row?);
        }
        Ok(history)
    }
}

/// Recursively copy the contents of `src` into `dst`.
pub fn copy_dir_contents(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_contents(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Recursively copy a directory tree from `src` to `dst`, creating `dst` first.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create a minimal .nap package and return its path.
    fn create_test_nap(tmp: &TempDir, name: &str, version: &str) -> PathBuf {
        let source = tmp.path().join(format!("{}_src", name));
        fs::create_dir_all(&source).unwrap();

        let manifest = format!(
            "name: {}\nversion: {}\ndescription: test agent\nauthor: tester\n",
            name, version
        );
        fs::write(source.join("manifest.yaml"), &manifest).unwrap();

        let nap_path = tmp.path().join(format!("{}.nap", name));
        super::super::package::create_package(&source, &nap_path).unwrap();
        nap_path
    }

    #[test]
    fn test_install_and_get() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("agents");
        let store = AgentStore::open(Path::new(":memory:"), &agents_dir).unwrap();

        let nap = create_test_nap(&tmp, "my-agent", "1.0.0");
        let agent = store.install(&nap).unwrap();

        assert_eq!(agent.id, "my-agent");
        assert_eq!(agent.version, "1.0.0");
        assert_eq!(agent.state, AgentState::Stopped);

        let fetched = store.get("my-agent").unwrap().unwrap();
        assert_eq!(fetched.id, "my-agent");
    }

    #[test]
    fn test_list() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("agents");
        let store = AgentStore::open(Path::new(":memory:"), &agents_dir).unwrap();

        let nap_a = create_test_nap(&tmp, "agent-a", "1.0.0");
        let nap_b = create_test_nap(&tmp, "agent-b", "2.0.0");
        store.install(&nap_a).unwrap();
        store.install(&nap_b).unwrap();

        let all = store.list().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_disable_enable() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("agents");
        let store = AgentStore::open(Path::new(":memory:"), &agents_dir).unwrap();

        let nap = create_test_nap(&tmp, "my-agent", "1.0.0");
        store.install(&nap).unwrap();

        store.disable("my-agent").unwrap();
        let agent = store.get("my-agent").unwrap().unwrap();
        assert_eq!(agent.state, AgentState::Disabled);

        store.enable("my-agent").unwrap();
        let agent = store.get("my-agent").unwrap().unwrap();
        assert_eq!(agent.state, AgentState::Stopped);
    }

    #[test]
    fn test_uninstall() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("agents");
        let store = AgentStore::open(Path::new(":memory:"), &agents_dir).unwrap();

        let nap = create_test_nap(&tmp, "my-agent", "1.0.0");
        store.install(&nap).unwrap();
        assert!(store.get("my-agent").unwrap().is_some());

        store.uninstall("my-agent").unwrap();
        assert!(store.get("my-agent").unwrap().is_none());
    }

    #[test]
    fn test_duplicate_rejected() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("agents");
        let store = AgentStore::open(Path::new(":memory:"), &agents_dir).unwrap();

        let nap = create_test_nap(&tmp, "my-agent", "1.0.0");
        store.install(&nap).unwrap();

        // Create second nap with same name
        let nap2 = create_test_nap(&tmp, "my-agent", "2.0.0");
        let result = store.install(&nap2);
        assert!(result.is_err());
    }

    #[test]
    fn test_version_history() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("agents");
        let store = AgentStore::open(Path::new(":memory:"), &agents_dir).unwrap();

        let nap = create_test_nap(&tmp, "my-agent", "1.0.0");
        store.install(&nap).unwrap();

        let history = store.version_history("my-agent").unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].0, "1.0.0");
    }

    #[test]
    fn test_install_from_dir() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("agents");
        let agent_src = tmp.path().join("src_agent");
        fs::create_dir_all(&agent_src).unwrap();

        // Create a minimal manifest.yaml
        fs::write(
            agent_src.join("manifest.yaml"),
            "name: test-agent\nversion: 1.0.0\ndescription: test\nauthor: test\npermissions: []\n",
        )
        .unwrap();

        let store = AgentStore::open(Path::new(":memory:"), &agents_dir).unwrap();
        let result = store.install_from_dir(&agent_src);
        assert!(result.is_ok());
        let installed = result.unwrap();
        assert_eq!(installed.id, "test-agent");
        assert_eq!(installed.version, "1.0.0");

        // Verify it shows in list
        let list = store.list().unwrap();
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_install_from_dir_duplicate_rejected() {
        let tmp = TempDir::new().unwrap();
        let agents_dir = tmp.path().join("agents");
        let agent_src = tmp.path().join("src_agent");
        fs::create_dir_all(&agent_src).unwrap();
        fs::write(
            agent_src.join("manifest.yaml"),
            "name: dup-agent\nversion: 1.0.0\ndescription: test\nauthor: test\npermissions: []\n",
        )
        .unwrap();

        let store = AgentStore::open(Path::new(":memory:"), &agents_dir).unwrap();
        store.install_from_dir(&agent_src).unwrap();
        let result = store.install_from_dir(&agent_src);
        assert!(result.is_err());
    }
}
