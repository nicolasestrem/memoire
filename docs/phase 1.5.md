# Phase 1.5: Web-Based Validation Viewer - Implementation Plan

## Overview

Build a web-based validation viewer to verify the Phase 1 capture pipeline correctness. This viewer will evolve into the Phase 5 production dashboard, avoiding throwaway work.

**Technology Stack:**
- Backend: Rust + Axum web framework
- Frontend: Vanilla HTML/CSS/JS (Phase 1.5) → React (Phase 5 migration)
- Database: Existing SQLite via `memoire-db` crate

## Goals

### Primary (Phase 1.5 Validation)
1. ✓ Verify MP4 files play correctly
2. ✓ Validate frame-accurate seeking to specific timestamps
3. ✓ Catch VFR (Variable Frame Rate) sync issues early
4. ✓ Display frame metadata (timestamp, file path, offset_index)
5. ✓ Verify no timestamp drift across 5-minute chunks

### Secondary (Phase 5 Foundation)
- REST API that's reusable for React dashboard
- Clean separation of backend/frontend
- No throwaway code - everything evolves into production

## Architecture

### New Crate: `memoire-web`

```
src/memoire-web/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API exports
│   ├── server.rs           # Axum server setup and routing
│   ├── state.rs            # Shared AppState (DB connection)
│   ├── error.rs            # HTTP error responses
│   └── routes/
│       ├── mod.rs
│       ├── api.rs          # REST API handlers
│       ├── video.rs        # Video streaming with range requests
│       └── static_files.rs # Serve HTML/CSS/JS
├── static/                 # Frontend assets
│   ├── index.html          # Main viewer UI
│   ├── style.css           # Styling
│   └── app.js              # Frontend validation logic
└── README.md               # Migration guide to Phase 5
```

### CLI Integration

Add new subcommand to `memoire-core`:
```bash
memoire viewer [--port 8080] [--data-dir PATH]
```

## REST API Specification

### Video Chunk Endpoints

**GET /api/chunks**
- Query params: `monitor`, `start_date`, `end_date`, `limit`, `offset`
- Returns: Paginated list of video chunks with frame counts

**GET /api/chunks/:id**
- Returns: Chunk details + validation (file exists, duration, size)

**GET /api/chunks/:id/frames**
- Query params: `limit`, `offset`
- Returns: All frames in the chunk

### Frame Endpoints

**GET /api/frames**
- Query params: `start`, `end`, `limit`, `offset` (time range query)
- Returns: Frames within timestamp range

**GET /api/frames/:id**
- Returns: Frame details + embedded chunk info

### Video Streaming

**GET /video/:chunk_id**
- Supports HTTP Range requests (required for browser seeking)
- Returns: MP4 file stream with proper Content-Range headers
- Security: Validates file path, prevents traversal attacks

### Statistics

**GET /api/stats**
- Returns: Total frames/chunks, date range, monitors, storage size

**GET /api/monitors**
- Returns: Per-monitor statistics and latest capture times

## Frontend Features

### UI Layout

```
┌─────────────────────────────────────────────┐
│ Memoire Validation Viewer      [Status: ●] │
├─────────────────────────────────────────────┤
│ Monitor: [All ▾]  Date: [2024-01-15 ▾]     │
│                                             │
│ ┌─ Chunk Browser ───────────────────────┐  │
│ │ 10:30 - Monitor_1 - 300 frames [Play] │  │
│ │ 10:25 - Monitor_1 - 300 frames [Play] │  │
│ │ ...                     [Load More]    │  │
│ └────────────────────────────────────────┘  │
│                                             │
│ ┌─ Video Player ─────────────────────────┐ │
│ │ [Video Playback Area]                  │ │
│ │ [►] ──●──────────── 02:35 / 05:00      │ │
│ │ Seek: Frame [4567] [Go]  Time [···] [Go]│ │
│ └────────────────────────────────────────┘ │
│                                             │
│ ┌─ Frame Metadata ───────────────────────┐ │
│ │ Frame ID: 4567                         │ │
│ │ Timestamp: 2024-01-15T10:32:35.000000Z │ │
│ │ Offset: 155  Chunk: chunk_10-30-00.mp4 │ │
│ │ App: Chrome  Window: GitHub PR         │ │
│ │ [◄ Prev] [Next ►]                      │ │
│ └────────────────────────────────────────┘ │
│                                             │
│ ┌─ Validation ───────────────────────────┐ │
│ │ ✓ MP4 playable                         │ │
│ │ ✓ Seeking accurate (±50ms)             │ │
│ │ ⚠ Timestamp drift: +120ms (acceptable) │ │
│ │ ✓ No VFR detected                      │ │
│ └────────────────────────────────────────┘ │
└─────────────────────────────────────────────┘
```

