# Memoire

Windows screen & audio capture with OCR and speech-to-text, creating a searchable local database of everything you see and hear.

## Features (Phase 1)

- DXGI Desktop Duplication screen capture
- MP4 video encoding (NVENC hardware or x264 software)
- SQLite database with FTS5 full-text search
- CLI for recording control

## Requirements

- Windows 10/11 (64-bit)
- Rust 1.75+ (install from https://rustup.rs)
- FFmpeg in PATH (download from https://ffmpeg.org/download.html)
- NVIDIA GPU with NVENC support (optional, for hardware encoding)

## Build

```bash
# Clone and build
git clone https://github.com/nicolasestrem/memoire
cd memoire
cargo build --release

# Binary will be at target/release/memoire.exe
```

## Usage

```bash
# Check dependencies
memoire check

# List available monitors
memoire monitors

# Start recording (primary monitor, 1 FPS)
memoire record

# Start with custom settings
memoire record --fps 2 --data-dir C:\MyData\Memoire

# Use software encoding (if no NVIDIA GPU)
memoire record --no-hw

# Check status
memoire status
```

## Data Storage

Default location: `%LOCALAPPDATA%\Memoire\`

```
Memoire/
├── memoire.db           # SQLite database
├── videos/              # MP4 video chunks (5-min segments)
│   └── YYYY-MM-DD/
│       └── chunk_HH-MM-SS_N.mp4
└── logs/                # Application logs
```

## Project Structure

```
src/
├── memoire-capture/     # Screen capture (DXGI)
├── memoire-processing/  # OCR, STT, video encoding
├── memoire-db/          # SQLite database layer
└── memoire-core/        # CLI application
```

## Development

```bash
# Run with verbose logging
memoire -v record

# Run tests
cargo test

# Check code
cargo clippy
```

## Roadmap

- [x] Phase 1: Screen capture + video encoding + SQLite
- [ ] Phase 1.5: Validation viewer
- [ ] Phase 2: OCR + FTS5 search
- [ ] Phase 3: Audio + STT
- [ ] Phase 4: C# supervisor + REST API
- [ ] Phase 5: Web dashboard
- [ ] Phase 6: Advanced features
- [ ] Phase 7: Ecosystem integration

## License

MIT
