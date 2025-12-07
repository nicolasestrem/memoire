# Memoire - Master Plan

## Project Overview

**Memoire** is a Windows desktop application that continuously captures screen video, system audio, and microphone input, performing real-time OCR and speech-to-text transcription to build a searchable local database of everything the user sees and hears.

---

## Confirmed Requirements

| Aspect | Decision |
|--------|----------|
| **Architecture** | Hybrid: Rust capture/processing core + C# API/Web layer |
| **Target Hardware** | Mid-range (8GB RAM, GTX 1650+ with CUDA) |
| **Security** | Minimal initially (development focus) |
| **STT Engine** | NVIDIA Parakeet TDT 0.6B v2 (local) + cloud fallback |
| **OCR Engine** | Windows OCR API (local) + cloud fallback |
| **UI** | Headless background service + Web-based dashboard |
| **Integrations** | Comprehensive REST API for LLMs, productivity tools, developers |
| **API Compatibility** | Screenpipe-compatible endpoints for ecosystem interoperability |

---

## Performance Targets

| Metric | Target |
|--------|--------|
| CPU Usage (idle) | <5% |
| GPU Usage (idle) | <15% |
| Memory Usage | <600MB |
| Disk Usage | ~30GB/month |
| OCR Latency | <500ms/frame |
| STT Latency | <2s/30s audio |
| Search Latency | <100ms |

---

## Phased Delivery Plan

### Phase 1: Foundation (Capture Only)

**Goal:** Prove capture and storage pipeline works

| # | Task |
|---|------|
| 1 | Initialize Rust workspace with crates structure |
| 2 | Implement DXGI screen capture (single monitor, raw frames) |
| 3 | Set up SQLite database with basic schema (`rusqlite`) |
| 4 | MP4 video encoding with FFmpeg (NVENC) |
| 5 | Store video chunks to filesystem, metadata to SQLite |
| 6 | Create CLI to start/stop recording |

**Deliverable:** Rust CLI that captures screen → encodes MP4 → stores to SQLite

---

### Phase 1.5: Validation Viewer

**Goal:** Prove we can read and playback captured data correctly

| # | Task |
|---|------|
| 1 | Build throwaway WinForms/WPF app (temporary, not production) |
| 2 | Query SQLite for video chunks and frame timestamps |
| 3 | Play MP4 at specific timestamp using MediaElement or VLC.NET |
| 4 | Verify frame-accurate seeking (catch VFR sync issues early) |
| 5 | Display basic frame metadata (timestamp, file path) |

**Deliverable:** Simple viewer that proves capture pipeline is correct before adding complexity

**Why this phase?** If encoding produces VFR issues or timestamp drift, catch it now—not after building OCR, STT, and React dashboard.

---

### Phase 2: OCR + Basic Search

**Goal:** Add text extraction and searchability

| # | Task |
|---|------|
| 1 | Integrate Windows OCR API via `windows` crate |
| 2 | Store OCR text in `ocr_text` table |
| 3 | Implement background FTS5 indexer (`indexer.rs`) |
| 4 | Basic FTS5 search queries |
| 5 | Update viewer to display OCR text for frames |

**Deliverable:** Captured screens are OCR'd and searchable via FTS5

---

### Phase 3: Audio + Transcription

**Goal:** Full capture with speech-to-text

| # | Task |
|---|------|
| 1 | WASAPI audio capture (loopback + microphone) via `cpal` |
| 2 | Parakeet TDT integration with CUDA |
| 3 | Audio chunk storage (30s segments) |
| 4 | Transcription storage with timestamps |
| 5 | FTS5 indexing for transcriptions |

**Deliverable:** Full screen + audio capture with searchable transcriptions

---

### Phase 4: C# Supervisor + API

**Goal:** Production process management and REST API

| # | Task |
|---|------|
| 1 | C# ASP.NET Core project setup |
| 2 | Process spawning/monitoring for Rust binary |
| 3 | System tray icon with start/stop/status |
| 4 | REST API endpoints (search, timeline, health) |
| 5 | SignalR hub for real-time updates |
| 6 | Autostart registry integration |

