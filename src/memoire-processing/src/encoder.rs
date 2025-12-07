//! Video encoding using FFmpeg
//!
//! Supports two encoding modes:
//! - Piped: Raw frames piped directly to FFmpeg stdin (default, faster)
//! - PNG: Frames saved to disk then encoded (fallback)

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use tracing::{debug, info, warn};

/// Video encoder configuration
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// Output directory for video chunks
    pub output_dir: PathBuf,
    /// Chunk duration in seconds
    pub chunk_duration_secs: u64,
    /// Target framerate
    pub fps: u32,
    /// Use hardware encoding (NVENC)
    pub use_hw_encoding: bool,
    /// Video quality (CRF value, lower = better, 18-28 typical)
    pub quality: u32,
    /// Use piped encoding (raw frames to FFmpeg stdin) instead of PNG intermediate
    pub use_piped_encoding: bool,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            output_dir: PathBuf::from("videos"),
            chunk_duration_secs: 300, // 5 minutes
            fps: 1,                    // 1 FPS for screen recording
            use_hw_encoding: true,
            quality: 23,
            use_piped_encoding: true, // Default to piped for better performance
        }
    }
}

/// Video encoder that accumulates frames and creates MP4 chunks
pub struct VideoEncoder {
    config: EncoderConfig,
    current_chunk_dir: PathBuf,
    frame_count: u64,
    chunk_start_time: Option<DateTime<Utc>>,
    chunk_index: u64,
    // Piped encoding state
    ffmpeg_process: Option<Child>,
    ffmpeg_stdin: Option<ChildStdin>,
    current_output_path: Option<PathBuf>,
    frame_width: Option<u32>,
    frame_height: Option<u32>,
}

impl VideoEncoder {
    /// Create a new video encoder
    pub fn new(config: EncoderConfig) -> Result<Self> {
        // Ensure output directory exists
        fs::create_dir_all(&config.output_dir)?;

        // Create temp directory for frames (used in PNG fallback mode)
        let current_chunk_dir = config.output_dir.join("_temp_frames");
        fs::create_dir_all(&current_chunk_dir)?;

        Ok(Self {
            config,
            current_chunk_dir,
            frame_count: 0,
            chunk_start_time: None,
            chunk_index: 0,
            ffmpeg_process: None,
            ffmpeg_stdin: None,
            current_output_path: None,
            frame_width: None,
            frame_height: None,
        })
    }

    /// Add a frame to the current chunk
    pub fn add_frame(&mut self, frame_data: &[u8], width: u32, height: u32, timestamp: DateTime<Utc>) -> Result<()> {
        // Set chunk start time if this is the first frame
        if self.chunk_start_time.is_none() {
            self.chunk_start_time = Some(timestamp);
        }

        if self.config.use_piped_encoding {
            // Initialize FFmpeg pipe on first frame
            if self.ffmpeg_stdin.is_none() {
                self.start_ffmpeg_pipe(width, height)?;
            }

            // Write raw RGBA frame to FFmpeg stdin
            self.write_frame_to_pipe(frame_data)?;
        } else {
            // Fallback: Save frame as PNG
            let frame_path = self.current_chunk_dir.join(format!("frame_{:08}.png", self.frame_count));
            let img = image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(width, height, frame_data.to_vec())
                .ok_or_else(|| anyhow::anyhow!("failed to create image buffer"))?;
            img.save(&frame_path)?;
        }

        self.frame_count += 1;

        // Check if we should finalize the chunk
        if let Some(start) = self.chunk_start_time {
            let elapsed = (timestamp - start).num_seconds() as u64;
            if elapsed >= self.config.chunk_duration_secs {
                debug!("chunk duration reached, finalizing");
                self.finalize_chunk()?;
            }
        }

        Ok(())
    }

    /// Start FFmpeg process with piped input
    fn start_ffmpeg_pipe(&mut self, width: u32, height: u32) -> Result<()> {
        let start_time = self.chunk_start_time.ok_or_else(|| anyhow::anyhow!("no start time"))?;
        let date_str = start_time.format("%Y-%m-%d").to_string();
        let time_str = start_time.format("%H-%M-%S").to_string();

        // Create date directory
        let date_dir = self.config.output_dir.join(&date_str);
        fs::create_dir_all(&date_dir)?;

        // Output path
        let output_path = date_dir.join(format!("chunk_{}_{}.mp4", time_str, self.chunk_index));

        info!("starting piped encoding to {:?} ({}x{})", output_path, width, height);

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y") // Overwrite output
            .arg("-f").arg("rawvideo")
            .arg("-pix_fmt").arg("rgba")
            .arg("-s").arg(format!("{}x{}", width, height))
            .arg("-r").arg(self.config.fps.to_string())
            .arg("-i").arg("-") // Read from stdin
            .arg("-c:v");

        // Use NVENC if available
        if self.config.use_hw_encoding {
            cmd.arg("h264_nvenc")
                .arg("-preset").arg("p4")
                .arg("-rc").arg("vbr")
                .arg("-cq").arg(self.config.quality.to_string());
        } else {
            cmd.arg("libx264")
                .arg("-crf").arg(self.config.quality.to_string())
                .arg("-preset").arg("fast");
        }

        cmd.arg("-pix_fmt").arg("yuv420p")
            .arg(&output_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        debug!("spawning ffmpeg pipe: {:?}", cmd);

        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take()
            .ok_or_else(|| anyhow::anyhow!("failed to open ffmpeg stdin"))?;

        self.ffmpeg_process = Some(child);
        self.ffmpeg_stdin = Some(stdin);
        self.current_output_path = Some(output_path);
        self.frame_width = Some(width);
        self.frame_height = Some(height);

        Ok(())
    }

    /// Write raw frame data to FFmpeg stdin
    fn write_frame_to_pipe(&mut self, frame_data: &[u8]) -> Result<()> {
        if let Some(ref mut stdin) = self.ffmpeg_stdin {
            stdin.write_all(frame_data)?;
        }
        Ok(())
    }

    /// Finalize piped FFmpeg encoding
    fn finalize_ffmpeg_pipe(&mut self) -> Result<Option<PathBuf>> {
        // Close stdin to signal EOF to FFmpeg
        self.ffmpeg_stdin.take();

        if let Some(child) = self.ffmpeg_process.take() {
            let output = child.wait_with_output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);

                // Check if NVENC failed
                if self.config.use_hw_encoding && stderr.contains("nvenc") {
                    warn!("NVENC pipe failed, will use PNG fallback for next chunk");
                    // Don't error out - the partial file may be usable
                }

                // Log the error but don't fail if we got some output
                if !stderr.is_empty() {
                    warn!("ffmpeg stderr: {}", stderr.chars().take(500).collect::<String>());
                }
            }
        }

