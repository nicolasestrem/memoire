use crate::error::{OcrError, Result};
use serde::{Deserialize, Serialize};
use tracing::debug;
use windows::{
    Foundation::IAsyncOperation,
    Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap},
    Media::Ocr::{OcrEngine, OcrResult as WinOcrResult},
    Globalization::Language,
};

/// OCR word with bounding box and confidence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrWord {
    pub text: String,
    pub confidence: f32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// OCR line containing multiple words
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrLine {
    pub text: String,
    pub words: Vec<OcrWord>,
}

/// Complete OCR result for a frame
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrFrameResult {
    pub text: String,
    pub lines: Vec<OcrLine>,
    pub confidence: f32,
}

/// Windows OCR engine wrapper
pub struct Engine {
    engine: OcrEngine,
}

impl Engine {
    /// Create a new OCR engine for the specified language
    pub fn new(language_tag: Option<&str>) -> Result<Self> {
        debug!("initializing OCR engine");

        let engine = if let Some(tag) = language_tag {
            let lang = Language::CreateLanguage(&tag.into())
                .map_err(|e| OcrError::EngineInitFailed(format!("invalid language tag '{}': {}", tag, e)))?;

            OcrEngine::TryCreateFromLanguage(&lang)
                .map_err(|e| OcrError::EngineInitFailed(format!("failed to create engine for language '{}': {}", tag, e)))?
        } else {
            OcrEngine::TryCreateFromUserProfileLanguages()
                .map_err(|e| OcrError::EngineInitFailed(format!("failed to create engine from user profile: {}", e)))?
        };

        debug!("OCR engine initialized successfully");
        Ok(Self { engine })
    }

    /// Create default OCR engine using system language
    pub fn default() -> Result<Self> {
        Self::new(None)
    }

    /// Create OCR engine for English
    pub fn english() -> Result<Self> {
        Self::new(Some("en-US"))
    }

    /// Perform OCR on a SoftwareBitmap
    pub async fn recognize(&self, bitmap: &SoftwareBitmap) -> Result<OcrFrameResult> {
        debug!("starting OCR recognition");

        // Perform async OCR
        let ocr_result: IAsyncOperation<WinOcrResult> = self.engine.RecognizeAsync(bitmap)
            .map_err(|e| OcrError::ProcessingError(format!("failed to start OCR: {}", e)))?;

        let result = ocr_result.get()
            .map_err(|e| OcrError::ProcessingError(format!("OCR recognition failed: {}", e)))?;

        // Parse results
        self.parse_result(&result)
    }

    /// Parse Windows OcrResult into our structured format
    fn parse_result(&self, result: &WinOcrResult) -> Result<OcrFrameResult> {
        let mut lines = Vec::new();
        let mut all_text = String::new();
        let mut total_confidence = 0.0;
        let mut word_count = 0;

        let win_lines = result.Lines()
            .map_err(|e| OcrError::ProcessingError(format!("failed to get OCR lines: {}", e)))?;

        for i in 0..win_lines.Size()? {
            let line = win_lines.GetAt(i)?;
            let line_text = line.Text()?.to_string();

            if !all_text.is_empty() {
                all_text.push('\n');
            }
            all_text.push_str(&line_text);

            let mut ocr_words = Vec::new();
            let words = line.Words()?;

            for j in 0..words.Size()? {
                let word = words.GetAt(j)?;
                let word_text = word.Text()?.to_string();
                let bbox = word.BoundingRect()?;

                // Windows OCR doesn't provide per-word confidence,
                // so we use a heuristic based on text characteristics
                let confidence = Self::estimate_confidence(&word_text);
                total_confidence += confidence;
                word_count += 1;

                ocr_words.push(OcrWord {
                    text: word_text,
                    confidence,
                    x: bbox.X,
                    y: bbox.Y,
                    width: bbox.Width,
                    height: bbox.Height,
                });
            }

            if !ocr_words.is_empty() {
                lines.push(OcrLine {
                    text: line_text,
                    words: ocr_words,
                });
            }
        }

        // Calculate average confidence
        let avg_confidence = if word_count > 0 {
            total_confidence / word_count as f32
        } else {
            0.0
        };

        debug!("OCR completed: {} lines, {} words, confidence: {:.2}",
               lines.len(), word_count, avg_confidence);

        Ok(OcrFrameResult {
            text: all_text,
            lines,
            confidence: avg_confidence,
        })
    }

    /// Estimate confidence based on text characteristics
    /// Since Windows OCR doesn't provide confidence scores, we use heuristics:
    /// - Length (longer words are usually more reliable)
    /// - Character variety (mix of upper/lower, presence of digits)
    /// - Common patterns (all caps, all numbers may be less reliable)
    fn estimate_confidence(text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }

        let mut score = 0.7; // base confidence

        // Length bonus (up to +0.15)
        let len_bonus = (text.len() as f32 / 20.0).min(0.15);
        score += len_bonus;

        // Character variety bonus
        let has_lower = text.chars().any(|c| c.is_lowercase());
        let has_upper = text.chars().any(|c| c.is_uppercase());
        let has_digit = text.chars().any(|c| c.is_numeric());

        if has_lower && has_upper {
            score += 0.05;
        }
        if has_digit && (has_lower || has_upper) {
            score += 0.05;
        }

        // Penalty for all caps or all digits (often OCR errors)
        if text.chars().all(|c| c.is_uppercase() || !c.is_alphabetic()) {
            score -= 0.1;
        }
        if text.chars().all(|c| c.is_numeric()) {
            score -= 0.15;
        }

        score.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidence_estimation() {
        assert!(Engine::estimate_confidence("Hello") > 0.7);
        assert!(Engine::estimate_confidence("HelloWorld123") > 0.8);
        assert!(Engine::estimate_confidence("ALLCAPS") < 0.7);
        assert!(Engine::estimate_confidence("12345") < 0.6);
        assert_eq!(Engine::estimate_confidence(""), 0.0);
    }
}
