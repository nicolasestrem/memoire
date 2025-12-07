//! Windows OCR integration for Memoire
//!
//! This crate provides OCR (Optical Character Recognition) capabilities using the
//! Windows.Media.Ocr API. It processes RGBA frames and extracts text with bounding
//! boxes and confidence scores.

mod engine;
mod error;
mod processor;

pub use engine::{Engine, OcrFrameResult, OcrLine, OcrWord};
pub use error::{OcrError, Result};
pub use processor::{FrameData, Processor};

/// Initialize OCR processor with default settings (English)
pub fn create_processor() -> Result<Processor> {
    Processor::new()
}

/// Initialize OCR processor with custom language
pub fn create_processor_with_language(language_tag: &str) -> Result<Processor> {
    Processor::with_language(language_tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_processor() {
        let processor = create_processor();
        assert!(processor.is_ok());
    }
}
