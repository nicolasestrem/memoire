//! Video streaming with HTTP Range requests

use crate::{ApiError, AppState};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use memoire_db;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

/// Maximum chunk size for range requests (10 MB)
const MAX_CHUNK_SIZE: usize = 10 * 1024 * 1024;

/// Parse Range header
fn parse_range_header(range: &str, file_size: u64) -> Option<(u64, u64)> {
    // Parse "bytes=start-end" format
    let range = range.strip_prefix("bytes=")?;

    if let Some((start, end)) = range.split_once('-') {
        let start: u64 = start.parse().ok()?;
        let end: u64 = if end.is_empty() {
            file_size - 1
        } else {
            end.parse::<u64>().ok()?.min(file_size - 1)
        };

        if start <= end && end < file_size {
            Some((start, end))
        } else {
            None
        }
    } else {
        None
    }
}

/// GET /video/:id - Stream video file with range request support
pub async fn stream_video(
    State(state): State<AppState>,
    Path(chunk_id): Path<i64>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // Get chunk from database
    let chunk = {
        let db = state.db.lock()
            .map_err(|_| ApiError::Internal(anyhow::anyhow!("database lock poisoned")))?;
        memoire_db::get_video_chunk(&db, chunk_id)?
            .ok_or_else(|| ApiError::NotFound(format!("chunk {} not found", chunk_id)))?
    };

    // Resolve file path (prevent path traversal)
    let file_path = state.data_dir.join(&chunk.file_path);

    // Security: Ensure file path is within data_dir
    if !file_path.starts_with(&state.data_dir) {
        return Err(ApiError::Forbidden("path traversal detected".to_string()));
    }

    // Check if file exists
    if !file_path.exists() {
        return Err(ApiError::NotFound(format!("video file not found: {}", chunk.file_path)));
    }

    // Get file metadata
    let metadata = tokio::fs::metadata(&file_path).await?;
    let file_size = metadata.len();

    // Check for Range header
    if let Some(range_header) = headers.get(header::RANGE) {
        let range_str = range_header.to_str().unwrap_or("");

        if let Some((start, end)) = parse_range_header(range_str, file_size) {
            let chunk_size = (end - start + 1) as usize;

            // Validate chunk size
            if chunk_size > MAX_CHUNK_SIZE {
                return Err(ApiError::BadRequest(format!(
                    "range too large: {} bytes (max: {} bytes)",
                    chunk_size, MAX_CHUNK_SIZE
                )));
            }

            // Use spawn_blocking for sync file I/O to avoid blocking tokio runtime
            let file_path_clone = file_path.clone();
            let buffer = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
                let mut file = File::open(&file_path_clone)?;
                file.seek(SeekFrom::Start(start))?;

                let mut buffer = vec![0u8; chunk_size];
                file.read_exact(&mut buffer)?;
                Ok(buffer)
            })
            .await
            .map_err(|e| ApiError::Internal(anyhow::anyhow!("task join error: {}", e)))??;

            let content_range = format!("bytes {}-{}/{}", start, end, file_size);

            return Ok((
                StatusCode::PARTIAL_CONTENT,
                [
                    (header::CONTENT_TYPE, "video/mp4"),
                    (header::CONTENT_LENGTH, &chunk_size.to_string()),
                    (header::CONTENT_RANGE, &content_range),
                    (header::ACCEPT_RANGES, "bytes"),
                ],
                buffer,
            ).into_response());
        } else {
            // Invalid range
            return Err(ApiError::RangeNotSatisfiable);
        }
    }

    // No range header - serve entire file
    let file = tokio::fs::File::open(&file_path).await?;
    let stream = tokio_util::io::ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "video/mp4"),
            (header::CONTENT_LENGTH, &file_size.to_string()),
            (header::ACCEPT_RANGES, "bytes"),
        ],
        body,
    ).into_response())
}
