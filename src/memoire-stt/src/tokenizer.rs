//! Tokenizer for Parakeet TDT models
//!
//! Loads the tokens.txt vocabulary file and converts token IDs to text.
//! Uses SentencePiece-style word boundaries (▁ = U+2581).

use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info};

/// Word boundary marker used by SentencePiece
const WORD_BOUNDARY: char = '\u{2581}'; // ▁

/// Tokenizer for converting token IDs to text
pub struct Tokenizer {
    /// Map from token ID to token string
    id_to_token: HashMap<i32, String>,
    /// The blank token ID (typically vocab_size - 1)
    blank_id: i32,
    /// Total vocabulary size
    vocab_size: usize,
}

impl Tokenizer {
    /// Load tokenizer from tokens.txt file
    ///
    /// Expected format: `token_string token_id` per line
    /// Example:
    /// ```text
    /// <unk> 0
    /// ▁t 1
    /// ▁the 5
    /// <blk> 1024
    /// ```
    pub fn from_file(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        debug!("loading tokenizer from {:?}", path);

        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read tokens file {:?}: {}", path, e))?;

        Self::from_str(&content)
    }

    /// Load tokenizer from string content
    pub fn from_str(content: &str) -> anyhow::Result<Self> {
        let mut id_to_token = HashMap::new();
        let mut max_id: i32 = -1;
        let mut blank_id: Option<i32> = None;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // Parse "token id" format
            // Note: token may contain spaces, so we split from the right
            let parts: Vec<&str> = line.rsplitn(2, ' ').collect();
            if parts.len() != 2 {
                continue;
            }

            let id: i32 = parts[0].parse().map_err(|e| {
                anyhow::anyhow!("failed to parse token ID '{}': {}", parts[0], e)
            })?;
            let token = parts[1].to_string();

            // Check for blank token
            if token == "<blk>" || token == "<blank>" {
                blank_id = Some(id);
            }

            if id > max_id {
                max_id = id;
            }

            id_to_token.insert(id, token);
        }

        let vocab_size = (max_id + 1) as usize;

        // If blank wasn't explicitly marked, assume it's the last token
        let blank_id = blank_id.unwrap_or(max_id);

        info!(
            "loaded tokenizer: vocab_size={}, blank_id={}",
            vocab_size, blank_id
        );

        Ok(Self {
            id_to_token,
            blank_id,
            vocab_size,
        })
    }

    /// Get the blank token ID
    pub fn blank_id(&self) -> i32 {
        self.blank_id
    }

    /// Get vocabulary size
    pub fn vocab_size(&self) -> usize {
        self.vocab_size
    }

    /// Decode a single token ID to its string representation
    pub fn decode_token(&self, id: i32) -> Option<&str> {
        self.id_to_token.get(&id).map(|s| s.as_str())
    }

    /// Decode a sequence of token IDs to text
    ///
    /// Handles SentencePiece word boundaries by replacing ▁ with space.
    /// Strips leading space from the result.
    pub fn decode(&self, tokens: &[i32]) -> String {
        let mut result = String::new();

        for &id in tokens {
            // Skip blank tokens
            if id == self.blank_id {
                continue;
            }

            if let Some(token) = self.id_to_token.get(&id) {
                // Replace word boundary marker with space
                let text = token.replace(WORD_BOUNDARY, " ");
                result.push_str(&text);
            }
        }

        // Trim leading/trailing whitespace
        result.trim().to_string()
    }

    /// Decode tokens with timestamps to create word-level segments
    ///
    /// Groups consecutive tokens into words based on word boundaries.
    /// Returns (word, start_time, end_time) tuples.
    pub fn decode_with_timestamps(
        &self,
        tokens: &[i32],
        timestamps: &[i32],
        frame_duration_ms: f64,
    ) -> Vec<(String, f64, f64)> {
        if tokens.is_empty() {
            return Vec::new();
        }

        let mut segments = Vec::new();
        let mut current_word = String::new();
        let mut word_start: Option<f64> = None;
        let mut word_end: f64 = 0.0;

        for (i, &token_id) in tokens.iter().enumerate() {
            if token_id == self.blank_id {
                continue;
            }

            let timestamp = timestamps.get(i).copied().unwrap_or(0);
            let time_sec = timestamp as f64 * frame_duration_ms / 1000.0;

            if let Some(token) = self.id_to_token.get(&token_id) {
                // Check if this token starts a new word
                let starts_word = token.starts_with(WORD_BOUNDARY);

                if starts_word && !current_word.is_empty() {
                    // Save the previous word
                    if let Some(start) = word_start {
                        segments.push((current_word.clone(), start, word_end));
                    }
                    current_word.clear();
                    word_start = None;
                }

                // Add token to current word (removing word boundary marker)
                let clean_token = token.replace(WORD_BOUNDARY, "");
                if !clean_token.is_empty() {
                    if word_start.is_none() {
                        word_start = Some(time_sec);
                    }
                    current_word.push_str(&clean_token);
                    word_end = time_sec;
                }
            }
        }

        // Don't forget the last word
        if !current_word.is_empty() {
            if let Some(start) = word_start {
                segments.push((current_word, start, word_end));
            }
        }

        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenizer_parse() {
        let content = r#"<unk> 0
▁t 1
▁th 2
▁a 3
in 4
▁the 5
<blk> 6"#;

        let tokenizer = Tokenizer::from_str(content).unwrap();
        assert_eq!(tokenizer.vocab_size(), 7);
        assert_eq!(tokenizer.blank_id(), 6);
    }

    #[test]
    fn test_decode() {
        let content = r#"<unk> 0
▁t 1
▁th 2
▁a 3
in 4
▁the 5
<blk> 6"#;

        let tokenizer = Tokenizer::from_str(content).unwrap();

        // "the" is token 5
        assert_eq!(tokenizer.decode(&[5]), "the");

        // "▁a" + "in" = " ain" -> "ain"
        assert_eq!(tokenizer.decode(&[3, 4]), "ain");
    }

    #[test]
    fn test_decode_skips_blank() {
        let content = r#"▁hello 0
▁world 1
<blk> 2"#;

        let tokenizer = Tokenizer::from_str(content).unwrap();
        assert_eq!(tokenizer.decode(&[0, 2, 1]), "hello world");
    }
}
