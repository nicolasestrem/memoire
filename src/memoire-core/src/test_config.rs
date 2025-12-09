//! Test configuration module for orchestrated testing
//!
//! Provides TOML-based configuration with profile support for running
//! all Memoire components simultaneously during testing.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Main test configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    #[serde(default)]
    pub general: GeneralConfig,

    #[serde(default)]
    pub record: RecordConfig,

    #[serde(default)]
    pub index: IndexConfig,

    #[serde(default)]
    pub audio: AudioConfig,

    #[serde(default)]
    pub viewer: ViewerConfig,

    /// Named profiles that can override base config
    #[serde(default)]
    pub profiles: std::collections::HashMap<String, ProfileConfig>,
}

/// General orchestration settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Data directory for test data (defaults to Memoire/test-data)
    pub data_dir: Option<String>,

    /// Automatically download models if missing
    #[serde(default = "default_true")]
    pub auto_download_models: bool,
}

/// Recording configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordConfig {
    /// Frames per second for screen capture
    #[serde(default = "default_test_fps")]
    pub fps: f64,

    /// Use hardware encoding (NVENC) if available
    #[serde(default = "default_true")]
    pub use_hw_encoding: bool,

    /// Video chunk duration in seconds (default 300 = 5 minutes)
    #[serde(default = "default_chunk_duration")]
    pub chunk_duration_secs: u64,
}

/// OCR indexing configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexConfig {
    /// OCR processing rate (frames per second)
    #[serde(default = "default_ocr_fps")]
    pub ocr_fps: u32,

    /// OCR language code (e.g., "en-US")
    pub ocr_language: Option<String>,
}

/// Audio capture and transcription configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    /// Enable audio capture and transcription
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Specific audio device name (None = default device)
    pub device: Option<String>,
}

/// Web viewer configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewerConfig {
    /// Port for web viewer HTTP server
    #[serde(default = "default_viewer_port")]
    pub port: u16,
}

/// Profile for overriding settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileConfig {
    pub record: Option<RecordConfig>,
    pub index: Option<IndexConfig>,
    pub audio: Option<AudioConfig>,
    pub viewer: Option<ViewerConfig>,
}

// Default value functions
fn default_test_fps() -> f64 { 0.25 }
fn default_ocr_fps() -> u32 { 10 }
fn default_viewer_port() -> u16 { 8080 }
fn default_chunk_duration() -> u64 { 300 }
fn default_true() -> bool { true }

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            data_dir: None,
            auto_download_models: true,
        }
    }
}

impl Default for RecordConfig {
    fn default() -> Self {
        Self {
            fps: 0.25,
            use_hw_encoding: true,
            chunk_duration_secs: 300,
        }
    }
}

impl Default for IndexConfig {
    fn default() -> Self {
        Self {
            ocr_fps: 10,
            ocr_language: None,
        }
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            device: None,
        }
    }
}

impl Default for ViewerConfig {
    fn default() -> Self {
        Self { port: 8080 }
    }
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            record: RecordConfig::default(),
            index: IndexConfig::default(),
            audio: AudioConfig::default(),
            viewer: ViewerConfig::default(),
            profiles: Default::default(),
        }
    }
}

impl TestConfig {
    /// Load configuration from TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .context("Failed to read test config file")?;
        toml::from_str(&content)
            .context("Failed to parse test config TOML")
    }

    /// Apply a named profile, merging settings
    ///
    /// Profile settings override base configuration values.
    pub fn apply_profile(mut self, profile_name: &str) -> Result<Self> {
        let profile = self.profiles.get(profile_name)
            .with_context(|| format!("Profile '{}' not found", profile_name))?
            .clone();

        // Merge profile settings (profile overrides base)
        if let Some(record) = profile.record {
            self.record = record;
        }
        if let Some(index) = profile.index {
            self.index = index;
        }
        if let Some(audio) = profile.audio {
            self.audio = audio;
        }
        if let Some(viewer) = profile.viewer {
            self.viewer = viewer;
        }

        Ok(self)
    }

    /// Resolve data directory with fallback to default
    ///
    /// Returns the configured data directory or defaults to:
    /// %LOCALAPPDATA%\Memoire\test-data on Windows
    pub fn resolve_data_dir(&self) -> PathBuf {
        if let Some(ref dir) = self.general.data_dir {
            PathBuf::from(dir)
        } else {
            dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Memoire")
                .join("test-data")
        }
    }
}
