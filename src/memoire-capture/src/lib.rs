//! memoire-capture - Screen and audio capture for Memoire
//!
//! Provides DXGI Desktop Duplication for screen capture
//! and WASAPI for audio capture.

pub mod screen;
pub mod monitor;
pub mod error;

pub use screen::ScreenCapture;
pub use monitor::{Monitor, MonitorInfo};
pub use error::CaptureError;
