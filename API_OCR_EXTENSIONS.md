# API OCR Extensions

This document describes the REST API extensions added to support OCR functionality in memoire-web.

## Summary of Changes

### 1. Database Layer (`src/memoire-db/`)

#### New Schema Types (`src/memoire-db/src/schema.rs`)
- **OcrStats**: Statistics structure for OCR indexing progress
  - `total_frames`: Total number of frames in database
  - `frames_with_ocr`: Number of frames with OCR data
  - `pending_frames`: Number of frames awaiting OCR processing
  - `processing_rate`: Frames processed in the last hour
  - `last_updated`: Timestamp of most recent OCR data

#### New Query Functions (`src/memoire-db/src/queries.rs`)
- **get_ocr_text_by_frame(conn, frame_id)**: Retrieves OCR text for a specific frame
- **get_ocr_stats(conn)**: Calculates OCR indexing statistics
- **get_search_count(conn, query)**: Returns total count of FTS5 search results

### 2. Web API Layer (`src/memoire-web/`)

#### Updated Endpoints

##### GET /api/frames/:id
**Changes**: Now includes optional OCR data in response

**Response Format**:
```json
{
  "id": 123,
  "video_chunk_id": 45,
  "offset_index": 10,
  "timestamp": "2025-01-15T10:30:00Z",
  "app_name": "Chrome",
  "window_name": "Example Page",
  "browser_url": "https://example.com",
  "focused": true,
  "chunk": {
    "file_path": "/path/to/chunk.mp4",
    "device_name": "monitor-0"
  },
  "ocr": {  // Optional field - only present if OCR data exists
    "text": "extracted text content",
    "text_json": "{...}",  // Optional: bounding boxes as JSON
    "confidence": 0.95     // Optional: OCR confidence score
  }
}
```

#### New Endpoints

##### GET /api/search
**Purpose**: Full-text search across OCR data using FTS5

**Query Parameters**:
- `q` (required): Search query string (FTS5 syntax)
- `limit` (optional): Max results per page (default: 50, max: 100)
- `offset` (optional): Pagination offset (default: 0)

**Response Format**:
```json
{
  "results": [
    {
      "frame": {
        "id": 123,
        "timestamp": "2025-01-15T10:30:00Z",
        "app_name": "Chrome",
        "window_name": "Example Page",
        "browser_url": "https://example.com"
      },
      "ocr": {
        "text": "matched text content",
        "confidence": 0.95
      }
    }
  ],
  "total": 150,
  "has_more": true,
  "limit": 50,
  "offset": 0
}
```

**Search Query Examples**:
- Simple search: `q=hello`
- Phrase search: `q="hello world"`
- Boolean operators: `q=hello AND world`
- See [SQLite FTS5 documentation](https://www.sqlite.org/fts5.html) for full syntax

##### GET /api/stats/ocr
**Purpose**: Returns OCR indexing progress and statistics

**Response Format**:
```json
{
  "total_frames": 10000,
  "frames_with_ocr": 7500,
  "pending_frames": 2500,
  "processing_rate": 120,  // frames/hour
  "last_updated": "2025-01-15T10:30:00Z"  // null if no OCR data
}
```

### 3. Router Configuration (`src/memoire-web/src/server.rs`)

Added route registrations:
- `/api/search` → `routes::search_ocr`
- `/api/stats/ocr` → `routes::get_ocr_stats`

## Implementation Details

### Error Handling
All endpoints follow the existing ApiError pattern:
- Database errors return 500 with error message
- Not found errors return 404
- Invalid parameters return appropriate error responses

### Database Queries
- **Search**: Uses FTS5 full-text search with `ocr_text_fts` table
- **Stats calculation**: Uses COUNT and aggregation queries
- **Frame OCR lookup**: Simple indexed query by frame_id

### Performance Considerations
- FTS5 search results are paginated (max 100 per request)
- Processing rate calculated over last hour to avoid expensive full-table scans
- OCR data included in frame endpoint only when available (LEFT JOIN pattern)

## Testing

### Manual Testing Examples

```bash
# Get frame with OCR data
curl http://localhost:3030/api/frames/123

# Search for text
curl 'http://localhost:3030/api/search?q=hello&limit=10'

# Get OCR statistics
curl http://localhost:3030/api/stats/ocr

# Complex search query
curl 'http://localhost:3030/api/search?q="error+message"+AND+chrome'
```

## Future Enhancements

Potential improvements for future phases:
1. Add snippet/highlighting in search results
2. Support for filtering by date range in search
3. Add OCR confidence threshold filtering
4. Implement search result ranking customization
5. Add aggregation endpoints (e.g., most common words)

## Dependencies

No new external dependencies were added. Implementation uses existing crates:
- `rusqlite`: For FTS5 queries
- `axum`: For HTTP routing
- `serde_json`: For JSON responses
- `chrono`: For timestamp handling