### Validation Logic (app.js)

**Key Functions:**
1. `seekToFrame(frameId)` - Calculate offset from offset_index, seek video
2. `validateSeekAccuracy()` - Compare expected vs actual timestamp (±50ms)
3. `detectTimestampDrift()` - Check consecutive frames for continuity
4. `updateValidationPanel()` - Display validation results with color coding

**Video Player Integration:**
- Native HTML5 `<video>` element
- Range request support for seeking
- `timeupdate` event → fetch frame metadata
- `seeked` event → validate seek accuracy

## Implementation Phases

### MVP (Core Viewer)
**Goal:** Prove video playback and metadata display work

1. Backend setup
   - Create `memoire-web` crate with Axum
   - Implement `/api/chunks`, `/api/frames/:id`, `/video/:id` endpoints
   - Video streaming with HTTP Range requests

2. Frontend skeleton
   - HTML structure with all sections
   - CSS grid layout (dark theme)
   - Fetch and display chunk list

3. Video playback
   - Load MP4 into `<video>` element
   - Play/pause controls
   - Display frame metadata on `timeupdate` events

**Success Criteria:** Can browse chunks, play videos, see metadata

### Validation Layer
**Goal:** Validate frame-accurate seeking and detect issues

1. Seeking implementation
   - Seek to specific frame ID (calculate offset)
   - Seek to timestamp (find chunk + offset)
   - Prev/Next frame navigation

2. Validation logic
   - Compare expected vs actual timestamp
   - Detect timestamp drift across frames
   - Flag VFR issues (inconsistent frame duration)

3. Validation UI
   - Display results with ✓/⚠/✗ indicators
   - Color coding (green/yellow/red)
   - Warning thresholds (>100ms = yellow, >500ms = red)

**Success Criteria:** Catches VFR issues, validates seeking accuracy

### Polish (Optional - can defer to Phase 5)
- Monitor/date filtering
- Keyboard shortcuts (Space, ←/→)
- Statistics dashboard
- API documentation

## Critical Files to Create/Modify

### New Files

1. **src/memoire-web/Cargo.toml**
   - Dependencies: axum, tower, tower-http, memoire-db

2. **src/memoire-web/src/server.rs**
   - Axum router setup
   - Route definitions
   - CORS configuration
   - Server initialization

3. **src/memoire-web/src/routes/video.rs**
   - HTTP Range request parsing
   - MP4 file streaming
   - Content-Range header generation
   - Path traversal protection

4. **src/memoire-web/src/routes/api.rs**
   - GET /api/chunks (with pagination)
   - GET /api/chunks/:id
   - GET /api/chunks/:id/frames
   - GET /api/frames
   - GET /api/frames/:id
   - GET /api/stats
   - GET /api/monitors

5. **src/memoire-web/src/state.rs**
   - AppState struct (DB connection, data_dir)
   - State initialization

6. **src/memoire-web/src/error.rs**
   - ApiError enum (NotFound, BadRequest, etc.)
   - HTTP error response formatting

7. **src/memoire-web/static/index.html**
   - Full UI layout
   - All sections (filters, chunks, player, metadata, validation)

8. **src/memoire-web/static/app.js**
   - Chunk loading and pagination
   - Video playback control
   - Seeking to frame/timestamp
   - Validation logic (drift detection, VFR check)
   - Metadata display

9. **src/memoire-web/static/style.css**
   - Dark theme styling
   - Grid layout
   - Color-coded validation indicators
   - Responsive design

### Modified Files

