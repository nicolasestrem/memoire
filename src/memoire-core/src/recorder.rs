//! Main recording orchestration with multi-monitor support

use anyhow::Result;
use chrono::Utc;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use memoire_capture::{Monitor, MonitorInfo, ScreenCapture, screen::CapturedFrame};
use memoire_db::{Database, NewFrame, NewVideoChunk};
use memoire_processing::{VideoEncoder, encoder::EncoderConfig};

use crate::config::Config;

/// Frame batch settings for database writes
const FRAME_BATCH_SIZE: usize = 30;
const FRAME_FLUSH_INTERVAL: Duration = Duration::from_secs(5);

/// Frame deduplication settings
/// Hamming distance threshold: frames with distance <= this are considered duplicates
/// 0 = exact match only, 5 = ~92% similar, 10 = ~85% similar
const DEFAULT_DEDUP_THRESHOLD: u32 = 5;

/// Event emitted when a video chunk is finalized and ready for indexing
#[derive(Debug, Clone)]
pub struct ChunkFinalizedEvent {
    pub chunk_id: i64,
    pub video_path: PathBuf,
    pub monitor_name: String,
}

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
    /// Last frame's perceptual hash for deduplication
    last_frame_hash: Option<u64>,
    /// Counter for skipped duplicate frames
    skipped_frames: u64,
    /// Broadcast channel for chunk finalization events
    chunk_finalized_tx: broadcast::Sender<ChunkFinalizedEvent>,
}

impl MonitorRecorder {
    fn new(
        monitor: Monitor,
        videos_dir: &std::path::Path,
        config: &Config,
        chunk_finalized_tx: broadcast::Sender<ChunkFinalizedEvent>,
    ) -> Result<Self> {
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
            last_frame_hash: None,
            skipped_frames: 0,
            chunk_finalized_tx,
        })
    }

    fn capture_frame(&mut self, db: &Database) -> Result<bool> {
        let frame = match self.capture.capture_frame(Duration::from_millis(100))? {
            Some(f) => f,
            None => return Ok(false),
        };

        // Calculate perceptual hash for deduplication
        let frame_hash = frame.compute_perceptual_hash();

        // Check for duplicate frame using Hamming distance
        if let Some(last_hash) = self.last_frame_hash {
            let distance = CapturedFrame::hash_distance(frame_hash, last_hash);
            if distance <= DEFAULT_DEDUP_THRESHOLD {
                // Frame is too similar to previous, skip it
                self.skipped_frames += 1;
                debug!(
                    "skipping duplicate frame (distance={}, threshold={}), total skipped: {}",
                    distance, DEFAULT_DEDUP_THRESHOLD, self.skipped_frames
                );
                return Ok(false);
            }
        }

        // Update last frame hash
        self.last_frame_hash = Some(frame_hash);

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

        // Buffer frame metadata for batch insert (store hash as i64 for SQLite)
        let new_frame = NewFrame {
            video_chunk_id: chunk_id,
            offset_index: self.frame_index,
            timestamp: frame.timestamp,
            app_name: None,
            window_name: None,
            browser_url: None,
            focused: true,
            frame_hash: Some(frame_hash as i64),
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
            width: Some(self.info.width),
            height: Some(self.info.height),
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

            // Emit chunk finalized event for indexers
            if let Some(chunk_id) = self.current_chunk_id {
                let event = ChunkFinalizedEvent {
                    chunk_id,
                    video_path: path.clone(),
                    monitor_name: self.info.name.clone(),
                };

                // Send event (ignore error if no receivers - indexers might not be running)
                let _ = self.chunk_finalized_tx.send(event);
            }

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
    chunk_finalized_tx: broadcast::Sender<ChunkFinalizedEvent>,
}

impl Recorder {
    /// Create a new recorder for all available monitors
    pub fn new(config: Config) -> Result<Self> {
        info!("initializing multi-monitor recorder");

        // Create broadcast channel for chunk finalization events
        // Capacity of 100 allows buffering events if indexers are slow to subscribe
        let (chunk_finalized_tx, _rx) = broadcast::channel(100);

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
                    match MonitorRecorder::new(monitor, &videos_dir, &config, chunk_finalized_tx.clone()) {
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
            chunk_finalized_tx,
        })
    }

    /// Subscribe to chunk finalization events
    ///
    /// Returns a receiver that will be notified when video chunks are finalized
    /// and ready for indexing. Each subscriber gets its own receiver.
    pub fn subscribe_to_chunk_events(&self) -> broadcast::Receiver<ChunkFinalizedEvent> {
        self.chunk_finalized_tx.subscribe()
    }

    /// Run the recording loop for all monitors
    pub fn run(&mut self, shutdown: Arc<AtomicBool>) -> Result<()> {
        info!(
            "starting recording loop at {} FPS for {} monitor(s)",
            self.config.fps,
            self.monitors.len()
        );

        let frame_interval = Duration::from_secs_f64(1.0 / self.config.fps as f64);
        let mut last_capture = Instant::now();
        let mut total_frames = 0u64;
        let mut capture_attempts = 0u64;
        let max_consecutive_errors = 10;

        while !shutdown.load(Ordering::SeqCst) {
            // Wait for next frame time
            let elapsed = last_capture.elapsed();
            if elapsed < frame_interval {
                std::thread::sleep(frame_interval - elapsed);
            }
            last_capture = Instant::now();
            capture_attempts += 1;

            // Capture from all monitors
            let mut any_captured = false;
            let mut monitors_to_reinit = Vec::new();

            let mut no_frame_count = 0;
            for (i, monitor) in self.monitors.iter_mut().enumerate() {
                match monitor.capture_frame(&self.db) {
                    Ok(true) => {
                        any_captured = true;
                    }
                    Ok(false) => {
                        // No new frame (static screen or DXGI timeout)
                        no_frame_count += 1;
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

            // Log if ALL monitors returned no frames (potential DXGI issue)
            if no_frame_count > 0 && no_frame_count == self.monitors.len() && capture_attempts % 30 == 0 {
                warn!(
                    "no frames captured in last 30 seconds ({} total attempts, {} successful) across {} monitors - screen may be static/locked or DXGI not working",
                    capture_attempts, total_frames, self.monitors.len()
                );
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
                    let total_skipped: u64 = self.monitors.iter().map(|m| m.skipped_frames).sum();
                    info!(
                        "captured {} frame sets across {} monitors (skipped {} duplicate frames)",
                        total_frames, self.monitors.len(), total_skipped
                    );
                }
            }
        }

        // Finalize all chunks
        info!("finalizing recording...");
        let mut total_skipped = 0u64;
        for monitor in &mut self.monitors {
            total_skipped += monitor.skipped_frames;
            if let Err(e) = monitor.finalize_chunk(&self.db) {
                warn!("error finalizing chunk for {}: {}", monitor.info.name, e);
            }
        }

        let dedup_percentage = if total_frames + total_skipped > 0 {
            (total_skipped as f64 / (total_frames + total_skipped) as f64) * 100.0
        } else {
            0.0
        };
        info!(
            "recording stopped. total frames: {}, skipped duplicates: {} ({:.1}% reduction)",
            total_frames, total_skipped, dedup_percentage
        );
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