**Deliverable:** C# supervisor that manages Rust, exposes REST API

---

### Phase 5: Web Dashboard

**Goal:** Visual interface

| # | Task |
|---|------|
| 1 | React project setup with TypeScript |
| 2 | Dashboard with status indicators |
| 3 | Timeline view with video preview |
| 4 | Search interface with filters |
| 5 | Settings management UI |

**Deliverable:** Web UI for browsing captured history

---

### Phase 6: Advanced Features

**Goal:** Production-ready

| # | Task |
|---|------|
| 1 | Multi-monitor support |
| 2 | Cloud AI fallback integration |
| 3 | Frame deduplication optimization |
| 4 | Vector embeddings for semantic search |
| 5 | Speaker diarization |
| 6 | Performance tuning (<5% CPU, <15% GPU idle) |

**Deliverable:** Full-featured system ready for daily use

---

### Phase 7: Polish & Extensions

**Goal:** Ecosystem integration

| # | Task |
|---|------|
| 1 | LLM chat interface integration (Open WebUI compatible) |
| 2 | Obsidian/Notion plugins |
| 3 | Data retention policies |
| 4 | Auto-update mechanism |
| 5 | Windows Service installation |

**Deliverable:** Complete ecosystem with third-party integrations

---

## Technology Decisions

### Why Hybrid Rust + C#?

| Component | Language | Rationale |
|-----------|----------|-----------|
| Capture Engine | Rust | Memory safety, low-level Windows APIs, performance |
| OCR/STT Processing | Rust | CUDA integration, async processing |
| REST API | C# | ASP.NET Core ecosystem, SignalR, OpenAPI tooling |
| System Tray/Windows Integration | C# | .NET has superior Windows desktop integration |
| Web Dashboard | React/TypeScript | Rich component ecosystem |

### Why Parakeet TDT over Whisper?

| Model | RTFx | WER | Parameters |
|-------|------|-----|------------|
| Parakeet TDT 0.6B v2 | 3386x | 6.05% | 600M |
| Whisper Large v3 | ~50x | 4.2% | 1.5B |

Parakeet is **68x faster** with acceptable accuracy tradeoff for real-time transcription.

### Why Windows OCR over Tesseract/PaddleOCR?

- Zero dependencies (built into Windows)
- Fast (hardware-accelerated)
- Good multilingual support
- Official Microsoft `windows` crate integration

---

## Risk Mitigation

| Risk | Mitigation |
|------|------------|
| VFR sync issues in MP4 | Phase 1.5 validation viewer catches early |
| Parakeet CUDA compatibility | Fallback to cloud STT (Deepgram/AssemblyAI) |
| SQLite WAL bloat | Media files on filesystem, only metadata in DB |
| FTS5 blocking capture | Async indexer on background thread |
| Rust process crashes | C# supervisor with auto-restart |

---

## Success Criteria

### MVP (Phase 4 Complete)

- [ ] Screen capture at 1 FPS with OCR
- [ ] Audio capture with transcription
- [ ] Full-text search via REST API
- [ ] System tray with start/stop
- [ ] <10% combined CPU+GPU usage idle

### Production Ready (Phase 6 Complete)

- [ ] Multi-monitor support
- [ ] <5% CPU, <15% GPU idle
- [ ] Semantic search via embeddings
- [ ] Speaker identification
- [ ] 99.9% uptime (auto-restart)

---

## Next Steps

1. Initialize git repository
2. Create `feature/phase-1-foundation` branch
3. Set up Rust workspace with initial crates
4. Implement DXGI screen capture
5. Iterate through Phase 1 tasks

---

## References

- [Screenpipe Architecture](https://docs.screenpi.pe/) - Inspiration for architecture patterns
- [DXGI Desktop Duplication](https://docs.microsoft.com/en-us/windows/win32/direct3ddxgi/desktop-dup-api) - Screen capture API
- [Parakeet TDT](https://catalog.ngc.nvidia.com/orgs/nvidia/teams/nemo/models/parakeet-tdt-0.6b) - STT model
- [Windows.Media.Ocr](https://docs.microsoft.com/en-us/uwp/api/windows.media.ocr) - OCR API
