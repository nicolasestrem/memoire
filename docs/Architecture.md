# Memoire - System Architecture

## Overview

Memoire is a Windows desktop application that continuously captures screen video, system audio, and microphone input, performing real-time OCR and speech-to-text transcription to build a searchable local database.

## High-Level Architecture

```
                         MEMOIRE ARCHITECTURE
    ┌──────────────────────────────────────────────────────────┐
    │                                                          │
    │  ┌─────────────────────────────────────────────────────┐ │
    │  │   C# SUPERVISOR (ASP.NET Core 8)                    │ │
    │  │   • Spawns/monitors Rust child process              │ │
    │  │   • System tray, autostart registry                 │ │
    │  │   • Restarts Rust on crash, alerts user             │ │
    │  │   • REST API, SignalR, OpenAPI                      │ │
    │  │   • SQLite reads                                    │ │
    │  └──────────────────────┬──────────────────────────────┘ │
    │                         │ spawn/monitor                  │
    │                         ▼                                │
    │  ┌─────────────────────────────────────────────────────┐ │
    │  │   RUST CORE (Child Process)                         │ │
    │  │   • DXGI Screen Capture                             │ │
    │  │   • WASAPI Audio (loopback + mic)                   │ │
    │  │   • Parakeet STT, Windows OCR                       │ │
    │  │   • SQLite writes                                   │ │
    │  │   • stdout/stderr → C# for logging                  │ │
    │  └─────────────────────────────────────────────────────┘ │
    │                                                          │
    │  ┌─────────────────────────────────────────────────────┐ │
    │  │   WEB DASHBOARD (React)                             │ │
    │  │   • Timeline view, Search, Settings                 │ │
    │  └─────────────────────────────────────────────────────┘ │
    │                                                          │
    └──────────────────────────────────────────────────────────┘
                              │
                    ┌─────────▼─────────┐
                    │  SQLite + FTS5    │
                    │  + MP4 chunks     │
                    │  (~30GB/month)    │
                    └───────────────────┘
```

---

## Process Lifecycle Model

### C# as Supervisor (Parent Process)

- Spawns `memoire-core.exe` as child process on startup
- Captures stdout/stderr for logging and health monitoring
- Monitors process health, restarts on crash with exponential backoff
- Handles Windows integration: system tray icon, autostart registry keys
- Graceful shutdown: sends SIGTERM to Rust, waits, then SIGKILL

### Rust as Worker (Child Process)

- Pure capture/processing engine, no Windows UI integration
- Communicates state via stdout (structured JSON logs)
- Named pipe IPC for commands (start/stop/config changes)
- Exits cleanly on pipe disconnect or SIGTERM

---

## Component Breakdown

### 1. Rust Capture Core

**Crate: `memoire-capture`**
| File | Responsibility |
|------|----------------|
| `screen.rs` | DXGI Desktop Duplication for all monitors |
| `audio.rs` | WASAPI loopback (system audio) + microphone |
| `device.rs` | Device enumeration and hot-plug detection |
| `monitor.rs` | Monitor management |

**Crate: `memoire-processing`**
| File | Responsibility |
|------|----------------|
| `ocr.rs` | Windows OCR API integration via `windows` crate |
| `stt.rs` | Parakeet TDT integration via NeMo/ONNX |
| `dedup.rs` | Frame deduplication (perceptual hash + SSIM) |
| `embedding.rs` | Vector embeddings for semantic search (future) |
| `indexer.rs` | Background FTS5 indexing worker (async from capture) |

**Crate: `memoire-db`**
| File | Responsibility |
|------|----------------|
| `schema.rs` | Table definitions |
| `migrations/` | SQL migrations |
| `queries.rs` | Common queries |

**Crate: `memoire-core`**
| File | Responsibility |
|------|----------------|
| `main.rs` | Entry point, orchestration |
| `config.rs` | Configuration management |
| `ipc.rs` | Named pipes for C# communication |

### 2. C# API Layer

