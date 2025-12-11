//! memoire-stt - Speech-to-Text for Memoire
//!
//! Provides speech-to-text transcription using Parakeet TDT via ONNX Runtime.
//! Supports GPU acceleration via CUDA with CPU fallback.

mod download;
mod engine;
mod error;
mod mel;
mod tokenizer;

pub use download::{ModelDownloader, ORT_DLL_NAME};
pub use engine::{SttEngine, SttConfig, TranscriptionResult, TranscriptionSegment};
pub use mel::{MelSpectrogram, ENCODER_FRAME_DURATION_SEC, SAMPLE_RATE};
pub use tokenizer::Tokenizer;
pub use error::SttError;

use std::path::Path;

/// Get the default model directory
pub fn default_model_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Memoire")
        .join("models")
}

/// Configure ONNX Runtime to use the bundled DLL instead of system-installed version.
///
/// This MUST be called before creating any `SttEngine` instance.
/// The `ort` crate 2.0.0-rc.10 requires ONNX Runtime 1.22.x, but the system may
/// have an older version (e.g., 1.17.1 from screenpipe or other applications).
///
/// Returns `Ok(())` if the DLL path was set, or an error if the DLL doesn't exist.
pub fn configure_onnx_runtime(model_dir: &Path) -> anyhow::Result<()> {
    let dll_path = model_dir.join(ORT_DLL_NAME);

    if !dll_path.exists() {
        return Err(anyhow::anyhow!(
            "ONNX Runtime DLL not found at {:?}. Run 'memoire download-models' first.",
            dll_path
        ));
    }

    // Set the ORT_DYLIB_PATH environment variable before ort initializes
    std::env::set_var("ORT_DYLIB_PATH", &dll_path);
    tracing::info!("configured ONNX Runtime path: {:?}", dll_path);

    Ok(())
}

/// Check if the bundled ONNX Runtime DLL exists
pub fn has_bundled_onnx_runtime(model_dir: &Path) -> bool {
    model_dir.join(ORT_DLL_NAME).exists()
}
