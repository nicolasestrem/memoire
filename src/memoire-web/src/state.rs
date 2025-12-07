//! Shared application state

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use rusqlite::Connection;

/// Shared state across all handlers
#[derive(Clone)]
pub struct AppState {
    /// Database connection (wrapped for thread safety)
    pub db: Arc<Mutex<Connection>>,

    /// Data directory (for resolving video file paths)
    pub data_dir: PathBuf,
}

impl AppState {
    /// Create new application state
    pub fn new(db: Connection, data_dir: PathBuf) -> Self {
        Self {
            db: Arc::new(Mutex::new(db)),
            data_dir,
        }
    }
}
