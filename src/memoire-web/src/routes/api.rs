//! REST API handlers

use crate::{ApiError, AppState};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use memoire_db;
use serde::{Deserialize, Serialize};

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
    let db = state.db.lock().unwrap();

    let limit = params.limit.unwrap_or(50).min(100);
    let offset = params.offset.unwrap_or(0);

    let chunks = memoire_db::get_chunks_paginated(
        &db,
        limit,
        offset,
        params.monitor.as_deref(),
        None, // start_date
        None, // end_date
    )
    .map_err(|e| ApiError::Database(e.to_string()))?;

    let total = chunks.len() as i64;

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
    let db = state.db.lock().unwrap();

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

/// GET /api/chunks/:id/frames
pub async fn get_chunk_frames(
    State(_state): State<AppState>,
    Path(_id): Path<i64>,
    Query(_params): Query<ChunksQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // TODO: Implement get_frames_by_chunk
    Ok(Json(serde_json::json!({
        "frames": [],
        "total": 0,
    })))
}

/// GET /api/frames
pub async fn get_frames(
    State(_state): State<AppState>,
    Query(_params): Query<FramesQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // TODO: Implement time range query
    Ok(Json(serde_json::json!({
        "frames": [],
        "total": 0,
    })))
}

/// GET /api/frames/:id
pub async fn get_frame(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock().unwrap();

    let frame = memoire_db::get_frame(&db, id)?
        .ok_or_else(|| ApiError::NotFound(format!("frame {} not found", id)))?;

    let chunk = memoire_db::get_video_chunk(&db, frame.video_chunk_id)?
        .ok_or_else(|| ApiError::NotFound("chunk not found".to_string()))?;

    Ok(Json(serde_json::json!({
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
    })))
}

/// GET /api/stats
pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let db = state.db.lock().unwrap();

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
    let db = state.db.lock().unwrap();

    let monitors = memoire_db::get_monitors_summary(&db)
        .map_err(|e| ApiError::Database(e.to_string()))?;

    Ok(Json(serde_json::json!({
        "monitors": monitors,
    })))
}