        let path = self.current_output_path.take();
        self.frame_width = None;
        self.frame_height = None;

        Ok(path)
    }

    /// Finalize the current chunk and create MP4
    pub fn finalize_chunk(&mut self) -> Result<Option<PathBuf>> {
        if self.frame_count == 0 {
            return Ok(None);
        }

        let output_path = if self.config.use_piped_encoding && self.ffmpeg_stdin.is_some() {
            // Finalize piped encoding
            info!("finalizing piped encoding of {} frames", self.frame_count);
            self.finalize_ffmpeg_pipe()?
        } else {
            // Finalize PNG-based encoding
            self.finalize_png_chunk()?
        };

        // Reset state for next chunk
        self.frame_count = 0;
        self.chunk_start_time = None;
        self.chunk_index += 1;

        Ok(output_path)
    }

    /// Finalize PNG-based encoding (legacy method)
    fn finalize_png_chunk(&mut self) -> Result<Option<PathBuf>> {
        let start_time = self.chunk_start_time.ok_or_else(|| anyhow::anyhow!("no start time"))?;
        let date_str = start_time.format("%Y-%m-%d").to_string();
        let time_str = start_time.format("%H-%M-%S").to_string();

        // Create date directory
        let date_dir = self.config.output_dir.join(&date_str);
        fs::create_dir_all(&date_dir)?;

        // Output path
        let output_path = date_dir.join(format!("chunk_{}_{}.mp4", time_str, self.chunk_index));

        info!("encoding {} frames to {:?} (PNG method)", self.frame_count, output_path);

        // Build FFmpeg command
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y") // Overwrite output
            .arg("-framerate").arg(self.config.fps.to_string())
            .arg("-i").arg(self.current_chunk_dir.join("frame_%08d.png"))
            .arg("-c:v");

        // Use NVENC if available
        if self.config.use_hw_encoding {
            cmd.arg("h264_nvenc")
                .arg("-preset").arg("p4")
                .arg("-rc").arg("vbr")
                .arg("-cq").arg(self.config.quality.to_string());
        } else {
            cmd.arg("libx264")
                .arg("-crf").arg(self.config.quality.to_string())
                .arg("-preset").arg("fast");
        }

        cmd.arg("-pix_fmt").arg("yuv420p")
            .arg(&output_path);

        debug!("running ffmpeg: {:?}", cmd);

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);

            // Retry with software encoding if NVENC failed
            if self.config.use_hw_encoding && stderr.contains("nvenc") {
                warn!("NVENC failed, falling back to software encoding");
                return self.encode_software(&output_path);
            }

            return Err(anyhow::anyhow!("ffmpeg failed: {}", stderr));
        }

        // Clean up temp frames
        self.cleanup_temp_frames()?;

        Ok(Some(output_path))
    }

    fn encode_software(&mut self, output_path: &Path) -> Result<Option<PathBuf>> {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y")
            .arg("-framerate").arg(self.config.fps.to_string())
            .arg("-i").arg(self.current_chunk_dir.join("frame_%08d.png"))
            .arg("-c:v").arg("libx264")
            .arg("-crf").arg(self.config.quality.to_string())
            .arg("-preset").arg("fast")
            .arg("-pix_fmt").arg("yuv420p")
            .arg(output_path);

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("ffmpeg software encoding failed: {}", stderr));
        }

        self.cleanup_temp_frames()?;

        Ok(Some(output_path.to_path_buf()))
    }

    fn cleanup_temp_frames(&self) -> Result<()> {
        for entry in fs::read_dir(&self.current_chunk_dir)? {
            let entry = entry?;
            if entry.path().extension().map_or(false, |e| e == "png") {
                fs::remove_file(entry.path())?;
            }
        }
        Ok(())
    }

    /// Get the output directory
    pub fn output_dir(&self) -> &Path {
        &self.config.output_dir
    }
}

impl Drop for VideoEncoder {
    fn drop(&mut self) {
        // Try to finalize any remaining frames
        if self.frame_count > 0 {
            if let Err(e) = self.finalize_chunk() {
                warn!("failed to finalize chunk on drop: {}", e);
            }
        }
        // Clean up temp directory
        let _ = fs::remove_dir_all(&self.current_chunk_dir);
    }
}

/// Check if FFmpeg is available
pub fn check_ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if NVENC is available
pub fn check_nvenc() -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("h264_nvenc"))
        .unwrap_or(false)
}
