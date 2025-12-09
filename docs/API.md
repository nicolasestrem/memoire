# Memoire REST API Documentation

## Overview

The Memoire REST API provides programmatic access to the screen capture database, OCR-indexed content, and video streaming. All endpoints return JSON responses (except video streaming) and support standard HTTP status codes.

**Base URL:** `http://localhost:8080` (configurable via `--port` flag)

**Authentication:** None (local-only application)

## Table of Contents

- [Statistics Endpoints](#statistics-endpoints)
- [Video Chunks](#video-chunks)
- [Frames](#frames)
- [Monitors](#monitors)
- [Search](#search)
- [Video Streaming](#video-streaming)
- [Error Responses](#error-responses)
- [Pagination](#pagination)

---

## Statistics Endpoints

### GET /api/stats

Get database statistics including frame counts and monitor summaries.

**Query Parameters:** None

**Response:**
```json
{
  "total_frames": 12450,
  "total_chunks": 42,
  "monitors": [
    {
      "device_name": "\\\\.\\DISPLAY1",
      "total_chunks": 25,
      "total_frames": 7500,
      "latest_capture": "2025-12-09T14:30:00Z"
    },
    {
      "device_name": "\\\\.\\DISPLAY2",
      "total_chunks": 17,
      "total_frames": 4950,
      "latest_capture": "2025-12-09T14:29:55Z"
    }
  ]
}
```

**Example:**
```bash
curl http://localhost:8080/api/stats
```

---

### GET /api/stats/ocr

Get OCR indexing progress and statistics.

**Query Parameters:** None

**Response:**
```json
{
  "total_frames": 12450,
  "frames_with_ocr": 9800,
  "pending_frames": 2650,
  "processing_rate": 145,
  "last_updated": "2025-12-09T14:28:30Z"
}
```

**Fields:**
- `total_frames`: Total number of captured frames in database
- `frames_with_ocr`: Frames that have been OCR-processed
- `pending_frames`: Frames waiting for OCR processing
- `processing_rate`: Frames indexed in the last hour
- `last_updated`: Timestamp of most recently indexed frame

**Example:**
```bash
curl http://localhost:8080/api/stats/ocr
```

---

## Video Chunks

### GET /api/chunks

List video chunks with pagination and optional filtering.

**Query Parameters:**
- `monitor` (optional): Filter by monitor device name (e.g., `\\\\.\\DISPLAY1`)
- `limit` (optional): Number of results per page (default: 50, max: 100)
- `offset` (optional): Number of results to skip (default: 0)

**Response:**
```json
{
  "chunks": [
    {
      "id": 42,
      "file_path": "C:\\Users\\user\\AppData\\Local\\Memoire\\2025-12-09_14-25-00_DISPLAY1.mp4",
      "device_name": "\\\\.\\DISPLAY1",
      "created_at": "2025-12-09T14:25:00Z",
      "frame_count": 300
    }
  ],
  "total": 42
}
```

**Example:**
```bash
# Get first 10 chunks
curl "http://localhost:8080/api/chunks?limit=10&offset=0"

# Filter by monitor
curl "http://localhost:8080/api/chunks?monitor=\\\\.\\DISPLAY1"
```

---

### GET /api/chunks/:id

Get details for a specific video chunk by ID.

**Path Parameters:**
- `id`: Video chunk ID (integer)

**Response:**
```json
{
  "id": 42,
  "file_path": "C:\\Users\\user\\AppData\\Local\\Memoire\\2025-12-09_14-25-00_DISPLAY1.mp4",
  "device_name": "\\\\.\\DISPLAY1",
  "created_at": "2025-12-09T14:25:00Z",
  "frame_count": 300
}
```

**Error:** Returns 404 if chunk not found

**Example:**
```bash
curl http://localhost:8080/api/chunks/42
```

---

### GET /api/chunks/:id/frames

Get all frames belonging to a specific video chunk.

**Status:** Not yet implemented (returns 501 Not Implemented)

---

## Frames

### GET /api/frames

List frames with optional time range filtering.

**Status:** Not yet implemented (returns 501 Not Implemented)

**Planned Query Parameters:**
- `start` (optional): Start timestamp (ISO 8601 format)
- `end` (optional): End timestamp (ISO 8601 format)
- `limit` (optional): Number of results per page
- `offset` (optional): Number of results to skip

---

### GET /api/frames/:id

Get details for a specific frame, including OCR text if available.

**Path Parameters:**
- `id`: Frame ID (integer)

**Response:**
```json
{
  "id": 12450,
  "video_chunk_id": 42,
  "offset_index": 150,
  "timestamp": "2025-12-09T14:27:30Z",
  "app_name": "chrome.exe",
  "window_name": "Memoire API Documentation - Google Chrome",
  "browser_url": "https://github.com/yourorg/memoire",
  "focused": true,
  "chunk": {
    "file_path": "C:\\Users\\user\\AppData\\Local\\Memoire\\2025-12-09_14-25-00_DISPLAY1.mp4",
    "device_name": "\\\\.\\DISPLAY1"
  },
  "ocr": {
    "text": "Memoire REST API Documentation\nOverview\nThe Memoire REST API provides...",
    "text_json": "[{\"text\":\"Memoire\",\"bounds\":{\"x\":120,\"y\":45,\"width\":180,\"height\":28}}]",
    "confidence": 0.96
  }
}
```

**Fields:**
- `id`: Frame unique identifier
- `video_chunk_id`: Parent video chunk ID
- `offset_index`: Frame position within the 5-minute video chunk
- `timestamp`: Capture timestamp (UTC)
- `app_name`: Foreground application executable name
- `window_name`: Window title text
- `browser_url`: URL if browser window detected (Chrome/Edge)
- `focused`: Whether window was focused during capture
- `chunk`: Parent video chunk metadata
- `ocr` (optional): OCR data if frame has been indexed
  - `text`: Concatenated extracted text
  - `text_json`: Bounding box data in JSON format
  - `confidence`: OCR confidence score (0.0 - 1.0)

**Error:** Returns 404 if frame not found

**Example:**
```bash
curl http://localhost:8080/api/frames/12450
```

---

## Monitors

### GET /api/monitors

List all monitors/displays with capture statistics.

**Query Parameters:** None

**Response:**
```json
{
  "monitors": [
    {
      "device_name": "\\\\.\\DISPLAY1",
      "total_chunks": 25,
      "total_frames": 7500,
      "latest_capture": "2025-12-09T14:30:00Z"
    },
    {
      "device_name": "\\\\.\\DISPLAY2",
      "total_chunks": 17,
      "total_frames": 4950,
      "latest_capture": "2025-12-09T14:29:55Z"
    }
  ]
}
```

**Example:**
```bash
curl http://localhost:8080/api/monitors
```

---

## Search

### GET /api/search

Full-text search across OCR-indexed content using SQLite FTS5.

**Query Parameters:**
- `q` (required): Search query string
- `limit` (optional): Number of results per page (default: 50, max: 100)
- `offset` (optional): Number of results to skip (default: 0)

**Response:**
```json
{
  "results": [
    {
      "frame": {
        "id": 12450,
        "timestamp": "2025-12-09T14:27:30Z",
        "app_name": "chrome.exe",
        "window_name": "Memoire API Documentation - Google Chrome",
        "browser_url": "https://github.com/yourorg/memoire"
      },
      "ocr": {
        "text": "Memoire REST API Documentation\nOverview\nThe Memoire REST API provides...",
        "confidence": 0.96
      }
    }
  ],
  "total": 15,
  "has_more": false,
  "limit": 50,
  "offset": 0
}
```

**Fields:**
- `results`: Array of matching frames with OCR data
- `total`: Total number of matches across all pages
- `has_more`: Boolean indicating if more results exist
- `limit`: Applied result limit
- `offset`: Applied result offset

**Search Notes:**
- Query is automatically wrapped in quotes for literal phrase matching
- Special FTS5 characters are escaped automatically
- Results are ranked by relevance (BM25 algorithm)
- Empty queries return 400 Bad Request

**Example:**
```bash
# Simple search
curl "http://localhost:8080/api/search?q=API+documentation"

# Paginated search
curl "http://localhost:8080/api/search?q=memoire&limit=10&offset=20"

# Search with special characters (automatically escaped)
curl "http://localhost:8080/api/search?q=error%3A+404"
```

---

## Video Streaming

### GET /video/:filename

Stream MP4 video files with HTTP range request support.

**Path Parameters:**
- `filename`: Video file basename (e.g., `2025-12-09_14-25-00_DISPLAY1.mp4`)

**Headers:**
- `Range` (optional): Byte range for partial content (e.g., `bytes=0-1023`)

**Response:**
- **200 OK**: Full file content (if no Range header)
- **206 Partial Content**: Requested byte range
- **416 Range Not Satisfiable**: Invalid range
- **404 Not Found**: File does not exist

**Response Headers:**
- `Content-Type: video/mp4`
- `Content-Length`: Size in bytes
- `Accept-Ranges: bytes`
- `Content-Range` (206 only): Byte range specification

**Example:**
```bash
# Stream full video
curl http://localhost:8080/video/2025-12-09_14-25-00_DISPLAY1.mp4 -o video.mp4

# Request first 1MB
curl -H "Range: bytes=0-1048575" http://localhost:8080/video/2025-12-09_14-25-00_DISPLAY1.mp4

# Use in HTML5 video player
<video controls>
  <source src="http://localhost:8080/video/2025-12-09_14-25-00_DISPLAY1.mp4" type="video/mp4">
</video>
```

**Security Notes:**
- Only files from the configured data directory are accessible
- Path traversal attacks (e.g., `../`) are blocked
- Only `.mp4` files are served

---

## Error Responses

All error responses follow a consistent JSON structure:

```json
{
  "error": "Error message describing what went wrong"
}
```

**HTTP Status Codes:**

| Code | Meaning | Example |
|------|---------|---------|
| 200 | Success | Request completed successfully |
| 206 | Partial Content | Video range request succeeded |
| 400 | Bad Request | Invalid query parameter or empty search |
| 404 | Not Found | Resource (chunk/frame/video) does not exist |
| 416 | Range Not Satisfiable | Invalid byte range for video |
| 500 | Internal Server Error | Database error or system failure |
| 501 | Not Implemented | Endpoint stub not yet implemented |

**Example Error:**
```bash
$ curl http://localhost:8080/api/chunks/99999
{
  "error": "chunk 99999 not found"
}
```

---

## Pagination

All list endpoints (`/api/chunks`, `/api/search`) support limit/offset pagination:

**Parameters:**
- `limit`: Results per page (default: 50, max: 100)
- `offset`: Number of results to skip (default: 0)

**Navigation Pattern:**
```bash
# Page 1 (first 50 results)
curl "http://localhost:8080/api/search?q=memoire&limit=50&offset=0"

# Page 2 (next 50 results)
curl "http://localhost:8080/api/search?q=memoire&limit=50&offset=50"

# Page 3 (next 50 results)
curl "http://localhost:8080/api/search?q=memoire&limit=50&offset=100"
```

**Response Fields:**
- `total`: Total results across all pages
- `has_more`: Boolean indicating additional pages exist
- `limit`: Applied limit value
- `offset`: Applied offset value

**Calculating Pages:**
```javascript
const totalPages = Math.ceil(response.total / response.limit);
const currentPage = Math.floor(response.offset / response.limit) + 1;
const hasNextPage = response.has_more;
```

---

## Data Types

**Timestamps:** All timestamps are in ISO 8601 format with UTC timezone:
```
2025-12-09T14:27:30Z
```

**Device Names:** Windows display identifiers:
```
\\\\.\\DISPLAY1
\\\\.\\DISPLAY2
```

**File Paths:** Absolute Windows paths:
```
C:\\Users\\user\\AppData\\Local\\Memoire\\2025-12-09_14-25-00_DISPLAY1.mp4
```

---

## Performance Considerations

- **Database Indexes**: All queries use indexes for optimal performance (< 100ms)
- **FTS5 Search**: Full-text search uses SQLite's FTS5 module with BM25 ranking
- **Video Streaming**: Range requests enable efficient seeking in large files
- **Pagination Limits**: Maximum 100 results per page to prevent memory issues
- **Concurrent Access**: SQLite WAL mode supports multiple readers during indexing

---

## Client Examples

### JavaScript/Fetch

```javascript
// Search for content
async function search(query, page = 0, limit = 50) {
  const offset = page * limit;
  const response = await fetch(
    `http://localhost:8080/api/search?q=${encodeURIComponent(query)}&limit=${limit}&offset=${offset}`
  );
  return await response.json();
}

// Get frame with OCR
async function getFrame(frameId) {
  const response = await fetch(`http://localhost:8080/api/frames/${frameId}`);
  if (!response.ok) {
    throw new Error(`Frame ${frameId} not found`);
  }
  return await response.json();
}
```

### Python/Requests

```python
import requests

# Get statistics
def get_stats():
    response = requests.get("http://localhost:8080/api/stats")
    return response.json()

# Search with pagination
def search(query, limit=50, offset=0):
    params = {"q": query, "limit": limit, "offset": offset}
    response = requests.get("http://localhost:8080/api/search", params=params)
    return response.json()

# Download video
def download_video(filename, output_path):
    response = requests.get(f"http://localhost:8080/video/{filename}", stream=True)
    with open(output_path, 'wb') as f:
        for chunk in response.iter_content(chunk_size=8192):
            f.write(chunk)
```

### PowerShell

```powershell
# Get OCR statistics
$stats = Invoke-RestMethod -Uri "http://localhost:8080/api/stats/ocr"
Write-Host "Indexed: $($stats.frames_with_ocr) / $($stats.total_frames)"

# Search
$results = Invoke-RestMethod -Uri "http://localhost:8080/api/search?q=error&limit=10"
$results.results | ForEach-Object {
    Write-Host "$($_.frame.timestamp) - $($_.frame.app_name)"
}
```

---

## Future Endpoints (Phase 3+)

These endpoints are planned for future releases:

- `GET /api/audio/chunks` - List audio chunks
- `GET /api/audio/transcriptions` - Search speech-to-text transcriptions
- `GET /api/timeline` - Unified timeline of screen + audio events
- `GET /api/activity` - Application usage statistics
- `POST /api/export` - Export data in various formats

---

## Support

For issues or questions:
- GitHub: [your-repo-url]
- Documentation: `docs/` directory
- CLI Help: `memoire --help`
