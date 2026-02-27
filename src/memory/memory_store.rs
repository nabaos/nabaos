//! SQLite-backed conversation turn storage.
//!
//! Stores user/assistant/system turns with token estimates for memory management.

use crate::core::error::Result;
use rusqlite::{params, Connection};
use std::path::Path;

/// Role of a conversation participant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnRole {
    User,
    Assistant,
    System,
}

impl TurnRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            TurnRole::User => "user",
            TurnRole::Assistant => "assistant",
            TurnRole::System => "system",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "user" => Some(Self::User),
            "assistant" => Some(Self::Assistant),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

impl std::fmt::Display for TurnRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A single conversation turn.
#[derive(Debug, Clone)]
pub struct ConversationTurn {
    pub id: i64,
    pub session_id: String,
    pub role: TurnRole,
    pub content: String,
    pub token_estimate: u32,
    pub created_at: i64, // unix millis
}

/// SQLite-backed conversation memory store.
pub struct MemoryStore {
    conn: Connection,
}

impl MemoryStore {
    /// Open (or create) the memory store at the given path.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS conversation_turns (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                token_estimate INTEGER NOT NULL,
                created_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_turns_session_time
                ON conversation_turns(session_id, created_at);",
        )?;
        Ok(Self { conn })
    }

    /// Add a conversation turn. Returns the row ID.
    /// Token estimate is `content.len() / 4` (rough approximation).
    pub fn add_turn(&self, session_id: &str, role: TurnRole, content: &str) -> Result<i64> {
        let token_estimate = (content.len() as u32) / 4;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        self.conn.execute(
            "INSERT INTO conversation_turns (session_id, role, content, token_estimate, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, role.as_str(), content, token_estimate, now_ms],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Retrieve the most recent N turns for a session, in chronological order.
    pub fn recent_turns(&self, session_id: &str, limit: u32) -> Result<Vec<ConversationTurn>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, token_estimate, created_at
             FROM conversation_turns
             WHERE session_id = ?1
             ORDER BY created_at DESC, id DESC
             LIMIT ?2",
        )?;

        let mut turns: Vec<ConversationTurn> = stmt
            .query_map(params![session_id, limit], |row| {
                Ok(ConversationTurn {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: TurnRole::from_str(&row.get::<_, String>(2)?).unwrap_or(TurnRole::System),
                    content: row.get(3)?,
                    token_estimate: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        turns.reverse(); // chronological order
        Ok(turns)
    }

    /// Get total token estimate for a session.
    pub fn session_token_count(&self, session_id: &str) -> Result<u32> {
        let count: u32 = self.conn.query_row(
            "SELECT COALESCE(SUM(token_estimate), 0) FROM conversation_turns WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// List all distinct session IDs.
    pub fn all_sessions(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT session_id FROM conversation_turns ORDER BY session_id")?;
        let sessions: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    /// Delete all turns for a session. Returns the number of deleted rows.
    pub fn delete_session(&self, session_id: &str) -> Result<u64> {
        let count = self.conn.execute(
            "DELETE FROM conversation_turns WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(count as u64)
    }

    /// Get all turns before a given ID (for compaction).
    pub fn turns_before(&self, session_id: &str, before_id: i64) -> Result<Vec<ConversationTurn>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, token_estimate, created_at
             FROM conversation_turns
             WHERE session_id = ?1 AND id < ?2
             ORDER BY created_at ASC, id ASC",
        )?;

        let turns: Vec<ConversationTurn> = stmt
            .query_map(params![session_id, before_id], |row| {
                Ok(ConversationTurn {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: TurnRole::from_str(&row.get::<_, String>(2)?).unwrap_or(TurnRole::System),
                    content: row.get(3)?,
                    token_estimate: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(turns)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_test_store() -> (TempDir, MemoryStore) {
        let dir = TempDir::new().unwrap();
        let store = MemoryStore::open(&dir.path().join("memory.db")).unwrap();
        (dir, store)
    }

    #[test]
    fn test_open_creates_table() {
        let (_dir, store) = open_test_store();
        // If open succeeded, table was created. Verify by inserting.
        let id = store.add_turn("s1", TurnRole::User, "hello").unwrap();
        assert!(id > 0);
    }

    #[test]
    fn test_add_and_retrieve_turns() {
        let (_dir, store) = open_test_store();
        store.add_turn("s1", TurnRole::User, "hello").unwrap();
        store
            .add_turn("s1", TurnRole::Assistant, "hi there")
            .unwrap();
        store
            .add_turn("s1", TurnRole::User, "how are you?")
            .unwrap();

        let turns = store.recent_turns("s1", 10).unwrap();
        assert_eq!(turns.len(), 3);
        assert_eq!(turns[0].role, TurnRole::User);
        assert_eq!(turns[0].content, "hello");
        assert_eq!(turns[1].role, TurnRole::Assistant);
        assert_eq!(turns[1].content, "hi there");
        assert_eq!(turns[2].role, TurnRole::User);
        assert_eq!(turns[2].content, "how are you?");
    }

    #[test]
    fn test_recent_turns_limit() {
        let (_dir, store) = open_test_store();
        for i in 0..5 {
            store
                .add_turn("s1", TurnRole::User, &format!("msg {}", i))
                .unwrap();
        }

        let turns = store.recent_turns("s1", 2).unwrap();
        assert_eq!(turns.len(), 2);
        // Should be the last 2 turns
        assert_eq!(turns[0].content, "msg 3");
        assert_eq!(turns[1].content, "msg 4");
    }

    #[test]
    fn test_session_token_count() {
        let (_dir, store) = open_test_store();
        store.add_turn("s1", TurnRole::User, "hello world").unwrap(); // 11 chars -> 2 tokens
        store
            .add_turn("s1", TurnRole::Assistant, "hi there friend")
            .unwrap(); // 15 chars -> 3 tokens

        let count = store.session_token_count("s1").unwrap();
        assert_eq!(count, 2 + 3); // 11/4 + 15/4 = 2 + 3
    }

    #[test]
    fn test_all_sessions() {
        let (_dir, store) = open_test_store();
        store.add_turn("session_a", TurnRole::User, "hi").unwrap();
        store
            .add_turn("session_b", TurnRole::User, "hello")
            .unwrap();
        store
            .add_turn("session_a", TurnRole::Assistant, "hey")
            .unwrap();

        let sessions = store.all_sessions().unwrap();
        assert_eq!(sessions.len(), 2);
        assert!(sessions.contains(&"session_a".to_string()));
        assert!(sessions.contains(&"session_b".to_string()));
    }

    #[test]
    fn test_delete_session() {
        let (_dir, store) = open_test_store();
        store.add_turn("s1", TurnRole::User, "hi").unwrap();
        store.add_turn("s1", TurnRole::Assistant, "hey").unwrap();
        store
            .add_turn("s2", TurnRole::User, "other session")
            .unwrap();

        let deleted = store.delete_session("s1").unwrap();
        assert_eq!(deleted, 2);

        let turns = store.recent_turns("s1", 10).unwrap();
        assert!(turns.is_empty());

        // s2 should be unaffected
        let s2_turns = store.recent_turns("s2", 10).unwrap();
        assert_eq!(s2_turns.len(), 1);
    }

    #[test]
    fn test_turn_role_roundtrip() {
        assert_eq!(
            TurnRole::from_str(TurnRole::User.as_str()),
            Some(TurnRole::User)
        );
        assert_eq!(
            TurnRole::from_str(TurnRole::Assistant.as_str()),
            Some(TurnRole::Assistant)
        );
        assert_eq!(
            TurnRole::from_str(TurnRole::System.as_str()),
            Some(TurnRole::System)
        );
        assert_eq!(TurnRole::from_str("unknown"), None);
    }

    #[test]
    fn test_token_estimate() {
        let (_dir, store) = open_test_store();
        store.add_turn("s1", TurnRole::User, "hello world").unwrap();
        let turns = store.recent_turns("s1", 1).unwrap();
        // "hello world" = 11 chars, 11/4 = 2
        assert_eq!(turns[0].token_estimate, 2);
    }
}
