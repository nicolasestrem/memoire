//! Configuration management

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Recorder configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Data directory for videos and database
    pub data_dir: PathBuf,

    /// Recording framerate
    pub fps: u32,

    /// Use hardware encoding (NVENC)
    pub use_hw_encoding: bool,

    /// Video chunk duration in seconds
    pub chunk_duration_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            data_dir: dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Memoire"),
            fps: 1,
            use_hw_encoding: true,
            chunk_duration_secs: 300,
        }
    }
}
