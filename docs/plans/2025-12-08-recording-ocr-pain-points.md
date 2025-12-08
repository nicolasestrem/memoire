# Recording, Capture & OCR Pain Points Action Plan

> **Generated:** 2025-12-08
> **Source:** Analysis of `/docs/failed_and_reverted/` documentation
> **Purpose:** Action plan for fixing recording, capture, and OCR processes

---

## Executive Summary

Analysis of 5 documentation files from failed/reverted fix attempts revealed **23 pain points** across three components:

| Component | Resolved | Partial | Open | Total |
|-----------|----------|---------|------|-------|
| OCR Processing | 4 | 1 | 4 | 9 |
| API & Frontend | 6 | 0 | 2 | 8 |
| FFmpeg & Capture | 3 | 1 | 2 | 6 |
| **TOTAL** | **13** | **2** | **8** | **23** |

### Critical Blockers
1. **Web UI OCR display broken** - Frontend expects `ocr_text[]` but API returns `ocr{}`
2. **No frame deduplication** - ~90% redundant frame processing at 10 FPS
3. **Sequential processing** - Async wrappers but synchronous I/O blocks entire runtime

---

## 1. OCR Processing Component

### 1.1 OCR Text Display ✅ RESOLVED

**Description:** OCR text extracted from frames was not displaying in the web interface.

**Resolution:**
- Web API endpoints implemented (`/api/search`, `/api/frames/:id`)
- FTS5 full-text search enabled via `ocr_text_fts` virtual table
- Database schema supports text storage with `OcrText` struct

**Files:** `api.rs`, `queries.rs`, `schema.rs`, `migrations.rs`

---

### 1.2 Poor OCR Performance ❌ OPEN

**Description:** OCR processing is slow due to sequential frame handling and inefficient extraction.

**Current Issues:**
- Sequential frame processing in `processor.rs:46-56` - frames processed one-by-one in loop
- FFmpeg/ffprobe spawned for every single frame in `indexer.rs:215-290`
- Batch size only 30 frames with no concurrent processing
- No frame caching or buffering between operations

**Key Learning:** Performance optimizations documented as "completed" in failed docs were not actually implemented in the codebase.

**Recommended Fix:**
- Implement concurrent frame processing with `tokio::spawn` or `rayon`
- Pre-extract all frames in batch before OCR processing
- Add frame pre-scaling/downsampling for faster OCR
- Cache video metadata to avoid repeated ffprobe calls

**Files:** `indexer.rs`, `processor.rs`

---

### 1.3 Multi-language OCR Support ⚠️ PARTIAL

**Description:** OCR should support multiple languages beyond English.

**What Works:**
- `Engine::new(language_tag: Option<&str>)` accepts language tags
- Windows OCR language support via `Language::CreateLanguage()`
- Helper method `Engine::english()` and system language detection

**What's Missing:**
- Indexer hardcodes English: `OcrProcessor::new()` at `indexer.rs:49`
- No language configuration in CLI or config file
- No language metadata stored in database
- No fallback for unsupported languages

**Recommended Fix:**
- Add `ocr_language` config option
- Pass language to indexer initialization
- Store language used in `ocr_text` table

**Files:** `indexer.rs`, `config.rs`, `engine.rs`

---

### 1.4 OCR Engine Caching ❌ OPEN

**Description:** Lack of caching for video metadata and OCR resources.

**Current Issues:**
- Single processor instance exists (good) but no video metadata cache
- Each frame extraction calls ffprobe individually at `indexer.rs:253-265`
- No frame buffer for consecutive frames
- No LRU cache for frequently re-processed regions

**Key Learning:** The single Processor instance provides basic caching, but ffprobe subprocess overhead dominates.

**Recommended Fix:**
- Cache VideoChunk metadata (dimensions, duration, fps) in memory
- Add video metadata fields to `VideoChunk` schema
- Implement sliding window frame buffer

**Files:** `indexer.rs`, `schema.rs`, `queries.rs`

---

### 1.5 Frame Deduplication ❌ OPEN

**Description:** Identical/similar consecutive frames waste OCR processing time.

