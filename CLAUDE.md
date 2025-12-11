# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Memoire is a Windows desktop application for continuous screen/audio capture with OCR and speech-to-text, creating a searchable local database. It's a privacy-first alternative to cloud-based monitoring tools.

**Current state:** Phase 2 complete (OCR + full-text search). Phase 3 (audio + STT) is next.

## Build Commands

```bash
# Build release
cargo build --release

# Run CLI (after build)
./target/release/memoire.exe <COMMAND>

# Or run directly via cargo
cargo run --bin memoire -- <COMMAND>

# Check workspace compiles
cargo check --workspace

# Run tests
cargo test

# Lint
cargo clippy
```

## CLI Commands

```bash
memoire record [--fps 1] [--data-dir PATH] [--no-hw]  # Start capture
memoire tray [--fps 1] [--data-dir PATH] [--no-hw]    # Run in system tray
memoire viewer [--port 8080] [--data-dir PATH]        # Web UI
memoire index [--data-dir PATH] [--ocr-fps 10]        # Background OCR indexer
memoire search "query" [--limit 10]                   # FTS5 search
memoire status                                        # Show status
memoire monitors                                      # List displays
memoire check                                         # Verify dependencies
```

## Unified Testing Command

For development and testing, use the `test-all` command to run all components simultaneously:

```bash
# Run with default test configuration
memoire test-all

# Use a specific profile (quick, full, stress)
memoire test-all --profile quick

# Override data directory
memoire test-all --data-dir my-test-data

# Use custom config file
memoire test-all --config custom-test.toml
```

### Configuration File

The `test-config.toml` file (project root) defines test parameters:

```toml
[general]
data_dir = "test-data"
auto_download_models = true

[record]
fps = 0.25  # 1 frame every 4 seconds for fast testing

[index]
ocr_fps = 10

[audio]
enabled = true

[viewer]
port = 8080

# Profiles for different test scenarios
[profiles.quick]
record = { fps = 0.1 }
index = { ocr_fps = 5 }

[profiles.full]
record = { fps = 1.0 }
index = { ocr_fps = 15 }
```

### Components Started

The orchestrator manages:
1. **Recorder** - Captures screens at configured FPS
2. **OCR Indexer** - Extracts text from frames
3. **Audio Indexer** - Transcribes audio (if enabled)
4. **Web Viewer** - Serves UI on http://localhost:8080

All components shut down gracefully with Ctrl+C.

---

2Do: Update with

PS C:\Users\nicol\Desktop\Memoire> C:\Users\nicol\Desktop\Memoire\target\release\memoire.exe
Screen & audio capture with OCR and speech-to-text

Usage: memoire.exe [OPTIONS] <COMMAND>

Commands:
  record           Start recording
  tray             Run in system tray mode
  status           Show system status
  monitors         List monitors
  check            Check dependencies (FFmpeg, etc.)
  viewer           Start validation viewer web interface
  index            Run OCR indexer on captured frames
  search           Search OCR text
  audio-devices    List available audio devices
  record-audio     Record audio only (for testing audio capture)
  audio-index      Run audio transcription indexer
  download-models  Download Parakeet TDT speech-to-text models
  help             Print this message or the help of the given subcommand(s)

Options:
  -c, --config <CONFIG>  Configuration file path
  -v, --verbose          Enable verbose logging
  -h, --help             Print help
  -V, --version          Print version
PS C:\Users\nicol\Desktop\Memoire># Next Steps

## Phase 3: Audio Capture and Speech-to-Text

1. **Audio Capture**:
   - Implement WASAPI capture for system audio
   - Add audio device selection UI
   - Implement audio chunking (5-minute WAV files)

2. **Speech-to-Text**:
   - Integrate Parakeet TDT models
   - Add language selection for STT
   - Implement audio transcription queue

3. **Database Enhancements**:
   - Add audio chunk metadata table
   - Add transcribed text table
   - Create full-text search index for audio transcriptions

4. **Web UI**:
   - Add audio visualization
   - Display transcribed text alongside OCR
   - Implement audio search functionality

## Performance Optimizations

