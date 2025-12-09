//! REST API handlers

use crate::{ApiError, AppState};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use memoire_db;
use serde::{Deserialize, Serialize};

// ============================================================================
// Audio API types
// ============================================================================

/// Query parameters for audio chunk listing
#[derive(Debug, Deserialize)]
pub struct AudioChunksQuery {
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub is_input: Option<bool>,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

/// Query parameters for audio search
#[derive(Debug, Deserialize)]
pub struct AudioSearchQuery {
    pub q: String,
    #[serde(default)]
    pub limit: Option<i64>,
    #[serde(default)]
    pub offset: Option<i64>,
}

/// Response for audio chunk listing
#[derive(Debug, Serialize)]
pub struct AudioChunksResponse {
    pub chunks: Vec<AudioChunkWithMetadata>,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct AudioChunkWithMetadata {
    pub id: i64,
    pub file_path: String,
    pub device_name: Option<String>,
    pub is_input_device: Option<bool>,
    pub timestamp: String,
    pub transcription_count: i64,
}

/// Query parameters for chunk listing
#[derive(Debug, Deserialize)]
pub struct ChunksQuery {
    #[serde(default)]
    monitor: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
}

/// Query parameters for frames
#[derive(Debug, Deserialize)]
pub struct FramesQuery {
    #[serde(default)]
    start: Option<String>,
    #[serde(default)]
    end: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
}

/// Query parameters for search
#[derive(Debug, Deserialize)]
pub struct SearchQuery {
    q: String,
    #[serde(default)]
    limit: Option<i64>,
    #[serde(default)]
    offset: Option<i64>,
}

/// Response for chunk listing
#[derive(Debug, Serialize)]
pub struct ChunksResponse {
    chunks: Vec<ChunkWithMetadata>,
    total: i64,
}

#[derive(Debug, Serialize)]
pub struct ChunkWithMetadata {
    id: i64,
    file_path: String,
    device_name: String,
    created_at: String,
    frame_count: i64,
}

/// GET /api/chunks
pub async fn get_chunks(
    State(state): State<AppState>,
    Query(params): Query<ChunksQuery>,
) -> Result<Json<ChunksResponse>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let limit = params.limit.unwrap_or(50).max(1).min(100);
    let offset = params.offset.unwrap_or(0).max(0);

    let chunks = memoire_db::get_chunks_paginated(
        &db,
        limit,
        offset,
        params.monitor.as_deref(),
        None, // start_date
        None, // end_date
    )
    .map_err(|e| ApiError::Database(e.to_string()))?;

    let total = memoire_db::get_total_chunk_count(
        &db,
        params.monitor.as_deref(),
        None, // start_date
        None, // end_date
    )
    .map_err(|e| ApiError::Database(e.to_string()))?;

    let chunks_with_metadata = chunks
        .into_iter()
        .map(|c| ChunkWithMetadata {
            id: c.id,
            file_path: c.file_path,
            device_name: c.device_name,
            created_at: c.created_at.to_rfc3339(),
            frame_count: c.frame_count,
        })
        .collect();

    Ok(Json(ChunksResponse {
        chunks: chunks_with_metadata,
        total,
    }))
}

/// GET /api/chunks/:id
pub async fn get_chunk(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let chunk = memoire_db::get_video_chunk(&db, id)?
        .ok_or_else(|| ApiError::NotFound(format!("chunk {} not found", id)))?;

    let frame_count = memoire_db::get_frame_count_by_chunk(&db, id)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "id": chunk.id,
        "file_path": chunk.file_path,
        "device_name": chunk.device_name,
        "created_at": chunk.created_at.to_rfc3339(),
        "frame_count": frame_count,
    })))
}

/// GET /api/chunks/:id/frames (stub)
pub async fn get_chunk_frames(
    State(_state): State<AppState>,
    Path(_id): Path<i64>,
    Query(_params): Query<ChunksQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::NotImplemented(
        "GET /api/chunks/:id/frames endpoint not yet implemented".to_string()
    ))
}

