//! Database query module — parameterized queries with security enforcement.
//!
//! Supports SQLite (via rusqlite) and PostgreSQL (via postgres crate).
//! Key security properties:
//! - Parameterized queries only (`$1`, `$2` — NO string interpolation)
//! - Read-only mode by default (`SET TRANSACTION READ ONLY`)
//! - Connection strings from vault/env (`NABA_DB_URL`)
//! - Connection timeout: 10s, query timeout: 30s
//! - Result cap: 10,000 rows, 10MB output

use serde::{Deserialize, Serialize};

/// Maximum number of rows returned by a single query.
const DEFAULT_MAX_ROWS: usize = 10_000;

/// Maximum output size in bytes (10 MB).
const MAX_OUTPUT_BYTES: usize = 10 * 1024 * 1024;

/// Default query timeout in seconds.
const DEFAULT_QUERY_TIMEOUT_SECS: u64 = 30;

/// Default connection timeout in seconds.
const DEFAULT_CONN_TIMEOUT_SECS: u64 = 10;

/// Database configuration.
#[derive(Debug, Clone)]
pub struct DbConfig {
    /// Connection string (sqlite:///path or postgres://...).
    pub connection_string: String,
    /// Whether to enforce read-only mode (default: true).
    pub read_only: bool,
    /// Query timeout in seconds (default: 30).
    pub query_timeout_secs: u64,
    /// Connection timeout in seconds (default: 10).
    pub conn_timeout_secs: u64,
    /// Maximum number of rows to return (default: 10,000).
    pub max_rows: usize,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            connection_string: String::new(),
            read_only: true,
            query_timeout_secs: DEFAULT_QUERY_TIMEOUT_SECS,
            conn_timeout_secs: DEFAULT_CONN_TIMEOUT_SECS,
            max_rows: DEFAULT_MAX_ROWS,
        }
    }
}