**Current Issues:**
- No hash/fingerprint field in frames table
- `get_frames_without_ocr()` returns ALL frames without filtering
- No similarity detection (SSIM, perceptual hash, pixel comparison)
- At 10 FPS, consecutive frames are likely >95% similar

**Key Learning:** This is the highest-impact optimization - could reduce OCR workload by ~90%.

**Recommended Fix:**
- Add `frame_hash` column to frames table
- Calculate perceptual hash during capture
- Skip frames with hash matching previous frame
- Add similarity threshold configuration

**Files:** `migrations.rs`, `schema.rs`, `queries.rs`, `screen.rs`

---

### 1.6 Confidence Estimation ✅ RESOLVED

**Description:** OCR results needed confidence scores for quality filtering.

**Resolution:**
- Heuristic-based confidence in `engine.rs:160-192`
- Per-word confidence with aggregation
- Length bonus, character variety bonus, penalties applied
- Results stored in `ocr_text.confidence` field

**Limitation:** Not actual OCR engine confidence (Windows OCR doesn't provide it), but functional heuristic.

**Files:** `engine.rs`, `schema.rs`

---

### 1.7 Performance Metrics ✅ RESOLVED

**Description:** No visibility into OCR processing progress and rates.

**Resolution:**
- `IndexerStats` struct tracks total/processed/pending frames
- Processing rate calculation (frames/sec)
- Stats updated every 10 seconds
- API endpoint `/api/stats/ocr` exposes metrics

**Limitation:** Basic metrics only - no per-frame timing, error rates, or time-series history.

**Files:** `indexer.rs`, `api.rs`

---

### 1.8 Batch OCR Insertion ✅ RESOLVED

**Description:** Individual OCR inserts were slow.

**Resolution:**
- Batch size of 30 frames defined at `indexer.rs:16`
- Batch retrieval via `get_frames_without_ocr()`
- Batch insertion method `insert_ocr_batch()` at `indexer.rs:293-315`

**Minor Issue:** Each insert is a separate transaction - could wrap in single transaction for better performance.

**Files:** `indexer.rs`, `queries.rs`

---

### 1.9 Video Metadata Caching ❌ OPEN

**Description:** ffprobe called repeatedly for same video files.

**Current Issues:**
- ffprobe spawned per frame at `indexer.rs:254-265`
- Queries width, height every time
- `VideoChunk` schema missing dimension fields

**Key Learning:** Easy win - cache once per video chunk, reuse for all frames.

**Recommended Fix:**
- Add `width`, `height`, `duration`, `fps` to `VideoChunk` struct
- Store during video creation
- Query from database instead of ffprobe

**Files:** `schema.rs`, `encoder.rs`, `indexer.rs`

---

## 2. API & Frontend Component

### 2.1 Property Name Mismatches ✅ RESOLVED

**Description:** Frontend JavaScript and API responses used different property names.

**Resolution:** API uses consistent snake_case (`total_frames`, `frames_with_ocr`, etc.)

**Files:** `api.rs`

---

### 2.2 FTS5 Query Sanitization ❌ OPEN

**Description:** Invalid FTS5 syntax can cause SQL errors.

**Current Issues:**
- Raw query passed directly to FTS5 at `api.rs:247`
- Parameter binding prevents injection but not syntax errors
- Invalid queries like `"unclosed quote` or `OR AND` will error

**Key Learning:** Parameter binding is necessary but not sufficient for FTS5.

**Recommended Fix:**
- Validate FTS5 query syntax before execution
- Escape or sanitize special characters
- Return user-friendly error for invalid queries

**Files:** `api.rs`, `queries.rs`

---

### 2.3 /api/search Endpoint ✅ RESOLVED

**Description:** Need API endpoint for full-text search.

**Resolution:** Complete implementation at `api.rs:231-279` with pagination support.

**Files:** `api.rs`, `server.rs`

---

### 2.4 /api/stats/ocr Endpoint ✅ RESOLVED

**Description:** Need API endpoint for OCR progress statistics.

**Resolution:** Complete implementation at `api.rs:281-298` returning all metrics.

**Files:** `api.rs`, `server.rs`

---

### 2.5 OCR in Frame Response ✅ RESOLVED

**Description:** Frame endpoint should include OCR data when available.

**Resolution:** OCR data fetched and included at `api.rs:166-191`.

**Files:** `api.rs`

---

### 2.6 Web UI OCR Display ❌ OPEN (CRITICAL)

**Description:** OCR text not displaying in web interface due to property mismatch.

**Current Issues:**
- Frontend expects `frame.ocr_text` (array) at `app.js:236`
- API returns `frame.ocr` (object)
- Frontend tries to map array: `frame.ocr_text.map(ocr => ocr.text)`
- Stats polling uses `stats.processed` but API returns `frames_with_ocr`

**Key Learning:** API structure changed but frontend wasn't updated to match.

**Recommended Fix:**
- Update `app.js` to read from `frame.ocr` instead of `frame.ocr_text`
- Access `frame.ocr.text` directly instead of mapping array
- Update stats references to use correct field names

**Files:** `app.js`

---

### 2.7 Search Interface ✅ RESOLVED

**Description:** Web UI needed search functionality.

**Resolution:** Complete search section in `index.html:109-116` with JavaScript handler.

**Files:** `index.html`, `app.js`

---

### 2.8 Progress Widget ✅ RESOLVED

**Description:** Web UI needed OCR progress visualization.

**Resolution:** Progress bar at `index.html:95-107` with 5-second polling.

**Files:** `index.html`, `app.js`

---

## 3. FFmpeg & Capture Component

### 3.1 FFmpeg Subprocess Deadlocks ✅ RESOLVED

**Description:** FFmpeg processes could deadlock on Windows due to improper stream handling.

**Resolution:**
- Proper stdin closure before `child.wait_with_output()` at `encoder.rs:186-215`
- Stderr piped (not null) allowing concurrent reading
- EOF signaled correctly to FFmpeg

**Files:** `encoder.rs`

---

### 3.2 std::process vs tokio::process ⚠️ PARTIAL

**Description:** Synchronous process commands should use async tokio equivalents.

**Current Issues:**
- `encoder.rs` uses `std::process::Command` at lines 136, 225, 256, 300
- `indexer.rs` uses `std::process::Command` at lines 225, 254
- Blocking calls like `cmd.output()` and `child.wait()`

**Key Learning:** Documentation claimed async migration was complete, but code still uses synchronous std::process.

**Recommendation:** Migrate to `tokio::process::Command` for non-blocking I/O.

**Files:** `encoder.rs`, `indexer.rs`

---

### 3.3 Stderr Piping ✅ RESOLVED

**Description:** FFmpeg errors weren't captured for debugging.

**Resolution:**
- `.stderr(Stdio::piped())` at `encoder.rs:161`
- Comprehensive error handling with NVENC failure detection
- Fallback to software encoding at `encoder.rs:282-290`

**Files:** `encoder.rs`

---

### 3.4 Async Subprocess Handling ❌ OPEN

**Description:** Subprocess operations block the tokio runtime.

**Current Issues:**
- `cmd.output()?` is synchronous blocking at `encoder.rs:279, 310`
- `child.wait()` blocks at `indexer.rs:248`
- Async wrapper functions hide synchronous internals

**Key Learning:** Async function signatures don't guarantee async execution - the underlying I/O must also be async.

**Recommended Fix:**
- Replace `std::process::Command` with `tokio::process::Command`
- Use `.await` on process operations
- Consider `spawn_blocking` for CPU-bound FFmpeg operations

**Files:** `encoder.rs`, `indexer.rs`

---

### 3.5 Concurrent Frame Processing ❌ OPEN

**Description:** Frames processed sequentially despite async context.

**Current Issues:**
- Sequential loop at `indexer.rs:164-181`: `for frame in &frames { ... }`
- `processor.rs:47-56` also uses sequential loop
- No use of `futures::join_all()`, `tokio::spawn()`, or `select!`

**Key Learning:** Documentation claims "concurrent frame processing with futures::join_all" but this is NOT implemented.

**Recommended Fix:**
- Use `futures::future::join_all()` for parallel frame extraction
- Limit concurrency with `futures::stream::buffer_unordered()`
- Consider rayon for CPU-bound OCR processing

**Files:** `indexer.rs`, `processor.rs`

---

### 3.6 SoftwareBitmap PNG Roundtrip ❌ OPEN

**Description:** Inefficient PNG encode/decode cycle for every frame.

**Current Issues at `processor.rs:80-130`:**
1. Create ImageBuffer from RGBA
2. Encode to PNG bytes (CPU-intensive compression)
3. Write PNG to Windows stream
4. Decode PNG back to SoftwareBitmap (decompression)

**Key Learning:** This is a workaround for windows-rs 0.58 lacking direct buffer access. Documented as deferred due to Windows API complexity.

**Recommendation:**
- Investigate windows-rs updates for direct buffer creation
- Consider using BMP format (no compression overhead)
- Profile to confirm this is actually a bottleneck

**Files:** `processor.rs`

---

## 4. Key Learnings from Failed Attempts

### Documentation vs Implementation Gap
Several optimizations documented as "completed" were not actually present in the codebase:
- Concurrent frame processing (claimed Phase 3, not implemented)
- Async subprocess handling (claimed Phase 5, still synchronous)
- Video metadata caching (mentioned but no implementation)

**Lesson:** Verify implementation in code, not just documentation.

### Windows-Specific Challenges
- FFmpeg subprocess handling requires careful stream management
- Windows OCR API doesn't provide confidence scores (heuristics needed)
- SoftwareBitmap creation limited by windows-rs API surface

### Performance Optimization Priority
Based on impact analysis:
1. **Frame deduplication** - Highest impact, reduces workload by ~90%
2. **Video metadata caching** - Easy win, eliminates subprocess overhead
3. **Concurrent processing** - Multiplies throughput with available cores
4. **PNG roundtrip optimization** - Complex, verify bottleneck first

---

## 5. Recommended Implementation Order

### Phase A: Critical Bug Fixes
1. **Fix Web UI OCR display** - Update `app.js` property references
2. **Add FTS5 query sanitization** - Prevent user-facing errors

### Phase B: Quick Performance Wins
3. **Video metadata caching** - Add fields to schema, cache on load
4. **Multi-language configuration** - Pass language to indexer

### Phase C: Major Performance Improvements
5. **Frame deduplication** - Add hash calculation and filtering
6. **Concurrent frame processing** - Implement `join_all` pattern
7. **Async subprocess handling** - Migrate to tokio::process

### Phase D: Advanced Optimizations (Verify Need First)
8. **PNG roundtrip optimization** - Profile before implementing
9. **Enhanced metrics** - Time-series data, per-operation timing

---

## Appendix: Files Reference

### Modified in Analysis
| File | Component | Issues Found |
|------|-----------|--------------|
| `src/memoire-core/src/indexer.rs` | OCR | Sequential processing, ffprobe per frame |
| `src/memoire-ocr/src/processor.rs` | OCR | PNG roundtrip, sequential loop |
| `src/memoire-ocr/src/engine.rs` | OCR | Language support exists |
| `src/memoire-db/src/queries.rs` | DB | Missing dedup queries |
| `src/memoire-db/src/schema.rs` | DB | Missing hash fields, video dimensions |
| `src/memoire-web/src/routes/api.rs` | API | FTS5 sanitization needed |
| `src/memoire-web/static/app.js` | Frontend | Property mismatch (CRITICAL) |
| `src/memoire-processing/src/encoder.rs` | FFmpeg | std::process, sync I/O |

### Source Documents Analyzed
- `docs/failed_and_reverted/ocr_implementation_changes_summary.md` (23KB)
- `docs/failed_and_reverted/OCR_ANALYSIS_REPORT.md` (8.8KB)
- `docs/failed_and_reverted/API_OCR_EXTENSIONS.md` (5.6KB)
- `docs/failed_and_reverted/OCR_QUERIES_IMPLEMENTATION.md` (6.9KB)
- `docs/failed_and_reverted/TESTING_OCR_API.md` (3.5KB)
