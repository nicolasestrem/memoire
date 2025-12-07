//! Main recording orchestration with multi-monitor support

use anyhow::Result;
use chrono::Utc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use memoire_capture::{Monitor, MonitorInfo, ScreenCapture};
use memoire_db::{Database, NewFrame, NewVideoChunk};
use memoire_processing::{VideoEncoder, encoder::EncoderConfig};

use crate::config::Config;

/// Frame batch settings for database writes
const FRAME_BATCH_SIZE: usize = 30;
const FRAME_FLUSH_INTERVAL: Duration = Duration::from_secs(5);

/// Per-monitor recording state
struct MonitorRecorder {
    info: MonitorInfo,
    capture: ScreenCapture,
    encoder: VideoEncoder,
    current_chunk_id: Option<i64>,
    frame_index: i64,
    chunk_index: u64,
    consecutive_errors: u32,
    pending_frames: Vec<NewFrame>,
    last_db_flush: Instant,
}

impl MonitorRecorder {
    fn new(monitor: Monitor, videos_dir: &std::path::Path, config: &Config) -> Result<Self> {
        info!(
            "initializing capture for monitor: {} ({}x{})",
            monitor.info.name, monitor.info.width, monitor.info.height
        );

        let capture = ScreenCapture::new(&monitor)?;

        // Create monitor-specific subdirectory
        let monitor_name = sanitize_monitor_name(&monitor.info.name);
        let monitor_dir = videos_dir.join(&monitor_name);
        std::fs::create_dir_all(&monitor_dir)?;

        let encoder_config = EncoderConfig {
            output_dir: monitor_dir,
            chunk_duration_secs: config.chunk_duration_secs,
            fps: config.fps,
            use_hw_encoding: config.use_hw_encoding,
            quality: 23,
            use_piped_encoding: true, // Use efficient piped encoding by default
        };
        let encoder = VideoEncoder::new(encoder_config)?;

        Ok(Self {
            info: monitor.info,
            capture,
            encoder,
            current_chunk_id: None,
            frame_index: 0,
            chunk_index: 0,
            consecutive_errors: 0,
            pending_frames: Vec::with_capacity(FRAME_BATCH_SIZE),
            last_db_flush: Instant::now(),
        })
    }

    fn capture_frame(&mut self, db: &Database) -> Result<bool> {
        let frame = match self.capture.capture_frame(Duration::from_millis(100))? {
            Some(f) => f,
            None => return Ok(false),
        };

        // Ensure we have a current chunk
        if self.current_chunk_id.is_none() {
            self.start_new_chunk(db)?;
        }

        let chunk_id = match self.current_chunk_id {
            Some(id) => id,
            None => {
                // This should not happen after start_new_chunk, but handle gracefully
                error!("chunk_id unexpectedly None after initialization - attempting recovery");
                self.start_new_chunk(db)?;
                self.current_chunk_id
                    .ok_or_else(|| anyhow::anyhow!("failed to initialize chunk_id after retry"))?
            }
        };

        // Buffer frame metadata for batch insert
        let new_frame = NewFrame {
            video_chunk_id: chunk_id,
            offset_index: self.frame_index,
            timestamp: frame.timestamp,
            app_name: None,
            window_name: None,
            browser_url: None,
            focused: true,
        };
        self.pending_frames.push(new_frame);

        // Add frame to encoder
        self.encoder.add_frame(&frame.data, frame.width, frame.height, frame.timestamp)?;

        self.frame_index += 1;
        self.consecutive_errors = 0;

        // Flush to database if batch is full or timeout reached
        if self.pending_frames.len() >= FRAME_BATCH_SIZE
            || self.last_db_flush.elapsed() >= FRAME_FLUSH_INTERVAL
        {
            self.flush_frames(db)?;
        }

        Ok(true)
    }

    /// Flush pending frames to database in a single transaction
    fn flush_frames(&mut self, db: &Database) -> Result<()> {
        if self.pending_frames.is_empty() {
            return Ok(());
        }

        debug!(
            "flushing {} frames to database for {}",
            self.pending_frames.len(),
            self.info.name
        );

        memoire_db::insert_frames_batch(db.connection(), &self.pending_frames)?;
        self.pending_frames.clear();
        self.last_db_flush = Instant::now();

        Ok(())
    }

    fn start_new_chunk(&mut self, db: &Database) -> Result<()> {
        let timestamp = Utc::now();
        let date_str = timestamp.format("%Y-%m-%d").to_string();
        let time_str = timestamp.format("%H-%M-%S").to_string();
        let monitor_name = sanitize_monitor_name(&self.info.name);

        // Note: chunk_index matches encoder's internal index for this monitor
        let file_path = format!("videos/{}/{}/chunk_{}_{}.mp4", monitor_name, date_str, time_str, self.chunk_index);

        let new_chunk = NewVideoChunk {
            file_path,
            device_name: self.info.name.clone(),
        };

        let chunk_id = memoire_db::insert_video_chunk(db.connection(), &new_chunk)?;
        self.current_chunk_id = Some(chunk_id);
        self.frame_index = 0;

        debug!("started new video chunk {} for {}", chunk_id, self.info.name);
        Ok(())
    }

    fn finalize_chunk(&mut self, db: &Database) -> Result<()> {
        // Flush any pending frames before finalizing the chunk
        self.flush_frames(db)?;

        if let Some(path) = self.encoder.finalize_chunk()? {
            info!("finalized chunk for {}: {:?}", self.info.name, path);
            self.chunk_index += 1;
        }
        self.current_chunk_id = None;
        Ok(())
    }
}

