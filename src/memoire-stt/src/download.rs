//! Model downloader for Parakeet TDT ONNX models
//!
//! Downloads pre-packaged int8 quantized models from HuggingFace.
//! Uses the csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8 repository.
//! Also downloads the required ONNX Runtime DLL (v1.22.x) for compatibility.

use anyhow::{Context, Result};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info, warn};

/// Base URL for HuggingFace model repository
const HF_BASE_URL: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/resolve/main";

/// GitHub releases URL for ONNX Runtime
const ORT_GITHUB_URL: &str = "https://github.com/microsoft/onnxruntime/releases/download/v1.22.0/onnxruntime-win-x64-1.22.0.zip";

/// Expected ONNX Runtime DLL name
pub const ORT_DLL_NAME: &str = "onnxruntime.dll";

/// Model files to download with their URLs and local names
/// Using sherpa-onnx int8 quantized models (~630 MB total)
const MODEL_FILES: &[(&str, &str, &str)] = &[
    // (remote_path, local_name, description)
    ("encoder.int8.onnx", "encoder.onnx", "Encoder model (~622 MB)"),
    ("decoder.int8.onnx", "decoder.onnx", "Decoder model (~6.9 MB)"),
    ("joiner.int8.onnx", "joiner.onnx", "Joiner model (~1.7 MB)"),
    ("tokens.txt", "tokens.txt", "Token vocabulary (~9 KB)"),
];

/// Model downloader
pub struct ModelDownloader {
    model_dir: PathBuf,
}

impl ModelDownloader {
    /// Create a new downloader targeting the specified model directory
    pub fn new(model_dir: PathBuf) -> Self {
        Self { model_dir }
    }

    /// Get the path to the ONNX Runtime DLL
    pub fn ort_dll_path(&self) -> PathBuf {
        self.model_dir.join(ORT_DLL_NAME)
    }

    /// Check if the ONNX Runtime DLL is present
    pub fn has_ort_dll(&self) -> bool {
        self.ort_dll_path().exists()
    }

    /// Check if all required model files are present
    pub fn is_complete(&self) -> bool {
        MODEL_FILES.iter().all(|(_, local_name, _)| {
            self.model_dir.join(local_name).exists()
        })
    }

    /// Check if all files including ONNX Runtime are present
    pub fn is_fully_complete(&self) -> bool {
        self.is_complete() && self.has_ort_dll()
    }

