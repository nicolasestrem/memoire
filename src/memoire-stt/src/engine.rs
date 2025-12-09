//! Speech-to-text engine using ONNX Runtime
//!
//! This module provides the main STT functionality using Parakeet TDT model.
//! Note: The actual model integration will be completed when the model is downloaded.

use anyhow::{Context, Result};
use ort::session::Session;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::error::SttError;

/// Configuration for the STT engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    /// Path to the ONNX model directory
    pub model_dir: PathBuf,
    /// Whether to use GPU acceleration
    pub use_gpu: bool,
    /// Language code (e.g., "en", "fr", "de")
    pub language: Option<String>,
    /// Number of threads for CPU inference
    pub num_threads: usize,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            model_dir: crate::default_model_dir(),
            use_gpu: true,
            language: None, // Auto-detect
            num_threads: 4,
        }
    }
}

/// A segment of transcription with timing information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    /// Start time in seconds
    pub start: f64,
    /// End time in seconds
    pub end: f64,
    /// Transcribed text
    pub text: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
}

/// Result of transcription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// Full transcribed text
    pub text: String,
    /// Individual segments with timestamps
    pub segments: Vec<TranscriptionSegment>,
    /// Detected language (if auto-detected)
    pub language: Option<String>,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
}

/// Speech-to-text engine
pub struct SttEngine {
    config: SttConfig,
    session: Option<Session>,
    is_gpu_enabled: bool,
}

impl SttEngine {
    /// Create a new STT engine with the given configuration
    pub fn new(config: SttConfig) -> Result<Self> {
        info!("initializing STT engine");
        info!("model directory: {:?}", config.model_dir);
        info!("GPU enabled: {}", config.use_gpu);

        // Check if model exists
        let encoder_path = config.model_dir.join("encoder.onnx");
        let decoder_path = config.model_dir.join("decoder.onnx");

        let has_model = encoder_path.exists() && decoder_path.exists();

        if !has_model {
            warn!("model files not found at {:?}", config.model_dir);
            warn!("STT engine will return placeholder results until model is downloaded");
            warn!("Run 'memoire download-models' to download the Parakeet TDT model");

            return Ok(Self {
                config,
                session: None,
                is_gpu_enabled: false,
            });
        }

        // Initialize ONNX Runtime
        let builder = Session::builder()?;

        // Set CPU thread count
        let builder = builder.with_intra_threads(config.num_threads)?;

        // Try to enable GPU if requested
        let mut is_gpu_enabled = false;
        let builder = if config.use_gpu {
            match builder.with_execution_providers([
                ort::execution_providers::CUDAExecutionProvider::default().build(),
            ]) {
                Ok(b) => {
                    is_gpu_enabled = true;
                    info!("CUDA execution provider enabled");
                    b
                }
                Err(e) => {
                    warn!("failed to enable CUDA, falling back to CPU: {}", e);
                    // Re-create builder for CPU-only
                    Session::builder()?
                        .with_intra_threads(config.num_threads)?
                }
            }
        } else {
            builder
        };

        // Load the encoder model (we'll need both encoder and decoder for full pipeline)
        let session = builder
            .commit_from_file(&encoder_path)
            .context("failed to load encoder model")?;

        info!("STT engine initialized successfully");
        info!("execution provider: {}", if is_gpu_enabled { "CUDA" } else { "CPU" });

        Ok(Self {
            config,
            session: Some(session),
            is_gpu_enabled,
        })
    }

    /// Check if GPU acceleration is enabled
    pub fn is_gpu_enabled(&self) -> bool {
        self.is_gpu_enabled
    }

    /// Check if the model is loaded
    pub fn is_model_loaded(&self) -> bool {
        self.session.is_some()
    }

    /// Transcribe audio from a WAV file
    pub fn transcribe_file(&self, path: impl AsRef<Path>) -> Result<TranscriptionResult> {
        let path = path.as_ref();
        debug!("transcribing file: {:?}", path);

        // Load audio
        let audio = self.load_audio(path)?;

        // Transcribe
        self.transcribe_samples(&audio.samples, audio.sample_rate)
    }

