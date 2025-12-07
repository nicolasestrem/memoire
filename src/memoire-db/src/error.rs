//! Database error types

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid data: {0}")]
    InvalidData(String),
}