**Project: `Memoire.Api` (ASP.NET Core 8)**
| File | Responsibility |
|------|----------------|
| `Controllers/SearchController.cs` | Full-text and semantic search |
| `Controllers/TimelineController.cs` | Time-range queries |
| `Controllers/HealthController.cs` | System status |
| `Controllers/SettingsController.cs` | Configuration CRUD |
| `Hubs/RealtimeHub.cs` | SignalR for live updates |
| `Services/RustBridgeService.cs` | IPC with Rust core |

### 3. Web Dashboard

**Project: `Memoire.Web` (React + TypeScript)**
- Dashboard - Recording status, resource usage
- Timeline - Scrollable with video preview
- Search - Full-text and filters
- Settings - Capture config, AI providers

---

## Database Schema

```sql
-- Video chunks (5-minute MP4 segments)
CREATE TABLE video_chunks (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL,
    device_name TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Frames with metadata
CREATE TABLE frames (
    id INTEGER PRIMARY KEY,
    video_chunk_id INTEGER NOT NULL,
    offset_index INTEGER NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    app_name TEXT,
    window_name TEXT,
    browser_url TEXT,
    focused BOOLEAN DEFAULT FALSE,
    FOREIGN KEY (video_chunk_id) REFERENCES video_chunks(id)
);

-- OCR extracted text
CREATE TABLE ocr_text (
    id INTEGER PRIMARY KEY,
    frame_id INTEGER NOT NULL,
    text TEXT NOT NULL,
    text_json TEXT,  -- Bounding boxes
    confidence REAL,
    FOREIGN KEY (frame_id) REFERENCES frames(id)
);

-- Audio chunks (30-second segments)
CREATE TABLE audio_chunks (
    id INTEGER PRIMARY KEY,
    file_path TEXT NOT NULL,
    device_name TEXT,
    is_input_device BOOLEAN,
    timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Audio transcriptions
CREATE TABLE audio_transcriptions (
    id INTEGER PRIMARY KEY,
    audio_chunk_id INTEGER NOT NULL,
    transcription TEXT NOT NULL,
    timestamp TIMESTAMP NOT NULL,
    speaker_id INTEGER,
    start_time REAL,
    end_time REAL,
    FOREIGN KEY (audio_chunk_id) REFERENCES audio_chunks(id)
);

-- FTS5 full-text search tables
CREATE VIRTUAL TABLE ocr_text_fts USING fts5(text, content='ocr_text', content_rowid='id');
CREATE VIRTUAL TABLE audio_fts USING fts5(transcription, content='audio_transcriptions', content_rowid='id');

-- Indexes
CREATE INDEX idx_frames_timestamp ON frames(timestamp);
CREATE INDEX idx_audio_timestamp ON audio_transcriptions(timestamp);
```

---

## API Endpoints (Screenpipe-Compatible)

Memoire implements a Screenpipe-compatible API for interoperability with existing tools and LLM integrations.

### Core Search