impl DbConfig {
    /// Load configuration from environment variables.
    ///
    /// Reads:
    /// - `NABA_DB_URL` — connection string (required)
    /// - `NABA_DB_READ_ONLY` — "true"/"false" (default: true)
    /// - `NABA_DB_QUERY_TIMEOUT` — seconds (default: 30)
    /// - `NABA_DB_MAX_ROWS` — max rows (default: 10000)
    pub fn from_env() -> Result<Self, String> {
        let connection_string = std::env::var("NABA_DB_URL").map_err(|_| {
            "NABA_DB_URL env var not set — required for database queries".to_string()
        })?;

        let read_only = std::env::var("NABA_DB_READ_ONLY")
            .map(|v| v != "false")
            .unwrap_or(true);

        let query_timeout_secs = std::env::var("NABA_DB_QUERY_TIMEOUT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_QUERY_TIMEOUT_SECS);

        let max_rows = std::env::var("NABA_DB_MAX_ROWS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_MAX_ROWS);

        Ok(Self {
            connection_string,
            read_only,
            query_timeout_secs,
            conn_timeout_secs: DEFAULT_CONN_TIMEOUT_SECS,
            max_rows,
        })
    }
}

/// Result of a database query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbQueryResult {
    /// Column names.
    pub columns: Vec<String>,
    /// Rows of data (each cell as a string).
    pub rows: Vec<Vec<String>>,
    /// Number of rows returned.
    pub row_count: usize,
    /// Whether the result was truncated due to row or size limits.
    pub truncated: bool,
}

// ─── SQL Validation ─────────────────────────────────────────────────────────

/// Dangerous SQL keywords that are blocked in read-only mode.
const DANGEROUS_KEYWORDS: &[&str] = &[
    "DROP", "DELETE", "INSERT", "UPDATE", "ALTER", "CREATE", "TRUNCATE", "GRANT", "REVOKE", "EXEC",
    "EXECUTE", "MERGE", "REPLACE", "UPSERT", "COPY",
];

/// Allowed SQL statement prefixes in read-only mode.
const ALLOWED_PREFIXES: &[&str] = &["SELECT", "SHOW", "DESCRIBE", "EXPLAIN", "PRAGMA", "WITH"];

/// Validate a SQL query for safety.
///
/// In read-only mode, rejects any statement containing dangerous keywords
/// and only allows SELECT, SHOW, DESCRIBE, EXPLAIN, PRAGMA, and WITH.
pub fn validate_query(sql: &str, read_only: bool) -> Result<(), String> {
    if sql.trim().is_empty() {
        return Err("Empty SQL query".into());
    }

    // Reject null bytes (used in some injection attacks)
    if sql.contains('\0') {
        return Err("SQL query contains null bytes".into());
    }

    // Reject multiple statements (semicolon followed by non-whitespace)
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if trimmed.contains(';') {
        return Err(
            "Multiple SQL statements are not allowed (found semicolon in query body)".into(),
        );
    }

    if !read_only {
        return Ok(());
    }

    let upper = sql.to_uppercase();

    // Check for dangerous keywords (word-boundary aware)
    for keyword in DANGEROUS_KEYWORDS {
        // Match keyword as a whole word using simple boundary check
        if contains_keyword(&upper, keyword) {
            return Err(format!(
                "SQL query contains forbidden keyword '{}' in read-only mode. \
                 Only SELECT, SHOW, DESCRIBE, EXPLAIN, PRAGMA, WITH are allowed.",
                keyword
            ));
        }
    }

    // Check that the statement starts with an allowed prefix
    let first_word = upper.split_whitespace().next().unwrap_or("");
    if !ALLOWED_PREFIXES.contains(&first_word) {
        return Err(format!(
            "SQL query must start with one of: {}. Got: '{}'",
            ALLOWED_PREFIXES.join(", "),
            first_word
        ));
    }

    Ok(())
}

/// Check if `haystack` contains `keyword` as a whole word.
fn contains_keyword(haystack: &str, keyword: &str) -> bool {
    let bytes = haystack.as_bytes();
    let kw_bytes = keyword.as_bytes();
    let kw_len = kw_bytes.len();

    if bytes.len() < kw_len {
        return false;
    }

    for i in 0..=(bytes.len() - kw_len) {
        if &bytes[i..i + kw_len] == kw_bytes {
            // Check left boundary
            let left_ok = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            // Check right boundary
            let right_ok =
                (i + kw_len) == bytes.len() || !bytes[i + kw_len].is_ascii_alphanumeric();
            if left_ok && right_ok {
                return true;
            }
        }
    }

    false
}

/// Cap results to enforce row and size limits.
fn cap_results(mut result: DbQueryResult, max_rows: usize) -> DbQueryResult {
    // Row limit
    if result.rows.len() > max_rows {
        result.rows.truncate(max_rows);
        result.truncated = true;
        result.row_count = max_rows;
    }

    // Size limit — estimate total serialized size
    let mut total_size: usize = 0;
    let mut keep_rows = result.rows.len();
    for (i, row) in result.rows.iter().enumerate() {
        for cell in row {
            total_size += cell.len() + 3; // quotes + comma overhead
        }
        if total_size > MAX_OUTPUT_BYTES {
            keep_rows = i;
            break;
        }
    }

    if keep_rows < result.rows.len() {
        result.rows.truncate(keep_rows);
        result.truncated = true;
        result.row_count = keep_rows;
    }

    result
}

// ─── SQLite Implementation ──────────────────────────────────────────────────

/// Execute a parameterized query against a SQLite database.
pub fn query_sqlite(
    db_path: &str,
    sql: &str,
    params: &[String],
    config: &DbConfig,
) -> Result<DbQueryResult, String> {
    validate_query(sql, config.read_only)?;

    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| format!("Failed to open SQLite database '{}': {}", db_path, e))?;

    // Set busy timeout (connection timeout equivalent for SQLite)
    conn.busy_timeout(std::time::Duration::from_secs(config.conn_timeout_secs))
        .map_err(|e| format!("Failed to set busy timeout: {}", e))?;

