use std::path::Path;

use rusqlite::Connection;

use crate::error::{Result, SecallError};

use super::schema::{
    CREATE_CONFIG, CREATE_GRAPH_EDGES, CREATE_GRAPH_INDEXES, CREATE_GRAPH_NODES, CREATE_INDEXES,
    CREATE_INGEST_LOG, CREATE_QUERY_CACHE, CREATE_SESSIONS, CREATE_TURNS, CREATE_TURNS_FTS,
    CURRENT_SCHEMA_VERSION,
};

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn migrate(&self) -> Result<()> {
        // Ensure config table exists first
        self.conn.execute_batch(CREATE_CONFIG)?;

        let version: Option<u32> = self
            .conn
            .query_row(
                "SELECT value FROM config WHERE key = 'schema_version'",
                [],
                |row| {
                    let v: String = row.get(0)?;
                    Ok(v.parse::<u32>().unwrap_or(0))
                },
            )
            .ok();

        let current = version.unwrap_or(0);

        if current < 1 {
            self.apply_v1()?;
        }
        if current < 2 {
            // Column migrations for v2
            if !self.column_exists("sessions", "host")? {
                self.conn
                    .execute("ALTER TABLE sessions ADD COLUMN host TEXT", [])?;
            }
            if !self.column_exists("sessions", "summary")? {
                self.conn
                    .execute("ALTER TABLE sessions ADD COLUMN summary TEXT", [])?;
            }
        }
        if current < 3 {
            self.conn.execute_batch(CREATE_GRAPH_NODES)?;
            self.conn.execute_batch(CREATE_GRAPH_EDGES)?;
            self.conn.execute_batch(CREATE_GRAPH_INDEXES)?;
        }
        if current < 4 && !self.column_exists("sessions", "session_type")? {
            self.conn.execute(
                "ALTER TABLE sessions ADD COLUMN session_type TEXT DEFAULT 'interactive'",
                [],
            )?;
        }
        if current < CURRENT_SCHEMA_VERSION {
            self.conn.execute(
                "INSERT OR REPLACE INTO config(key, value) VALUES ('schema_version', ?1)",
                [CURRENT_SCHEMA_VERSION.to_string()],
            )?;
        }

        // Non-versioned additions: always apply (CREATE IF NOT EXISTS)
        self.conn.execute_batch(CREATE_QUERY_CACHE)?;

        Ok(())
    }

    fn column_exists(&self, table: &str, column: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2",
            rusqlite::params![table, column],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    fn apply_v1(&self) -> Result<()> {
        self.conn.execute_batch(CREATE_SESSIONS)?;
        self.conn.execute_batch(CREATE_TURNS)?;
        self.conn.execute_batch(CREATE_TURNS_FTS)?;
        self.conn.execute_batch(CREATE_INGEST_LOG)?;
        self.conn.execute_batch(CREATE_INDEXES)?;
        Ok(())
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Execute a closure within a SQLite transaction.
    /// Commits on Ok, rolls back on Err.
    pub fn with_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce() -> Result<T>,
    {
        self.conn.execute_batch("BEGIN")?;
        match f() {
            Ok(val) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(val)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// Get database statistics
    pub fn get_stats(&self) -> Result<DbStats> {
        let session_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        let turn_count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM turns", [], |r| r.get(0))?;
        let vector_count: i64 = {
            let exists: i64 = self.conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='turn_vectors'",
                [],
                |r| r.get(0),
            )?;
            if exists > 0 {
                self.conn
                    .query_row("SELECT COUNT(*) FROM turn_vectors", [], |r| r.get(0))?
            } else {
                0
            }
        };

        let mut stmt = self.conn.prepare(
            "SELECT il.session_id, s.agent, il.timestamp
             FROM ingest_log il
             LEFT JOIN sessions s ON il.session_id = s.id
             WHERE il.action = 'ingest'
             ORDER BY il.id DESC LIMIT 5",
        )?;
        let recent_ingests = stmt
            .query_map([], |row| {
                let sid: String = row.get(0)?;
                let agent: Option<String> = row.get(1)?;
                let ts: String = row.get(2)?;
                Ok(IngestLogEntry {
                    session_id_prefix: sid[..sid.len().min(8)].to_string(),
                    agent: agent.unwrap_or_else(|| "unknown".to_string()),
                    timestamp: ts[..ts.len().min(10)].to_string(),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(DbStats {
            session_count,
            turn_count,
            vector_count,
            recent_ingests,
        })
    }

    /// Get a specific turn by session_id and turn_index
    pub fn get_turn(&self, session_id: &str, turn_index: u32) -> Result<TurnRow> {
        self.conn
            .query_row(
                "SELECT turn_index, role, content FROM turns WHERE session_id = ?1 AND turn_index = ?2",
                rusqlite::params![session_id, turn_index as i64],
                |row| {
                    Ok(TurnRow {
                        turn_index: row.get::<_, i64>(0)? as u32,
                        role: row.get(1)?,
                        content: row.get(2)?,
                    })
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => SecallError::TurnNotFound {
                    session_id: session_id.to_string(),
                    turn_index,
                },
                _ => SecallError::Database(e),
            })
    }

    pub fn count_sessions(&self) -> Result<i64> {
        let count = self
            .conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        Ok(count)
    }

    pub fn list_projects(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT project FROM sessions WHERE project IS NOT NULL")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn list_agents(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT DISTINCT agent FROM sessions")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn has_embeddings(&self) -> Result<bool> {
        let exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='turn_vectors'",
            [],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Ok(false);
        }
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM turn_vectors", [], |r| r.get(0))?;
        Ok(count > 0)
    }

    // ─── Lint helpers ────────────────────────────────────────────────────────

    /// Return vault_path for a single session
    pub fn get_session_vault_path(&self, session_id: &str) -> Result<Option<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT vault_path FROM sessions WHERE id = ?1")?;
        match stmt.query_row([session_id], |row| row.get::<_, Option<String>>(0)) {
            Ok(vp) => Ok(vp),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Return (session_id, vault_path) for all sessions
    pub fn list_session_vault_paths(&self) -> Result<Vec<(String, Option<String>)>> {
        let mut stmt = self.conn.prepare("SELECT id, vault_path FROM sessions")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Count rows in the turns_fts virtual table
    pub fn count_fts_rows(&self) -> Result<i64> {
        let count = self
            .conn
            .query_row("SELECT COUNT(*) FROM turns_fts", [], |r| r.get(0))?;
        Ok(count)
    }

    /// Count rows in the turns table
    pub fn count_turns(&self) -> Result<i64> {
        let count = self
            .conn
            .query_row("SELECT COUNT(*) FROM turns", [], |r| r.get(0))?;
        Ok(count)
    }

    /// turn_vectors 테이블의 총 벡터 수. ANN stale 감지에 사용.
    pub fn count_vectors(&self) -> Result<usize> {
        let exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='turn_vectors'",
            [],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Ok(0);
        }
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM turn_vectors", [], |r| r.get(0))?;
        Ok(count as usize)
    }

    /// Sessions that have no rows in turn_vectors
    pub fn find_sessions_without_vectors(&self) -> Result<Vec<String>> {
        let table_exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='turn_vectors'",
            [],
            |r| r.get(0),
        )?;

        let query = if table_exists == 0 {
            "SELECT id FROM sessions"
        } else {
            "SELECT id FROM sessions WHERE id NOT IN (SELECT DISTINCT session_id FROM turn_vectors)"
        };

        let mut stmt = self.conn.prepare(query)?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Vector rows whose session_id does not exist in sessions
    pub fn find_orphan_vectors(&self) -> Result<Vec<(i64, String)>> {
        let table_exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='turn_vectors'",
            [],
            |r| r.get(0),
        )?;

        if table_exists == 0 {
            return Ok(Vec::new());
        }

        let mut stmt = self.conn.prepare(
            "SELECT id, session_id FROM turn_vectors WHERE session_id NOT IN (SELECT id FROM sessions)",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Count sessions per agent
    pub fn agent_counts(&self) -> Result<std::collections::HashMap<String, usize>> {
        let mut stmt = self
            .conn
            .prepare("SELECT agent, COUNT(*) FROM sessions GROUP BY agent")?;
        let rows = stmt.query_map([], |row| {
            let agent: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((agent, count as usize))
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 세션과 관련된 모든 데이터를 삭제 (sessions, turns, turn_vectors).
    /// `--force` 재수집 시 기존 데이터를 정리하는 데 사용.
    pub fn delete_session(&self, session_id: &str) -> Result<()> {
        self.delete_session_vectors(session_id)?;
        self.conn.execute(
            "DELETE FROM turns WHERE session_id = ?1",
            rusqlite::params![session_id],
        )?;
        self.conn.execute(
            "DELETE FROM sessions WHERE id = ?1",
            rusqlite::params![session_id],
        )?;
        Ok(())
    }

    /// 세션의 모든 벡터를 삭제. 부분 임베딩 정리 및 재임베딩 전 DELETE-first에 사용.
    pub fn delete_session_vectors(&self, session_id: &str) -> Result<usize> {
        // turn_vectors 테이블이 없으면 0 반환 (정상)
        let table_exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='turn_vectors'",
            [],
            |r| r.get(0),
        )?;
        if table_exists == 0 {
            return Ok(0);
        }
        let deleted = self.conn.execute(
            "DELETE FROM turn_vectors WHERE session_id = ?1",
            rusqlite::params![session_id],
        )?;
        Ok(deleted)
    }

    /// Return all session IDs in the database
    pub fn list_all_session_ids(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT id FROM sessions")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// session summary 업데이트
    pub fn update_session_summary(&self, session_id: &str, summary: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE sessions SET summary = ?1 WHERE id = ?2",
            rusqlite::params![summary, session_id],
        )?;
        Ok(())
    }

    /// Find session IDs ingested more than once in ingest_log
    pub fn find_duplicate_ingest_entries(&self) -> Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id, COUNT(*) as cnt FROM ingest_log WHERE action='ingest' GROUP BY session_id HAVING cnt > 1",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// 기존 절대경로 vault_path를 상대경로로 변환 (one-time migration)
    pub fn migrate_vault_paths_to_relative(&self, vault_root: &Path) -> Result<usize> {
        let vault_root_str = vault_root.to_string_lossy();
        let prefix = format!("{}/", vault_root_str.trim_end_matches('/'));

        let mut stmt = self
            .conn
            .prepare("SELECT id, vault_path FROM sessions WHERE vault_path IS NOT NULL")?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        let mut migrated = 0;
        for (session_id, vault_path) in &rows {
            if vault_path.starts_with(&prefix) {
                let relative = &vault_path[prefix.len()..];
                self.conn.execute(
                    "UPDATE sessions SET vault_path = ?1 WHERE id = ?2",
                    rusqlite::params![relative, session_id],
                )?;
                migrated += 1;
            }
        }
        Ok(migrated)
    }

    /// vault 마크다운의 frontmatter로 sessions 테이블에 insert.
    /// turns 테이블에는 본문 전체를 단일 FTS 청크로 저장.
    pub fn insert_session_from_vault(
        &self,
        fm: &crate::ingest::markdown::SessionFrontmatter,
        body_text: &str,
        vault_path: &str,
    ) -> Result<()> {
        self.conn().execute(
            "INSERT OR IGNORE INTO sessions(
                id, agent, model, project, cwd, git_branch, host,
                start_time, end_time, turn_count, tokens_in, tokens_out,
                tools_used, vault_path, summary, ingested_at, status
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, NULL, ?6,
                ?7, ?8, ?9, ?10, ?11,
                ?12, ?13, ?14, datetime('now'), 'reindexed'
            )",
            rusqlite::params![
                fm.session_id,
                fm.agent,
                fm.model,
                fm.project,
                fm.cwd,
                fm.host,
                fm.start_time,
                fm.end_time,
                fm.turns.unwrap_or(0),
                fm.tokens_in.unwrap_or(0),
                fm.tokens_out.unwrap_or(0),
                fm.tools_used.as_ref().map(|t| t.join(",")),
                vault_path,
                fm.summary,
            ],
        )?;

        // FTS 인덱싱 — 본문 전체를 하나의 청크로
        if !body_text.trim().is_empty() {
            self.conn().execute(
                "INSERT INTO turns_fts(content, session_id, turn_id) VALUES (?1, ?2, 0)",
                rusqlite::params![body_text, fm.session_id],
            )?;
        }

        Ok(())
    }

    /// session_id로 Session 구조체를 재구성 (벡터 임베딩용).
    /// turns 테이블에서 content를 읽어 Session.turns를 채운다.
    pub fn get_session_for_embedding(&self, session_id: &str) -> Result<crate::ingest::Session> {
        use crate::ingest::{AgentKind, Role, Session, TokenUsage, Turn};
        use chrono::DateTime;

        // 세션 메타 조회
        let (
            agent_str,
            model,
            project,
            cwd_str,
            start_time_str,
            end_time_str,
            tokens_in,
            tokens_out,
            session_type,
        ) = self
            .conn
            .query_row(
                "SELECT agent, model, project, cwd, start_time, end_time, tokens_in, tokens_out, session_type
                 FROM sessions WHERE id = ?1",
                [session_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    SecallError::SessionNotFound(session_id.to_string())
                }
                _ => SecallError::Database(e),
            })?;

        let agent = match agent_str.as_str() {
            "claude-ai" => AgentKind::ClaudeAi,
            "codex" => AgentKind::Codex,
            "gemini-cli" => AgentKind::GeminiCli,
            "chatgpt" => AgentKind::ChatGpt,
            _ => AgentKind::ClaudeCode,
        };

        let start_time = DateTime::parse_from_rfc3339(&start_time_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());

        let end_time = end_time_str.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .ok()
        });

        let cwd = cwd_str.map(std::path::PathBuf::from);

        // turns 조회
        let mut stmt = self.conn.prepare(
            "SELECT turn_index, role, content, timestamp FROM turns
             WHERE session_id = ?1 ORDER BY turn_index ASC",
        )?;
        let turns: Vec<Turn> = stmt
            .query_map([session_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .map(|(idx, role_str, content, ts_str)| {
                let role = match role_str.as_str() {
                    "assistant" => Role::Assistant,
                    "system" => Role::System,
                    _ => Role::User,
                };
                let timestamp = ts_str.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&chrono::Utc))
                        .ok()
                });
                Turn {
                    index: idx as u32,
                    role,
                    timestamp,
                    content,
                    actions: Vec::new(),
                    tokens: None,
                    thinking: None,
                    is_sidechain: false,
                }
            })
            .collect();

        Ok(Session {
            id: session_id.to_string(),
            agent,
            model,
            project,
            cwd,
            git_branch: None,
            host: None,
            start_time,
            end_time,
            turns,
            total_tokens: TokenUsage {
                input: tokens_in as u64,
                output: tokens_out as u64,
                cached: 0,
            },
            session_type: session_type.unwrap_or_else(|| "interactive".to_string()),
        })
    }

    /// 캐시에서 확장된 쿼리 조회. TTL 7일 초과 시 None.
    pub fn get_query_cache(&self, query: &str) -> Option<String> {
        let hash = Self::query_hash(query);
        self.conn
            .query_row(
                "SELECT expanded FROM query_cache
                 WHERE query_hash = ?1
                   AND datetime(created_at, '+7 days') > datetime('now')",
                [&hash],
                |row| row.get(0),
            )
            .ok()
    }

    /// 확장 결과를 캐시에 저장.
    pub fn set_query_cache(&self, query: &str, expanded: &str) -> Result<()> {
        let hash = Self::query_hash(query);
        self.conn.execute(
            "INSERT OR REPLACE INTO query_cache(query_hash, original, expanded, created_at)
             VALUES (?1, ?2, ?3, datetime('now'))",
            rusqlite::params![hash, query, expanded],
        )?;
        Ok(())
    }

    /// 전체 세션의 (id, cwd, project, agent, 첫 user turn content) 반환 (backfill용)
    #[allow(clippy::type_complexity)]
    pub fn get_all_sessions_for_classify(
        &self,
    ) -> Result<Vec<(String, Option<String>, Option<String>, String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.cwd, s.project, s.agent, COALESCE(t.content, '')
             FROM sessions s
             LEFT JOIN turns t ON t.session_id = s.id AND t.turn_index = (
                 SELECT MIN(t2.turn_index) FROM turns t2
                 WHERE t2.session_id = s.id AND t2.role = 'user'
             )",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// 세션의 session_type 업데이트
    pub fn update_session_type(&self, session_id: &str, session_type: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET session_type = ?1 WHERE id = ?2",
            rusqlite::params![session_type, session_id],
        )?;
        Ok(())
    }

    fn query_hash(query: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        query.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    #[cfg(test)]
    pub fn schema_version(&self) -> Result<u32> {
        let v: String = self.conn.query_row(
            "SELECT value FROM config WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )?;
        v.parse()
            .map_err(|e: std::num::ParseIntError| SecallError::Other(e.into()))
    }

    #[cfg(test)]
    pub fn table_exists(&self, name: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
            [name],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }
}

#[derive(Debug)]
pub struct DbStats {
    pub session_count: i64,
    pub turn_count: i64,
    pub vector_count: i64,
    pub recent_ingests: Vec<IngestLogEntry>,
}

#[derive(Debug)]
pub struct IngestLogEntry {
    pub session_id_prefix: String,
    pub agent: String,
    pub timestamp: String,
}

#[derive(Debug)]
pub struct TurnRow {
    pub turn_index: u32,
    pub role: String,
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::{AgentKind, Session, TokenUsage, Turn};
    use crate::store::SessionRepo;
    use chrono::TimeZone;

    fn make_test_session(id: &str) -> Session {
        Session {
            id: id.to_string(),
            agent: AgentKind::ClaudeCode,
            model: Some("claude-sonnet-4-6".to_string()),
            project: Some("test-project".to_string()),
            cwd: None,
            git_branch: None,
            host: None,
            start_time: chrono::Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap(),
            end_time: None,
            turns: vec![],
            total_tokens: TokenUsage {
                input: 100,
                output: 50,
                cached: 0,
            },
            session_type: "interactive".to_string(),
        }
    }

    #[test]
    fn test_open_memory_success() {
        let db = Database::open_memory().unwrap();
        assert!(db.table_exists("sessions").unwrap());
    }

    #[test]
    fn test_migrate_creates_sessions_table() {
        let db = Database::open_memory().unwrap();
        assert!(db.table_exists("sessions").unwrap());
    }

    #[test]
    fn test_migrate_creates_turns_fts() {
        let db = Database::open_memory().unwrap();
        // FTS tables appear as 'table' in sqlite_master
        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='turns_fts'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0);
    }

    #[test]
    fn test_schema_version_stored() {
        let db = Database::open_memory().unwrap();
        assert_eq!(db.schema_version().unwrap(), 4);
    }

    #[test]
    fn test_migrate_idempotent() {
        let db = Database::open_memory().unwrap();
        // Second migrate call should not error
        db.migrate().unwrap();
        assert_eq!(db.schema_version().unwrap(), 4);
    }

    // ─── CRUD tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_insert_session_and_exists() {
        let db = Database::open_memory().unwrap();
        let session = make_test_session("sess-001");

        assert!(!db.session_exists("sess-001").unwrap());
        db.insert_session(&session).unwrap();
        assert!(db.session_exists("sess-001").unwrap());
    }

    #[test]
    fn test_insert_session_idempotent() {
        let db = Database::open_memory().unwrap();
        let session = make_test_session("sess-idem");
        db.insert_session(&session).unwrap();
        // INSERT OR IGNORE — second insert must not error
        db.insert_session(&session).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 1);
    }

    #[test]
    fn test_count_sessions() {
        let db = Database::open_memory().unwrap();
        assert_eq!(db.count_sessions().unwrap(), 0);
        db.insert_session(&make_test_session("s1")).unwrap();
        db.insert_session(&make_test_session("s2")).unwrap();
        assert_eq!(db.count_sessions().unwrap(), 2);
    }

    #[test]
    fn test_session_exists_by_prefix() {
        let db = Database::open_memory().unwrap();
        db.insert_session(&make_test_session("abcdef1234567890"))
            .unwrap();
        assert!(db.session_exists_by_prefix("abcdef").unwrap());
        assert!(!db.session_exists_by_prefix("xxxxxx").unwrap());
    }

    #[test]
    fn test_update_vault_path() {
        let db = Database::open_memory().unwrap();
        db.insert_session(&make_test_session("sess-vp")).unwrap();
        db.update_session_vault_path("sess-vp", "raw/sessions/2026-04-01/sess-vp.md")
            .unwrap();
        let paths = db.list_session_vault_paths().unwrap();
        let found = paths.iter().any(|(id, vp)| {
            id == "sess-vp" && vp.as_deref() == Some("raw/sessions/2026-04-01/sess-vp.md")
        });
        assert!(found);
    }

    #[test]
    fn test_update_session_type() {
        let db = Database::open_memory().unwrap();
        db.insert_session(&make_test_session("sess-type")).unwrap();
        db.update_session_type("sess-type", "automated").unwrap();
        let sessions = db.get_all_sessions_for_classify().unwrap();
        let updated = sessions.iter().find(|(id, ..)| id == "sess-type").unwrap();
        assert_eq!(updated.0, "sess-type");
    }

    #[test]
    fn test_delete_session() {
        let db = Database::open_memory().unwrap();
        db.insert_session(&make_test_session("sess-del")).unwrap();
        assert!(db.session_exists("sess-del").unwrap());
        db.delete_session("sess-del").unwrap();
        assert!(!db.session_exists("sess-del").unwrap());
    }

    #[test]
    fn test_insert_turn_and_retrieve() {
        let db = Database::open_memory().unwrap();
        db.insert_session(&make_test_session("sess-turn")).unwrap();
        let turn = Turn {
            index: 0,
            role: crate::ingest::Role::User,
            content: "Hello, world!".to_string(),
            timestamp: None,
            actions: vec![],
            thinking: None,
            tokens: None,
            is_sidechain: false,
        };
        db.insert_turn("sess-turn", &turn).unwrap();
        let row = db.get_turn("sess-turn", 0).unwrap();
        assert_eq!(row.content, "Hello, world!");
    }

    #[test]
    fn test_insert_session_from_vault_and_fts() {
        use crate::ingest::markdown::SessionFrontmatter;
        let db = Database::open_memory().unwrap();
        let fm = SessionFrontmatter {
            session_id: "vault-001".to_string(),
            agent: "claude-code".to_string(),
            start_time: "2026-04-01T00:00:00+00:00".to_string(),
            ..Default::default()
        };
        db.insert_session_from_vault(
            &fm,
            "some body text about Rust",
            "raw/sessions/vault-001.md",
        )
        .unwrap();
        assert!(db.session_exists("vault-001").unwrap());
        // FTS row should be present
        let fts_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM turns_fts WHERE session_id = 'vault-001'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_count, 1);
    }
}