1. **Audio Processing**:
   - Implement parallel audio chunk processing
   - Add audio frame deduplication
   - Optimize audio encoding (Opus/FLAC)

2. **STT Pipeline**:
   - Implement batch transcription
   - Add model caching
   - Optimize GPU utilization for STT

## Testing Plan

1. **Audio Capture**:
   - Test with multiple audio devices
   - Verify audio quality
   - Test with different audio formats

2. **Speech-to-Text**:
   - Test with various languages
   - Verify transcription accuracy
   - Test with different audio qualities

3. **Integration**:
   - Test audio-OCR synchronization
   - Verify search functionality across both modalities
   - Test system performance with audio capture enabled

## Timeline

- Audio capture implementation: 2 weeks
- Speech-to-text integration: 3 weeks
- Testing and optimization: 2 weeks
- Documentation: 1 week


## Architecture

Six-crate Rust workspace with layered architecture: Capture → Processing → Storage → Web

```
src/
├── memoire-capture/     # DXGI Desktop Duplication, monitor enumeration
├── memoire-processing/  # FFmpeg video encoding (NVENC/x264)
├── memoire-ocr/         # Windows.Media.Ocr API wrapper
├── memoire-db/          # SQLite + FTS5, schema, migrations, queries
├── memoire-core/        # CLI entry point, recorder orchestration, tray UI, indexer
└── memoire-web/         # Axum REST API, video streaming, static web UI
```

### Data Flow

1. **Capture**: DXGI grabs frames from all monitors
2. **Encode**: Frames piped to FFmpeg → 5-minute MP4 chunks
3. **Store**: Frame metadata → SQLite, videos → filesystem
4. **Index**: Background worker extracts OCR text → FTS5 tables
5. **Query**: Web API serves search results + video streaming

### Key Patterns

- **Channels over mutexes**: Uses `tokio::sync::mpsc` for data flow between components
- **WAL mode**: SQLite with Write-Ahead Log for concurrent read/write
- **Async indexing**: OCR runs in separate tokio task, doesn't block capture
- **Batch inserts**: 30 frames per transaction for efficiency

## Database Schema

SQLite at `%LOCALAPPDATA%\Memoire\memoire.db`:

- `video_chunks` - MP4 file metadata
- `frames` - Frame timestamps, app/window names, browser URL
- `ocr_text` - Extracted text with bounding boxes and confidence
- `ocr_text_fts` - FTS5 virtual table (auto-synced via triggers)

**Query pattern**: Always filter by time first with `julianday()`, use FTS5 tables for text search, join back for metadata.

## REST API Endpoints

```
GET  /api/stats              # Database statistics
GET  /api/stats/ocr          # OCR indexing progress
GET  /api/chunks             # List video chunks
GET  /api/frames?chunk_id=N  # Frames for a chunk
GET  /api/frames/:id         # Single frame with OCR
GET  /api/search?q=text      # Full-text search
GET  /video/:filename        # MP4 streaming with range support
```

## Requirements

- Windows 10/11 64-bit
- Rust 1.75+
- FFmpeg in PATH
- NVIDIA GPU with NVENC (optional, falls back to x264)

## Coding Conventions

- Use `anyhow` for error handling in applications, `thiserror` for libraries
- Prefer `tokio` for async, channels over locks
- Keep files under 600 lines
- Structured logging with `tracing`

## Performance Targets

| Metric | Target |
|--------|--------|
| OCR latency | <500ms/frame |
| Search latency | <100ms |
| CPU usage (idle) | <5% |
| Memory | <600MB |

## Phased Roadmap

- [x] Phase 1: Screen capture + video encoding + SQLite
- [x] Phase 1.5: Web-based validation viewer
- [x] Phase 2: OCR + FTS5 search
- [ ] Phase 3: Audio capture + speech-to-text (Parakeet TDT)
- [ ] Phase 4: C# supervisor + REST API wrapper
- [ ] Phase 5: React dashboard
- [ ] Phase 6: Multi-monitor optimization, semantic search
- [ ] Phase 7: LLM integrations, Obsidian plugins
- Always make sure the project build after your changes and the following parameters can run: memoire.exe record memoire.exe index memoire.exe viewer