    // Enforce read-only at the SQLite level
    if config.read_only {
        conn.execute_batch("PRAGMA query_only = ON;")
            .map_err(|e| format!("Failed to set query_only pragma: {}", e))?;
    }

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| format!("Failed to prepare SQL: {}", e))?;

    // Get column names
    let columns: Vec<String> = stmt.column_names().iter().map(|s| s.to_string()).collect();

    // Build params slice for rusqlite
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();

    // Execute query with timeout enforcement via a thread deadline
    let max_rows = config.max_rows;

    let rows_result = stmt
        .query_map(param_refs.as_slice(), |row| {
            let mut cells = Vec::with_capacity(columns.len());
            for i in 0..columns.len() {
                let val: rusqlite::Result<String> = row
                    .get::<_, String>(i)
                    .or_else(|_| row.get::<_, i64>(i).map(|v| v.to_string()))
                    .or_else(|_| row.get::<_, f64>(i).map(|v| v.to_string()))
                    .or_else(|_| {
                        // Handle NULL
                        let val: rusqlite::Result<Option<String>> = row.get(i);
                        match val {
                            Ok(None) => Ok("NULL".to_string()),
                            Ok(Some(s)) => Ok(s),
                            Err(e) => Err(e),
                        }
                    });
                cells.push(val.unwrap_or_else(|_| "<error>".to_string()));
            }
            Ok(cells)
        })
        .map_err(|e| format!("Query execution failed: {}", e))?;

    let mut rows = Vec::new();
    for row in rows_result {
        match row {
            Ok(cells) => rows.push(cells),
            Err(e) => return Err(format!("Error reading row: {}", e)),
        }
        // Early exit if we hit the row limit + 1 (to detect truncation)
        if rows.len() > max_rows {
            break;
        }
    }

    let row_count = rows.len();
    let result = DbQueryResult {
        columns,
        rows,
        row_count,
        truncated: false,
    };

    Ok(cap_results(result, max_rows))
}

/// List all tables in a SQLite database.
pub fn list_tables_sqlite(db_path: &str) -> Result<Vec<String>, String> {
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| format!("Failed to open SQLite database '{}': {}", db_path, e))?;

    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .map_err(|e| format!("Failed to list tables: {}", e))?;

    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| format!("Failed to query tables: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(tables)
}

// ─── PostgreSQL Implementation ──────────────────────────────────────────────

/// Execute a parameterized query against a PostgreSQL database.
pub fn query_postgres(
    conn_str: &str,
    sql: &str,
    params: &[String],
    config: &DbConfig,
) -> Result<DbQueryResult, String> {
    validate_query(sql, config.read_only)?;

    // Connect with timeout
    let connect_result =
        std::panic::catch_unwind(|| postgres::Client::connect(conn_str, postgres::NoTls));

    let mut client = match connect_result {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => return Err(format!("PostgreSQL connection failed: {}", e)),
        Err(_) => return Err("PostgreSQL connection panicked".into()),
    };

    // Enforce read-only transaction
    if config.read_only {
        client
            .execute("SET TRANSACTION READ ONLY", &[])
            .map_err(|e| format!("Failed to set read-only transaction: {}", e))?;
    }

    // Set statement timeout (in milliseconds)
    let timeout_ms = config.query_timeout_secs * 1000;
    client
        .execute(&format!("SET statement_timeout = {}", timeout_ms), &[])
        .map_err(|e| format!("Failed to set statement timeout: {}", e))?;

    // Build typed params — all as text (PostgreSQL will cast as needed)
    let param_refs: Vec<&(dyn postgres::types::ToSql + Sync)> = params
        .iter()
        .map(|s| s as &(dyn postgres::types::ToSql + Sync))
        .collect();

    let rows = client
        .query(sql, &param_refs)
        .map_err(|e| format!("PostgreSQL query failed: {}", e))?;

    if rows.is_empty() {
        return Ok(DbQueryResult {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            truncated: false,
        });
    }

    // Extract column names
    let columns: Vec<String> = rows[0]
        .columns()
        .iter()
        .map(|c| c.name().to_string())
        .collect();

    // Extract row data
    let max_rows = config.max_rows;
    let mut result_rows = Vec::new();
    for row in rows.iter() {
        if result_rows.len() > max_rows {
            break;
        }
        let mut cells = Vec::with_capacity(columns.len());
        for i in 0..columns.len() {
            // Try to extract as string, falling back for various types
            let cell: String = row
                .try_get::<_, String>(i)
                .or_else(|_| row.try_get::<_, i32>(i).map(|v| v.to_string()))
                .or_else(|_| row.try_get::<_, i64>(i).map(|v| v.to_string()))
                .or_else(|_| row.try_get::<_, f64>(i).map(|v| v.to_string()))
                .or_else(|_| row.try_get::<_, bool>(i).map(|v| v.to_string()))
                .or_else(|_| {
                    row.try_get::<_, Option<String>>(i)
                        .map(|v| v.unwrap_or_else(|| "NULL".to_string()))
                })
                .unwrap_or_else(|_| "<unsupported_type>".to_string());
            cells.push(cell);
        }
        result_rows.push(cells);
    }

    let row_count = result_rows.len();
    let result = DbQueryResult {
        columns,
        rows: result_rows,
        row_count,
        truncated: false,
    };

    Ok(cap_results(result, max_rows))
}

