# Screen Capture API

## Overview

The `memoire-capture` crate provides Windows screen capture using the DXGI Desktop Duplication API. It supports multi-monitor enumeration and efficient GPU-accelerated frame capture.

## Components

### Monitor Enumeration

```rust
use memoire_capture::{Monitor, MonitorInfo};

// Get all available monitors
let monitors: Vec<MonitorInfo> = Monitor::enumerate_all()?;

for info in &monitors {
    println!("{}: {}x{} primary={}",
        info.name, info.width, info.height, info.is_primary);
}

// Get the primary monitor
let primary = Monitor::get_primary()?;

// Create monitor from info
let monitor = Monitor::from_info(monitors[0].clone())?;
```

### MonitorInfo Structure

```rust
pub struct MonitorInfo {
    pub name: String,        // e.g., "\\.\DISPLAY1"
    pub width: u32,
    pub height: u32,
    pub adapter_index: u32,  // DXGI adapter index
    pub output_index: u32,   // DXGI output index
    pub is_primary: bool,    // Primary display flag
}
```

### Screen Capture

```rust
use memoire_capture::ScreenCapture;
use std::time::Duration;

// Create capture for a monitor
let mut capture = ScreenCapture::new(&monitor)?;

// Get dimensions
let (width, height) = capture.dimensions();

// Capture a frame (with timeout)
match capture.capture_frame(Duration::from_millis(100))? {
    Some(frame) => {
        // Frame captured successfully
        println!("Captured {}x{} at {}",
            frame.width, frame.height, frame.timestamp);

        // Access raw RGBA data
        let pixels: &[u8] = &frame.data;

        // Save as PNG
        frame.save_png("screenshot.png")?;

        // Convert to image buffer
        let img = frame.to_image();
    }
    None => {
        // No new frame (screen unchanged)
    }
}
```

### CapturedFrame Structure

```rust
pub struct CapturedFrame {
    pub data: Vec<u8>,              // RGBA pixel data
    pub width: u32,
    pub height: u32,
    pub timestamp: DateTime<Utc>,   // Capture timestamp
}

impl CapturedFrame {
    /// Convert to image buffer
    pub fn to_image(&self) -> ImageBuffer<Rgba<u8>, Vec<u8>>;

    /// Save as PNG file
    pub fn save_png(&self, path: &str) -> Result<()>;
}
```

## DXGI Desktop Duplication

### How It Works

```
┌─────────────┐    ┌──────────────┐    ┌─────────────┐
│   Desktop   │ -> │ DXGI Output  │ -> │  GPU Texture │
│  Compositor │    │  Duplication │    │  (Staging)   │
└─────────────┘    └──────────────┘    └─────────────┘
                                              │
                                              v
                                       ┌─────────────┐
                                       │  CPU Memory │
                                       │  (RGBA Vec) │
                                       └─────────────┘
```

1. **AcquireNextFrame**: Get desktop texture from DXGI
2. **CopyResource**: Copy GPU texture to staging texture
3. **Map**: Map staging texture to CPU-accessible memory
4. **Pixel Copy**: Convert BGRA → RGBA with bounds validation
5. **Unmap + ReleaseFrame**: Clean up resources

### Memory Safety

The pixel copy loop includes comprehensive bounds validation:

```rust
// Validate row pitch
let min_row_pitch = width.checked_mul(4)?;
if row_pitch < min_row_pitch {
    return Err(CaptureError::FrameAcquisition("invalid row_pitch"));
}

// Validate total buffer size
let total_size = width.checked_mul(height)?.checked_mul(4)?;

// Validate pointer
if mapped.pData.is_null() {
    return Err(CaptureError::FrameAcquisition("null pData"));
}

// Validate row offsets
for y in 0..height {
    let row_offset = y.checked_mul(row_pitch)?;
    // ... safe pixel access
}
```

## Error Handling

### CaptureError Types

```rust
pub enum CaptureError {
    NoMonitors,                    // No displays found
    NotInitialized,                // D3D11 device creation failed
    DeviceRemoved,                 // GPU disconnected / access lost
    FrameAcquisition(String),      // Frame capture failed
    Windows(windows::core::Error), // Windows API error
}
```

### Access Lost Recovery

When the desktop duplication loses access (e.g., UAC prompt, secure desktop):

```rust
match capture.capture_frame(timeout) {
    Err(e) if matches!(e.downcast_ref(), Some(CaptureError::DeviceRemoved)) => {
        // Reinitialize capture
        capture = ScreenCapture::new(&monitor)?;
    }
    Err(e) => return Err(e),
    Ok(frame) => { /* process frame */ }
}
```

## Performance Characteristics

| Metric | Value |
|--------|-------|
| Frame capture latency | ~5-10ms |
| Memory per frame | width × height × 4 bytes |
| GPU memory | One staging texture per capture |

### Optimization Tips

1. **Reuse ScreenCapture**: Don't recreate for each frame
2. **Staging texture caching**: Automatically reused
3. **Timeout tuning**: 100ms typical, 0 for immediate return if no change

## Integration Example

```rust
use memoire_capture::{Monitor, ScreenCapture};
use std::time::{Duration, Instant};

fn main() -> anyhow::Result<()> {
    let monitors = Monitor::enumerate_all()?;
    let mut captures: Vec<ScreenCapture> = monitors
        .iter()
        .filter_map(|info| {
            Monitor::from_info(info.clone())
                .ok()
                .and_then(|m| ScreenCapture::new(&m).ok())
        })
        .collect();

    let fps = 1;
    let frame_interval = Duration::from_secs_f64(1.0 / fps as f64);

    loop {
        let start = Instant::now();

        for (i, capture) in captures.iter_mut().enumerate() {
            if let Ok(Some(frame)) = capture.capture_frame(Duration::from_millis(100)) {
                println!("Monitor {}: {}x{}", i, frame.width, frame.height);
                // Process frame...
            }
        }

        let elapsed = start.elapsed();
        if elapsed < frame_interval {
            std::thread::sleep(frame_interval - elapsed);
        }
    }
}
```

## Platform Requirements

- Windows 8+ (Desktop Duplication API)
- DirectX 11 capable GPU
- Screen capture permissions (automatic on Windows)

## Limitations

- Cannot capture DRM-protected content
- UAC prompts cause temporary access loss
- Secure desktop (login screen) not accessible
