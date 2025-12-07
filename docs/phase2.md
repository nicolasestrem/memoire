 Phase 2 Implementation Plan: OCR + Basic Search

 Overview

 Phase 2 adds Windows OCR integration and full-text search capabilities to the Memoire foundation. This phase builds on
  the solid capture pipeline from Phase 1 to extract text from screen frames and enable searchability.

 Current State Analysis

 Foundation From Phase 1 ✅

 - Multi-monitor DXGI screen capture working
 - MP4 encoding with NVENC/x264 fallback
 - SQLite database with WAL mode
 - Frame metadata collection and batch insertion
 - Web-based validation viewer with frame-accurate seeking
 - System tray application with start/stop controls

 Database Schema Ready ✅

 ocr_text (
   id INTEGER PRIMARY KEY,
   frame_id INTEGER NOT NULL,
   text TEXT NOT NULL,
   text_json TEXT,
   confidence REAL
 )

 ocr_text_fts (
   text TEXT
 )
 - FTS5 triggers automatically sync on insert/update/delete
 - Query functions already implemented: insert_ocr_text(), search_ocr()

 Extension Points Available ✅

 - API endpoint /api/frames/:id ready for OCR response
 - Frontend metadata display has placeholder for OCR text
 - Batch insertion pattern established (used for frames, reusable for OCR)
 - Background worker pattern proven in encoder.rs

 ---
 Phase 2 Implementation Tasks

 Task 1: Windows OCR Integration (memoire-ocr crate)

 Goal: Create dedicated OCR processing crate using Windows.Media.Ocr API

 Implementation Steps:

 1. Create new crate structure
 src/memoire-ocr/
 ├── Cargo.toml
 ├── src/
 │   ├── lib.rs          # Public API
 │   ├── engine.rs       # Windows OCR engine wrapper
 │   ├── processor.rs    # Frame processing logic
 │   └── error.rs        # Error types
 2. Add dependencies to Cargo.toml
 [dependencies]
 windows = { version = "0.58", features = [
     "Media_Ocr",
     "Graphics_Imaging",
     "Storage_Streams",
     "Foundation"
 ]}
 tokio = { version = "1.35", features = ["full"] }
 image = "0.25"  # For RGBA → SoftwareBitmap conversion
 anyhow = "1.0"
 thiserror = "1.0"
 tracing = "0.1"
 serde = { version = "1.0", features = ["derive"] }
 serde_json = "1.0"
 3. Implement OcrEngine struct (engine.rs)
   - Initialize Windows.Media.Ocr.OcrEngine for English (extendable to other languages)
   - Handle async OCR API calls (RecognizeAsync)
   - Parse OcrResult into structured data (words, lines, bounding boxes, confidence)
   - Cache engine instance for reuse across frames
 4. Implement OcrProcessor (processor.rs)
   - Convert RGBA raw frames to Windows SoftwareBitmap
   - Batch multiple frames for efficient processing
   - Extract full text + structured JSON (bounding boxes, confidence per word)
   - Return OcrResult { text: String, text_json: String, confidence: f32 }
 5. Error handling
   - Define OcrError enum (EngineInitFailed, ConversionError, ProcessingError)
   - Implement Fromwindows::core::Error for OcrError
   - Add tracing for debugging OCR failures

 Acceptance Criteria:
 - ✅ Windows OCR engine initializes successfully
 - ✅ Can process RGBA frames and return OCR results
 - ✅ Structured JSON includes bounding boxes and confidence scores
 - ✅ Async API doesn't block capture thread
 - ✅ Error handling covers all failure modes

 Files to Create:
 - src/memoire-ocr/Cargo.toml
 - src/memoire-ocr/src/lib.rs
 - src/memoire-ocr/src/engine.rs
 - src/memoire-ocr/src/processor.rs
 - src/memoire-ocr/src/error.rs

 ---
 Task 2: Background OCR Indexer (indexer.rs in memoire-core)

 Goal: Process recorded frames in background without blocking capture

 Implementation Steps:

 1. Create indexer module (src/memoire-core/src/indexer.rs)
   - Spawns dedicated tokio task for OCR processing
   - Polls database for frames without OCR text (WHERE NOT EXISTS in ocr_text)
   - Reads video chunk MP4, extracts frame at offset_index using FFmpeg
   - Calls OcrProcessor with RGBA frame data
   - Inserts results into ocr_text table (batch 30 frames like frame insertion)
 2. Implement frame extraction
   - Use ffmpeg-next to seek to specific frame offset
   - Decode to RGBA format (same as capture)
   - Cache decoder instances per video chunk for efficiency
 3. Add rate limiting
   - Process max 10 frames/second to avoid CPU spikes
   - Configurable via CLI flag --ocr-fps (default: 10)
   - Pause if CPU usage exceeds threshold (optional, future enhancement)
 4. Progress tracking
   - Log progress every 100 frames processed
   - Expose /api/stats/ocr endpoint showing:
       - Total frames
     - Frames with OCR
     - Pending frames
     - Processing rate (frames/sec)
 5. Graceful shutdown
   - Listen for shutdown signal (Ctrl+C, SIGTERM)
   - Finish current batch before exit
   - Log final progress

 Acceptance Criteria:
 - ✅ Indexer runs in background without blocking recorder
 - ✅ Automatically processes new frames as they're captured
 - ✅ Batch insertion pattern used (30 frames/transaction)
 - ✅ Graceful shutdown completes in-progress work
 - ✅ Progress visible via logs and API endpoint

 Files to Create/Modify:
 - src/memoire-core/src/indexer.rs (new)
 - src/memoire-core/src/main.rs (add index subcommand)
 - src/memoire-core/src/recorder.rs (spawn indexer task alongside recording)

 ---
 Task 3: Update Database Queries

 Goal: Extend existing query functions to retrieve OCR data

 Implementation Steps:

 1. Add OCR retrieval functions (src/memoire-db/src/queries.rs)
 pub fn get_frame_with_ocr(conn: &Connection, frame_id: i64)
     -> Result<FrameWithOcr>

 pub fn get_frames_with_ocr_in_range(
     conn: &Connection,
     start: DateTime<Utc>,
     end: DateTime<Utc>
 ) -> Result<Vec<FrameWithOcr>>
 2. Define new response types (src/memoire-db/src/schema.rs)
 pub struct FrameWithOcr {
     pub frame: Frame,
     pub ocr: Option<OcrText>,
 }

 pub struct OcrText {
     pub id: i64,
     pub frame_id: i64,
     pub text: String,
     pub text_json: Option<String>,
     pub confidence: Option<f32>,
 }
 3. Optimize queries
   - Use LEFT JOIN to include frames without OCR
   - Add index on ocr_text.frame_id (already exists in schema)
   - Use prepared statements for caching

 Acceptance Criteria:
 - ✅ Can retrieve frame + OCR in single query
 - ✅ Handles frames without OCR gracefully (returns None)
 - ✅ Query performance acceptable (<50ms for single frame)

 Files to Modify:
 - src/memoire-db/src/queries.rs (add functions)
 - src/memoire-db/src/schema.rs (add FrameWithOcr, OcrText structs)

 ---
 Task 4: Extend REST API for OCR

 Goal: Expose OCR data via web API

 Implementation Steps:

 1. Update existing endpoints (src/memoire-web/src/routes/api.rs)
 GET /api/frames/:id
 // Response now includes:
 {
   "frame": { ... },
   "ocr": {
     "text": "full extracted text",
     "text_json": "{ words: [...], lines: [...] }",
     "confidence": 0.95
   }
 }
 2. Add search endpoint
 GET /api/search?q=<query>&limit=50&offset=0
 // Uses existing search_ocr() function
 // Returns frames matching FTS5 query
 {
   "results": [
     {
       "frame": { ... },
       "ocr": { ... },
       "rank": 0.85,  // FTS5 relevance score
       "snippet": "...matching text..."
     }
   ],
   "total": 1234,
   "has_more": true
 }
 3. Add OCR stats endpoint
 GET /api/stats/ocr
 {
   "total_frames": 10000,
   "frames_with_ocr": 8500,
   "pending_frames": 1500,
   "processing_rate": 12.5,  // frames/sec
   "last_updated": "2025-01-15T10:30:00Z"
 }

 Acceptance Criteria:
 - ✅ /api/frames/:id includes OCR if available
 - ✅ /api/search returns ranked results with snippets
 - ✅ /api/stats/ocr shows indexer progress
 - ✅ All endpoints handle missing OCR gracefully

 Files to Modify:
 - src/memoire-web/src/routes/api.rs (update endpoints)
 - src/memoire-web/src/state.rs (share indexer stats if needed)

 ---
 Task 5: Update Web UI for OCR Display

 Goal: Show OCR text and enable search in validation viewer

 Implementation Steps:

 1. Update frame metadata display (src/memoire-web/static/index.html)
   - Add OCR text section below video player
   - Display full text and confidence score
   - Show "Processing..." if OCR not yet available
   - Optionally render bounding boxes overlay on video (advanced, deferred)
 2. Add search interface (src/memoire-web/static/index.html)
 <div class="search-container">
   <input type="search" id="search-input" placeholder="Search OCR text...">
   <button id="search-btn">Search</button>
 </div>
 <div id="search-results">
   <!-- Results displayed as timeline cards -->
 </div>
 3. Implement search logic (src/memoire-web/static/app.js)
   - Fetch /api/search?q=<query>
   - Display results with timestamp, snippet, and relevance
   - Click result to jump to frame in video player
   - Highlight matching text in OCR display
 4. Add OCR stats widget
   - Show indexer progress bar
   - Update every 5 seconds via polling /api/stats/ocr

 Acceptance Criteria:
 - ✅ OCR text displays alongside video player
 - ✅ Search interface returns relevant results
 - ✅ Clicking search result jumps to correct frame
 - ✅ Stats widget shows processing progress
 - ✅ UI handles missing OCR gracefully

 Files to Modify:
 - src/memoire-web/static/index.html (add search UI)
 - src/memoire-web/static/style.css (style search interface)
 - src/memoire-web/static/app.js (add search logic)

 ---
 Task 6: CLI Commands for OCR

 Goal: Add CLI commands for OCR indexing and search

 Implementation Steps:

 1. Add index subcommand (src/memoire-core/src/main.rs)
 memoire index [--data-dir PATH] [--ocr-fps 10]
 # Runs background indexer until stopped with Ctrl+C
 2. Add search subcommand
 memoire search "query text" [--limit 10] [--data-dir PATH]
 # Prints matching frames with timestamps and snippets
 3. Update status subcommand
 memoire status
 # Now shows:
 # - Recording status
 # - Total frames captured
 # - Frames with OCR
 # - Pending frames
 # - Indexer status (running/stopped)

 Acceptance Criteria:
 - ✅ memoire index runs indexer standalone
 - ✅ memoire search performs FTS5 search from CLI
 - ✅ memoire status shows OCR progress
 - ✅ All commands work with custom data directories

 Files to Modify:
 - src/memoire-core/src/main.rs (add subcommands)
 - src/memoire-core/src/indexer.rs (make public API)

 ---
 Testing Strategy

 Unit Tests

 1. OcrEngine tests (src/memoire-ocr/src/engine.rs)
   - Mock Windows API responses
   - Test error handling (engine init failure, API timeout)
   - Verify confidence score calculation
 2. OcrProcessor tests (src/memoire-ocr/src/processor.rs)
   - Test RGBA → SoftwareBitmap conversion
   - Verify batch processing logic
   - Test JSON serialization
 3. Database query tests (src/memoire-db/src/queries.rs)
   - Test get_frame_with_ocr() with/without OCR
   - Test search_ocr() with various FTS5 queries
   - Test batch OCR insertion

 Integration Tests

 1. Indexer pipeline test
   - Record 10 frames → run indexer → verify OCR in database
   - Test graceful shutdown (finish current batch)
   - Test resume after restart (doesn't reprocess existing OCR)
 2. API endpoint tests
   - Test /api/frames/:id with OCR
   - Test /api/search with pagination
   - Test /api/stats/ocr accuracy
 3. Web UI tests (manual)
   - Search returns correct results
   - OCR text displays properly
   - Stats widget updates correctly

 Performance Tests

 1. OCR latency (<500ms/frame target)
 2. Indexer throughput (>10 frames/sec target)
 3. Search latency (<100ms target)
 4. Database size growth (verify FTS5 index overhead)

 ---
 Deployment & Migration

 Database Migration

 - No schema changes needed (tables already exist)
 - Indexer automatically processes existing frames on first run

 CLI Updates

 - Add new subcommands: index, search
 - Update help text to explain OCR functionality
 - Backward compatible (old recordings work without OCR)

 Validation

 - Use Phase 1.5 viewer to verify OCR accuracy
 - Manually review OCR results for common failure modes
 - Test search with known queries against ground truth

 ---
 Success Criteria (Phase 2 Complete)

 Functional Requirements

 - ✅ Windows OCR API integration working
 - ✅ Background indexer processes frames without blocking capture
 - ✅ FTS5 full-text search returns relevant results
 - ✅ Web UI displays OCR text and search interface
 - ✅ CLI supports standalone indexing and search

 Performance Requirements

 - ✅ OCR latency <500ms/frame
 - ✅ Indexer throughput >10 frames/sec
 - ✅ Search latency <100ms for typical queries
 - ✅ No impact on capture performance (still 1 FPS)

 Quality Requirements

 - ✅ OCR accuracy >90% on typical UI text
 - ✅ FTS5 search recall >95% (finds relevant frames)
 - ✅ Graceful handling of OCR failures (empty text, low confidence)
 - ✅ No database corruption from concurrent indexer + capture

 ---
 Risk Mitigation

 | Risk                                  | Mitigation Strategy                                |
 |---------------------------------------|----------------------------------------------------|
 | Windows OCR API slow on some machines | Add cloud fallback (Azure Vision, future Phase 6)  |
 | FTS5 index grows too large            | Implement retention policies (future Phase 7)      |
 | Indexer can't keep up with capture    | Queue frames, process during idle time             |
 | OCR accuracy poor on certain UIs      | Log low-confidence results for future model tuning |
 | Concurrent access to SQLite           | Already mitigated by WAL mode, batch inserts       |

 ---
 Files to Create/Modify Summary

 New Files

 - src/memoire-ocr/Cargo.toml
 - src/memoire-ocr/src/lib.rs
 - src/memoire-ocr/src/engine.rs
 - src/memoire-ocr/src/processor.rs
 - src/memoire-ocr/src/error.rs
 - src/memoire-core/src/indexer.rs

 Modified Files

 - Cargo.toml (workspace: add memoire-ocr)
 - src/memoire-core/Cargo.toml (add memoire-ocr dependency)
 - src/memoire-core/src/main.rs (add index/search subcommands)
 - src/memoire-core/src/recorder.rs (spawn indexer task)
 - src/memoire-db/src/queries.rs (add OCR query functions)
 - src/memoire-db/src/schema.rs (add FrameWithOcr, OcrText)
 - src/memoire-web/src/routes/api.rs (add search endpoint, update frame endpoint)
 - src/memoire-web/static/index.html (add search UI)
 - src/memoire-web/static/style.css (style search)
 - src/memoire-web/static/app.js (add search logic)

 ---
 Next Steps After Phase 2

 Once Phase 2 is complete, the system will have:
 - ✅ Continuous screen capture
 - ✅ OCR text extraction
 - ✅ Full-text search via FTS5
 - ✅ Web-based viewer with search
 - ✅ CLI for recording, indexing, and searching

 Ready for Phase 3: Audio capture + speech-to-text transcription
 - Reuse indexer pattern for audio processing
 - Add speaker diarization (future enhancement)
 - Sync audio transcriptions with video frames by timestamp