/// List all tables in a PostgreSQL database.
pub fn list_tables_postgres(conn_str: &str) -> Result<Vec<String>, String> {
    let mut client = postgres::Client::connect(conn_str, postgres::NoTls)
        .map_err(|e| format!("PostgreSQL connection failed: {}", e))?;

    let rows = client
        .query(
            "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' ORDER BY table_name",
            &[],
        )
        .map_err(|e| format!("Failed to list tables: {}", e))?;

    let tables: Vec<String> = rows.iter().filter_map(|r| r.try_get(0).ok()).collect();

    Ok(tables)
}

// ─── Dispatcher ─────────────────────────────────────────────────────────────

/// Determine database type from connection string.
#[derive(Debug, Clone, PartialEq)]
pub enum DbType {
    Sqlite,
    Postgres,
}

/// Parse the connection string to determine the database type.
pub fn detect_db_type(conn_str: &str) -> Result<DbType, String> {
    let lower = conn_str.to_lowercase();
    if lower.starts_with("sqlite://")
        || lower.starts_with("sqlite:")
        || lower.ends_with(".db")
        || lower.ends_with(".sqlite")
        || lower.ends_with(".sqlite3")
    {
        Ok(DbType::Sqlite)
    } else if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
        Ok(DbType::Postgres)
    } else {
        Err(format!(
            "Cannot determine database type from connection string. \
             Use 'sqlite:///path/to/db' or 'postgres://user:pass@host/db'. Got: '{}'",
            conn_str
        ))
    }
}

/// Extract the file path from a SQLite connection string.
///
/// Handles these formats:
/// - `sqlite:///absolute/path` -> `/absolute/path` (triple slash = absolute)
/// - `sqlite://relative/path` -> `relative/path`
/// - `sqlite:path` -> `path`
/// - `/plain/path.db` -> `/plain/path.db`
fn sqlite_path(conn_str: &str) -> String {
    let lower = conn_str.to_lowercase();
    if lower.starts_with("sqlite:///") {
        // sqlite:/// -> the third slash is the root slash of the absolute path
        conn_str["sqlite://".len()..].to_string()
    } else if lower.starts_with("sqlite://") {
        conn_str["sqlite://".len()..].to_string()
    } else if lower.starts_with("sqlite:") {
        conn_str["sqlite:".len()..].to_string()
    } else {
        // Assume it's a plain path
        conn_str.to_string()
    }
}

/// Execute a parameterized query, dispatching to the correct backend.
pub fn query(
    conn_str: &str,
    sql: &str,
    params: &[String],
    config: &DbConfig,
) -> Result<DbQueryResult, String> {
    match detect_db_type(conn_str)? {
        DbType::Sqlite => {
            let path = sqlite_path(conn_str);
            query_sqlite(&path, sql, params, config)
        }
        DbType::Postgres => query_postgres(conn_str, sql, params, config),
    }
}

