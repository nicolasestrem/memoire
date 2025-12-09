# Memoire User Guide

**Version:** Phase 2 (OCR + Full-Text Search)
**Last Updated:** December 2025

## Table of Contents

1. [Introduction](#introduction)
2. [Requirements](#requirements)
3. [Installation](#installation)
4. [Getting Started](#getting-started)
5. [CLI Commands Reference](#cli-commands-reference)
6. [Web Viewer Guide](#web-viewer-guide)
7. [Configuration](#configuration)
8. [Troubleshooting](#troubleshooting)
9. [Data Management](#data-management)

---

## Introduction

### What is Memoire?

Memoire is a Windows desktop application that continuously captures your screen and extracts text using Optical Character Recognition (OCR). It creates a searchable local database of everything visible on your monitors, making it easy to find and review past activities.

**Key Features:**
- Continuous screen recording at customizable frame rates (default: 1 FPS)
- Hardware-accelerated video encoding (NVENC) with software fallback (x264)
- Windows OCR API integration for text extraction
- Full-text search across all captured content
- Web-based validation viewer for playback and search
- Multi-monitor support

### Privacy Benefits

Unlike cloud-based screen monitoring tools, Memoire keeps all your data **100% local**:
- All videos stored on your machine (`%LOCALAPPDATA%\Memoire`)
- SQLite database with no external connections
- No telemetry, analytics, or cloud uploads
- Complete control over your data retention

**Perfect for:**
- Personal productivity tracking
- Time tracking and activity logging
- Finding that webpage you saw three days ago
- Building a searchable visual memory

---

## Requirements

### Operating System
- **Windows 10 or 11** (64-bit)
- Administrator privileges for initial setup

### Software Dependencies
- **Rust 1.75+** - Required for building from source
  - Install from: https://rustup.rs
- **FFmpeg** - Required for video encoding
  - Must be in your system PATH
  - Download from: https://ffmpeg.org/download.html

### Hardware
- **Minimum:**
  - 4GB RAM
  - 10GB free disk space (for initial usage)
  - Dual-core CPU

- **Recommended:**
  - 8GB+ RAM
  - SSD with 100GB+ free space
  - NVIDIA GPU with NVENC support (GTX 600+ series)
  - Multi-core CPU

### Storage Considerations

Recording at 1 FPS (default) generates approximately:
- **1-2 GB per hour** of recording (with NVENC)
- **2-4 GB per hour** (software encoding)

Plan disk space accordingly. A typical 8-hour workday generates 8-16GB of video data.

---

## Installation

### Step 1: Install Rust

1. Download and run the Rust installer from https://rustup.rs
2. Follow the installation wizard
3. Restart your terminal/command prompt
4. Verify installation:
   ```cmd
   rustc --version
   ```

### Step 2: Install FFmpeg

**Option A: Using Chocolatey (Recommended)**
```cmd
choco install ffmpeg
```

**Option B: Manual Installation**
1. Download FFmpeg from https://ffmpeg.org/download.html
2. Extract to a folder (e.g., `C:\ffmpeg`)
3. Add `C:\ffmpeg\bin` to your system PATH:
   - Open System Properties → Environment Variables
   - Edit the `Path` variable
   - Add the FFmpeg bin folder path
4. Verify:
   ```cmd
   ffmpeg -version
   ```

### Step 3: Build Memoire

```bash
# Clone the repository
git clone https://github.com/nicolasestrem/memoire
cd memoire

# Build release binary (optimized)
cargo build --release

# Binary will be at: target\release\memoire.exe
```

Build time: 5-15 minutes depending on your CPU.

### Step 4: Verify Installation

```cmd
# Check dependencies
.\target\release\memoire.exe check
```

Expected output:
```
checking dependencies...

  ffmpeg: OK
  nvenc:  OK
  monitors: 2 found

all checks passed!
```

If NVENC is not available (no NVIDIA GPU), you'll see:
```
  nvenc:  not available (will use software encoding)
```
This is fine - software encoding will be used automatically.

---

## Getting Started

### Quick Start: 5-Minute Workflow

1. **Verify system readiness:**
   ```cmd
   memoire check
   ```

2. **List your monitors:**
   ```cmd
   memoire monitors
   ```

3. **Start recording in one terminal:**
   ```cmd
   memoire record
   ```
   Press `Ctrl+C` to stop recording.

4. **Start OCR indexing in another terminal:**
   ```cmd
   memoire index
   ```
   This processes captured frames and extracts text.

5. **Open the web viewer in a third terminal:**
   ```cmd
   memoire viewer
   ```
   Open browser to: http://localhost:8080

6. **Search from command line:**
   ```cmd
   memoire search "your search term"
   ```

### Recommended Workflow

For continuous use, run three processes simultaneously:

**Terminal 1 - Recording:**
```cmd
memoire tray
```
Runs in system tray with minimize support.

**Terminal 2 - OCR Indexing:**
```cmd
memoire index --ocr-fps 10
```
Processes 10 frames per second from captured videos.

**Terminal 3 - Web Viewer (optional):**
```cmd
memoire viewer --port 8080
```
Access anytime at http://localhost:8080

---

## CLI Commands Reference

### `memoire record`

Start screen capture and video encoding.

**Usage:**
```cmd
memoire record [OPTIONS]
```

**Options:**
| Option | Description | Default |
|--------|-------------|---------|
| `--fps <FPS>` | Recording framerate (frames per second) | 1 |
| `--data-dir <PATH>` | Custom data directory | `%LOCALAPPDATA%\Memoire` |
| `--no-hw` | Disable hardware encoding (use x264) | Off (NVENC enabled) |

**Examples:**
```cmd
# Basic recording (1 FPS, default location)
memoire record

# High-quality recording (5 FPS)
memoire record --fps 5

# Custom data directory
memoire record --data-dir D:\MemoireData

# Force software encoding
memoire record --no-hw

# High FPS + custom location
memoire record --fps 2 --data-dir C:\MyRecordings
```

**When to use:**
- First-time testing
- Short recording sessions
- When you want direct console output

**Note:** Press `Ctrl+C` to stop recording gracefully.

---

### `memoire tray`

Run recorder in system tray mode (recommended for daily use).

**Usage:**
```cmd
memoire tray [OPTIONS]
```

**Options:**
Same as `memoire record`:
- `--fps <FPS>`
- `--data-dir <PATH>`
- `--no-hw`

**Examples:**
```cmd
# Run in tray with default settings
memoire tray

# Tray with 2 FPS recording
memoire tray --fps 2
```

**Features:**
- Minimizes to system tray
- Shows recording status icon
- Right-click menu for quick controls
- Graceful shutdown on exit

**Recommended for:**
- All-day recording sessions
- Background operation
- Production usage

---

### `memoire index`

Run OCR indexer to extract text from captured frames.

**Usage:**
```cmd
memoire index [OPTIONS]
```

**Options:**
| Option | Description | Default |
|--------|-------------|---------|
| `--data-dir <PATH>` | Data directory to index | `%LOCALAPPDATA%\Memoire` |
| `--ocr-fps <FPS>` | OCR processing rate | 10 |
| `--ocr-language <LANG>` | OCR language (BCP47 code) | `en-US` |

**Examples:**
```cmd
# Basic indexing (10 frames/sec, English)
memoire index

# Faster indexing (20 frames/sec)
memoire index --ocr-fps 20

# Slower indexing (lower CPU usage)
memoire index --ocr-fps 5

# French OCR
memoire index --ocr-language fr-FR

# Japanese OCR
memoire index --ocr-language ja-JP

# Custom data directory
memoire index --data-dir D:\MemoireData
```

**Supported Languages:**
Windows OCR supports 40+ languages. Common BCP47 codes:
- `en-US` - English (United States)
- `en-GB` - English (United Kingdom)
- `fr-FR` - French
- `de-DE` - German
- `es-ES` - Spanish
- `ja-JP` - Japanese
- `zh-CN` - Chinese (Simplified)
- `ko-KR` - Korean

**Performance Notes:**
- OCR FPS = 10: ~5-10% CPU usage (recommended)
- OCR FPS = 20: ~15-20% CPU usage (fast indexing)
- OCR FPS = 5: ~2-5% CPU usage (background mode)

**When to run:**
- Continuously alongside recording
- As a background task after recording sessions
- During idle time to catch up on backlog

---

### `memoire viewer`

Launch web-based validation viewer for playback and search.

**Usage:**
```cmd
memoire viewer [OPTIONS]
```

**Options:**
| Option | Description | Default |
|--------|-------------|---------|
| `--port <PORT>` | Web server port | 8080 |
| `--data-dir <PATH>` | Data directory | `%LOCALAPPDATA%\Memoire` |

**Examples:**
```cmd
# Start viewer on default port 8080
memoire viewer

# Custom port
memoire viewer --port 3000

# Custom data directory
memoire viewer --data-dir D:\MemoireData
```

**Access:**
Open your browser to: `http://localhost:<PORT>`

**Features:**
- Browse all video chunks
- Frame-by-frame navigation
- Full-text search with highlighting
- OCR text display with confidence scores
- Video playback with seek controls
- Responsive design (works on mobile)

---

### `memoire search`

Search OCR text from the command line.

**Usage:**
```cmd
memoire search "query" [OPTIONS]
```

**Options:**
| Option | Description | Default |
|--------|-------------|---------|
| `--limit <NUM>` | Maximum results to show | 10 |
| `--data-dir <PATH>` | Data directory | `%LOCALAPPDATA%\Memoire` |

**Examples:**
```cmd
# Basic search
memoire search "meeting notes"

# Limit to 5 results
memoire search "password" --limit 5

# Search in custom data directory
memoire search "invoice" --data-dir D:\MemoireData

# Search with quotes for exact phrases
memoire search "quarterly report Q4"
```

**Search Syntax:**
Powered by SQLite FTS5, supports:
- **Simple terms:** `meeting`
- **Phrases:** `"exact phrase"`
- **AND operator:** `meeting AND notes`
- **OR operator:** `meeting OR conference`
- **NOT operator:** `meeting NOT zoom`
- **Prefix matching:** `meet*` (matches meeting, meetings, etc.)

**Output Format:**
```
found 3 result(s):

1. Frame ID: 12847
   Timestamp: 2025-12-09 14:23:15
   Device: Monitor 1
   Text: Meeting notes from quarterly review session...
   Confidence: 92.45%

2. Frame ID: 13012
   ...
```

---

### `memoire status`

Show recording system status and statistics.

**Usage:**
```cmd
memoire status
```

**Example Output:**
```
status: ready
database: C:\Users\YourName\AppData\Local\Memoire\memoire.db
total frames: 45,281
frames with OCR: 12,456
OCR progress: 27.5% (32,825 pending)
latest chunk: videos\2025-12-09\chunk_14-30-00_1.mp4
recorded at: 2025-12-09 14:35:12
```

**Use Cases:**
- Check if system is initialized
- Monitor OCR indexing progress
- Verify database location
- See latest recording timestamp

---

### `memoire monitors`

List all available display monitors.

**Usage:**
```cmd
memoire monitors
```

**Example Output:**
```
found 2 monitor(s):

  [0] Generic PnP Monitor - 2560x1440 (primary)
  [1] Dell U2415 - 1920x1200
```

**Use Cases:**
- Verify monitor detection before recording
- Check resolution and names
- Identify primary display

**Note:** All monitors are captured simultaneously during recording.

---

### `memoire check`

Verify all dependencies and system readiness.

**Usage:**
```cmd
memoire check
```

**Example Output:**
```
checking dependencies...

  ffmpeg: OK
  nvenc:  OK
  monitors: 2 found

all checks passed!
```

**Checks Performed:**
1. FFmpeg installed and in PATH
2. NVENC hardware encoding support
3. Monitor enumeration

**Troubleshooting:**
If checks fail, see [Troubleshooting](#troubleshooting) section.

---

## Web Viewer Guide

### Accessing the Viewer

1. Start the viewer:
   ```cmd
   memoire viewer
   ```

2. Open browser: http://localhost:8080

3. Alternative ports:
   ```cmd
   memoire viewer --port 3000
   ```

### Interface Overview

The web viewer has three main sections:

#### 1. Statistics Dashboard
- **Total Frames:** Number of captured frames
- **Frames with OCR:** Number of indexed frames
- **Video Chunks:** Number of 5-minute video segments
- **OCR Progress:** Indexing completion percentage

#### 2. Search Interface
- **Search Bar:** Enter search queries
- **Search Button:** Execute search
- **Results List:** Displays matching frames with:
  - Timestamp
  - Device/monitor name
  - Text snippet with highlighting
  - OCR confidence score
  - Link to view full frame

#### 3. Video Chunk Browser
- **Chunk List:** All recorded video segments
- **Filters:** By date, device, time range
- **Playback:** Click to view video
- **Frame Navigator:** Scrub through individual frames

### Using Search

**Basic Search:**
1. Type your search term in the search box
2. Press Enter or click Search
3. Results appear below with snippets

**Advanced Search:**
```
# Exact phrase
"quarterly report"

# Multiple terms (AND)
meeting notes agenda

# Either term (OR)
invoice OR receipt

# Exclude terms (NOT)
meeting NOT zoom

# Prefix matching
meet* (finds meeting, meetings, meet, etc.)
```

**Viewing Results:**
1. Click on a result
2. Opens frame detail view with:
   - Full OCR text
   - Confidence scores per text region
   - Bounding boxes overlay
   - Timestamp and metadata
   - Link to source video chunk

### Video Playback

**Features:**
- HTML5 video player with range request support
- Seek to any timestamp
- Playback speed controls (0.5x - 2x)
- Fullscreen mode
- Keyboard shortcuts:
  - `Space` - Play/Pause
  - `←/→` - Skip 5 seconds
  - `↑/↓` - Volume control
  - `F` - Fullscreen

### Frame Navigation

**Frame-by-Frame View:**
1. Click "View Frames" on a video chunk
2. Use arrow buttons or keyboard:
   - `N` - Next frame
   - `P` - Previous frame
   - `Home` - First frame
   - `End` - Last frame

**OCR Overlay:**
- Toggle "Show OCR Boxes" to see detected text regions
- Color-coded by confidence:
  - Green: >90%
  - Yellow: 70-90%
  - Red: <70%

---

## Configuration

### Data Directory Structure

Default location: `C:\Users\<YourName>\AppData\Local\Memoire\`

```
Memoire/
├── memoire.db              # SQLite database
├── memoire.db-wal          # Write-Ahead Log (concurrent access)
├── memoire.db-shm          # Shared memory file
└── videos/                 # Video storage
    └── YYYY-MM-DD/         # Date-based folders
        └── chunk_HH-MM-SS_N.mp4
```

### Custom Data Directory

**Set via command line:**
```cmd
memoire record --data-dir D:\MemoireData
memoire index --data-dir D:\MemoireData
memoire viewer --data-dir D:\MemoireData
```

**Important:** All commands must use the same data directory!

**Create config file (optional):**
Save to `config.toml`:
```toml
data_dir = "D:\\MemoireData"
fps = 2
use_hw_encoding = true
chunk_duration_secs = 300
```

Then use:
```cmd
memoire --config config.toml record
```

### Performance Tuning

**Recording FPS:**
- `--fps 0.5` - 1 frame every 2 seconds (minimal storage)
- `--fps 1` - Default, good balance
- `--fps 2` - Higher quality, 2x storage
- `--fps 5` - Very high quality, 5x storage

**OCR Processing Rate:**
- `--ocr-fps 5` - Light CPU usage
- `--ocr-fps 10` - Recommended
- `--ocr-fps 20` - Fast indexing, higher CPU
- `--ocr-fps 30` - Maximum speed

**Encoding Settings:**
- NVENC (hardware): ~50% faster, better quality
- x264 (software): Works without NVIDIA GPU

**Database Optimization:**
SQLite automatically optimizes, but you can:
```sql
-- Compact database (run periodically)
VACUUM;

-- Analyze for query optimization
ANALYZE;
```

### Multi-Language OCR

Configure OCR language for better accuracy:

```cmd
# English (default)
memoire index --ocr-language en-US

# French
memoire index --ocr-language fr-FR

# German
memoire index --ocr-language de-DE

# Japanese
memoire index --ocr-language ja-JP

# Chinese (Simplified)
memoire index --ocr-language zh-CN
```

**Note:** Language packs must be installed in Windows Settings → Time & Language → Language.

---

## Troubleshooting

### FFmpeg Not Found

**Symptom:**
```
ffmpeg not found in PATH - please install FFmpeg
```

**Solution:**
1. Install FFmpeg (see [Installation](#installation))
2. Verify it's in PATH:
   ```cmd
   ffmpeg -version
   ```
3. Restart terminal/command prompt
4. If still failing, add FFmpeg bin folder to system PATH manually

---

### NVENC Not Available

**Symptom:**
```
nvenc: not available (will use software encoding)
```

**This is not an error!** It means:
- No NVIDIA GPU detected, or
- GPU doesn't support NVENC (GTX 600+ required)

**Solution:**
- Use software encoding (automatic fallback)
- Or upgrade to NVIDIA GPU with NVENC support

---

### Database Not Found

**Symptom:**
```
database not found at C:\Users\...\Memoire\memoire.db
please run 'memoire record' first to initialize the database
```

**Solution:**
1. Run `memoire record` first to create database
2. Let it record for at least 30 seconds
3. Stop with `Ctrl+C`
4. Now run `memoire viewer` or `memoire search`

---

### No Search Results

**Symptom:**
Search returns "no results found" despite having recordings.

**Possible Causes:**
1. OCR indexing not run yet
2. OCR indexing still in progress
3. No text detected in frames

**Solution:**
1. Check OCR status:
   ```cmd
   memoire status
   ```
2. Run OCR indexer:
   ```cmd
   memoire index
   ```
3. Wait for indexing to complete (check status periodically)
4. Retry search

---

### High CPU Usage

**Symptom:**
CPU usage at 50%+ during recording or indexing.

**Solutions:**

**For Recording:**
```cmd
# Reduce FPS
memoire record --fps 0.5

# Use hardware encoding
memoire record  # (default, uses NVENC if available)
```

**For Indexing:**
```cmd
# Reduce OCR FPS
memoire index --ocr-fps 5
```

---

### High Disk Usage

**Symptom:**
Disk filling up quickly.

**Causes:**
- High recording FPS
- Long recording sessions
- Software encoding (larger files)

**Solutions:**

1. **Reduce FPS:**
   ```cmd
   memoire record --fps 0.5
   ```

2. **Use Hardware Encoding:**
   - NVENC produces smaller files than x264
   - Enable by default (remove `--no-hw` flag)

3. **Regular Cleanup:**
   - Delete old video chunks manually
   - Database will automatically mark them as missing
   - Or write cleanup script:
   ```cmd
   # Delete videos older than 30 days
   forfiles /p "%LOCALAPPDATA%\Memoire\videos" /s /m *.mp4 /d -30 /c "cmd /c del @path"
   ```

---

### Port Already in Use

**Symptom:**
```
Error: address already in use
```

**Solution:**
```cmd
# Use different port
memoire viewer --port 8081

# Or kill process using port 8080
netstat -ano | findstr :8080
taskkill /PID <PID> /F
```

---

### OCR Language Not Available

**Symptom:**
OCR fails or produces poor results for non-English text.

**Solution:**
1. Open Windows Settings
2. Go to Time & Language → Language
3. Click "Add a language"
4. Search for your language (e.g., "French")
5. Install language pack
6. Wait for download to complete
7. Restart indexer with language code:
   ```cmd
   memoire index --ocr-language fr-FR
   ```

---

## Data Management

### Backup Strategy

**What to Backup:**
1. **Database:** `memoire.db`
2. **Videos:** `videos/` folder (optional, large)

**Recommended Backup:**
```cmd
# Backup database only (small)
copy "%LOCALAPPDATA%\Memoire\memoire.db" "D:\Backups\memoire-backup-%DATE%.db"

# Full backup (database + videos)
robocopy "%LOCALAPPDATA%\Memoire" "D:\Backups\Memoire" /MIR
```

**Backup Frequency:**
- Database: Daily (automated script)
- Videos: Weekly or as needed

### Storage Cleanup

**Delete Old Videos:**
```cmd
# Delete videos older than 30 days
forfiles /p "%LOCALAPPDATA%\Memoire\videos" /s /m *.mp4 /d -30 /c "cmd /c del @path"
```

**Vacuum Database:**
```sql
-- Compact database after deletions
sqlite3 memoire.db "VACUUM;"
```

### Data Retention Policy

Recommended approach:
1. Keep recent 7 days: Full recordings
2. Days 8-30: Daily snapshots (delete most chunks)
3. Older than 30 days: Delete all unless important

**Example Script:**
```bash
# Keep last 7 days fully
# Delete 90% of chunks from 8-30 days ago
# Delete everything older than 30 days
```

### Export Data

**Export Search Results:**
```cmd
# Search and save to file
memoire search "meeting" --limit 100 > meetings.txt
```

**Export Database Schema:**
```cmd
sqlite3 memoire.db .schema > schema.sql
```

**Export Frames as Images:**
Use FFmpeg:
```cmd
ffmpeg -i chunk_14-30-00_1.mp4 -vf fps=1 frame_%04d.png
```

---

## Best Practices

### Daily Workflow

**Morning Startup:**
```cmd
# Terminal 1 - Start recording in tray
memoire tray

# Terminal 2 - Start OCR indexing
memoire index --ocr-fps 10
```

**During Day:**
- Recorder runs in background
- Use web viewer for quick searches: http://localhost:8080

**End of Day:**
1. Check status:
   ```cmd
   memoire status
   ```
2. Stop indexer (`Ctrl+C`)
3. Exit tray app (right-click → Exit)

### Search Tips

**Finding Specific Content:**
```cmd
# Exact document name
memoire search "\"Q4 Financial Report\""

# App-specific content (search by window name)
memoire search "Chrome email inbox"

# Time-based searches (via web viewer filters)
# Search → Filter by date range
```

**Improving Search Accuracy:**
- Let indexer run continuously for best coverage
- Use specific terms rather than generic words
- Combine multiple terms: `report Q4 finance`

### Performance Optimization

**Low-End Systems:**
```cmd
# Recording
memoire record --fps 0.5 --no-hw

# Indexing (low priority)
memoire index --ocr-fps 3
```

**High-End Systems:**
```cmd
# Recording
memoire record --fps 2

# Indexing (fast)
memoire index --ocr-fps 20
```

---

## FAQ

**Q: How much disk space do I need?**
A: At 1 FPS, expect 1-2GB per hour. A typical 8-hour workday = 8-16GB.

**Q: Can I record only one monitor?**
A: Currently, all monitors are recorded. Single-monitor selection is planned for Phase 6.

**Q: Is my data encrypted?**
A: Not by default. Use Windows BitLocker or third-party encryption for the data directory.

**Q: Can I run this on a VM?**
A: Yes, but NVENC may not be available. Use `--no-hw` flag.

**Q: What happens if I close the lid?**
A: Recording pauses when display turns off, resumes when active.

**Q: Can I search by date/time?**
A: Use the web viewer's filter controls. CLI search coming in Phase 5.

**Q: Does this record audio?**
A: Not yet. Audio capture planned for Phase 3.

**Q: Can I use this on macOS or Linux?**
A: Not currently. Windows-only (uses DXGI and Windows.Media.Ocr).

---

## Support & Resources

**Documentation:**
- Architecture Guide: `docs/Architecture.md`
- API Reference: `docs/api/`
- Security Policies: `docs/SECURITY.md`

**Development:**
- GitHub: https://github.com/nicolasestrem/memoire
- Build from source: See [Installation](#installation)
- Report bugs: GitHub Issues

**Community:**
- Discussions: GitHub Discussions
- Feature requests: GitHub Issues

---

## Appendix: Technical Details

### Database Schema

**Tables:**
- `video_chunks` - Video file metadata
- `frames` - Frame timestamps, app/window context
- `ocr_text` - Extracted text with bounding boxes
- `ocr_text_fts` - FTS5 full-text search index

**Query Pattern:**
```sql
-- Time-filtered search
SELECT * FROM ocr_text_fts
WHERE ocr_text_fts MATCH 'query'
  AND julianday(timestamp) >= julianday('now','-7 days')
LIMIT 10;
```

### API Endpoints

REST API (when viewer is running):

```
GET  /api/stats              # Database statistics
GET  /api/stats/ocr          # OCR indexing progress
GET  /api/chunks             # List video chunks
GET  /api/frames?chunk_id=N  # Frames for a chunk
GET  /api/frames/:id         # Single frame with OCR
GET  /api/search?q=text      # Full-text search
GET  /video/:filename        # MP4 streaming
```

### Performance Metrics

**Targets (Phase 2):**
- OCR latency: <500ms per frame
- Search latency: <100ms
- CPU usage (idle): <5%
- Memory usage: <600MB

**Actual (typical):**
- OCR: 200-400ms per frame
- Search: 20-50ms
- CPU: 2-4% idle, 8-12% recording+indexing
- Memory: 400-500MB

---

**End of User Guide**

*Last updated: December 2025 | Phase 2 Complete*
