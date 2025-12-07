//! Database query functions

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Row};

use crate::schema::*;

/// Insert a new video chunk
pub fn insert_video_chunk(conn: &Connection, chunk: &NewVideoChunk) -> Result<i64> {
    conn.execute(
        "INSERT INTO video_chunks (file_path, device_name) VALUES (?1, ?2)",
        params![chunk.file_path, chunk.device_name],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Insert a new frame
pub fn insert_frame(conn: &Connection, frame: &NewFrame) -> Result<i64> {
    conn.execute(
        r#"INSERT INTO frames
           (video_chunk_id, offset_index, timestamp, app_name, window_name, browser_url, focused)
           VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
        params![
            frame.video_chunk_id,
            frame.offset_index,
            frame.timestamp.to_rfc3339(),
            frame.app_name,
            frame.window_name,
            frame.browser_url,
            frame.focused as i32,
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
               (video_chunk_id, offset_index, timestamp, app_name, window_name, browser_url, focused)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"#,
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
        "SELECT id, file_path, device_name, created_at FROM video_chunks WHERE id = ?1",
    )?;

    let chunk = stmt.query_row(params![id], |row| {
        Ok(VideoChunk {
            id: row.get(0)?,
            file_path: row.get(1)?,
            device_name: row.get(2)?,
            created_at: parse_datetime(row, 3)?,
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
           window_name, browser_url, focused
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
           window_name, browser_url, focused
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
           f.window_name, f.browser_url, f.focused
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
            };
            Ok((ocr, frame))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(results)
}

/// Get total frame count
pub fn get_frame_count(conn: &Connection) -> Result<i64> {
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM frames", [], |row| row.get(0))?;
    Ok(count)
}

/// Get latest video chunk
pub fn get_latest_video_chunk(conn: &Connection) -> Result<Option<VideoChunk>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path, device_name, created_at FROM video_chunks ORDER BY id DESC LIMIT 1",
    )?;

    let chunk = stmt.query_row([], |row| {
        Ok(VideoChunk {
            id: row.get(0)?,
            file_path: row.get(1)?,
            device_name: row.get(2)?,
            created_at: parse_datetime(row, 3)?,
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
           COUNT(f.id) as frame_count
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
