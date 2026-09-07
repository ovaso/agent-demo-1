use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use super::{Context, ContextStore, ContextStoreError, store::validate_session_id};

/// 基于 SQLite 的本地持久化上下文存储。
pub struct SqliteContextStore {
    connection: Connection,
}

impl SqliteContextStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ContextStoreError> {
        let connection = Connection::open(path).map_err(ContextStoreError::storage)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self, ContextStoreError> {
        let connection = Connection::open_in_memory().map_err(ContextStoreError::storage)?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self, ContextStoreError> {
        connection
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS agent_contexts (
                    session_id TEXT PRIMARY KEY NOT NULL,
                    context_json TEXT NOT NULL
                );
                ",
            )
            .map_err(ContextStoreError::storage)?;

        Ok(Self { connection })
    }
}

impl ContextStore for SqliteContextStore {
    fn load(&self, session_id: &str) -> Result<Option<Context>, ContextStoreError> {
        validate_session_id(session_id)?;

        let context_json = self
            .connection
            .query_row(
                "SELECT context_json FROM agent_contexts WHERE session_id = ?1",
                [session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(ContextStoreError::storage)?;

        context_json
            .map(|json| serde_json::from_str(&json).map_err(ContextStoreError::serialization))
            .transpose()
    }

    fn save(&mut self, session_id: &str, context: &Context) -> Result<(), ContextStoreError> {
        validate_session_id(session_id)?;
        let context_json =
            serde_json::to_string(context).map_err(ContextStoreError::serialization)?;

        self.connection
            .execute(
                "
                INSERT INTO agent_contexts (session_id, context_json)
                VALUES (?1, ?2)
                ON CONFLICT(session_id) DO UPDATE SET context_json = excluded.context_json
                ",
                params![session_id, context_json],
            )
            .map_err(ContextStoreError::storage)?;

        Ok(())
    }

    fn delete(&mut self, session_id: &str) -> Result<bool, ContextStoreError> {
        validate_session_id(session_id)?;
        let removed = self
            .connection
            .execute(
                "DELETE FROM agent_contexts WHERE session_id = ?1",
                [session_id],
            )
            .map_err(ContextStoreError::storage)?;

        Ok(removed > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_a_context() {
        let mut store = SqliteContextStore::open_in_memory().unwrap();
        let mut context = Context::with_system("保持简洁。");
        context.push_user("你好");

        store.save("session-1", &context).unwrap();

        assert_eq!(store.load("session-1").unwrap(), Some(context));
    }
}
