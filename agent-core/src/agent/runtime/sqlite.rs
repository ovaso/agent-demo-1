use super::{RunLease, RunState, RunStatus, RunStore, RuntimeError};
use crate::context::Context;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::{
    fs::{File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    time::Duration,
};

/// 本地运行存储。一个数据库同时只允许一个执行者；读取状态不持有执行锁。
pub struct SqliteRunStore {
    connection: Connection,
    lock_path: PathBuf,
}

impl SqliteRunStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, RuntimeError> {
        if path.as_ref() == Path::new(":memory:") {
            return Err(RuntimeError::Invalid(
                "内存运行请使用 MemoryRunStore".into(),
            ));
        }
        let connection = Connection::open(path.as_ref()).map_err(RuntimeError::storage)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(RuntimeError::storage)?;
        let mut lock_path = std::fs::canonicalize(path.as_ref())
            .map_err(RuntimeError::storage)?
            .into_os_string();
        lock_path.push(".runtime.lock");
        let store = Self {
            connection,
            lock_path: lock_path.into(),
        };
        // Schema initialization is serialized by SQLite; opening a reader must
        // remain possible while another runtime holds the execution lease.
        store
            .connection
            .execute_batch(
                "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS agent_runs (
                 run_id TEXT PRIMARY KEY NOT NULL,
                 session_id TEXT NOT NULL,
                 revision INTEGER NOT NULL,
                 status TEXT NOT NULL,
                 checkpoint TEXT NOT NULL
             );
             CREATE INDEX IF NOT EXISTS agent_runs_session ON agent_runs(session_id);
             CREATE UNIQUE INDEX IF NOT EXISTS agent_runs_active_session
                 ON agent_runs(session_id) WHERE status NOT IN ('completed', 'cancelled');
             CREATE TABLE IF NOT EXISTS agent_contexts (
                 session_id TEXT PRIMARY KEY NOT NULL,
                 context_json TEXT NOT NULL
             );",
            )
            .map_err(RuntimeError::storage)?;
        Ok(store)
    }

    /// 与原 CLI 使用同一会话表；检查点是运行恢复的权威来源。
    pub fn session_context(&self, session_id: &str) -> Result<Option<Context>, RuntimeError> {
        let json: Option<String> = self
            .connection
            .query_row(
                "SELECT context_json FROM agent_contexts WHERE session_id=?1",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(RuntimeError::storage)?;
        json.map(|json| serde_json::from_str(&json).map_err(RuntimeError::storage))
            .transpose()
    }

    pub fn latest(&self, session_id: &str) -> Result<Option<RunState>, RuntimeError> {
        let id: Option<String> = self
            .connection
            .query_row(
                "SELECT run_id FROM agent_runs WHERE session_id=?1 ORDER BY rowid DESC LIMIT 1",
                [session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(RuntimeError::storage)?;
        match id {
            Some(id) => self.load(&id),
            None => Ok(None),
        }
    }

    /// 只清理会话投影，保留历史任务检查点；未结束任务需要先显式取消或完成。
    pub fn reset_session(&mut self, session_id: &str) -> Result<(), RuntimeError> {
        let _lease = self.acquire()?;
        if self.latest(session_id)?.is_some_and(|state| {
            !matches!(state.status, RunStatus::Completed | RunStatus::Cancelled)
        }) {
            return Err(RuntimeError::Invalid(
                "会话仍有未结束任务，请先恢复或取消".into(),
            ));
        }
        let mut context = self.session_context(session_id)?.unwrap_or_default();
        context.clear_history();
        self.connection
            .execute(
                "INSERT INTO agent_contexts(session_id,context_json) VALUES (?1,?2)
             ON CONFLICT(session_id) DO UPDATE SET context_json=excluded.context_json",
                params![
                    session_id,
                    serde_json::to_string(&context).map_err(RuntimeError::storage)?
                ],
            )
            .map_err(RuntimeError::storage)?;
        Ok(())
    }
}

impl RunStore for SqliteRunStore {
    type Lease = RunLease;

    fn acquire(&self) -> Result<RunLease, RuntimeError> {
        let file: File = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&self.lock_path)
            .map_err(RuntimeError::storage)?;
        file.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => RuntimeError::Busy,
            TryLockError::Error(error) => RuntimeError::storage(error),
        })?;
        Ok(RunLease {
            memory: None,
            file: Some(file),
        })
    }

    fn load(&self, run_id: &str) -> Result<Option<RunState>, RuntimeError> {
        let checkpoint: Option<(i64, String)> = self
            .connection
            .query_row(
                "SELECT revision,checkpoint FROM agent_runs WHERE run_id=?1",
                [run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(RuntimeError::storage)?;
        checkpoint
            .map(|(revision, json)| {
                let state: RunState = serde_json::from_str(&json).map_err(RuntimeError::storage)?;
                if state.id != run_id || i64::try_from(state.revision).ok() != Some(revision) {
                    return Err(RuntimeError::Invalid("检查点标识或版本与索引不一致".into()));
                }
                super::store::check_size(&state)?;
                Ok(state)
            })
            .transpose()
    }

    fn create(&mut self, state: &RunState) -> Result<(), RuntimeError> {
        let json = encode(state)?;
        let tx = self
            .connection
            .transaction()
            .map_err(RuntimeError::storage)?;
        tx.execute("INSERT INTO agent_runs(run_id,session_id,revision,status,checkpoint) VALUES (?1,?2,?3,?4,?5)",
            params![state.id, state.session_id, revision(state.revision)?, status(state), json]).map_err(storage_error)?;
        save_context(&tx, state)?;
        tx.commit().map_err(RuntimeError::storage)
    }

    fn save(&mut self, state: &RunState, expected_revision: u64) -> Result<(), RuntimeError> {
        if expected_revision.checked_add(1) != Some(state.revision) {
            return Err(RuntimeError::Conflict);
        }
        let json = encode(state)?;
        let tx = self
            .connection
            .transaction()
            .map_err(RuntimeError::storage)?;
        let changed = tx.execute("UPDATE agent_runs SET revision=?1,status=?2,checkpoint=?3 WHERE run_id=?4 AND revision=?5 AND session_id=?6",
            params![revision(state.revision)?, status(state), json, state.id, revision(expected_revision)?, state.session_id]).map_err(storage_error)?;
        if changed != 1 {
            return Err(RuntimeError::Conflict);
        }
        save_context(&tx, state)?;
        tx.commit().map_err(RuntimeError::storage)
    }
}

fn encode(state: &RunState) -> Result<String, RuntimeError> {
    // Count before allocation; the in-memory model response may exceed the saved-state cap.
    super::store::check_size(state)?;
    serde_json::to_string(state).map_err(RuntimeError::storage)
}

fn revision(value: u64) -> Result<i64, RuntimeError> {
    i64::try_from(value).map_err(|_| RuntimeError::Invalid("检查点版本超过 SQLite 整数范围".into()))
}

fn status(state: &RunState) -> &'static str {
    match state.status {
        RunStatus::Running => "running",
        RunStatus::Paused(_) => "paused",
        RunStatus::Completed => "completed",
        RunStatus::Cancelled => "cancelled",
    }
}

fn save_context(tx: &Transaction<'_>, state: &RunState) -> Result<(), RuntimeError> {
    if state
        .result()
        .is_some_and(|result| result.session_finished())
    {
        tx.execute(
            "DELETE FROM agent_contexts WHERE session_id=?1",
            [&state.session_id],
        )
        .map_err(RuntimeError::storage)?;
    } else {
        tx.execute(
            "INSERT INTO agent_contexts(session_id,context_json) VALUES (?1,?2)
            ON CONFLICT(session_id) DO UPDATE SET context_json=excluded.context_json
            WHERE agent_contexts.context_json != excluded.context_json",
            params![
                state.session_id,
                serde_json::to_string(&state.context).map_err(RuntimeError::storage)?
            ],
        )
        .map_err(RuntimeError::storage)?;
    }
    Ok(())
}

fn storage_error(error: rusqlite::Error) -> RuntimeError {
    if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
        RuntimeError::Conflict
    } else {
        RuntimeError::storage(error)
    }
}