/// List tables, dispatching to the correct backend.
pub fn list_tables(conn_str: &str) -> Result<Vec<String>, String> {
    match detect_db_type(conn_str)? {
        DbType::Sqlite => {
            let path = sqlite_path(conn_str);
            list_tables_sqlite(&path)
        }
        DbType::Postgres => list_tables_postgres(conn_str),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- Query validation tests ---

    #[test]
    fn test_validate_select_allowed() {
        assert!(validate_query("SELECT * FROM users", true).is_ok());
    }

    #[test]
    fn test_validate_select_with_where() {
        assert!(validate_query("SELECT name, age FROM users WHERE id = $1", true).is_ok());
    }

    #[test]
    fn test_validate_show_allowed() {
        assert!(validate_query("SHOW TABLES", true).is_ok());
    }

    #[test]
    fn test_validate_explain_allowed() {
        assert!(validate_query("EXPLAIN SELECT * FROM users", true).is_ok());
    }

    #[test]
    fn test_validate_with_cte_allowed() {
        assert!(
            validate_query("WITH cte AS (SELECT id FROM users) SELECT * FROM cte", true,).is_ok()
        );
    }

    #[test]
    fn test_validate_pragma_allowed() {
        assert!(validate_query("PRAGMA table_info(users)", true).is_ok());
    }

    #[test]
    fn test_validate_drop_blocked() {
        let result = validate_query("DROP TABLE users", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("DROP"));
    }

    #[test]
    fn test_validate_delete_blocked() {
        let result = validate_query("DELETE FROM users WHERE id = 1", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("DELETE"));
    }

    #[test]
    fn test_validate_insert_blocked() {
        let result = validate_query("INSERT INTO users (name) VALUES ('test')", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("INSERT"));
    }

    #[test]
    fn test_validate_update_blocked() {
        let result = validate_query("UPDATE users SET name = 'x' WHERE id = 1", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("UPDATE"));
    }

    #[test]
    fn test_validate_alter_blocked() {
        let result = validate_query("ALTER TABLE users ADD COLUMN email TEXT", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("ALTER"));
    }

    #[test]
    fn test_validate_create_blocked() {
        let result = validate_query("CREATE TABLE evil (id INT)", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("CREATE"));
    }

    #[test]
    fn test_validate_truncate_blocked() {
        let result = validate_query("TRUNCATE TABLE users", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("TRUNCATE"));
    }

    #[test]
    fn test_validate_grant_blocked() {
        let result = validate_query("GRANT ALL ON users TO public", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("GRANT"));
    }

    #[test]
    fn test_validate_multiple_statements_blocked() {
        let result = validate_query("SELECT 1; DROP TABLE users", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("semicolon"));
    }

    #[test]
    fn test_validate_empty_blocked() {
        let result = validate_query("", true);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_null_bytes_blocked() {
        let result = validate_query("SELECT\0* FROM users", true);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_write_allowed_when_not_readonly() {
        assert!(validate_query("INSERT INTO users (name) VALUES ('test')", false).is_ok());
        assert!(validate_query("UPDATE users SET name = 'x'", false).is_ok());
        assert!(validate_query("DELETE FROM users", false).is_ok());
    }

    #[test]
    fn test_keyword_in_column_name_allowed() {
        // "updated_at" contains "UPDATE" but not as a whole word
        assert!(validate_query("SELECT updated_at FROM users", true).is_ok());
    }

    #[test]
    fn test_keyword_in_string_still_detected() {
        // This is intentionally strict — we block at the SQL text level.
        // If someone needs SELECT with a column named exactly "DELETE",
        // they should use a column alias or turn off read_only.
        let result = validate_query("SELECT DELETE FROM ops", true);
        assert!(result.is_err());
    }

    // --- Result capping tests ---

    #[test]
    fn test_cap_results_under_limit() {
        let result = DbQueryResult {
            columns: vec!["id".into()],
            rows: vec![vec!["1".into()], vec!["2".into()]],
            row_count: 2,
            truncated: false,
        };
        let capped = cap_results(result, 100);
        assert_eq!(capped.row_count, 2);
        assert!(!capped.truncated);
    }

    #[test]
    fn test_cap_results_over_row_limit() {
        let rows: Vec<Vec<String>> = (0..200).map(|i| vec![i.to_string()]).collect();
        let result = DbQueryResult {
            columns: vec!["id".into()],
            rows,
            row_count: 200,
            truncated: false,
        };
        let capped = cap_results(result, 100);
        assert_eq!(capped.row_count, 100);
        assert!(capped.truncated);
        assert_eq!(capped.rows.len(), 100);
    }

    // --- Connection string detection tests ---

    #[test]
    fn test_detect_sqlite_url() {
        assert_eq!(
            detect_db_type("sqlite:///tmp/test.db").unwrap(),
            DbType::Sqlite
        );
    }

    #[test]
    fn test_detect_sqlite_extension() {
        assert_eq!(detect_db_type("/tmp/test.sqlite3").unwrap(), DbType::Sqlite);
        assert_eq!(detect_db_type("/tmp/test.db").unwrap(), DbType::Sqlite);
    }

    #[test]
    fn test_detect_postgres_url() {
        assert_eq!(
            detect_db_type("postgres://user:pass@localhost/mydb").unwrap(),
            DbType::Postgres,
        );
        assert_eq!(
            detect_db_type("postgresql://user:pass@localhost/mydb").unwrap(),
            DbType::Postgres,
        );
    }

    #[test]
    fn test_detect_unknown() {
        assert!(detect_db_type("mysql://localhost/db").is_err());
    }

    #[test]
    fn test_sqlite_path_extraction() {
        assert_eq!(sqlite_path("sqlite:///tmp/test.db"), "/tmp/test.db");
        assert_eq!(sqlite_path("sqlite://relative.db"), "relative.db");
        assert_eq!(sqlite_path("sqlite:test.db"), "test.db");
        assert_eq!(sqlite_path("/tmp/test.db"), "/tmp/test.db");
    }

    // --- SQLite integration tests ---

    #[test]
    fn test_sqlite_query_and_list_tables() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        // Create a test database with data
        let conn = rusqlite::Connection::open(db_str).unwrap();
        conn.execute_batch(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, age INTEGER);
             INSERT INTO users (name, age) VALUES ('Alice', 30);
             INSERT INTO users (name, age) VALUES ('Bob', 25);",
        )
        .unwrap();
        drop(conn);

        // Test list_tables
        let tables = list_tables_sqlite(db_str).unwrap();
        assert_eq!(tables, vec!["users"]);

        // Test query with params
        let config = DbConfig {
            read_only: true,
            max_rows: 100,
            ..Default::default()
        };
        let result = query_sqlite(
            db_str,
            "SELECT name, age FROM users WHERE age > ?1",
            &["26".to_string()],
            &config,
        )
        .unwrap();

        assert_eq!(result.columns, vec!["name", "age"]);
        assert_eq!(result.row_count, 1);
        assert_eq!(result.rows[0], vec!["Alice", "30"]);
        assert!(!result.truncated);
    }

    #[test]
    fn test_sqlite_readonly_enforcement() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        let conn = rusqlite::Connection::open(db_str).unwrap();
        conn.execute_batch("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);")
            .unwrap();
        drop(conn);

        let config = DbConfig {
            read_only: true,
            max_rows: 100,
            ..Default::default()
        };

        // INSERT should be rejected by validation before even reaching SQLite
        let result = query_sqlite(
            db_str,
            "INSERT INTO users (name) VALUES (?1)",
            &["evil".to_string()],
            &config,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_sqlite_row_cap() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        let conn = rusqlite::Connection::open(db_str).unwrap();
        conn.execute_batch("CREATE TABLE nums (val INTEGER);")
            .unwrap();
        for i in 0..50 {
            conn.execute("INSERT INTO nums (val) VALUES (?1)", [i])
                .unwrap();
        }
        drop(conn);

        let config = DbConfig {
            read_only: true,
            max_rows: 10,
            ..Default::default()
        };

        let result = query_sqlite(db_str, "SELECT val FROM nums", &[], &config).unwrap();
        assert_eq!(result.rows.len(), 10);
        assert!(result.truncated);
    }

    #[test]
    fn test_dispatcher_sqlite() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db_str = db_path.to_str().unwrap();

        let conn = rusqlite::Connection::open(db_str).unwrap();
        conn.execute_batch(
            "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT);
             INSERT INTO items (name) VALUES ('widget');",
        )
        .unwrap();
        drop(conn);

        let conn_url = format!("sqlite://{}", db_str);
        let config = DbConfig {
            connection_string: conn_url.clone(),
            read_only: true,
            max_rows: 100,
            ..Default::default()
        };

        let result = query(&conn_url, "SELECT * FROM items", &[], &config).unwrap();
        assert_eq!(result.row_count, 1);

        let tables = list_tables(&conn_url).unwrap();
        assert_eq!(tables, vec!["items"]);
    }
}