```
GET /search
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `q` | string | Full-text search query |
| `content_type` | enum | `all`, `ocr`, `audio`, `ocr+audio` |
| `start_time` | ISO 8601 | Filter start timestamp |
| `end_time` | ISO 8601 | Filter end timestamp |
| `app_name` | string | Filter by application name |
| `window_name` | string | Filter by window title |
| `browser_url` | string | Filter by browser URL |
| `focused` | boolean | Only focused windows |
| `speaker_ids` | int[] | Filter by speaker IDs |
| `min_length` | int | Minimum text length |
| `max_length` | int | Maximum text length |
| `include_frames` | boolean | Include base64 frame images |
| `limit` | int | Results per page (default: 20) |
| `offset` | int | Pagination offset |

**Response:**
```json
{
  "results": [
    {
      "type": "OCR",
      "content": {
        "frame_id": 123,
        "text": "extracted text...",
        "timestamp": "2024-01-15T10:30:00Z",
        "file_path": "videos/2024-01-15/chunk_01.mp4",
        "offset_index": 450,
        "app_name": "Chrome",
        "window_name": "GitHub - Pull Request",
        "browser_url": "https://github.com/...",
        "focused": true,
        "tags": ["work", "code-review"]
      }
    },
    {
      "type": "Audio",
      "content": {
        "transcription_id": 456,
        "transcription": "spoken text...",
        "timestamp": "2024-01-15T10:30:05Z",
        "file_path": "audio/2024-01-15/chunk_01.wav",
        "speaker_id": 1,
        "speaker_name": "User",
        "start_time": 12.5,
        "end_time": 15.2
      }
    }
  ],
  "total": 1542
}
```

### Device Management

```
GET /health                    # System status
GET /audio/list                # List audio devices with status
GET /vision/list               # List monitors with resolution/status
POST /audio/device/start       # Start specific audio device
POST /audio/device/stop        # Stop specific audio device
```

### Frame Access

```
GET /frames/{frame_id}         # Get frame metadata + optional image
GET /frames/{frame_id}/video   # Stream video at frame timestamp
```

### Tagging System

```
POST /tags/{content_type}/{id}    # Add tags (content_type: ocr, audio)
DELETE /tags/{content_type}/{id}  # Remove tags
Body: { "tags": ["tag1", "tag2"] }
```

### Speaker Management

```
GET /speakers/search?name=        # Search speakers by name
GET /speakers/unnamed             # List unnamed speakers
POST /speakers/update             # Update speaker name
POST /speakers/merge              # Merge duplicate speakers
POST /speakers/delete             # Delete speaker
```

### Memoire Extensions

```
GET /timeline?start=&end=&resolution=    # Aggregated timeline view
POST /settings                           # Update configuration
GET /settings                            # Get current configuration
WS /ws/realtime                          # Live OCR/transcription stream (SignalR)
POST /raw_sql                            # Execute raw SQL (dev/debug only)
```

---

## Technology Stack

| Component | Technology | Justification |
|-----------|------------|---------------|
| Screen Capture | DXGI Desktop Duplication | Fastest on Windows, hardware-accelerated |
| Audio Capture | WASAPI via `cpal` | Low-latency, loopback support |
| OCR | `windows` crate (Windows.Media.Ocr) | Official Microsoft crate, built-in, fast |
| STT | Parakeet TDT 0.6B | 3386x RTFx, 6% WER, CUDA support |
| Database | `rusqlite` + FTS5 | Mature SQLite bindings, bundled feature |
| API | ASP.NET Core 8 | Mature, performant, excellent tooling |
| Web UI | React + TypeScript | Rich ecosystem, component libraries |
| Real-time | SignalR | Built into ASP.NET, WebSocket abstraction |
| Video Encoding | FFmpeg (NVENC) | Hardware-accelerated encoding |
| IPC | `interprocess` crate | Cross-platform named pipes |

---

## Key Implementation Notes

### Architecture Decisions

1. **Use channels over mutexes** for async data flow (`tokio::sync::mpsc`)
2. **WAL mode for SQLite** to enable concurrent read/write
3. **Async FTS5 indexing** - capture thread writes raw text, background `indexer.rs` worker handles FTS5 updates to keep capture loop tight
4. **File-based media storage** - store MP4/audio files on filesystem, only paths and metadata in SQLite (prevents WAL bloat)

### Rust Crate Selection

- `rusqlite` with `bundled` feature for SQLite
- `windows` (official Microsoft) for DXGI/OCR/WASAPI
- `interprocess` for cross-platform named pipes IPC
- `cpal` for audio device abstraction
- `tokio` for async runtime

### Processing Strategy

- **Perceptual hashing** for frame deduplication (skip <0.6% change)
- **30-second audio chunks** with 2-second overlap for transcription continuity
- **5-minute video chunks** for easy seeking and cleanup
- **NVENC encoding** on GTX 1650 for hardware-accelerated MP4

---

## Phase 1 Implementation Details (Current)

### Multi-Monitor Recording Pipeline

The current Rust implementation provides multi-monitor screen capture with system tray control:

```
┌────────────────────────────────────────────────────────────────────┐
│                        RECORDING PIPELINE                          │
├────────────────────────────────────────────────────────────────────┤
│                                                                    │
│  1. Monitor Enumeration                                            │
│     Monitor::enumerate_all() -> Vec<MonitorInfo>                   │
│                                                                    │
│  2. Per-Monitor Capture Loop (MonitorRecorder)                     │
│     ScreenCapture::capture_frame() -> CapturedFrame                │
│     │                                                              │
│     ├─> Bounds validation (row_pitch, buffer size, null check)     │
│     └─> BGRA to RGBA conversion                                    │
│                                                                    │
│  3. Piped FFmpeg Encoding                                          │
│     VideoEncoder::add_frame() -> Writes raw RGBA to FFmpeg stdin   │
│     │                                                              │
│     ├─> NVENC (h264_nvenc) - hardware accelerated                  │
│     └─> libx264 fallback on NVENC failure                          │
│                                                                    │
│  4. Frame Metadata Buffering                                       │
│     MonitorRecorder.pending_frames (up to 30 frames)               │
│     Flush every 30 frames OR 5 seconds                             │
│                                                                    │
│  5. Batch Database Insert                                          │
│     insert_frames_batch() - single transaction                     │
│                                                                    │
│  6. Chunk Finalization (every 5 minutes)                           │
│     Close FFmpeg stdin -> Wait for MP4 -> Start new chunk          │
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
```

### Threading Model

```
Main Thread (Tao Event Loop)
│
├── Menu Event Handler Thread
│   └── Receives tray menu events, toggles AtomicBool flags
│
├── Recorder Thread (when recording)
│   ├── Capture loop for all monitors
│   └── Spawns FFmpeg child processes for encoding
│
└── State Monitor Thread
    └── Watches is_recording flag, signals recorder to stop
