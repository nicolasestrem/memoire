//! Error types for speech-to-text

use thiserror::Error;

/// Errors that can occur during speech-to-text operations
#[derive(Error, Debug)]
pub enum SttError {
    /// Model file not found
    #[error("model not found at {path}: {message}")]
    ModelNotFound {
        path: String,
        message: String,
    },

    /// Model loading failed
    #[error("failed to load model: {0}")]
    ModelLoadError(String),

    /// Audio processing error
    #[error("audio processing error: {0}")]
    AudioError(String),

    /// Inference error
    #[error("inference error: {0}")]
    InferenceError(String),

    /// IO error
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    /// ONNX Runtime error
    #[error("ONNX Runtime error: {0}")]
    OrtError(String),
}

impl From<ort::Error> for SttError {
    fn from(e: ort::Error) -> Self {
        SttError::OrtError(e.to_string())
    }
}