    /// Get list of missing model files
    pub fn missing_files(&self) -> Vec<&'static str> {
        MODEL_FILES
            .iter()
            .filter(|(_, local_name, _)| !self.model_dir.join(local_name).exists())
            .map(|(_, local_name, _)| *local_name)
            .collect()
    }

    /// Download all model files
    ///
    /// If `force` is true, re-downloads all files even if they exist.
    /// Otherwise, skips files that already exist.
    pub async fn download_all(&self, force: bool) -> Result<()> {
        // Create model directory if it doesn't exist
        tokio::fs::create_dir_all(&self.model_dir)
            .await
            .context("Failed to create model directory")?;

        info!("Downloading Parakeet TDT models to {:?}", self.model_dir);

        let client = reqwest::Client::new();
        let total_files = MODEL_FILES.len();

        for (i, (remote_path, local_name, description)) in MODEL_FILES.iter().enumerate() {
            let local_path = self.model_dir.join(local_name);

            // Skip if file exists and not forcing
            if local_path.exists() && !force {
                info!(
                    "[{}/{}] {} already exists, skipping",
                    i + 1,
                    total_files,
                    local_name
                );
                continue;
            }

            let url = format!("{}/{}", HF_BASE_URL, remote_path);
            info!(
                "[{}/{}] Downloading {} ({})",
                i + 1,
                total_files,
                local_name,
                description
            );

            self.download_file(&client, &url, &local_path).await?;
        }

        info!("Download complete! Models saved to {:?}", self.model_dir);
        Ok(())
    }

    /// Download ONNX Runtime DLL (v1.22.x) required for model inference
    ///
    /// This is needed because the system may have an incompatible version installed.
    /// The `ort` crate 2.0.0-rc.10 requires ONNX Runtime 1.22.x.
    pub async fn download_onnx_runtime(&self, force: bool) -> Result<()> {
        let dll_path = self.ort_dll_path();

        if dll_path.exists() && !force {
            info!("ONNX Runtime DLL already exists at {:?}, skipping", dll_path);
            return Ok(());
        }

        // Create model directory if it doesn't exist
        tokio::fs::create_dir_all(&self.model_dir)
            .await
            .context("Failed to create model directory")?;

        info!("Downloading ONNX Runtime 1.22.0 for Windows x64...");

        let client = reqwest::Client::new();

        // Download the zip file
        let zip_path = self.model_dir.join("onnxruntime.zip");
        self.download_file(&client, ORT_GITHUB_URL, &zip_path).await?;

        // Extract the DLL from the zip
        info!("Extracting onnxruntime.dll from archive...");
        self.extract_ort_dll(&zip_path, &dll_path).await?;

        // Clean up the zip file
        if let Err(e) = tokio::fs::remove_file(&zip_path).await {
            warn!("Failed to clean up zip file: {}", e);
        }

        info!("ONNX Runtime DLL installed at {:?}", dll_path);
        Ok(())
    }

    /// Extract onnxruntime.dll from the downloaded zip archive
    async fn extract_ort_dll(&self, zip_path: &Path, dll_path: &Path) -> Result<()> {
        use std::io::Read;

        // Read the zip file synchronously (zip crate doesn't support async)
        let zip_data = tokio::fs::read(zip_path)
            .await
            .context("Failed to read zip file")?;

        let reader = std::io::Cursor::new(zip_data);
        let mut archive = zip::ZipArchive::new(reader)
            .context("Failed to open zip archive")?;

        // Find and extract onnxruntime.dll
        // The DLL is typically at: onnxruntime-win-x64-1.22.0/lib/onnxruntime.dll
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)?;
            let name = file.name();

            if name.ends_with("onnxruntime.dll") && !name.contains("providers") {
                info!("Found DLL at: {}", name);

                let mut contents = Vec::new();
                file.read_to_end(&mut contents)
                    .context("Failed to read DLL from archive")?;

                tokio::fs::write(dll_path, &contents)
                    .await
                    .context("Failed to write DLL file")?;

                return Ok(());
            }
        }

        Err(anyhow::anyhow!("onnxruntime.dll not found in archive"))
    }

    /// Download a single file with progress reporting
    async fn download_file(
        &self,
        client: &reqwest::Client,
        url: &str,
        local_path: &Path,
    ) -> Result<()> {
        debug!("Downloading from {}", url);

        // Start the download request
        let response = client
            .get(url)
            .send()
            .await
            .context("Failed to start download")?;

        // Check for successful response
        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Download failed with status: {}",
                response.status()
            ));
        }

        // Get content length for progress bar
        let total_size = response.content_length().unwrap_or(0);

        // Create progress bar
        let pb = ProgressBar::new(total_size);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})")
                .expect("Invalid progress bar template")
                .progress_chars("#>-"),
        );

        // Download to a temporary file first
        let temp_path = local_path.with_extension("tmp");
        let mut file = File::create(&temp_path)
            .await
            .context("Failed to create temp file")?;

        // Stream the download
        let mut stream = response.bytes_stream();
        let mut downloaded: u64 = 0;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Error reading download stream")?;
            file.write_all(&chunk)
                .await
                .context("Error writing to file")?;

            downloaded += chunk.len() as u64;
            pb.set_position(downloaded);
        }

        // Flush and close the file
        file.flush().await.context("Failed to flush file")?;
        drop(file);

        // Rename temp file to final name
        tokio::fs::rename(&temp_path, local_path)
            .await
            .context("Failed to rename temp file")?;

        pb.finish_with_message("done");
        info!(
            "Downloaded {} ({} bytes)",
            local_path.file_name().unwrap_or_default().to_string_lossy(),
            downloaded
        );

        Ok(())
    }
}

/// Format bytes as human-readable string
#[allow(dead_code)]
fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
    }
}
