//! `SQLite` database layer for session persistence.
//!
//! Provides CRUD operations for sessions, messages, and tool calls backed by
//! a single `SQLite` database at `~/.seval/seval.db`. Uses WAL mode and foreign
//! keys for concurrent access and referential integrity.

use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use super::models::{MemoryRecord, SessionRecord, StoredMessage, StoredToolCall};

/// SQL migrations applied in order. Each entry advances the schema version by 1.
const MIGRATIONS: &[&str] = &[
    // Version 1: Initial schema
    "CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY,
        project_path TEXT NOT NULL,
        name TEXT,
        model TEXT,
        message_count INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE TABLE IF NOT EXISTS messages (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
        role TEXT NOT NULL CHECK(role IN ('user', 'assistant', 'system')),
        content TEXT NOT NULL,
        token_input INTEGER,
        token_output INTEGER,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE TABLE IF NOT EXISTS tool_calls (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
        name TEXT NOT NULL,
        args_json TEXT NOT NULL,
        result_text TEXT,
        status TEXT NOT NULL DEFAULT 'pending',
        duration_ms INTEGER,
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE TABLE IF NOT EXISTS memory (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        project_path TEXT NOT NULL,
        content TEXT NOT NULL,
        source TEXT NOT NULL CHECK(source IN ('auto', 'user')),
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );
    CREATE INDEX IF NOT EXISTS idx_sessions_project ON sessions(project_path);
    CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
    CREATE INDEX IF NOT EXISTS idx_tool_calls_message ON tool_calls(message_id);
    CREATE INDEX IF NOT EXISTS idx_memory_project ON memory(project_path);",
];

/// Database handle wrapping a shared `SQLite` connection.
///
/// All operations are synchronous (rusqlite). Callers in async contexts should
/// use `tokio::task::spawn_blocking` to avoid blocking the event loop.
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Acquire the database connection lock.
    ///
    /// Panics if the mutex is poisoned, which indicates a prior panic
    /// while holding the lock — an unrecoverable state.
    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("Database mutex poisoned")
    }

    /// Open the database at `~/.seval/seval.db`.
    ///
    /// Creates the directory and file if they do not exist, enables WAL mode
    /// and foreign keys, and runs any pending migrations.
    pub fn open() -> Result<Self> {
        let path = directories::BaseDirs::new()
            .context("Cannot determine home directory")?
            .home_dir()
            .join(".seval")
            .join("seval.db");
        std::fs::create_dir_all(path.parent().unwrap())?;
        let conn = Connection::open(&path)?;
        Self::configure_and_migrate(conn)
    }

    /// Open an in-memory database for testing.
    ///
    /// Applies the same pragmas and migrations as the on-disk database.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure_and_migrate(conn)
    }

    /// Apply pragmas, run migrations, and wrap in Arc<Mutex>.
    fn configure_and_migrate(conn: Connection) -> Result<Self> {
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        run_migrations(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create a new session in the database.
    pub fn create_session(
        &self,
        project_path: &str,
        model: Option<&str>,
    ) -> Result<SessionRecord> {
        let id = uuid::Uuid::new_v4().to_string();
        let conn = self.conn();
        conn.execute(
            "INSERT INTO sessions (id, project_path, model) VALUES (?1, ?2, ?3)",
            params![id, project_path, model],
        )?;
        // Read back the full record to get server-generated defaults.
        let record = conn.query_row(
            "SELECT id, project_path, name, model, message_count, created_at, updated_at
             FROM sessions WHERE id = ?1",
            params![id],
            map_session_row,
        )?;
        Ok(record)
    }

    /// Save a message to the database.
    ///
    /// Increments the session's `message_count` and `updated_at` timestamp.
    /// Returns the auto-generated message row ID.
    pub fn save_message(
        &self,
        session_id: &str,
        role: &str,
        content: &str,
        token_input: Option<i64>,
        token_output: Option<i64>,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO messages (session_id, role, content, token_input, token_output)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, role, content, token_input, token_output],
        )?;
        let msg_id = conn.last_insert_rowid();

        // Update session counters.
        conn.execute(
            "UPDATE sessions SET message_count = message_count + 1,
                                 updated_at = datetime('now')
             WHERE id = ?1",
            params![session_id],
        )?;

        Ok(msg_id)
    }

    /// Save a tool call associated with a message.
    ///
    /// Returns the auto-generated tool call row ID.
    pub fn save_tool_call(
        &self,
        message_id: i64,
        name: &str,
        args_json: &str,
        result_text: Option<&str>,
        status: &str,
        duration_ms: Option<i64>,
    ) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO tool_calls (message_id, name, args_json, result_text, status, duration_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![message_id, name, args_json, result_text, status, duration_ms],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// List sessions, optionally filtered by project path.
    ///
    /// Returns up to 20 sessions ordered by most recently updated first.
    pub fn list_sessions(&self, project_path: Option<&str>) -> Result<Vec<SessionRecord>> {
        let conn = self.conn();
        if let Some(path) = project_path {
            let mut stmt = conn.prepare(
                "SELECT id, project_path, name, model, message_count, created_at, updated_at
                 FROM sessions WHERE project_path = ?1
                 ORDER BY updated_at DESC LIMIT 20",
            )?;
            let rows = stmt
                .query_map(params![path], map_session_row)?
                .filter_map(Result::ok)
                .collect();
            Ok(rows)
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, project_path, name, model, message_count, created_at, updated_at
                 FROM sessions ORDER BY updated_at DESC LIMIT 20",
            )?;
            let rows = stmt
                .query_map([], map_session_row)?
                .filter_map(Result::ok)
                .collect();
            Ok(rows)
        }
    }

    /// Get all messages for a session in chronological order.
    pub fn get_session_messages(&self, session_id: &str) -> Result<Vec<StoredMessage>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content, token_input, token_output, created_at
             FROM messages WHERE session_id = ?1
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt
            .query_map(params![session_id], |row| {
                Ok(StoredMessage {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    role: row.get(2)?,
                    content: row.get(3)?,
                    token_input: row.get(4)?,
                    token_output: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    /// Get all tool calls for a message.
    pub fn get_message_tool_calls(&self, message_id: i64) -> Result<Vec<StoredToolCall>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, message_id, name, args_json, result_text, status, duration_ms, created_at
             FROM tool_calls WHERE message_id = ?1
             ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map(params![message_id], |row| {
                Ok(StoredToolCall {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    name: row.get(2)?,
                    args_json: row.get(3)?,
                    result_text: row.get(4)?,
                    status: row.get(5)?,
                    duration_ms: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    /// Delete a session and all its messages and tool calls (via CASCADE).
    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![session_id])?;
        Ok(())
    }

    // --- Memory CRUD ---

    /// Save a memory entry for a project.
    ///
    /// Returns the auto-generated row ID.
    pub fn save_memory(&self, project_path: &str, content: &str, source: &str) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO memory (project_path, content, source) VALUES (?1, ?2, ?3)",
            params![project_path, content, source],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get all memories for a project, ordered by most recent first.
    pub fn get_memories(&self, project_path: &str) -> Result<Vec<MemoryRecord>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, project_path, content, source, created_at
             FROM memory WHERE project_path = ?1
             ORDER BY id DESC",
        )?;
        let rows = stmt
            .query_map(params![project_path], |row| {
                Ok(MemoryRecord {
                    id: row.get(0)?,
                    project_path: row.get(1)?,
                    content: row.get(2)?,
                    source: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(Result::ok)
            .collect();
        Ok(rows)
    }

    /// Delete a memory entry by ID.
    pub fn delete_memory(&self, id: i64) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM memory WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Update the name of a session.
    pub fn update_session_name(&self, session_id: &str, name: &str) -> Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE sessions SET name = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![name, session_id],
        )?;
        Ok(())
    }
}

/// Map a database row to a `SessionRecord`.
fn map_session_row(row: &rusqlite::Row) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        id: row.get(0)?,
        project_path: row.get(1)?,
        name: row.get(2)?,
        model: row.get(3)?,
        message_count: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

/// Run schema migrations.
///
/// Creates the `schema_version` table if missing, then applies each migration
/// whose version is greater than the current schema version.
fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        )",
    )?;
    let current: i64 =
        conn.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| {
            r.get(0)
        })?;
    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let ver = i64::try_from(i + 1).unwrap_or(0);
        if ver > current {
            conn.execute_batch(sql)?;
            conn.execute("INSERT INTO schema_version (version) VALUES (?1)", params![ver])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_session_returns_valid_record() {
        let db = Database::open_in_memory().unwrap();
        let session = db.create_session("/tmp/project", Some("claude-sonnet")).unwrap();
        assert!(!session.id.is_empty());
        assert_eq!(session.project_path, "/tmp/project");
        assert_eq!(session.model.as_deref(), Some("claude-sonnet"));
        assert_eq!(session.message_count, 0);
        assert!(session.name.is_none());
    }

    #[test]
    fn save_message_increments_count_and_updates_timestamp() {
        let db = Database::open_in_memory().unwrap();
        let session = db.create_session("/tmp/project", None).unwrap();
        let original_updated = session.updated_at.clone();

        let msg_id = db
            .save_message(&session.id, "user", "hello", None, None)
            .unwrap();
        assert!(msg_id > 0);

        // Check session was updated.
        let sessions = db.list_sessions(None).unwrap();
        let updated = &sessions[0];
        assert_eq!(updated.message_count, 1);
        // updated_at should be >= original (may be same second in fast tests).
        assert!(updated.updated_at >= original_updated);
    }

    #[test]
    fn save_tool_call_inserts_correctly() {
        let db = Database::open_in_memory().unwrap();
        let session = db.create_session("/tmp/project", None).unwrap();
        let msg_id = db
            .save_message(&session.id, "assistant", "let me check", None, None)
            .unwrap();
        let tc_id = db
            .save_tool_call(msg_id, "shell", r#"{"command":"ls"}"#, Some("file.txt"), "success", Some(42))
            .unwrap();
        assert!(tc_id > 0);

        let calls = db.get_message_tool_calls(msg_id).unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "shell");
        assert_eq!(calls[0].status, "success");
        assert_eq!(calls[0].duration_ms, Some(42));
    }

    #[test]
    fn list_sessions_ordered_by_updated_desc_limited_to_20() {
        let db = Database::open_in_memory().unwrap();
        for i in 0..25 {
            let s = db
                .create_session(&format!("/project/{i}"), None)
                .unwrap();
            // Add a message to update the timestamp (so ordering is deterministic).
            db.save_message(&s.id, "user", &format!("msg {i}"), None, None)
                .unwrap();
        }
        let sessions = db.list_sessions(None).unwrap();
        assert_eq!(sessions.len(), 20);
        // Most recently updated should be first.
        assert!(sessions[0].updated_at >= sessions[19].updated_at);
    }

    #[test]
    fn list_sessions_with_project_filter() {
        let db = Database::open_in_memory().unwrap();
        db.create_session("/project/a", None).unwrap();
        db.create_session("/project/b", None).unwrap();
        db.create_session("/project/a", None).unwrap();

        let filtered = db.list_sessions(Some("/project/a")).unwrap();
        assert_eq!(filtered.len(), 2);
        for s in &filtered {
            assert_eq!(s.project_path, "/project/a");
        }
    }

    #[test]
    fn get_session_messages_chronological_with_tool_calls() {
        let db = Database::open_in_memory().unwrap();
        let session = db.create_session("/tmp/project", None).unwrap();

        let m1 = db
            .save_message(&session.id, "user", "first", None, None)
            .unwrap();
        let m2 = db
            .save_message(&session.id, "assistant", "second", Some(100), Some(50))
            .unwrap();
        db.save_tool_call(m2, "shell", r#"{"cmd":"ls"}"#, Some("ok"), "success", Some(10))
            .unwrap();

        let messages = db.get_session_messages(&session.id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "first");
        assert_eq!(messages[1].role, "assistant");
        assert_eq!(messages[1].token_input, Some(100));

        let tool_calls = db.get_message_tool_calls(m2).unwrap();
        assert_eq!(tool_calls.len(), 1);
        assert_eq!(tool_calls[0].name, "shell");
        // Verify m1 has no tool calls.
        let empty = db.get_message_tool_calls(m1).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn delete_session_cascades_to_messages_and_tool_calls() {
        let db = Database::open_in_memory().unwrap();
        let session = db.create_session("/tmp/project", None).unwrap();
        let msg_id = db
            .save_message(&session.id, "assistant", "answer", None, None)
            .unwrap();
        db.save_tool_call(msg_id, "read", "{}", None, "success", None)
            .unwrap();

        db.delete_session(&session.id).unwrap();

        // Session gone.
        let sessions = db.list_sessions(None).unwrap();
        assert!(sessions.is_empty());

        // Messages gone.
        let messages = db.get_session_messages(&session.id).unwrap();
        assert!(messages.is_empty());

        // Tool calls gone.
        let tool_calls = db.get_message_tool_calls(msg_id).unwrap();
        assert!(tool_calls.is_empty());
    }

    #[test]
    fn update_session_name_sets_name() {
        let db = Database::open_in_memory().unwrap();
        let session = db.create_session("/tmp/project", None).unwrap();
        assert!(session.name.is_none());

        db.update_session_name(&session.id, "My Chat Session")
            .unwrap();

        let sessions = db.list_sessions(None).unwrap();
        assert_eq!(sessions[0].name.as_deref(), Some("My Chat Session"));
    }

    #[test]
    fn save_memory_inserts_row_with_correct_fields() {
        let db = Database::open_in_memory().unwrap();
        let id = db
            .save_memory("/tmp/project", "Found SSH key at /home/user/.ssh/id_rsa", "auto")
            .unwrap();
        assert!(id > 0);

        let memories = db.get_memories("/tmp/project").unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].project_path, "/tmp/project");
        assert_eq!(
            memories[0].content,
            "Found SSH key at /home/user/.ssh/id_rsa"
        );
        assert_eq!(memories[0].source, "auto");
        assert!(!memories[0].created_at.is_empty());
    }

    #[test]
    fn get_memories_returns_desc_order() {
        let db = Database::open_in_memory().unwrap();
        db.save_memory("/tmp/project", "First finding", "auto")
            .unwrap();
        db.save_memory("/tmp/project", "Second finding", "auto")
            .unwrap();
        db.save_memory("/tmp/project", "Third finding", "user")
            .unwrap();

        let memories = db.get_memories("/tmp/project").unwrap();
        assert_eq!(memories.len(), 3);
        // Most recent first (DESC order by id since created_at can be same second).
        assert_eq!(memories[0].content, "Third finding");
        assert_eq!(memories[2].content, "First finding");
    }

    #[test]
    fn get_memories_returns_empty_for_unknown_project() {
        let db = Database::open_in_memory().unwrap();
        db.save_memory("/tmp/project", "Something", "auto")
            .unwrap();

        let memories = db.get_memories("/unknown/path").unwrap();
        assert!(memories.is_empty());
    }

    #[test]
    fn delete_memory_removes_specific_entry() {
        let db = Database::open_in_memory().unwrap();
        let id1 = db
            .save_memory("/tmp/project", "Keep this", "auto")
            .unwrap();
        let id2 = db
            .save_memory("/tmp/project", "Delete this", "auto")
            .unwrap();

        db.delete_memory(id2).unwrap();

        let memories = db.get_memories("/tmp/project").unwrap();
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].id, id1);
        assert_eq!(memories[0].content, "Keep this");
    }

    #[test]
    fn migrations_run_idempotently() {
        let db = Database::open_in_memory().unwrap();
        // Running open_in_memory again with the same connection would re-run
        // migrations. Instead, we simulate by running configure_and_migrate twice.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .unwrap();
        run_migrations(&conn).unwrap();
        // Running again should not error.
        run_migrations(&conn).unwrap();

        // Verify the DB is usable.
        let db2 = Database {
            conn: Arc::new(Mutex::new(conn)),
        };
        let _ = db.create_session("/test", None).unwrap();
        let _ = db2.create_session("/test2", None).unwrap();
    }
}