    /// Transcribe audio samples directly
    pub fn transcribe_samples(&self, samples: &[f32], sample_rate: u32) -> Result<TranscriptionResult> {
        let start_time = std::time::Instant::now();

        // If no model is loaded, return placeholder
        if self.session.is_none() {
            warn!("no model loaded, returning placeholder result");
            return Ok(TranscriptionResult {
                text: "[Model not loaded - run 'memoire download-models' first]".to_string(),
                segments: vec![TranscriptionSegment {
                    start: 0.0,
                    end: samples.len() as f64 / sample_rate as f64,
                    text: "[Model not loaded]".to_string(),
                    confidence: 0.0,
                }],
                language: None,
                processing_time_ms: start_time.elapsed().as_millis() as u64,
            });
        }

        // Preprocess audio (ensure 16kHz mono)
        let processed_samples = self.preprocess_audio(samples, sample_rate)?;

        // Run inference
        let result = self.run_inference(&processed_samples)?;

        let processing_time_ms = start_time.elapsed().as_millis() as u64;
        debug!("transcription completed in {}ms", processing_time_ms);

        Ok(TranscriptionResult {
            text: result.text,
            segments: result.segments,
            language: result.language,
            processing_time_ms,
        })
    }

    /// Load audio from a WAV file
    fn load_audio(&self, path: &Path) -> Result<AudioData> {
        let reader = hound::WavReader::open(path)
            .context("failed to open WAV file")?;

        let spec = reader.spec();
        let sample_rate = spec.sample_rate;
        let channels = spec.channels as usize;

        debug!("loading audio: {} Hz, {} channels", sample_rate, channels);

        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => {
                reader.into_samples::<f32>()
                    .filter_map(|s| s.ok())
                    .collect()
            }
            hound::SampleFormat::Int => {
                let bits = spec.bits_per_sample;
                let max_val = (1 << (bits - 1)) as f32;
                reader.into_samples::<i32>()
                    .filter_map(|s| s.ok())
                    .map(|s| s as f32 / max_val)
                    .collect()
            }
        };

        // Convert to mono if stereo
        let mono_samples = if channels > 1 {
            samples
                .chunks(channels)
                .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
                .collect()
        } else {
            samples
        };

        Ok(AudioData {
            samples: mono_samples,
            sample_rate,
        })
    }

    /// Preprocess audio for the model (resample to 16kHz if needed)
    fn preprocess_audio(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<f32>> {
        if sample_rate == 16000 {
            return Ok(samples.to_vec());
        }

        // Resample to 16kHz
        debug!("resampling from {} Hz to 16000 Hz", sample_rate);

        use rubato::{FftFixedIn, Resampler};

        let mut resampler = FftFixedIn::<f32>::new(
            sample_rate as usize,
            16000,
            samples.len(),
            1, // chunk size
            1, // channels (mono)
        )?;

        let input = vec![samples.to_vec()];
        let output = resampler.process(&input, None)?;

        Ok(output.into_iter().flatten().collect())
    }

    /// Run model inference (placeholder - actual implementation depends on model)
    fn run_inference(&self, samples: &[f32]) -> Result<TranscriptionResult> {
        let _session = self.session.as_ref()
            .ok_or_else(|| SttError::ModelLoadError("model not loaded".to_string()))?;

        // TODO: Implement actual Parakeet TDT inference
        // For now, return a placeholder indicating the model is not fully integrated
        //
        // The actual implementation would:
        // 1. Convert audio to mel spectrogram features
        // 2. Run encoder to get hidden states
        // 3. Run decoder with beam search for transcription
        // 4. Post-process to get timestamps

        let duration = samples.len() as f64 / 16000.0;

        Ok(TranscriptionResult {
            text: "[Parakeet TDT model inference not yet implemented]".to_string(),
            segments: vec![TranscriptionSegment {
                start: 0.0,
                end: duration,
                text: "[Inference placeholder]".to_string(),
                confidence: 0.0,
            }],
            language: self.config.language.clone(),
            processing_time_ms: 0,
        })
    }
}

/// Internal struct for loaded audio data
struct AudioData {
    samples: Vec<f32>,
    sample_rate: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = SttConfig::default();
        assert!(config.use_gpu);
        assert_eq!(config.num_threads, 4);
    }
}
