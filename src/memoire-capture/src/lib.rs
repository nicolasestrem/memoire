//! memoire-capture - Screen and audio capture for Memoire
//!
//! Provides DXGI Desktop Duplication for screen capture
//! and WASAPI for audio capture.

pub mod screen;
pub mod monitor;
pub mod error;
pub mod audio;

pub use screen::ScreenCapture;
pub use monitor::{Monitor, MonitorInfo};
pub use error::CaptureError;
pub use audio::{AudioCapture, AudioCaptureConfig, AudioDeviceInfo, CapturedAudio, save_wav, load_wav};