/// GET /api/frames (stub)
pub async fn get_frames(
    State(_state): State<AppState>,
    Query(_params): Query<FramesQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::NotImplemented(
        "GET /api/frames endpoint not yet implemented".to_string()
    ))
}

/// GET /api/frames/:id
pub async fn get_frame(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let frame = memoire_db::get_frame(&db, id)?
        .ok_or_else(|| ApiError::NotFound(format!("frame {} not found", id)))?;

    let chunk = memoire_db::get_video_chunk(&db, frame.video_chunk_id)?
        .ok_or_else(|| ApiError::NotFound("chunk not found".to_string()))?;

    // Get OCR text if available
    let ocr = memoire_db::get_ocr_text_by_frame(&db, id)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    let mut response = serde_json::json!({
        "id": frame.id,
        "video_chunk_id": frame.video_chunk_id,
        "offset_index": frame.offset_index,
        "timestamp": frame.timestamp.to_rfc3339(),
        "app_name": frame.app_name,
        "window_name": frame.window_name,
        "browser_url": frame.browser_url,
        "focused": frame.focused,
        "chunk": {
            "file_path": chunk.file_path,
            "device_name": chunk.device_name,
        },
    });

    // Add OCR data if available
    if let Some(ocr_data) = ocr {
        response["ocr"] = serde_json::json!({
            "text": ocr_data.text,
            "text_json": ocr_data.text_json,
            "confidence": ocr_data.confidence,
        });
    }

    Ok(Json(response))
}

/// GET /api/stats
pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let total_frames = memoire_db::get_frame_count(&db)?;
    let monitors = memoire_db::get_monitors_summary(&db)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    let total_chunks: i64 = monitors.iter().map(|m| m.total_chunks).sum();

    Ok(Json(serde_json::json!({
        "total_frames": total_frames,
        "total_chunks": total_chunks,
        "monitors": monitors,
    })))
}

/// GET /api/monitors
pub async fn get_monitors(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let monitors = memoire_db::get_monitors_summary(&db)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "monitors": monitors,
    })))
}

/// GET /api/search
pub async fn search_ocr(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let limit = params.limit.unwrap_or(50).max(1).min(100);
    let offset = params.offset.unwrap_or(0).max(0);

    // Sanitize the search query for FTS5
    let sanitized_query = memoire_db::sanitize_fts5_query(&params.q)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Get total count
    let total = memoire_db::get_search_count(&db, &sanitized_query)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    // Get search results
    let results = memoire_db::search_ocr(&db, &sanitized_query, limit, offset)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    // Transform results into response format
    let results_json: Vec<serde_json::Value> = results
        .into_iter()
        .map(|(ocr, frame)| {
            serde_json::json!({
                "frame": {
                    "id": frame.id,
                    "timestamp": frame.timestamp.to_rfc3339(),
                    "app_name": frame.app_name,
                    "window_name": frame.window_name,
                    "browser_url": frame.browser_url,
                },
                "ocr": {
                    "text": ocr.text,
                    "confidence": ocr.confidence,
                },
            })
        })
        .collect();

    let has_more = offset + limit < total;

    Ok(Json(serde_json::json!({
        "results": results_json,
        "total": total,
        "has_more": has_more,
        "limit": limit,
        "offset": offset,
    })))
}

/// GET /api/stats/ocr
pub async fn get_ocr_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let stats = memoire_db::get_ocr_stats(&db)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "total_frames": stats.total_frames,
        "frames_with_ocr": stats.frames_with_ocr,
        "pending_frames": stats.pending_frames,
        "processing_rate": stats.processing_rate,
        "last_updated": stats.last_updated.map(|dt| dt.to_rfc3339()),
    })))
}

// ============================================================================
// Audio API handlers
// ============================================================================

