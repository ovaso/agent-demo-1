use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use super::{Memory, MemoryStore, MemoryStoreError, validate_id};

/// 基于 SQLite 的长期记忆存储。
pub struct SqliteMemoryStore {
    connection: Connection,
}

impl SqliteMemoryStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MemoryStoreError> {
        let connection = Connection::open(path).map_err(MemoryStoreError::storage)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self, MemoryStoreError> {
        let connection = Connection::open_in_memory().map_err(MemoryStoreError::storage)?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self, MemoryStoreError> {
        connection
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS agent_memories (
                    id TEXT PRIMARY KEY NOT NULL,
                    content TEXT NOT NULL,
                    tags_json TEXT NOT NULL
                );
                ",
            )
            .map_err(MemoryStoreError::storage)?;
        Ok(Self { connection })
    }

    fn decode_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Memory> {
        let tags_json: String = row.get(2)?;
        let tags = serde_json::from_str(&tags_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        Ok(Memory {
            id: row.get(0)?,
            content: row.get(1)?,
            tags,
        })
    }
}

impl MemoryStore for SqliteMemoryStore {
    fn get(&self, id: &str) -> Result<Option<Memory>, MemoryStoreError> {
        validate_id(id)?;
        self.connection
            .query_row(
                "SELECT id, content, tags_json FROM agent_memories WHERE id = ?1",
                [id],
                Self::decode_row,
            )
            .optional()
            .map_err(MemoryStoreError::storage)
    }

    fn save(&mut self, memory: Memory) -> Result<(), MemoryStoreError> {
        validate_id(memory.id())?;
        let tags_json =
            serde_json::to_string(memory.tags()).map_err(MemoryStoreError::serialization)?;
        self.connection
            .execute(
                "
                INSERT INTO agent_memories (id, content, tags_json)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(id) DO UPDATE SET
                    content = excluded.content,
                    tags_json = excluded.tags_json
                ",
                params![memory.id(), memory.content(), tags_json],
            )
            .map_err(MemoryStoreError::storage)?;
        Ok(())
    }

    fn list(&self) -> Result<Vec<Memory>, MemoryStoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT id, content, tags_json FROM agent_memories ORDER BY id")
            .map_err(MemoryStoreError::storage)?;
        let rows = statement
            .query_map([], Self::decode_row)
            .map_err(MemoryStoreError::storage)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(MemoryStoreError::storage)
    }

    fn search(&self, query: &str) -> Result<Vec<Memory>, MemoryStoreError> {
        let query = format!("%{}%", query.to_lowercase());
        let mut statement = self
            .connection
            .prepare(
                "
                SELECT id, content, tags_json
                FROM agent_memories
                WHERE lower(id) LIKE ?1
                   OR lower(content) LIKE ?1
                   OR lower(tags_json) LIKE ?1
                ORDER BY id
                ",
            )
            .map_err(MemoryStoreError::storage)?;
        let rows = statement
            .query_map([query], Self::decode_row)
            .map_err(MemoryStoreError::storage)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(MemoryStoreError::storage)
    }

    fn delete(&mut self, id: &str) -> Result<bool, MemoryStoreError> {
        validate_id(id)?;
        let removed = self
            .connection
            .execute("DELETE FROM agent_memories WHERE id = ?1", [id])
            .map_err(MemoryStoreError::storage)?;
        Ok(removed > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_searches_memories() {
        let mut store = SqliteMemoryStore::open_in_memory().unwrap();
        let memory = Memory::new("rust", "Rust 的所有权模型避免悬垂引用")
            .with_tag("语言")
            .with_tag("所有权");
        store.save(memory.clone()).unwrap();
        assert_eq!(store.get("rust").unwrap(), Some(memory));
        assert_eq!(store.search("所有权").unwrap().len(), 1);
        assert!(store.delete("rust").unwrap());
    }
}
