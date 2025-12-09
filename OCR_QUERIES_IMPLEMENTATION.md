# OCR Query Functions Implementation

## Summary

Added four new OCR-related database query functions to the memoire-db crate for efficient retrieval and processing of OCR data.

## Files Modified

### 1. `src/memoire-db/src/schema.rs`

Added new struct:

```rust
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
    pub ocr_text: Option<OcrText>,
}
```

**Key Design**: The `ocr_text` field is `Option<OcrText>` to handle frames without OCR gracefully.

### 2. `src/memoire-db/src/queries.rs`

Added four new public query functions:

#### `get_frames_without_ocr(conn, limit)`

**Purpose**: Find frames that haven't been processed with OCR yet (for batch processing)

**Query**: Uses LEFT JOIN to find frames where `ocr_text.id IS NULL`

**Returns**: `Vec<Frame>` ordered by timestamp ASC

**Use Case**: OCR batch processor can call this to get next batch of unprocessed frames

```rust
pub fn get_frames_without_ocr(conn: &Connection, limit: i64) -> Result<Vec<Frame>>
```

#### `get_ocr_count(conn)`

**Purpose**: Get count of frames that have been processed with OCR

**Query**: `COUNT(DISTINCT frame_id)` from ocr_text table

**Returns**: `i64` count

**Use Case**: Progress tracking, statistics, validation

```rust
pub fn get_ocr_count(conn: &Connection) -> Result<i64>
```

#### `get_frame_with_ocr(conn, frame_id)`

**Purpose**: Get a single frame with its OCR text (if available)

**Query**: LEFT JOIN between frames and ocr_text

**Returns**: `Option<FrameWithOcr>` where `ocr_text` field is None if no OCR exists

**Use Case**: Display frame details with OCR in viewer/API

```rust
pub fn get_frame_with_ocr(conn: &Connection, frame_id: i64) -> Result<Option<FrameWithOcr>>
```

#### `get_frames_with_ocr_in_range(conn, start, end, limit, offset)`

**Purpose**: Get frames in time range with OCR data (paginated)

**Query**: LEFT JOIN with time range filter and pagination

**Returns**: `Vec<FrameWithOcr>` ordered by timestamp DESC

**Use Case**: Timeline view, API queries, search results

```rust
pub fn get_frames_with_ocr_in_range(
    conn: &Connection,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    limit: i64,
    offset: i64,
) -> Result<Vec<FrameWithOcr>>
```

## Implementation Details

### Performance Optimizations

1. **Prepared Statements**: All functions use `conn.prepare()` for efficient query execution
2. **Indexed Queries**: Leverage existing indexes on `frame_id` and `timestamp`
3. **LEFT JOIN Strategy**: Efficiently handles optional OCR data without multiple queries
4. **Limit/Offset**: Support pagination to avoid loading large result sets

### Error Handling

- All functions return `Result<T>` with `anyhow::Error`
- `QueryReturnedNoRows` is handled gracefully, returning `None` for optional results
- Uses existing `parse_datetime()` helper for timestamp conversion

### NULL Handling Pattern

The `get_frame_with_ocr` and `get_frames_with_ocr_in_range` functions use this pattern to handle optional OCR data:

```rust
let ocr_text = if let Ok(ocr_id) = row.get::<_, i64>(8) {
    Some(OcrText {
        id: ocr_id,
        frame_id: row.get(9)?,
        text: row.get(10)?,
        text_json: row.get(11)?,
        confidence: row.get(12)?,
    })
} else {
    None
};
```

This gracefully handles NULL values from LEFT JOIN without panicking.

## Usage Examples

### Find unprocessed frames for OCR batch job

```rust
let db = Database::open("memoire.db")?;
let conn = db.connection();

// Get next 100 frames without OCR
let frames = get_frames_without_ocr(conn, 100)?;

for frame in frames {
    // Process frame with OCR engine
    let ocr_result = run_ocr(&frame)?;

    // Save OCR result
    insert_ocr_text(conn, &NewOcrText {
        frame_id: frame.id,
        text: ocr_result.text,
        text_json: Some(ocr_result.bounding_boxes_json),
        confidence: Some(ocr_result.confidence),
    })?;
}
```

### Get progress statistics

```rust
let total_frames = get_frame_count(conn)?;
let processed_frames = get_ocr_count(conn)?;
let progress_pct = (processed_frames as f64 / total_frames as f64) * 100.0;

println!("OCR Progress: {}/{} ({:.1}%)",
    processed_frames, total_frames, progress_pct);
```

### Display frame with OCR in viewer

```rust
let frame_with_ocr = get_frame_with_ocr(conn, frame_id)?;

if let Some(frame) = frame_with_ocr {
    println!("Frame: {}", frame.id);
    println!("Timestamp: {}", frame.timestamp);

    if let Some(ocr) = frame.ocr_text {
        println!("OCR Text: {}", ocr.text);
        println!("Confidence: {:?}", ocr.confidence);
    } else {
        println!("No OCR data available");
    }
}
```

### Query time range with OCR

```rust
use chrono::{Duration, Utc};

let end = Utc::now();
let start = end - Duration::hours(24); // Last 24 hours

// Get frames with OCR from last 24 hours (paginated)
let frames = get_frames_with_ocr_in_range(conn, start, end, 50, 0)?;

for frame in frames {
    println!("{}: {} - {}",
        frame.timestamp,
        frame.app_name.unwrap_or_default(),
        frame.ocr_text.map(|o| o.text).unwrap_or_default()
    );
}
```

## Testing

The implementation follows the existing patterns in queries.rs and uses:
- Standard rusqlite error handling
- Existing helper functions (`row_to_frame`, `parse_datetime`)
- Consistent parameter binding with `params![]` macro
- Standard Result<T> return types

Integration tests can be added to verify:
1. Frames without OCR are correctly identified
2. OCR count matches actual processed frames
3. LEFT JOIN correctly handles NULL values
4. Time range queries respect boundaries
5. Pagination works correctly

## Database Schema Reference

```sql
-- Frames table
CREATE TABLE frames (
    id INTEGER PRIMARY KEY,
    video_chunk_id INTEGER NOT NULL,
    offset_index INTEGER NOT NULL,
    timestamp TEXT NOT NULL,
    app_name TEXT,
    window_name TEXT,
    browser_url TEXT,
    focused INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (video_chunk_id) REFERENCES video_chunks(id)
);

-- OCR text table
CREATE TABLE ocr_text (
    id INTEGER PRIMARY KEY,
    frame_id INTEGER NOT NULL,
    text TEXT NOT NULL,
    text_json TEXT,
    confidence REAL,
    FOREIGN KEY (frame_id) REFERENCES frames(id)
);

-- FTS5 full-text search
CREATE VIRTUAL TABLE ocr_text_fts USING fts5(
    text,
    content=ocr_text,
    content_rowid=id
);
```

## Next Steps

1. Add unit tests for new query functions
2. Add integration tests with sample data
3. Consider adding batch insert for OCR text (similar to `insert_frames_batch`)
4. Add index on `ocr_text.frame_id` if not already present
5. Monitor query performance with EXPLAIN QUERY PLAN
