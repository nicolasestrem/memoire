# Database API

## Overview

The `memoire-db` crate provides SQLite database access with FTS5 full-text search support. It handles storage of video chunks, frame metadata, OCR text, and audio transcriptions.

## Schema

### Tables

```sql
-- Video chunks (5-minute MP4 segments)
CREATE TABLE video_chunks (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL,
    device_name TEXT NOT NULL,
    created_at TEXT DEFAULT (datetime('now'))
);

-- Frame metadata
CREATE TABLE frames (
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
CREATE TABLE ocr_text (
    id INTEGER PRIMARY KEY,
    frame_id INTEGER NOT NULL,
    text TEXT NOT NULL,
    text_json TEXT,     -- Bounding boxes as JSON
    confidence REAL,
    FOREIGN KEY (frame_id) REFERENCES frames(id)
);

-- Audio chunks (30-second segments)
CREATE TABLE audio_chunks (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL,
    device_name TEXT,
    is_input_device INTEGER,
    timestamp TEXT DEFAULT (datetime('now'))
);

-- Audio transcriptions
CREATE TABLE audio_transcriptions (
    id INTEGER PRIMARY KEY,
    audio_chunk_id INTEGER NOT NULL,
    transcription TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    speaker_id INTEGER,
    start_time REAL,
    end_time REAL,
    FOREIGN KEY (audio_chunk_id) REFERENCES audio_chunks(id)
);
```

### FTS5 Full-Text Search

```sql
-- OCR text search index
CREATE VIRTUAL TABLE ocr_text_fts USING fts5(
    text,
    content='ocr_text',
    content_rowid='id'
);

-- Audio transcription search index
CREATE VIRTUAL TABLE audio_fts USING fts5(
    transcription,
    content='audio_transcriptions',
    content_rowid='id'
);
```

### Indexes

```sql
CREATE INDEX idx_frames_timestamp ON frames(timestamp);
CREATE INDEX idx_frames_video_chunk ON frames(video_chunk_id);
CREATE INDEX idx_ocr_frame ON ocr_text(frame_id);
CREATE INDEX idx_audio_timestamp ON audio_transcriptions(timestamp);
CREATE INDEX idx_audio_chunk ON audio_transcriptions(audio_chunk_id);
```

## Rust Types

### Schema Types

```rust
// Query results
pub struct VideoChunk {
    pub id: i64,
    pub file_path: String,
    pub device_name: String,
    pub created_at: DateTime<Utc>,
}

pub struct Frame {
    pub id: i64,
    pub video_chunk_id: i64,
    pub offset_index: i64,
    pub timestamp: DateTime<Utc>,
    pub app_name: Option<String>,
    pub window_name: Option<String>,
    pub browser_url: Option<String>,
    pub focused: bool,
}

pub struct OcrText {
    pub id: i64,
    pub frame_id: i64,
    pub text: String,
    pub text_json: Option<String>,
    pub confidence: Option<f64>,
}

// Insert types
pub struct NewVideoChunk {
    pub file_path: String,
    pub device_name: String,
}

pub struct NewFrame {
    pub video_chunk_id: i64,
    pub offset_index: i64,
    pub timestamp: DateTime<Utc>,
    pub app_name: Option<String>,
    pub window_name: Option<String>,
    pub browser_url: Option<String>,
    pub focused: bool,
}

pub struct NewOcrText {
    pub frame_id: i64,
    pub text: String,
    pub text_json: Option<String>,
    pub confidence: Option<f64>,
}
```

## Database Connection

```rust
use memoire_db::Database;

// Open database (creates if not exists, runs migrations)
let db = Database::open(&path)?;

// Access raw connection for queries
let conn = db.connection();
```

## Query Functions

### Video Chunks

```rust
use memoire_db::{insert_video_chunk, get_video_chunk, get_latest_video_chunk};

// Insert new chunk
let chunk = NewVideoChunk {
    file_path: "videos/DISPLAY1/2024-01-15/chunk_10-30-00_0.mp4".to_string(),
    device_name: "DISPLAY1".to_string(),
};
let chunk_id = insert_video_chunk(conn, &chunk)?;

// Get by ID
let chunk = get_video_chunk(conn, chunk_id)?;

// Get latest chunk
let latest = get_latest_video_chunk(conn)?;
```