/// GET /api/audio-chunks - List audio chunks with pagination
pub async fn get_audio_chunks(
    State(state): State<AppState>,
    Query(params): Query<AudioChunksQuery>,
) -> Result<Json<AudioChunksResponse>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let limit = params.limit.unwrap_or(50).max(1).min(100);
    let offset = params.offset.unwrap_or(0).max(0);

    let chunks = memoire_db::get_audio_chunks_paginated(
        &db,
        limit,
        offset,
        params.device.as_deref(),
        params.is_input,
    )
    .map_err(|e| ApiError::Database(e.to_string()))?;

    let total = memoire_db::get_total_audio_chunk_count(&db, params.device.as_deref())
        .map_err(|e| ApiError::Database(e.to_string()))?;

    let chunks_with_metadata = chunks
        .into_iter()
        .map(|c| AudioChunkWithMetadata {
            id: c.id,
            file_path: c.file_path,
            device_name: c.device_name,
            is_input_device: c.is_input_device,
            timestamp: c.timestamp.to_rfc3339(),
            transcription_count: c.transcription_count,
        })
        .collect();

    Ok(Json(AudioChunksResponse {
        chunks: chunks_with_metadata,
        total,
    }))
}

/// GET /api/audio-chunks/:id - Get single audio chunk with transcription
pub async fn get_audio_chunk(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let chunk = memoire_db::get_audio_chunk(&db, id)?
        .ok_or_else(|| ApiError::NotFound(format!("audio chunk {} not found", id)))?;

    // Get all transcriptions for this chunk
    let transcriptions = memoire_db::get_transcriptions_by_chunk(&db, id)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    // Build combined transcription text
    let full_text: String = transcriptions
        .iter()
        .map(|t| t.transcription.as_str())
        .collect::<Vec<_>>()
        .join(" ");

    // Build segments array
    let segments: Vec<serde_json::Value> = transcriptions
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "text": t.transcription,
                "start_time": t.start_time,
                "end_time": t.end_time,
                "speaker_id": t.speaker_id,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({
        "id": chunk.id,
        "file_path": chunk.file_path,
        "device_name": chunk.device_name,
        "is_input_device": chunk.is_input_device,
        "timestamp": chunk.timestamp.to_rfc3339(),
        "transcription": {
            "text": full_text,
            "segments": segments,
        },
    })))
}

/// GET /api/audio-search - Full-text search on audio transcriptions
pub async fn search_audio(
    State(state): State<AppState>,
    Query(params): Query<AudioSearchQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let limit = params.limit.unwrap_or(50).max(1).min(100);
    let offset = params.offset.unwrap_or(0).max(0);

    // Sanitize the search query for FTS5
    let sanitized_query = memoire_db::sanitize_fts5_query(&params.q)
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    // Get total count
    let total = memoire_db::get_audio_search_count(&db, &sanitized_query)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    // Get search results
    let results = memoire_db::search_transcriptions(&db, &sanitized_query, limit, offset)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    // Transform results into response format
    let results_json: Vec<serde_json::Value> = results
        .into_iter()
        .map(|(transcription, chunk)| {
            serde_json::json!({
                "chunk": {
                    "id": chunk.id,
                    "file_path": chunk.file_path,
                    "device_name": chunk.device_name,
                    "timestamp": chunk.timestamp.to_rfc3339(),
                },
                "transcription": {
                    "text": transcription.transcription,
                    "start_time": transcription.start_time,
                    "end_time": transcription.end_time,
                    "speaker_id": transcription.speaker_id,
                },
            })
        })
        .collect();

    let has_more = offset + limit < total;

    Ok(Json(serde_json::json!({
        "results": results_json,
        "total": total,
        "has_more": has_more,
        "limit": limit,
        "offset": offset,
    })))
}

/// GET /api/stats/audio - Get audio transcription statistics
pub async fn get_audio_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock()
        .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;

    let stats = memoire_db::get_audio_stats(&db)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "total_chunks": stats.total_chunks,
        "chunks_with_transcription": stats.chunks_with_transcription,
        "pending_chunks": stats.pending_chunks,
        "processing_rate": stats.processing_rate,
        "last_updated": stats.last_updated.map(|dt| dt.to_rfc3339()),
    })))
}
