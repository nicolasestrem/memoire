//! memoire-stt - Speech-to-Text for Memoire
//!
//! Provides speech-to-text transcription using Parakeet TDT via ONNX Runtime.
//! Supports GPU acceleration via CUDA with CPU fallback.

mod engine;
mod error;

pub use engine::{SttEngine, SttConfig, TranscriptionResult, TranscriptionSegment};
pub use error::SttError;

/// Get the default model directory
pub fn default_model_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Memoire")
        .join("models")
}
