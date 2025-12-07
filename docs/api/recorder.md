# Recorder API

## Overview

The `Recorder` module (`memoire-core/src/recorder.rs`) orchestrates multi-monitor screen capture, coordinating screen capture, video encoding, and database writes.

## Architecture

```
Recorder
├── Config
├── Database
└── MonitorRecorder[] (one per monitor)
    ├── MonitorInfo
    ├── ScreenCapture
    ├── VideoEncoder
    ├── pending_frames[]
    └── current_chunk_id
```

## Configuration

```rust
pub struct Config {
    /// Data directory for videos and database
    pub data_dir: PathBuf,

    /// Recording framerate (default: 1 FPS)
    pub fps: u32,

    /// Use hardware encoding - NVENC (default: true)
    pub use_hw_encoding: bool,

    /// Video chunk duration in seconds (default: 300 = 5 minutes)
    pub chunk_duration_secs: u64,
}
```

## Usage

### Basic Recording

```rust
use memoire_core::{recorder::Recorder, config::Config};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Create recorder with default config
let config = Config::default();
let mut recorder = Recorder::new(config)?;

// Running flag for graceful shutdown
let running = Arc::new(AtomicBool::new(true));
let running_clone = running.clone();

// Stop on Ctrl+C
ctrlc::set_handler(move || {
    running_clone.store(false, Ordering::SeqCst);
})?;

// Run recording loop (blocks until running=false)
recorder.run(running)?;
```

### Integration with System Tray

```rust
// In tray menu handler
fn handle_start_stop(state: &Arc<RecordingState>, config: &Config) {
    if state.is_recording.load(Ordering::SeqCst) {
        // Stop recording
        state.is_recording.store(false, Ordering::SeqCst);
    } else {
        // Start recording in background thread
        state.is_recording.store(true, Ordering::SeqCst);
        let state_clone = state.clone();
        let config_clone = config.clone();

        thread::spawn(move || {
            let mut recorder = Recorder::new(config_clone).unwrap();
            let running = Arc::new(AtomicBool::new(true));

            // Monitor state flag
            let running_clone = running.clone();
            thread::spawn(move || {
                while running_clone.load(Ordering::SeqCst) {
                    if !state_clone.is_recording.load(Ordering::SeqCst) {
                        running_clone.store(false, Ordering::SeqCst);
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
            });

            recorder.run(running).unwrap();
        });
    }
}
```

## Per-Monitor Recording

Each `MonitorRecorder` handles one display:

```rust
struct MonitorRecorder {
    info: MonitorInfo,           // Display name, dimensions
    capture: ScreenCapture,      // DXGI capture instance
    encoder: VideoEncoder,       // FFmpeg encoder
    current_chunk_id: Option<i64>,
    frame_index: i64,
    chunk_index: u64,
    consecutive_errors: u32,
    pending_frames: Vec<NewFrame>,
    last_db_flush: Instant,
}
```

### Frame Capture Flow

```
1. capture_frame()
   ├── ScreenCapture::capture_frame() -> CapturedFrame
   ├── Ensure current_chunk_id exists
   ├── Buffer frame metadata in pending_frames
   ├── VideoEncoder::add_frame() -> Write to FFmpeg
   └── Flush to DB if batch full (30 frames) or timeout (5s)

2. flush_frames()
   └── insert_frames_batch() -> Single DB transaction

3. finalize_chunk()
   ├── flush_frames()
   ├── VideoEncoder::finalize_chunk() -> Close MP4
   └── Reset current_chunk_id
```

## Database Integration

### Frame Metadata

```rust
let new_frame = NewFrame {
    video_chunk_id: chunk_id,
    offset_index: self.frame_index,
    timestamp: frame.timestamp,
    app_name: None,       // Future: Active window detection
    window_name: None,
    browser_url: None,
    focused: true,
};
```

### Batch Inserts

Frames are buffered and flushed in batches:

```rust
const FRAME_BATCH_SIZE: usize = 30;
const FRAME_FLUSH_INTERVAL: Duration = Duration::from_secs(5);

// Flush when batch full OR timeout reached
if pending_frames.len() >= FRAME_BATCH_SIZE
    || last_db_flush.elapsed() >= FRAME_FLUSH_INTERVAL
{
    flush_frames(db)?;
}
```

## Error Recovery

### Consecutive Capture Errors

After 10 consecutive capture failures, the monitor is reinitialized:

```rust
const MAX_CONSECUTIVE_ERRORS: u32 = 10;

if monitor.consecutive_errors >= MAX_CONSECUTIVE_ERRORS {
    reinitialize_monitor(monitor, db)?;
}
```

### Monitor Reinitialization

```rust
fn reinitialize_monitor(monitor: &mut MonitorRecorder, db: &Database) -> Result<()> {
    // Finalize current chunk (flushes pending frames)
    monitor.finalize_chunk(db)?;

    // Re-create capture from same monitor info
    let new_monitor = Monitor::from_info(monitor.info.clone())?;
    monitor.capture = ScreenCapture::new(&new_monitor)?;
    monitor.consecutive_errors = 0;

    Ok(())
}
```

## Path Sanitization

Monitor names are sanitized for safe directory creation:

```rust
fn sanitize_monitor_name(name: &str) -> String {
    // 1. Replace invalid characters: \ / : * ? " < > |
    // 2. Remove path traversal: ..
    // 3. Block Windows reserved names: CON, PRN, AUX, NUL, COM1-9, LPT1-9
    // 4. Truncate to 100 characters
    // 5. Fallback to "monitor" if empty
}
```

**Example:**
- `\\.\DISPLAY1` → `DISPLAY1`
- `CON` → `_CON`
- `../../../etc` → `etc`

## File Structure

```
{data_dir}/
├── memoire.db
└── videos/
    ├── DISPLAY1/
    │   └── 2024-01-15/
    │       ├── chunk_10-30-00_0.mp4
    │       └── chunk_10-35-00_1.mp4
    └── DISPLAY2/
        └── 2024-01-15/
            └── chunk_10-30-00_0.mp4
```

## API Reference

### `Recorder::new(config: Config) -> Result<Self>`

Create a new recorder for all available monitors.

**Errors:**
- No monitors available
- Database open failed
- Directory creation failed

### `Recorder::run(running: Arc<AtomicBool>) -> Result<()>`

Run the recording loop until `running` is set to `false`.

**Behavior:**
1. Captures frames at configured FPS
2. Encodes to MP4 chunks
3. Writes metadata to database
4. Finalizes all chunks on shutdown

## Constants

```rust
const FRAME_BATCH_SIZE: usize = 30;           // Frames per DB batch
const FRAME_FLUSH_INTERVAL: Duration = 5s;    // Max time between flushes
const MAX_CONSECUTIVE_ERRORS: u32 = 10;       // Before reinitialization
```
