//! Capture error types

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CaptureError {
    #[error("windows error: {0}")]
    Windows(#[from] windows::core::Error),

    #[error("no monitors found")]
    NoMonitors,

    #[error("monitor not found: {0}")]
    MonitorNotFound(String),

    #[error("capture not initialized")]
    NotInitialized,

    #[error("frame acquisition failed: {0}")]
    FrameAcquisition(String),

    #[error("timeout waiting for frame")]
    Timeout,

    #[error("access denied - ensure running with appropriate permissions")]
    AccessDenied,

    #[error("device removed or reset")]
    DeviceRemoved,

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),
}
