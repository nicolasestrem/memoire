//! memoire-processing - Video and audio processing for Memoire
//!
//! Handles video encoding and audio chunk management.

pub mod encoder;
pub mod audio_encoder;

pub use encoder::VideoEncoder;
pub use audio_encoder::{AudioEncoder, AudioEncoderConfig};
