//! Audio encoding and chunk management
//!
//! Manages audio chunks similar to VideoEncoder, saving WAV files
//! at configured intervals.

use anyhow::Result;
use chrono::{DateTime, Utc};
use hound::{WavSpec, WavWriter};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Audio encoder configuration
#[derive(Debug, Clone)]
pub struct AudioEncoderConfig {
    /// Output directory for audio chunks
    pub output_dir: PathBuf,
    /// Chunk duration in seconds
    pub chunk_duration_secs: u32,
    /// Sample rate (typically 16000 for STT)
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u16,
}

impl Default for AudioEncoderConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("audio"),
            chunk_duration_secs: 30,
            sample_rate: 16000,
            channels: 1,
        }
    }
}

/// Audio encoder that accumulates samples and creates WAV chunks
pub struct AudioEncoder {
    config: AudioEncoderConfig,
    /// Current audio samples buffer
    current_samples: Vec<f32>,
    /// Timestamp when current chunk started
    chunk_start_time: Option<DateTime<Utc>>,
    /// Current chunk index
    chunk_index: u64,
    /// Device name for path organization
    device_name: String,
}

impl AudioEncoder {
    /// Create a new audio encoder
    pub fn new(config: AudioEncoderConfig, device_name: &str) -> Result<Self> {
        // Ensure output directory exists
        fs::create_dir_all(&config.output_dir)?;

        // Calculate expected samples per chunk
        let samples_per_chunk = config.chunk_duration_secs as usize
            * config.sample_rate as usize
            * config.channels as usize;

        Ok(Self {
            config,
            current_samples: Vec::with_capacity(samples_per_chunk),
            chunk_start_time: None,
            chunk_index: 0,
            device_name: sanitize_device_name(device_name),
        })
    }

    /// Add audio samples to current chunk
    /// Returns the path to a completed chunk if one was finalized
    pub fn add_samples(&mut self, samples: &[f32], timestamp: DateTime<Utc>) -> Result<Option<PathBuf>> {
        // Set chunk start time if this is the first samples
        if self.chunk_start_time.is_none() {
            self.chunk_start_time = Some(timestamp);
        }

        // Add samples to buffer
        self.current_samples.extend_from_slice(samples);

        // Calculate expected samples per chunk
        let samples_per_chunk = self.config.chunk_duration_secs as usize
            * self.config.sample_rate as usize
            * self.config.channels as usize;

        // Check if we have enough samples for a complete chunk
        if self.current_samples.len() >= samples_per_chunk {
            return self.finalize_chunk();
        }

        Ok(None)
    }

    /// Force finalize the current chunk (even if not full)
    pub fn finalize_chunk(&mut self) -> Result<Option<PathBuf>> {
        if self.current_samples.is_empty() {
            return Ok(None);
        }

        let start_time = match self.chunk_start_time {
            Some(t) => t,
            None => Utc::now(),
        };

        // Create device/date directory structure
        let date_str = start_time.format("%Y-%m-%d").to_string();
        let time_str = start_time.format("%H-%M-%S").to_string();

        let device_dir = self.config.output_dir.join(&self.device_name);
        let date_dir = device_dir.join(&date_str);
        fs::create_dir_all(&date_dir)?;

        // Output path
        let output_path = date_dir.join(format!("chunk_{}_{}.wav", time_str, self.chunk_index));

        info!(
            "saving audio chunk: {:?} ({} samples, {:.1}s)",
            output_path,
            self.current_samples.len(),
            self.current_samples.len() as f32 / self.config.sample_rate as f32 / self.config.channels as f32
        );

        // Write WAV file
        self.save_wav(&output_path)?;

        // Reset state for next chunk
        self.current_samples.clear();
        self.chunk_start_time = None;
        self.chunk_index += 1;

        Ok(Some(output_path))
    }

    /// Save current samples as WAV file
    fn save_wav(&self, path: &Path) -> Result<()> {
        let spec = WavSpec {
            channels: self.config.channels,
            sample_rate: self.config.sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };

        let mut writer = WavWriter::create(path, spec)?;

        // Convert f32 samples to i16
        for &sample in &self.current_samples {
            let clamped = sample.clamp(-1.0, 1.0);
            let i16_sample = (clamped * i16::MAX as f32) as i16;
            writer.write_sample(i16_sample)?;
        }

        writer.finalize()?;
        debug!("saved WAV file: {:?}", path);

        Ok(())
    }

    /// Get the output directory
    pub fn output_dir(&self) -> &Path {
        &self.config.output_dir
    }

    /// Get the device name
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Get current buffer size in samples
    pub fn buffered_samples(&self) -> usize {
        self.current_samples.len()
    }

    /// Get current buffer duration in seconds
    pub fn buffered_duration(&self) -> f32 {
        self.current_samples.len() as f32 / self.config.sample_rate as f32 / self.config.channels as f32
    }
}

impl Drop for AudioEncoder {
    fn drop(&mut self) {
        // Try to finalize any remaining samples
        if !self.current_samples.is_empty() {
            if let Err(e) = self.finalize_chunk() {
                tracing::warn!("failed to finalize audio chunk on drop: {}", e);
            }
        }
    }
}

/// Windows reserved device names that cannot be used as filenames
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Sanitize device name for use as directory name
fn sanitize_device_name(name: &str) -> String {
    // Replace invalid filesystem characters
    let sanitized: String = name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            '.' if name.starts_with('.') => '_',
            '\0'..='\x1f' => '_', // Control characters
            _ => c,
        })
        .collect();

    // Remove path traversal sequences
    let sanitized = sanitized.replace("..", "_");

    // Trim leading/trailing whitespace, underscores, and dots
    let sanitized = sanitized
        .trim()
        .trim_matches(|c| c == '_' || c == '.' || c == ' ')
        .to_string();

    // Check for Windows reserved names (case-insensitive)
    let upper = sanitized.to_uppercase();
    let base_name = upper.split('.').next().unwrap_or(&upper);
    let sanitized = if WINDOWS_RESERVED_NAMES.contains(&base_name) {
        format!("_{}", sanitized)
    } else {
        sanitized
    };

    // Truncate to safe length
    let max_name_len = 100;
    let sanitized: String = sanitized.chars().take(max_name_len).collect();

    // Fallback for empty result
    if sanitized.is_empty() {
        "audio_device".to_string()
    } else {
        sanitized
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_device_name() {
        assert_eq!(sanitize_device_name("Microphone (HD Pro Webcam C920)"), "Microphone (HD Pro Webcam C920)");
        assert_eq!(sanitize_device_name("Device:with:colons"), "Device_with_colons");
        assert_eq!(sanitize_device_name("CON"), "_CON");
        assert_eq!(sanitize_device_name(""), "audio_device");
    }

    #[test]
    fn test_audio_encoder_config_default() {
        let config = AudioEncoderConfig::default();
        assert_eq!(config.chunk_duration_secs, 30);
        assert_eq!(config.sample_rate, 16000);
        assert_eq!(config.channels, 1);
    }
}
