//! Database migrations

use anyhow::Result;
use rusqlite::Connection;
use tracing::info;

/// Current schema version
const SCHEMA_VERSION: i64 = 1;

/// Run all pending migrations
pub fn run_all(conn: &Connection) -> Result<()> {
    let current_version = get_schema_version(conn)?;

    if current_version < SCHEMA_VERSION {
        info!("running migrations from v{} to v{}", current_version, SCHEMA_VERSION);

        if current_version < 1 {
            migrate_v1(conn)?;
        }

        set_schema_version(conn, SCHEMA_VERSION)?;
    }

    Ok(())
}

fn get_schema_version(conn: &Connection) -> Result<i64> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    Ok(version)
}

fn set_schema_version(conn: &Connection, version: i64) -> Result<()> {
    conn.pragma_update(None, "user_version", version)?;
    Ok(())
}

/// Initial schema (v1)
fn migrate_v1(conn: &Connection) -> Result<()> {
    info!("applying migration v1: initial schema");

    conn.execute_batch(r#"
        -- Video chunks (5-minute MP4 segments)
        CREATE TABLE IF NOT EXISTS video_chunks (
            id INTEGER PRIMARY KEY,
            file_path TEXT NOT NULL,
            device_name TEXT NOT NULL,
            created_at TEXT DEFAULT (datetime('now'))
        );

        -- Frames with metadata
        CREATE TABLE IF NOT EXISTS frames (
            id INTEGER PRIMARY KEY,
            video_chunk_id INTEGER NOT NULL,
            offset_index INTEGER NOT NULL,
            timestamp TEXT NOT NULL,
            app_name TEXT,
            window_name TEXT,
            browser_url TEXT,
            focused INTEGER DEFAULT 0,
            FOREIGN KEY (video_chunk_id) REFERENCES video_chunks(id)
        );

        -- OCR extracted text
        CREATE TABLE IF NOT EXISTS ocr_text (
            id INTEGER PRIMARY KEY,
            frame_id INTEGER NOT NULL,
            text TEXT NOT NULL,
            text_json TEXT,
            confidence REAL,
            FOREIGN KEY (frame_id) REFERENCES frames(id)
        );

        -- Audio chunks (30-second segments)
        CREATE TABLE IF NOT EXISTS audio_chunks (
            id INTEGER PRIMARY KEY,
            file_path TEXT NOT NULL,
            device_name TEXT,
            is_input_device INTEGER,
            timestamp TEXT DEFAULT (datetime('now'))
        );

        -- Audio transcriptions
        CREATE TABLE IF NOT EXISTS audio_transcriptions (
            id INTEGER PRIMARY KEY,
            audio_chunk_id INTEGER NOT NULL,
            transcription TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            speaker_id INTEGER,
            start_time REAL,
            end_time REAL,
            FOREIGN KEY (audio_chunk_id) REFERENCES audio_chunks(id)
        );

        -- FTS5 full-text search tables
        CREATE VIRTUAL TABLE IF NOT EXISTS ocr_text_fts USING fts5(
            text,
            content='ocr_text',
            content_rowid='id'
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS audio_fts USING fts5(
            transcription,
            content='audio_transcriptions',
            content_rowid='id'
        );

        -- Indexes for common queries
        CREATE INDEX IF NOT EXISTS idx_frames_timestamp ON frames(timestamp);
        CREATE INDEX IF NOT EXISTS idx_frames_video_chunk ON frames(video_chunk_id);
        CREATE INDEX IF NOT EXISTS idx_ocr_frame ON ocr_text(frame_id);
        CREATE INDEX IF NOT EXISTS idx_audio_timestamp ON audio_transcriptions(timestamp);
        CREATE INDEX IF NOT EXISTS idx_audio_chunk ON audio_transcriptions(audio_chunk_id);

        -- Triggers to sync FTS tables
        CREATE TRIGGER IF NOT EXISTS ocr_text_ai AFTER INSERT ON ocr_text BEGIN
            INSERT INTO ocr_text_fts(rowid, text) VALUES (new.id, new.text);
        END;

        CREATE TRIGGER IF NOT EXISTS ocr_text_ad AFTER DELETE ON ocr_text BEGIN
            INSERT INTO ocr_text_fts(ocr_text_fts, rowid, text) VALUES('delete', old.id, old.text);
        END;

        CREATE TRIGGER IF NOT EXISTS ocr_text_au AFTER UPDATE ON ocr_text BEGIN
            INSERT INTO ocr_text_fts(ocr_text_fts, rowid, text) VALUES('delete', old.id, old.text);
            INSERT INTO ocr_text_fts(rowid, text) VALUES (new.id, new.text);
        END;

        CREATE TRIGGER IF NOT EXISTS audio_fts_ai AFTER INSERT ON audio_transcriptions BEGIN
            INSERT INTO audio_fts(rowid, transcription) VALUES (new.id, new.transcription);
        END;

        CREATE TRIGGER IF NOT EXISTS audio_fts_ad AFTER DELETE ON audio_transcriptions BEGIN
            INSERT INTO audio_fts(audio_fts, rowid, transcription) VALUES('delete', old.id, old.transcription);
        END;

        CREATE TRIGGER IF NOT EXISTS audio_fts_au AFTER UPDATE ON audio_transcriptions BEGIN
            INSERT INTO audio_fts(audio_fts, rowid, transcription) VALUES('delete', old.id, old.transcription);
            INSERT INTO audio_fts(rowid, transcription) VALUES (new.id, new.transcription);
        END;
    "#)?;

    Ok(())
}