```

### Performance Optimizations

| Optimization | Implementation | Impact |
|-------------|----------------|--------|
| Piped FFmpeg | Raw RGBA to stdin | ~2x I/O reduction (no PNG intermediate) |
| Batch DB writes | 30 frames/transaction | ~30x fewer SQLite transactions |
| Hardware encoding | NVENC with libx264 fallback | GPU offload, CPU free |
| Frame buffering | 5-second flush interval | Reduced DB write frequency |

### Security Hardening

| Issue | Fix | Location |
|-------|-----|----------|
| Unsafe memory access | Bounds validation, null checks, overflow protection | `screen.rs:162-218` |
| Panic-inducing unwrap | Match expression with error recovery | `recorder.rs:81-90` |
| Path traversal | Sanitize monitor names, block `..` | `recorder.rs:319-366` |
| Windows reserved names | Detect and prefix CON, PRN, NUL, etc. | `recorder.rs:320-324` |
| Filename length | Truncate to 100 characters | `recorder.rs:357` |

### Error Recovery

- **Capture errors**: After 10 consecutive failures, reinitialize monitor capture
- **NVENC failures**: Automatic fallback to libx264 software encoding
- **Database flush**: Guaranteed flush before chunk finalization
- **Graceful shutdown**: 30-second timeout waiting for recorder thread

---

## Data Storage Location

```
%LOCALAPPDATA%\Memoire\
├── memoire.db           # SQLite database
├── videos/              # MP4 video chunks
│   └── YYYY-MM-DD/
├── audio/               # Audio chunks
│   └── YYYY-MM-DD/
├── config.json          # User configuration
└── logs/                # Application logs
```

---

## Project Structure

```
Memoire/
├── src/
│   ├── memoire-capture/       # Rust: Screen + Audio capture
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── screen.rs
│   │       ├── audio.rs
│   │       ├── device.rs
│   │       └── monitor.rs
│   │
│   ├── memoire-processing/    # Rust: OCR + STT
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── ocr.rs
│   │       ├── stt.rs
│   │       ├── dedup.rs
│   │       └── indexer.rs
│   │
│   ├── memoire-db/            # Rust: Database
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── schema.rs
│   │       └── migrations/
│   │
│   ├── memoire-core/          # Rust: Main binary
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── main.rs
│   │       ├── config.rs
│   │       └── ipc.rs
│   │
│   ├── Memoire.Api/           # C#: ASP.NET Core API
│   │   ├── Memoire.Api.csproj
│   │   ├── Program.cs
│   │   ├── Controllers/
│   │   ├── Services/
│   │   └── Hubs/
│   │
│   └── Memoire.Web/           # React Web UI
│       ├── package.json
│       └── src/
│
├── Cargo.toml                 # Rust workspace
├── Memoire.sln                # .NET solution
└── README.md
```
