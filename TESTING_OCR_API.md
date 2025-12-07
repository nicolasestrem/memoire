# Testing OCR API Endpoints

Quick reference for testing the new OCR API endpoints.

## Prerequisites

1. Start the memoire-web server:
```bash
cargo run -p memoire-web -- --data-dir ./data
```

2. Ensure you have some frames with OCR data in the database

## Test Endpoints

### 1. Test Updated Frame Endpoint (with OCR)

```bash
# Get a frame that has OCR data
curl http://localhost:3030/api/frames/1 | jq

# Expected: Frame metadata + optional "ocr" field if OCR data exists
```

### 2. Test Search Endpoint

```bash
# Basic search
curl 'http://localhost:3030/api/search?q=test' | jq

# Search with pagination
curl 'http://localhost:3030/api/search?q=error&limit=10&offset=0' | jq

# Phrase search
curl 'http://localhost:3030/api/search?q="error+message"' | jq

# Boolean search
curl 'http://localhost:3030/api/search?q=chrome+AND+error' | jq
```

### 3. Test OCR Statistics Endpoint

```bash
# Get OCR indexing statistics
curl http://localhost:3030/api/stats/ocr | jq

# Expected output:
# {
#   "total_frames": 1000,
#   "frames_with_ocr": 750,
#   "pending_frames": 250,
#   "processing_rate": 120,
#   "last_updated": "2025-01-15T10:30:00Z"
# }
```

## Response Validation

### Frame with OCR Response
```json
{
  "id": 123,
  "video_chunk_id": 45,
  "offset_index": 10,
  "timestamp": "2025-01-15T10:30:00Z",
  "app_name": "Chrome",
  "window_name": "Example",
  "browser_url": "https://example.com",
  "focused": true,
  "chunk": {
    "file_path": "/path/to/chunk.mp4",
    "device_name": "monitor-0"
  },
  "ocr": {  // Only present if OCR data exists
    "text": "extracted text",
    "text_json": null,
    "confidence": 0.95
  }
}
```

### Search Response
```json
{
  "results": [
    {
      "frame": {
        "id": 123,
        "timestamp": "2025-01-15T10:30:00Z",
        "app_name": "Chrome",
        "window_name": "Example",
        "browser_url": "https://example.com"
      },
      "ocr": {
        "text": "matched text",
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

### OCR Stats Response
```json
{
  "total_frames": 10000,
  "frames_with_ocr": 7500,
  "pending_frames": 2500,
  "processing_rate": 120,
  "last_updated": "2025-01-15T10:30:00Z"
}
```

## Error Cases to Test

```bash
# Non-existent frame
curl http://localhost:3030/api/frames/999999 
# Expected: 404 Not Found

# Search without query parameter
curl http://localhost:3030/api/search
# Expected: 400 Bad Request (missing required parameter 'q')

# Invalid limit parameter
curl 'http://localhost:3030/api/search?q=test&limit=1000'
# Expected: Results capped at 100 (max limit)
```

## Integration with Existing Data

The endpoints work with the existing database schema:
- `frames` table for frame metadata
- `ocr_text` table for OCR data
- `ocr_text_fts` FTS5 table for full-text search

## Performance Notes

- Search endpoint is paginated (max 100 results per request)
- Processing rate calculated over last hour only
- OCR stats endpoint uses COUNT aggregations (may be slow on very large databases)

## Common FTS5 Search Patterns

```bash
# Simple word search
q=hello

# Phrase search (exact match)
q="hello world"

# Boolean AND
q=hello AND world

# Boolean OR (default)
q=hello world
# or explicitly:
q=hello OR world

# Boolean NOT
q=hello NOT world

# Prefix search
q=hel*

# Column-specific search (if configured)
q=text:hello
```

See [SQLite FTS5 docs](https://www.sqlite.org/fts5.html) for full query syntax.
