use thiserror::Error;

#[derive(Error, Debug)]
pub enum OcrError {
    #[error("failed to initialize OCR engine: {0}")]
    EngineInitFailed(String),

    #[error("frame conversion error: {0}")]
    ConversionError(String),

    #[error("OCR processing error: {0}")]
    ProcessingError(String),

    #[error("windows API error: {0}")]
    WindowsError(#[from] windows::core::Error),

    #[error("image processing error: {0}")]
    ImageError(#[from] image::ImageError),

    #[error("serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, OcrError>;
