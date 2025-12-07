//! memoire-db - Database layer for Memoire
//!
//! Handles SQLite database operations with FTS5 full-text search.

mod schema;
mod migrations;
mod queries;
mod error;

pub use schema::*;
pub use queries::*;
pub use error::DatabaseError;

use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use tracing::{info, debug};

/// Database connection wrapper with initialization
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create database at the given path
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        info!("opening database at {:?}", path);

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Enable WAL mode for concurrent reads
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        conn.execute_batch("PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        let mut db = Self { conn };
        db.run_migrations()?;

        Ok(db)
    }

    /// Open an in-memory database (for testing)
    pub fn open_in_memory() -> Result<Self> {
        debug!("opening in-memory database");
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;

        let mut db = Self { conn };
        db.run_migrations()?;

        Ok(db)
    }

    /// Get a reference to the underlying connection
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Get a mutable reference to the underlying connection
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Run all pending migrations
    fn run_migrations(&mut self) -> Result<()> {
        migrations::run_all(&self.conn)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let db = Database::open_in_memory().unwrap();
        assert!(db.connection().is_autocommit());
    }
}
