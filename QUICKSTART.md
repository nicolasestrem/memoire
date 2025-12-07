# Memoire - Quick Start Guide

## Prerequisites

- **Rust** (latest stable): https://rustup.rs
- **FFmpeg**: Must be in your system PATH
  - Windows: Download from https://ffmpeg.org/download.html
  - Add to PATH or place `ffmpeg.exe` in project directory

## Build

```bash
# Build the entire workspace
cargo build --release

# Check dependencies
cargo run --bin memoire -- check
```

The compiled binary will be at: `target/release/memoire.exe`

## Usage

### 1. Start Recording

```bash
# Record with default settings (1 FPS, hardware encoding)
memoire record

# Custom settings
memoire record --fps 1 --data-dir "D:\Memoire"

# Disable hardware encoding (use software x264)
memoire record --no-hw

# Run in system tray mode
memoire tray
```

**Recording Details**:
- **Framerate**: 1 FPS (default)
- **Chunk Duration**: 5 minutes per MP4 file
- **Data Location**: `%LOCALAPPDATA%\Memoire\` (Windows)
- **Database**: `memoire.db` (SQLite with WAL mode)
- **Video Files**: `videos/*.mp4`

**Stop Recording**: Press `Ctrl+C`

### 2. Validation Viewer (Phase 1.5)

After recording some data, launch the web viewer:

```bash
# Start viewer on default port 8080
memoire viewer

# Custom port
memoire viewer --port 3000

# Custom data directory
memoire viewer --data-dir "D:\Memoire"
```

Then open in browser: **http://localhost:8080**

**Viewer Features**:
- Browse recorded video chunks
- Play MP4 files with frame-accurate seeking
- Seek to specific frame ID
- Navigate frame-by-frame (Prev/Next)
- View frame metadata (app, window, URL, monitor)
- Validate timestamp accuracy (±50ms drift detection)

### 3. Other Commands

```bash
# Show system status
memoire status

# List available monitors
memoire monitors

# Check dependencies (FFmpeg, NVENC)
memoire check
```

## Development

```bash
# Run in debug mode with verbose logging
cargo run --bin memoire -- record --verbose

# Check compilation without building
cargo check --workspace

# Run tests
cargo test

# Build documentation
cargo doc --open
```

## Project Structure

```
Memoire/
├── src/
│   ├── memoire-core/       # CLI entry point
│   ├── memoire-capture/    # Screen capture (DXGI)
│   ├── memoire-processing/ # Video encoding (FFmpeg)
│   ├── memoire-db/         # SQLite database layer
│   └── memoire-web/        # Validation viewer (Axum + HTML/CSS/JS)
├── docs/
│   └── Master-Plan.md      # Full project roadmap
└── target/
    └── release/
        └── memoire.exe     # Compiled binary
```

## Data Directory Layout

```
%LOCALAPPDATA%\Memoire\
├── memoire.db              # SQLite database
├── memoire.db-wal          # Write-ahead log
├── memoire.db-shm          # Shared memory
└── videos/
    ├── chunk_2024-01-15_10-00-00.mp4
    ├── chunk_2024-01-15_10-05-00.mp4
    └── ...
```

## Troubleshooting

**"FFmpeg not found"**
- Ensure FFmpeg is in your PATH: `ffmpeg -version`
- Or place `ffmpeg.exe` in the project root

**"NVENC not available"**
- NVIDIA GPU not detected or drivers outdated
- Will automatically fall back to software encoding (x264)

**"Database not found" (viewer)**
- Run `memoire record` first to create the database
- Or specify correct data directory with `--data-dir`

**Video won't play in viewer**
- Check browser console for errors (F12)
- Verify MP4 file exists in `videos/` directory
- Ensure database has frame metadata

## Next Steps

- **Phase 2**: OCR text extraction
- **Phase 3**: Speech-to-text
- **Phase 4**: Search API
- **Phase 5**: Production React dashboard

See `docs/Master-Plan.md` for complete roadmap.
