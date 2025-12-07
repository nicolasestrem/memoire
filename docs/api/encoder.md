# VideoEncoder API

## Overview

The `VideoEncoder` module (`memoire-processing/src/encoder.rs`) handles video encoding using FFmpeg with support for both piped (raw frame) and PNG-based encoding modes.

## Configuration

```rust
pub struct EncoderConfig {
    /// Output directory for video chunks
    pub output_dir: PathBuf,

    /// Chunk duration in seconds (default: 300 = 5 minutes)
    pub chunk_duration_secs: u64,

    /// Target framerate (default: 1 FPS)
    pub fps: u32,

    /// Use hardware encoding - NVENC (default: true)
    pub use_hw_encoding: bool,

    /// Video quality CRF value, lower = better (default: 23)
    pub quality: u32,

    /// Use piped encoding - raw frames to FFmpeg stdin (default: true)
    pub use_piped_encoding: bool,
}
```

## Usage

### Basic Usage

```rust
use memoire_processing::{VideoEncoder, encoder::EncoderConfig};

// Create encoder with piped encoding (recommended)
let config = EncoderConfig {
    output_dir: PathBuf::from("videos/monitor1"),
    chunk_duration_secs: 300,
    fps: 1,
    use_hw_encoding: true,
    quality: 23,
    use_piped_encoding: true,
};

let mut encoder = VideoEncoder::new(config)?;

// Add frames (RGBA format)
encoder.add_frame(&frame_data, width, height, timestamp)?;

// Finalize when chunk duration reached or stopping
let output_path = encoder.finalize_chunk()?;
```

### Integration with Recorder

```rust
// In MonitorRecorder::new()
let encoder_config = EncoderConfig {
    output_dir: monitor_dir,
    chunk_duration_secs: config.chunk_duration_secs,
    fps: config.fps,
    use_hw_encoding: config.use_hw_encoding,
    quality: 23,
    use_piped_encoding: true,
};
let encoder = VideoEncoder::new(encoder_config)?;
```

## Encoding Modes

### Piped Encoding (Default)

Raw RGBA frames are piped directly to FFmpeg's stdin:

```
Frame Data (RGBA) -> FFmpeg stdin -> H.264 MP4
```

**Advantages:**
- No intermediate files (2x I/O reduction)
- Lower disk wear
- Faster encoding

**FFmpeg command:**
```bash
ffmpeg -y -f rawvideo -pix_fmt rgba -s {width}x{height} -r {fps} -i - \
    -c:v h264_nvenc -preset p4 -rc vbr -cq {quality} \
    -pix_fmt yuv420p output.mp4
```

### PNG Fallback

Frames saved as PNG, then encoded:

```
Frame Data -> PNG files -> FFmpeg -> H.264 MP4 -> Delete PNGs
```

**Used when:**
- `use_piped_encoding: false`
- NVENC pipe fails mid-stream

## Hardware Encoding

### NVENC (Default)

```
-c:v h264_nvenc -preset p4 -rc vbr -cq {quality}
```

- Preset `p4`: Balanced speed/quality
- VBR mode with constant quality target
- Offloads encoding to GPU

### libx264 Fallback

```
-c:v libx264 -crf {quality} -preset fast
```

Automatically used when:
- No NVIDIA GPU available
- NVENC fails (driver issues)

## Output File Structure

```
videos/
└── {monitor_name}/
    └── {YYYY-MM-DD}/
        └── chunk_{HH-MM-SS}_{index}.mp4
```

Example: `videos/DISPLAY1/2024-01-15/chunk_10-30-00_0.mp4`

## API Reference

### `VideoEncoder::new(config: EncoderConfig) -> Result<Self>`

Create a new encoder instance.

### `VideoEncoder::add_frame(data: &[u8], width: u32, height: u32, timestamp: DateTime<Utc>) -> Result<()>`

Add a frame to the current chunk. Automatically finalizes when chunk duration is reached.

**Parameters:**
- `data`: Raw RGBA pixel data
- `width`, `height`: Frame dimensions
- `timestamp`: Frame capture timestamp

### `VideoEncoder::finalize_chunk() -> Result<Option<PathBuf>>`

Finalize the current chunk and return the output path.

Returns `None` if no frames were added.

### `VideoEncoder::output_dir() -> &Path`

Get the configured output directory.

## Error Handling

```rust
match encoder.add_frame(&data, width, height, timestamp) {
    Ok(()) => { /* Frame added */ }
    Err(e) => {
        // FFmpeg spawn failed, pipe broken, etc.
        warn!("Encoding error: {}", e);
    }
}
```

## Utility Functions

### `check_ffmpeg() -> bool`

Check if FFmpeg is available in PATH.

### `check_nvenc() -> bool`

Check if NVENC encoder is available.

## Implementation Notes

1. **Drop behavior**: Encoder finalizes any remaining frames when dropped
2. **Temp cleanup**: PNG fallback mode cleans up `_temp_frames` directory
3. **NVENC detection**: Checked via `ffmpeg -encoders | grep h264_nvenc`