1. **Cargo.toml** (workspace root)
   - Add `src/memoire-web` to members
   - Add workspace dependencies: axum, tower, tower-http

2. **src/memoire-db/src/queries.rs**
   - Add `get_chunks_paginated(limit, offset, monitor, date_filter)`
   - Add `get_frame_count_by_chunk(chunk_id)`
   - Add `get_monitors_summary()` (stats per monitor)

3. **src/memoire-core/src/main.rs**
   - Add `Commands::Viewer` variant
   - Implement `cmd_viewer(port, data_dir)` function
   - Call `memoire_web::serve()`

## Database Query Additions

Add to `src/memoire-db/src/queries.rs`:

```rust
// Paginated chunk listing with filters
pub fn get_chunks_paginated(
    conn: &Connection,
    limit: i64,
    offset: i64,
    monitor: Option<&str>,
    start_date: Option<DateTime<Utc>>,
    end_date: Option<DateTime<Utc>>,
) -> Result<Vec<VideoChunk>>

// Count frames in a chunk
pub fn get_frame_count_by_chunk(
    conn: &Connection,
    chunk_id: i64,
) -> Result<i64>

// Monitor statistics
pub fn get_monitors_summary(
    conn: &Connection,
) -> Result<Vec<MonitorSummary>>
```

## Validation Test Cases

### 1. Frame-Accurate Seeking
- Seek to frame with known timestamp
- Expected: Video time matches frame.timestamp ± 50ms
- Validation: `drift = |video.currentTime - expected_time|`

### 2. Timestamp Continuity
- Iterate through consecutive frames
- Expected: `timestamp[i+1] - timestamp[i] ≈ 1000ms` (1 FPS)
- Validation: Flag drift >100ms as warning, >500ms as error

### 3. Chunk Boundary Transitions
- Seek from end of chunk N to start of chunk N+1
- Expected: Smooth playback, no missing frames

### 4. Range Request Handling
- Browser seeks within video (drag playhead)
- Expected: Server responds with 206 Partial Content

## Security Considerations

1. **Path Traversal Prevention:**
   - Resolve all file paths relative to data_dir
   - Validate result with `.starts_with(data_dir)`
   - Return 403 Forbidden if path escapes data_dir

2. **CORS Configuration:**
   - Allow only localhost origins for Phase 1.5
   - Expand for Phase 5 if needed

## Migration Path to Phase 5

### What Stays (100% Reusable)
- All REST API endpoints (backend)
- Video streaming logic
- Database queries
- Axum server configuration

### What Changes (Frontend Only)
- Replace `static/` with React SPA
- Build with Vite/Webpack to `dist/`
- Update `server.rs` to serve `dist/` instead of `static/`
- Same API calls, now in React components

**Key Benefit:** No backend rewrites. Backend becomes production API immediately.

## Dependencies

### Workspace (Cargo.toml)
```toml
axum = { version = "0.7", features = ["macros"] }
tower = "0.4"
tower-http = { version = "0.5", features = ["fs", "cors", "trace"] }
hyper = { version = "1.0", features = ["full"] }
http-body-util = "0.1"
```

### memoire-web (src/memoire-web/Cargo.toml)
```toml
[dependencies]
memoire-db = { path = "../memoire-db" }
axum = { workspace = true }
tower = { workspace = true }
tower-http = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
anyhow = { workspace = true }
chrono = { workspace = true }
bytes = "1.5"
http-body-util = "0.1"
```

## Usage

```bash
# Start validation viewer
memoire viewer

# Custom port and data directory
memoire viewer --port 3000 --data-dir "D:\Memoire"

# Open browser to http://localhost:8080
```

## Success Metrics

- [ ] Can play MP4 videos from all monitors
- [ ] Frame seeking accurate within ±50ms
- [ ] No VFR issues detected in 1 FPS captures
- [ ] Timestamp drift <100ms across chunk boundaries
- [ ] All metadata displayed correctly
- [ ] Works with 1000+ chunks (pagination)
- [ ] Clear migration path documented for Phase 5

---

**This plan delivers a production-ready validation viewer that evolves into the Phase 5 dashboard with zero throwaway work. Backend API is immediately production-grade; only frontend swaps from vanilla JS to React.**