### Frames

```rust
use memoire_db::{insert_frame, insert_frames_batch, get_frame, get_frames_in_range};
use chrono::{Utc, Duration};

// Insert single frame
let frame = NewFrame {
    video_chunk_id: chunk_id,
    offset_index: 0,
    timestamp: Utc::now(),
    app_name: Some("Chrome".to_string()),
    window_name: Some("GitHub".to_string()),
    browser_url: Some("https://github.com".to_string()),
    focused: true,
};
let frame_id = insert_frame(conn, &frame)?;

// Batch insert (RECOMMENDED for performance)
let frames: Vec<NewFrame> = vec![/* ... */];
let ids = insert_frames_batch(conn, &frames)?;

// Get by ID
let frame = get_frame(conn, frame_id)?;

// Get frames in time range
let start = Utc::now() - Duration::hours(1);
let end = Utc::now();
let frames = get_frames_in_range(conn, start, end, 100, 0)?;
```

### OCR Text

```rust
use memoire_db::{insert_ocr_text, search_ocr};

// Insert OCR result
let ocr = NewOcrText {
    frame_id,
    text: "Hello World".to_string(),
    text_json: Some(r#"[{"text":"Hello","bbox":[0,0,50,20]}]"#.to_string()),
    confidence: Some(0.95),
};
let ocr_id = insert_ocr_text(conn, &ocr)?;

// Full-text search
let results = search_ocr(conn, "Hello", 20, 0)?;
for (ocr, frame) in results {
    println!("{}: {} at {}", frame.id, ocr.text, frame.timestamp);
}
```

### Statistics

```rust
use memoire_db::get_frame_count;

let total_frames = get_frame_count(conn)?;
```

## Batch Insert Performance

The `insert_frames_batch` function uses a single transaction for all frames:

```rust
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
            stmt.execute(params![/* ... */])?;
            ids.push(tx.last_insert_rowid());
        }
    }

    tx.commit()?;
    Ok(ids)
}
```

**Performance improvement:** ~30x fewer transactions compared to individual inserts.

## FTS5 Search Queries

### OCR Text Search

```rust
let results = search_ocr(conn, "search query", limit, offset)?;
```

Internally uses:
```sql
SELECT o.*, f.*
FROM ocr_text o
JOIN ocr_text_fts fts ON o.id = fts.rowid
JOIN frames f ON o.frame_id = f.id
WHERE ocr_text_fts MATCH ?1
ORDER BY rank
LIMIT ?2 OFFSET ?3
```

### FTS5 Match Syntax

```
"exact phrase"     -- Exact match
word1 word2        -- Both words (AND)
word1 OR word2     -- Either word
word*              -- Prefix match
"word1 word2"~10   -- Within 10 words
```

## Migrations

Schema versioning via SQLite `user_version` pragma:

```rust
// In migrations.rs
const SCHEMA_VERSION: i64 = 1;

pub fn run_all(conn: &Connection) -> Result<()> {
    let current = get_schema_version(conn)?;

    if current < 1 {
        migrate_v1(conn)?;  // Initial schema
    }
    // Future: if current < 2 { migrate_v2(conn)?; }

    set_schema_version(conn, SCHEMA_VERSION)?;
    Ok(())
}
```

## Timestamp Handling

Timestamps are stored as RFC 3339 strings:

```rust
// Writing
frame.timestamp.to_rfc3339()  // "2024-01-15T10:30:00+00:00"

// Reading (with fallback)
DateTime::parse_from_rfc3339(&s)
    .or_else(|_| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S"))
```

## Error Handling

```rust
use anyhow::Result;

// All query functions return Result<T>
match get_frame(conn, id) {
    Ok(Some(frame)) => { /* found */ }
    Ok(None) => { /* not found */ }
    Err(e) => { /* database error */ }
}
```