/// Main recorder that orchestrates capture across all monitors
pub struct Recorder {
    config: Config,
    db: Database,
    monitors: Vec<MonitorRecorder>,
}

impl Recorder {
    /// Create a new recorder for all available monitors
    pub fn new(config: Config) -> Result<Self> {
        info!("initializing multi-monitor recorder");

        // Create directories
        std::fs::create_dir_all(&config.data_dir)?;
        let videos_dir = config.data_dir.join("videos");
        std::fs::create_dir_all(&videos_dir)?;

        // Open database
        let db_path = config.data_dir.join("memoire.db");
        let db = Database::open(&db_path)?;
        info!("database opened at {:?}", db_path);

        // Get all monitors
        let monitor_infos = Monitor::enumerate_all()?;
        info!("found {} monitor(s)", monitor_infos.len());

        let mut monitors = Vec::new();
        for info in monitor_infos {
            match Monitor::from_info(info.clone()) {
                Ok(monitor) => {
                    match MonitorRecorder::new(monitor, &videos_dir, &config) {
                        Ok(recorder) => {
                            monitors.push(recorder);
                        }
                        Err(e) => {
                            warn!("failed to initialize recorder for {}: {}", info.name, e);
                        }
                    }
                }
                Err(e) => {
                    warn!("failed to get monitor {}: {}", info.name, e);
                }
            }
        }

        if monitors.is_empty() {
            return Err(anyhow::anyhow!("no monitors available for capture"));
        }

        info!("initialized {} monitor recorder(s)", monitors.len());

        Ok(Self {
            config,
            db,
            monitors,
        })
    }

    /// Run the recording loop for all monitors
    pub fn run(&mut self, running: Arc<AtomicBool>) -> Result<()> {
        info!(
            "starting recording loop at {} FPS for {} monitor(s)",
            self.config.fps,
            self.monitors.len()
        );

        let frame_interval = Duration::from_secs_f64(1.0 / self.config.fps as f64);
        let mut last_capture = Instant::now();
        let mut total_frames = 0u64;
        let max_consecutive_errors = 10;

        while running.load(Ordering::SeqCst) {
            // Wait for next frame time
            let elapsed = last_capture.elapsed();
            if elapsed < frame_interval {
                std::thread::sleep(frame_interval - elapsed);
            }
            last_capture = Instant::now();

            // Capture from all monitors
            let mut any_captured = false;
            let mut monitors_to_reinit = Vec::new();

            for (i, monitor) in self.monitors.iter_mut().enumerate() {
                match monitor.capture_frame(&self.db) {
                    Ok(true) => {
                        any_captured = true;
                    }
                    Ok(false) => {
                        // No new frame (static screen)
                    }
                    Err(e) => {
                        error!("capture error on {}: {}", monitor.info.name, e);
                        monitor.consecutive_errors += 1;

                        if monitor.consecutive_errors >= max_consecutive_errors {
                            monitors_to_reinit.push(i);
                        }
                    }
                }
            }

            // Reinitialize monitors that had too many errors
            for i in monitors_to_reinit {
                let monitor = &mut self.monitors[i];
                warn!("too many errors on {}, attempting reinitialize", monitor.info.name);
                if let Err(e) = Self::reinitialize_monitor(monitor, &self.db) {
                    error!("failed to reinitialize {}: {}", monitor.info.name, e);
                }
            }

            if any_captured {
                total_frames += 1;
                if total_frames % 60 == 0 {
                    info!("captured {} frame sets across {} monitors", total_frames, self.monitors.len());
                }
            }
        }

        // Finalize all chunks
        info!("finalizing recording...");
        for monitor in &mut self.monitors {
            if let Err(e) = monitor.finalize_chunk(&self.db) {
                warn!("error finalizing chunk for {}: {}", monitor.info.name, e);
            }
        }

        info!("recording stopped. total frame sets: {}", total_frames);
        Ok(())
    }

    fn reinitialize_monitor(monitor: &mut MonitorRecorder, db: &Database) -> Result<()> {
        // Finalize current chunk (flushes pending frames)
        let _ = monitor.finalize_chunk(db);

        // Re-create monitor and capture
        let new_monitor = Monitor::from_info(monitor.info.clone())?;
        monitor.capture = ScreenCapture::new(&new_monitor)?;
        monitor.consecutive_errors = 0;

        info!("reinitialized capture for {}", monitor.info.name);
        Ok(())
    }
}

/// Windows reserved device names that cannot be used as filenames
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Sanitize monitor name for use as directory name
fn sanitize_monitor_name(name: &str) -> String {
    // Step 1: Replace invalid filesystem characters and control characters
    let sanitized: String = name.chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            '.' if name.starts_with('.') => '_',
            '\0'..='\x1f' => '_', // Control characters
            _ => c,
        })
        .collect();

    // Step 2: Remove path traversal sequences
    let sanitized = sanitized.replace("..", "_");

    // Step 3: Trim leading/trailing whitespace, underscores, and dots
    let sanitized = sanitized
        .trim()
        .trim_matches(|c| c == '_' || c == '.' || c == ' ')
        .to_string();

    // Step 4: Check for Windows reserved names (case-insensitive)
    let upper = sanitized.to_uppercase();
    let base_name = upper.split('.').next().unwrap_or(&upper);
    let sanitized = if WINDOWS_RESERVED_NAMES.contains(&base_name) {
        format!("_{}", sanitized)
    } else {
        sanitized
    };

    // Step 5: Truncate to safe length (leave room for path components)
    let max_name_len = 100;
    let sanitized: String = sanitized.chars().take(max_name_len).collect();

    // Step 6: Fallback for empty result
    if sanitized.is_empty() {
        "monitor".to_string()
    } else {
        sanitized
    }
}
