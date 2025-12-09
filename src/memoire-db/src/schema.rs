//! Database schema types

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Video chunk metadata (5-minute MP4 segments)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoChunk {
    pub id: i64,
    pub file_path: String,
    pub device_name: String,
    pub created_at: DateTime<Utc>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Frame metadata within a video chunk
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub id: i64,
    pub video_chunk_id: i64,
    pub offset_index: i64,
    pub timestamp: DateTime<Utc>,
    pub app_name: Option<String>,
    pub window_name: Option<String>,
    pub browser_url: Option<String>,
    pub focused: bool,
    pub frame_hash: Option<i64>,
}

/// OCR extracted text from a frame
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrText {
    pub id: i64,
    pub frame_id: i64,
    pub text: String,
    pub text_json: Option<String>, // Bounding boxes as JSON
    pub confidence: Option<f64>,
}

/// Audio chunk metadata (30-second segments)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioChunk {
    pub id: i64,
    pub file_path: String,
    pub device_name: Option<String>,
    pub is_input_device: Option<bool>,
    pub timestamp: DateTime<Utc>,
}

/// Audio transcription with timestamps
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioTranscription {
    pub id: i64,
    pub audio_chunk_id: i64,
    pub transcription: String,
    pub timestamp: DateTime<Utc>,
    pub speaker_id: Option<i64>,
    pub start_time: Option<f64>,
    pub end_time: Option<f64>,
}

/// New video chunk to insert
#[derive(Debug, Clone)]
pub struct NewVideoChunk {
    pub file_path: String,
    pub device_name: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// New frame to insert
#[derive(Debug, Clone)]
pub struct NewFrame {
    pub video_chunk_id: i64,
    pub offset_index: i64,
    pub timestamp: DateTime<Utc>,
    pub app_name: Option<String>,
    pub window_name: Option<String>,
    pub browser_url: Option<String>,
    pub focused: bool,
    pub frame_hash: Option<i64>,
}

/// New OCR text to insert
#[derive(Debug, Clone)]
pub struct NewOcrText {
    pub frame_id: i64,
    pub text: String,
    pub text_json: Option<String>,
    pub confidence: Option<f64>,
}

/// Video chunk with frame count (for validation viewer)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkWithFrameCount {
    pub id: i64,
    pub file_path: String,
    pub device_name: String,
    pub created_at: DateTime<Utc>,
    pub frame_count: i64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Monitor statistics summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorSummary {
    pub device_name: String,
    pub total_chunks: i64,
    pub total_frames: i64,
    pub latest_capture: Option<DateTime<Utc>>,
}

/// Frame with optional OCR text (from LEFT JOIN)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameWithOcr {
    pub id: i64,
    pub video_chunk_id: i64,
    pub offset_index: i64,
    pub timestamp: DateTime<Utc>,
    pub app_name: Option<String>,
    pub window_name: Option<String>,
    pub browser_url: Option<String>,
    pub focused: bool,
    pub frame_hash: Option<i64>,
    pub ocr_text: Option<OcrText>,
}

/// OCR indexing statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrStats {
    pub total_frames: i64,
    pub frames_with_ocr: i64,
    pub pending_frames: i64,
    pub processing_rate: i64,
    pub last_updated: Option<DateTime<Utc>>,
}
