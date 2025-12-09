//! Database query functions

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Row};

use crate::schema::*;

/// Sanitize a user query for FTS5 search
/// - Trims whitespace
/// - Escapes special FTS5 characters by wrapping in quotes
/// - Returns error for empty queries
pub fn sanitize_fts5_query(query: &str) -> Result<String> {
    let trimmed = query.trim();

    if trimmed.is_empty() {
        anyhow::bail!("Search query cannot be empty");
    }

    // For simple word/phrase search, wrap in quotes to treat as literal
    // This avoids FTS5 syntax errors from special characters
    let sanitized = if trimmed.contains('"') {
        // If already has quotes, escape internal quotes and wrap
        format!("\"{}\"", trimmed.replace('"', "\"\""))
    } else {
        // Simple case: wrap in quotes for literal match
        format!("\"{}\"", trimmed)
    };

    Ok(sanitized)
}

/// Insert a new video chunk
pub fn insert_video_chunk(conn: &Connection, chunk: &NewVideoChunk) -> Result<i64> {
    conn.execute(
        "INSERT INTO video_chunks (file_path, device_name, width, height) VALUES (?1, ?2, ?3, ?4)",
        params![chunk.file_path, chunk.device_name, chunk.width, chunk.height],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Insert a new frame
pub fn insert_frame(conn: &Connection, frame: &NewFrame) -> Result<i64> {
    conn.execute(
        r#"INSERT INTO frames
           (video_chunk_id, offset_index, timestamp, app_name, window_name, browser_url, focused, frame_hash)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
        params![
            frame.video_chunk_id,
            frame.offset_index,
            frame.timestamp.to_rfc3339(),
            frame.app_name,
            frame.window_name,
            frame.browser_url,
            frame.focused as i32,
            frame.frame_hash,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Batch insert multiple frames in a single transaction for better performance
pub fn insert_frames_batch(conn: &Connection, frames: &[NewFrame]) -> Result<Vec<i64>> {
    if frames.is_empty() {
        return Ok(vec![]);
    }

    let tx = conn.unchecked_transaction()?;
    let mut ids = Vec::with_capacity(frames.len());

    {
        let mut stmt = tx.prepare_cached(
            r#"INSERT INTO frames
               (video_chunk_id, offset_index, timestamp, app_name, window_name, browser_url, focused, frame_hash)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"#,
        )?;

        for frame in frames {
            stmt.execute(params![
                frame.video_chunk_id,
                frame.offset_index,
                frame.timestamp.to_rfc3339(),
                frame.app_name,
                frame.window_name,
                frame.browser_url,
                frame.focused as i32,
                frame.frame_hash,
            ])?;
            ids.push(tx.last_insert_rowid());
        }
    }

    tx.commit()?;
    Ok(ids)
}

/// Insert OCR text for a frame
pub fn insert_ocr_text(conn: &Connection, ocr: &NewOcrText) -> Result<i64> {
    conn.execute(
        "INSERT INTO ocr_text (frame_id, text, text_json, confidence) VALUES (?1, ?2, ?3, ?4)",
        params![ocr.frame_id, ocr.text, ocr.text_json, ocr.confidence],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get video chunk by ID
pub fn get_video_chunk(conn: &Connection, id: i64) -> Result<Option<VideoChunk>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, device_name, created_at, width, height FROM video_chunks WHERE id = ?1",
    )?;

    let chunk = stmt.query_row(params![id], |row| {
        Ok(VideoChunk {
            id: row.get(0)?,
            file_path: row.get(1)?,
            device_name: row.get(2)?,
            created_at: parse_datetime(row, 3)?,
            width: row.get::<_, Option<i64>>(4)?.map(|v| v as u32),
            height: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
        })
    });

    match chunk {
        Ok(c) => Ok(Some(c)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get frame by ID
pub fn get_frame(conn: &Connection, id: i64) -> Result<Option<Frame>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, video_chunk_id, offset_index, timestamp, app_name,
           window_name, browser_url, focused, frame_hash
           FROM frames WHERE id = ?1"#,
    )?;

    let frame = stmt.query_row(params![id], row_to_frame);

    match frame {
        Ok(f) => Ok(Some(f)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get frames in time range
pub fn get_frames_in_range(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Frame>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, video_chunk_id, offset_index, timestamp, app_name,
           window_name, browser_url, focused, frame_hash
           FROM frames
           WHERE timestamp >= ?1 AND timestamp <= ?2
           ORDER BY timestamp DESC
           LIMIT ?3 OFFSET ?4"#,
    )?;

    let frames = stmt
        .query_map(
            params![start.to_rfc3339(), end.to_rfc3339(), limit, offset],
            row_to_frame,
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(frames)
}

/// Full-text search on OCR text
pub fn search_ocr(
    conn: &Connection,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<(OcrText, Frame)>> {
    let mut stmt = conn.prepare(
        r#"SELECT o.id, o.frame_id, o.text, o.text_json, o.confidence,
           f.id, f.video_chunk_id, f.offset_index, f.timestamp, f.app_name,
           f.window_name, f.browser_url, f.focused, f.frame_hash
           FROM ocr_text o
           JOIN ocr_text_fts fts ON o.id = fts.rowid
           JOIN frames f ON o.frame_id = f.id
           WHERE ocr_text_fts MATCH ?1
           ORDER BY rank
           LIMIT ?2 OFFSET ?3"#,
    )?;

    let results = stmt
        .query_map(params![query, limit, offset], |row| {
            let ocr = OcrText {
                id: row.get(0)?,
                frame_id: row.get(1)?,
                text: row.get(2)?,
                text_json: row.get(3)?,
                confidence: row.get(4)?,
            };
            let frame = Frame {
                id: row.get(5)?,
                video_chunk_id: row.get(6)?,
                offset_index: row.get(7)?,
                timestamp: parse_datetime(row, 8)?,
                app_name: row.get(9)?,
                window_name: row.get(10)?,
                browser_url: row.get(11)?,
                focused: row.get::<_, i32>(12)? != 0,
                frame_hash: row.get(13)?,
            };
            Ok((ocr, frame))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

/// Get frames without OCR text (for batch processing)
pub fn get_frames_without_ocr(conn: &Connection, limit: i64) -> Result<Vec<Frame>> {
    let mut stmt = conn.prepare(
        r#"SELECT f.id, f.video_chunk_id, f.offset_index, f.timestamp, f.app_name,
           f.window_name, f.browser_url, f.focused, f.frame_hash
           FROM frames f
           LEFT JOIN ocr_text o ON f.id = o.frame_id
           WHERE o.id IS NULL
           ORDER BY f.timestamp ASC
           LIMIT ?1"#,
    )?;

    let frames = stmt
        .query_map(params![limit], row_to_frame)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(frames)
}

/// Get count of frames that have OCR text
pub fn get_ocr_count(conn: &Connection) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT frame_id) FROM ocr_text",
        [],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Get frame with OCR text (if available) using LEFT JOIN
pub fn get_frame_with_ocr(conn: &Connection, frame_id: i64) -> Result<Option<FrameWithOcr>> {
    let mut stmt = conn.prepare(
        r#"SELECT f.id, f.video_chunk_id, f.offset_index, f.timestamp, f.app_name,
           f.window_name, f.browser_url, f.focused, f.frame_hash,
           o.id, o.frame_id, o.text, o.text_json, o.confidence
           FROM frames f
           LEFT JOIN ocr_text o ON f.id = o.frame_id
           WHERE f.id = ?1"#,
    )?;

    let result = stmt.query_row(params![frame_id], |row| {
        let ocr_text = if let Ok(ocr_id) = row.get::<_, i64>(9) {
            Some(OcrText {
                id: ocr_id,
                frame_id: row.get(10)?,
                text: row.get(11)?,
                text_json: row.get(12)?,
                confidence: row.get(13)?,
            })
        } else {
            None
        };

        Ok(FrameWithOcr {
            id: row.get(0)?,
            video_chunk_id: row.get(1)?,
            offset_index: row.get(2)?,
            timestamp: parse_datetime(row, 3)?,
            app_name: row.get(4)?,
            window_name: row.get(5)?,
            browser_url: row.get(6)?,
            focused: row.get::<_, i32>(7)? != 0,
            frame_hash: row.get(8)?,
            ocr_text,
        })
    });

    match result {
        Ok(frame) => Ok(Some(frame)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get frames with OCR text in time range
pub fn get_frames_with_ocr_in_range(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    limit: i64,
    offset: i64,
) -> Result<Vec<FrameWithOcr>> {
    let mut stmt = conn.prepare(
        r#"SELECT f.id, f.video_chunk_id, f.offset_index, f.timestamp, f.app_name,
           f.window_name, f.browser_url, f.focused, f.frame_hash,
           o.id, o.frame_id, o.text, o.text_json, o.confidence
           FROM frames f
           LEFT JOIN ocr_text o ON f.id = o.frame_id
           WHERE f.timestamp >= ?1 AND f.timestamp <= ?2
           ORDER BY f.timestamp DESC
           LIMIT ?3 OFFSET ?4"#,
    )?;

    let frames = stmt
        .query_map(
            params![start.to_rfc3339(), end.to_rfc3339(), limit, offset],
            |row| {
                let ocr_text = if let Ok(ocr_id) = row.get::<_, i64>(9) {
                    Some(OcrText {
                        id: ocr_id,
                        frame_id: row.get(10)?,
                        text: row.get(11)?,
                        text_json: row.get(12)?,
                        confidence: row.get(13)?,
                    })
                } else {
                    None
                };

                Ok(FrameWithOcr {
                    id: row.get(0)?,
                    video_chunk_id: row.get(1)?,
                    offset_index: row.get(2)?,
                    timestamp: parse_datetime(row, 3)?,
                    app_name: row.get(4)?,
                    window_name: row.get(5)?,
                    browser_url: row.get(6)?,
                    focused: row.get::<_, i32>(7)? != 0,
                    frame_hash: row.get(8)?,
                    ocr_text,
                })
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(frames)
}

/// Get total frame count
pub fn get_frame_count(conn: &Connection) -> Result<i64> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM frames", [], |row| row.get(0))?;
    Ok(count)
}

/// Get latest video chunk
pub fn get_latest_video_chunk(conn: &Connection) -> Result<Option<VideoChunk>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, device_name, created_at, width, height FROM video_chunks ORDER BY id DESC LIMIT 1",
    )?;

    let chunk = stmt.query_row([], |row| {
        Ok(VideoChunk {
            id: row.get(0)?,
            file_path: row.get(1)?,
            device_name: row.get(2)?,
            created_at: parse_datetime(row, 3)?,
            width: row.get::<_, Option<i64>>(4)?.map(|v| v as u32),
            height: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
        })
    });

    match chunk {
        Ok(c) => Ok(Some(c)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get paginated video chunks with optional filters
pub fn get_chunks_paginated(
    conn: &Connection,
    limit: i64,
    offset: i64,
    monitor: Option<&str>,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
) -> Result<Vec<ChunkWithFrameCount>> {
    let mut query = String::from(
        r#"SELECT vc.id, vc.file_path, vc.device_name, vc.created_at,
           COUNT(f.id) as frame_count, vc.width, vc.height
           FROM video_chunks vc
           LEFT JOIN frames f ON vc.id = f.video_chunk_id"#,
    );

    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(mon) = monitor {
        conditions.push("vc.device_name = ?");
        params.push(Box::new(mon.to_string()));
    }

    if let Some(start) = start_date {
        conditions.push("vc.created_at >= ?");
        params.push(Box::new(start.to_rfc3339()));
    }

    if let Some(end) = end_date {
        conditions.push("vc.created_at <= ?");
        params.push(Box::new(end.to_rfc3339()));
    }

    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }

    query.push_str(" GROUP BY vc.id ORDER BY vc.created_at DESC LIMIT ? OFFSET ?");

    let mut stmt = conn.prepare(&query)?;

    // Build params slice for query
    let mut all_params: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    all_params.push(&limit);
    all_params.push(&offset);

    let chunks = stmt
        .query_map(all_params.as_slice(), |row| {
            Ok(ChunkWithFrameCount {
                id: row.get(0)?,
                file_path: row.get(1)?,
                device_name: row.get(2)?,
                created_at: parse_datetime(row, 3)?,
                frame_count: row.get(4)?,
                width: row.get::<_, Option<i64>>(5)?.map(|v| v as u32),
                height: row.get::<_, Option<i64>>(6)?.map(|v| v as u32),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(chunks)
}

/// Get frame count for a specific video chunk
pub fn get_frame_count_by_chunk(conn: &Connection, chunk_id: i64) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM frames WHERE video_chunk_id = ?1",
        params![chunk_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Get total chunk count with optional filters
pub fn get_total_chunk_count(
    conn: &Connection,
    monitor: Option<&str>,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
) -> Result<i64> {
    let mut query = String::from("SELECT COUNT(*) FROM video_chunks");

    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(mon) = monitor {
        conditions.push("device_name = ?");
        params.push(Box::new(mon.to_string()));
    }

    if let Some(start) = start_date {
        conditions.push("created_at >= ?");
        params.push(Box::new(start.to_rfc3339()));
    }

    if let Some(end) = end_date {
        conditions.push("created_at <= ?");
        params.push(Box::new(end.to_rfc3339()));
    }

    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }

    let mut stmt = conn.prepare(&query)?;
    let all_params: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let count: i64 = stmt.query_row(all_params.as_slice(), |row| row.get(0))?;
    Ok(count)
}

/// Get statistics summary for each monitor
pub fn get_monitors_summary(conn: &Connection) -> Result<Vec<MonitorSummary>> {
    let mut stmt = conn.prepare(
        r#"SELECT vc.device_name,
           COUNT(DISTINCT vc.id) as total_chunks,
           COUNT(f.id) as total_frames,
           MAX(vc.created_at) as latest_capture
           FROM video_chunks vc
           LEFT JOIN frames f ON vc.id = f.video_chunk_id
           GROUP BY vc.device_name
           ORDER BY vc.device_name"#,
    )?;

    let summaries = stmt
        .query_map([], |row| {
            let latest: Option<String> = row.get(3)?;
            Ok(MonitorSummary {
                device_name: row.get(0)?,
                total_chunks: row.get(1)?,
                total_frames: row.get(2)?,
                latest_capture: latest.and_then(|s| {
                    DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(summaries)
}

/// Get OCR text for a specific frame
pub fn get_ocr_text_by_frame(conn: &Connection, frame_id: i64) -> Result<Option<OcrText>> {
    let mut stmt = conn.prepare(
        "SELECT id, frame_id, text, text_json, confidence FROM ocr_text WHERE frame_id = ?1",
    )?;

    let ocr = stmt.query_row(params![frame_id], |row| {
        Ok(OcrText {
            id: row.get(0)?,
            frame_id: row.get(1)?,
            text: row.get(2)?,
            text_json: row.get(3)?,
            confidence: row.get(4)?,
        })
    });

    match ocr {
        Ok(o) => Ok(Some(o)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get OCR statistics
pub fn get_ocr_stats(conn: &Connection) -> Result<OcrStats> {
    let total_frames: i64 = conn.query_row("SELECT COUNT(*) FROM frames", [], |row| row.get(0))?;

    let frames_with_ocr: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT frame_id) FROM ocr_text",
        [],
        |row| row.get(0),
    )?;

    let pending_frames = total_frames - frames_with_ocr;

    // Calculate processing rate (frames processed in last hour)
    let processing_rate: i64 = conn.query_row(
        r#"SELECT COUNT(DISTINCT o.frame_id)
           FROM ocr_text o
           JOIN frames f ON o.frame_id = f.id
           WHERE f.timestamp >= datetime('now', '-1 hour')"#,
        [],
        |row| row.get(0),
    )?;

    // Get last updated timestamp
    let last_updated: Option<DateTime<Utc>> = {
        let result: Result<String, _> = conn.query_row(
            "SELECT MAX(f.timestamp) FROM frames f JOIN ocr_text o ON f.id = o.frame_id",
            [],
            |row| row.get(0),
        );

        result.ok().and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        })
    };

    Ok(OcrStats {
        total_frames,
        frames_with_ocr,
        pending_frames,
        processing_rate,
        last_updated,
    })
}

/// Get total count of search results
pub fn get_search_count(conn: &Connection, query: &str) -> Result<i64> {
    let count: i64 = conn.query_row(
        r#"SELECT COUNT(*)
           FROM ocr_text o
           JOIN ocr_text_fts fts ON o.id = fts.rowid
           WHERE ocr_text_fts MATCH ?1"#,
        params![query],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Get the last frame hash for a video chunk (for deduplication)
pub fn get_last_frame_hash(conn: &Connection, chunk_id: i64) -> Result<Option<i64>> {
    let result: rusqlite::Result<i64> = conn.query_row(
        r#"SELECT frame_hash FROM frames
           WHERE video_chunk_id = ?1 AND frame_hash IS NOT NULL
           ORDER BY offset_index DESC
           LIMIT 1"#,
        params![chunk_id],
        |row| row.get(0),
    );

    match result {
        Ok(hash) => Ok(Some(hash)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Count duplicate frames skipped (frames with same hash as previous)
pub fn get_skipped_frame_count(conn: &Connection) -> Result<i64> {
    // Count frames where the previous frame in the same chunk has the same hash
    let count: i64 = conn.query_row(
        r#"SELECT COUNT(*) FROM frames f1
           WHERE EXISTS (
               SELECT 1 FROM frames f2
               WHERE f2.video_chunk_id = f1.video_chunk_id
               AND f2.offset_index = f1.offset_index - 1
               AND f2.frame_hash = f1.frame_hash
               AND f1.frame_hash IS NOT NULL
           )"#,
        [],
        |row| row.get(0),
    )?;
    Ok(count)
}

// Helper functions

fn row_to_frame(row: &Row) -> rusqlite::Result<Frame> {
    Ok(Frame {
        id: row.get(0)?,
        video_chunk_id: row.get(1)?,
        offset_index: row.get(2)?,
        timestamp: parse_datetime(row, 3)?,
        app_name: row.get(4)?,
        window_name: row.get(5)?,
        browser_url: row.get(6)?,
        focused: row.get::<_, i32>(7)? != 0,
        frame_hash: row.get(8)?,
    })
}

fn parse_datetime(row: &Row, idx: usize) -> rusqlite::Result<DateTime<Utc>> {
    let s: String = row.get(idx)?;
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            // Try SQLite datetime format
            chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                .map(|dt| dt.and_utc())
        })
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(
            idx,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
}

// ============================================================================
// Audio-related query functions (Phase 3)
// ============================================================================

/// Insert a new audio chunk
pub fn insert_audio_chunk(conn: &Connection, chunk: &NewAudioChunk) -> Result<i64> {
    conn.execute(
        "INSERT INTO audio_chunks (file_path, device_name, is_input_device) VALUES (?1, ?2, ?3)",
        params![chunk.file_path, chunk.device_name, chunk.is_input_device.map(|b| b as i32)],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get audio chunk by ID
pub fn get_audio_chunk(conn: &Connection, id: i64) -> Result<Option<AudioChunk>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, device_name, is_input_device, timestamp FROM audio_chunks WHERE id = ?1",
    )?;

    let chunk = stmt.query_row(params![id], |row| {
        Ok(AudioChunk {
            id: row.get(0)?,
            file_path: row.get(1)?,
            device_name: row.get(2)?,
            is_input_device: row.get::<_, Option<i32>>(3)?.map(|v| v != 0),
            timestamp: parse_datetime(row, 4)?,
        })
    });

    match chunk {
        Ok(c) => Ok(Some(c)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get audio chunks without transcription (for batch processing)
pub fn get_audio_chunks_without_transcription(conn: &Connection, limit: i64) -> Result<Vec<AudioChunk>> {
    let mut stmt = conn.prepare(
        r#"SELECT ac.id, ac.file_path, ac.device_name, ac.is_input_device, ac.timestamp
           FROM audio_chunks ac
           LEFT JOIN audio_transcriptions at ON ac.id = at.audio_chunk_id
           WHERE at.id IS NULL
           ORDER BY ac.timestamp ASC
           LIMIT ?1"#,
    )?;

    let chunks = stmt
        .query_map(params![limit], |row| {
            Ok(AudioChunk {
                id: row.get(0)?,
                file_path: row.get(1)?,
                device_name: row.get(2)?,
                is_input_device: row.get::<_, Option<i32>>(3)?.map(|v| v != 0),
                timestamp: parse_datetime(row, 4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(chunks)
}

/// Get total audio chunk count
pub fn get_audio_chunk_count(conn: &Connection) -> Result<i64> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM audio_chunks", [], |row| row.get(0))?;
    Ok(count)
}

/// Insert audio transcription
pub fn insert_audio_transcription(conn: &Connection, transcription: &NewAudioTranscription) -> Result<i64> {
    conn.execute(
        r#"INSERT INTO audio_transcriptions
           (audio_chunk_id, transcription, timestamp, speaker_id, start_time, end_time)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6)"#,
        params![
            transcription.audio_chunk_id,
            transcription.transcription,
            transcription.timestamp.to_rfc3339(),
            transcription.speaker_id,
            transcription.start_time,
            transcription.end_time,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Get transcription by audio chunk ID
pub fn get_transcription_by_chunk(conn: &Connection, chunk_id: i64) -> Result<Option<AudioTranscription>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, audio_chunk_id, transcription, timestamp, speaker_id, start_time, end_time
           FROM audio_transcriptions WHERE audio_chunk_id = ?1"#,
    )?;

    let transcription = stmt.query_row(params![chunk_id], |row| {
        Ok(AudioTranscription {
            id: row.get(0)?,
            audio_chunk_id: row.get(1)?,
            transcription: row.get(2)?,
            timestamp: parse_datetime(row, 3)?,
            speaker_id: row.get(4)?,
            start_time: row.get(5)?,
            end_time: row.get(6)?,
        })
    });

    match transcription {
        Ok(t) => Ok(Some(t)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Get all transcriptions for an audio chunk (ordered by start_time)
pub fn get_transcriptions_by_chunk(conn: &Connection, chunk_id: i64) -> Result<Vec<AudioTranscription>> {
    let mut stmt = conn.prepare(
        r#"SELECT id, audio_chunk_id, transcription, timestamp, speaker_id, start_time, end_time
           FROM audio_transcriptions
           WHERE audio_chunk_id = ?1
           ORDER BY start_time ASC NULLS LAST"#,
    )?;

    let transcriptions = stmt
        .query_map(params![chunk_id], |row| {
            Ok(AudioTranscription {
                id: row.get(0)?,
                audio_chunk_id: row.get(1)?,
                transcription: row.get(2)?,
                timestamp: parse_datetime(row, 3)?,
                speaker_id: row.get(4)?,
                start_time: row.get(5)?,
                end_time: row.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    Ok(transcriptions)
}

/// Get total count of audio chunks
pub fn get_total_audio_chunk_count(conn: &Connection, device: Option<&str>) -> Result<i64> {
    let count: i64 = if let Some(dev) = device {
        conn.query_row(
            "SELECT COUNT(*) FROM audio_chunks WHERE device_name = ?1",
            params![dev],
            |row| row.get(0),
        )?
    } else {
        conn.query_row("SELECT COUNT(*) FROM audio_chunks", [], |row| row.get(0))?
    };
    Ok(count)
}

/// Get count of chunks with transcription
pub fn get_transcription_count(conn: &Connection) -> Result<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT audio_chunk_id) FROM audio_transcriptions",
        [],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Full-text search on audio transcriptions
pub fn search_transcriptions(
    conn: &Connection,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<(AudioTranscription, AudioChunk)>> {
    let mut stmt = conn.prepare(
        r#"SELECT at.id, at.audio_chunk_id, at.transcription, at.timestamp,
           at.speaker_id, at.start_time, at.end_time,
           ac.id, ac.file_path, ac.device_name, ac.is_input_device, ac.timestamp
           FROM audio_transcriptions at
           JOIN audio_fts fts ON at.id = fts.rowid
           JOIN audio_chunks ac ON at.audio_chunk_id = ac.id
           WHERE audio_fts MATCH ?1
           ORDER BY rank
           LIMIT ?2 OFFSET ?3"#,
    )?;

    let results = stmt
        .query_map(params![query, limit, offset], |row| {
            let transcription = AudioTranscription {
                id: row.get(0)?,
                audio_chunk_id: row.get(1)?,
                transcription: row.get(2)?,
                timestamp: parse_datetime(row, 3)?,
                speaker_id: row.get(4)?,
                start_time: row.get(5)?,
                end_time: row.get(6)?,
            };
            let chunk = AudioChunk {
                id: row.get(7)?,
                file_path: row.get(8)?,
                device_name: row.get(9)?,
                is_input_device: row.get::<_, Option<i32>>(10)?.map(|v| v != 0),
                timestamp: parse_datetime(row, 11)?,
            };
            Ok((transcription, chunk))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

/// Unified search across OCR and transcriptions
pub fn search_all(
    conn: &Connection,
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<SearchResult>> {
    let mut results = Vec::new();

    // Search OCR
    let ocr_results = search_ocr(conn, query, limit / 2, 0)?;
    for (ocr, frame) in ocr_results {
        results.push(SearchResult::Ocr { ocr, frame });
    }

    // Search transcriptions
    let audio_results = search_transcriptions(conn, query, limit / 2, 0)?;
    for (transcription, chunk) in audio_results {
        results.push(SearchResult::Audio { transcription, chunk });
    }

    // Sort by timestamp (newest first) and apply pagination
    // Note: This is a simple implementation. For production, use UNION in SQL
    results.sort_by(|a, b| {
        let ts_a = match a {
            SearchResult::Ocr { frame, .. } => frame.timestamp,
            SearchResult::Audio { transcription, .. } => transcription.timestamp,
        };
        let ts_b = match b {
            SearchResult::Ocr { frame, .. } => frame.timestamp,
            SearchResult::Audio { transcription, .. } => transcription.timestamp,
        };
        ts_b.cmp(&ts_a)
    });

    // Apply offset and limit
    let start = offset as usize;
    let end = (offset + limit) as usize;
    let paginated: Vec<_> = results.into_iter().skip(start).take(end - start).collect();

    Ok(paginated)
}

/// Get audio indexing statistics
pub fn get_audio_stats(conn: &Connection) -> Result<AudioStats> {
    let total_chunks: i64 = conn.query_row("SELECT COUNT(*) FROM audio_chunks", [], |row| row.get(0))?;

    let chunks_with_transcription: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT audio_chunk_id) FROM audio_transcriptions",
        [],
        |row| row.get(0),
    )?;

    let pending_chunks = total_chunks - chunks_with_transcription;

    // Calculate processing rate (chunks processed in last hour)
    let processing_rate: i64 = conn.query_row(
        r#"SELECT COUNT(DISTINCT at.audio_chunk_id)
           FROM audio_transcriptions at
           JOIN audio_chunks ac ON at.audio_chunk_id = ac.id
           WHERE ac.timestamp >= datetime('now', '-1 hour')"#,
        [],
        |row| row.get(0),
    )?;

    // Get last updated timestamp
    let last_updated: Option<DateTime<Utc>> = {
        let result: Result<String, _> = conn.query_row(
            "SELECT MAX(ac.timestamp) FROM audio_chunks ac JOIN audio_transcriptions at ON ac.id = at.audio_chunk_id",
            [],
            |row| row.get(0),
        );

        result.ok().and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .ok()
        })
    };

    Ok(AudioStats {
        total_chunks,
        chunks_with_transcription,
        pending_chunks,
        processing_rate,
        last_updated,
    })
}

/// Get paginated audio chunks with optional filters
pub fn get_audio_chunks_paginated(
    conn: &Connection,
    limit: i64,
    offset: i64,
    device: Option<&str>,
    is_input: Option<bool>,
) -> Result<Vec<AudioChunkWithTranscription>> {
    let mut query = String::from(
        r#"SELECT ac.id, ac.file_path, ac.device_name, ac.is_input_device, ac.timestamp,
           COUNT(at.id) as transcription_count
           FROM audio_chunks ac
           LEFT JOIN audio_transcriptions at ON ac.id = at.audio_chunk_id"#,
    );

    let mut conditions = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(dev) = device {
        conditions.push("ac.device_name = ?");
        params.push(Box::new(dev.to_string()));
    }

    if let Some(input) = is_input {
        conditions.push("ac.is_input_device = ?");
        params.push(Box::new(input as i32));
    }

    if !conditions.is_empty() {
        query.push_str(" WHERE ");
        query.push_str(&conditions.join(" AND "));
    }

    query.push_str(" GROUP BY ac.id ORDER BY ac.timestamp DESC LIMIT ? OFFSET ?");

    let mut stmt = conn.prepare(&query)?;

    let mut all_params: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    all_params.push(&limit);
    all_params.push(&offset);

    let chunks = stmt
        .query_map(all_params.as_slice(), |row| {
            Ok(AudioChunkWithTranscription {
                id: row.get(0)?,
                file_path: row.get(1)?,
                device_name: row.get(2)?,
                is_input_device: row.get::<_, Option<i32>>(3)?.map(|v| v != 0),
                timestamp: parse_datetime(row, 4)?,
                transcription_count: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(chunks)
}

/// Get total count of search results for audio
pub fn get_audio_search_count(conn: &Connection, query: &str) -> Result<i64> {
    let count: i64 = conn.query_row(
        r#"SELECT COUNT(*)
           FROM audio_transcriptions at
           JOIN audio_fts fts ON at.id = fts.rowid
           WHERE audio_fts MATCH ?1"#,
        params![query],
        |row| row.get(0),
    )?;
    Ok(count)
}

/// Delete all OCR records with empty text (for re-indexing after bug fixes)
pub fn reset_empty_ocr(conn: &Connection) -> Result<usize> {
    let deleted = conn.execute(
        "DELETE FROM ocr_text WHERE text = ''",
        [],
    )?;
    Ok(deleted)
}

/// Delete ALL OCR records (for complete re-indexing)
pub fn reset_all_ocr(conn: &Connection) -> Result<usize> {
    let deleted = conn.execute("DELETE FROM ocr_text", [])?;
    Ok(deleted)
}
