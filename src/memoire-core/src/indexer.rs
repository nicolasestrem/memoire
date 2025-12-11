//! Background OCR indexer for processing captured frames

use anyhow::Result;
use chrono::{DateTime, Utc};
use futures::{stream, StreamExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

use memoire_db::Database;
use memoire_ocr::{FrameData, Processor as OcrProcessor};

use crate::recorder::ChunkFinalizedEvent;

/// OCR batch settings
const OCR_BATCH_SIZE: usize = 30;
const DEFAULT_OCR_FPS: u32 = 10;
/// Maximum concurrent frame extractions (limited by FFmpeg processes)
const MAX_CONCURRENT_EXTRACTIONS: usize = 4;

/// Statistics for OCR processing
#[derive(Debug, Clone)]
pub struct IndexerStats {
    pub total_frames: u64,
    pub frames_with_ocr: u64,
    pub pending_frames: u64,
    pub processing_rate: f64, // frames/sec
    pub last_updated: DateTime<Utc>,
}

/// OCR Indexer that processes frames in background
pub struct Indexer {
    db: Database,
    processor: OcrProcessor,
    data_dir: PathBuf,
    ocr_fps: u32,
    running: Arc<AtomicBool>,
    stats: Arc<RwLock<IndexerStats>>,
    processed_count: Arc<AtomicU64>,
    chunk_events_rx: Option<broadcast::Receiver<ChunkFinalizedEvent>>,
}

impl Indexer {
    /// Create a new indexer with optional language configuration
    pub fn new(data_dir: PathBuf, ocr_fps: Option<u32>, ocr_language: Option<String>) -> Result<Self> {
        info!("initializing OCR indexer");

        let db_path = data_dir.join("memoire.db");
        let db = Database::open(&db_path)?;
        info!("database opened at {:?}", db_path);

        // Create processor with specified language or default to English
        let processor = match ocr_language {
            Some(ref lang) => {
                info!("initializing OCR processor with language: {}", lang);
                OcrProcessor::with_language(lang)?
            }
            None => {
                info!("initializing OCR processor with default language (en-US)");
                OcrProcessor::new()?
            }
        };
        info!("OCR processor initialized");

        let stats = IndexerStats {
            total_frames: 0,
            frames_with_ocr: 0,
            pending_frames: 0,
            processing_rate: 0.0,
            last_updated: Utc::now(),
        };

        Ok(Self {
            db,
            processor,
            data_dir,
            ocr_fps: ocr_fps.unwrap_or(DEFAULT_OCR_FPS),
            running: Arc::new(AtomicBool::new(true)), // Start as running
            stats: Arc::new(RwLock::new(stats)),
            processed_count: Arc::new(AtomicU64::new(0)),
            chunk_events_rx: None, // Will be set via set_chunk_events_receiver()
        })
    }

    /// Set the chunk finalization event receiver
    ///
    /// This enables event-driven processing for immediate indexing of finalized chunks.
    pub fn set_chunk_events_receiver(&mut self, rx: broadcast::Receiver<ChunkFinalizedEvent>) {
        info!("OCR indexer now using event-driven chunk processing");
        self.chunk_events_rx = Some(rx);
    }

    /// Get current statistics
    pub async fn get_stats(&self) -> IndexerStats {
        self.stats.read().await.clone()
    }

    /// Check if indexer is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Run the indexer (blocks until stopped)
    pub async fn run(&mut self, shutdown: Arc<AtomicBool>) -> Result<()> {
        info!("starting OCR indexer at {} fps", self.ocr_fps);
        self.running.store(true, Ordering::Relaxed);

        let poll_interval = Duration::from_secs(10); // Poll every 10 seconds as fallback
        let mut batch_start = Instant::now();
        let mut batch_count = 0u64;

        // Take ownership of the receiver if present
        let mut chunk_rx = self.chunk_events_rx.take();
        let use_events = chunk_rx.is_some();

        if use_events {
            info!("OCR indexer using event-driven mode with {} fallback polling",
                  poll_interval.as_secs());
        } else {
            info!("OCR indexer using polling mode every {} seconds",
                  poll_interval.as_secs());
        }

        while !shutdown.load(Ordering::SeqCst) && self.running.load(Ordering::Relaxed) {
            // Event-driven mode: wait for chunk events or timeout
            if let Some(ref mut rx) = chunk_rx {
                tokio::select! {
                    // Branch 1: Chunk finalized event received
                    event = rx.recv() => {
                        match event {
                            Ok(evt) => {
                                debug!("received chunk finalized event for chunk {}", evt.chunk_id);
                                // Process this specific chunk immediately
                                match self.process_chunk_frames(evt.chunk_id).await {
                                    Ok(count) if count > 0 => {
                                        batch_count += count as u64;
                                        info!("processed {} frames from newly finalized chunk {}",
                                              count, evt.chunk_id);
                                    }
                                    Ok(_) => {
                                        debug!("no frames to process in chunk {}", evt.chunk_id);
                                    }
                                    Err(e) => {
                                        error!("error processing chunk {}: {}", evt.chunk_id, e);
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                warn!("indexer lagged, skipped {} chunk events - processing all pending", skipped);
                                // Fall through to polling below
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                warn!("chunk event channel closed, switching to polling mode");
                                chunk_rx = None; // Switch to polling mode
                            }
                        }
                    }

                    // Branch 2: Timeout - poll for any missed frames (fallback)
                    _ = tokio::time::sleep(poll_interval) => {
                        match self.process_batch().await {
                            Ok(count) if count > 0 => {
                                batch_count += count as u64;
                                debug!("processed {} frames via polling fallback", count);
                            }
                            Ok(_) => {} // No frames pending
                            Err(e) => {
                                error!("polling batch processing error: {}", e);
                            }
                        }
                    }
                }
            } else {
                // Polling-only mode (no event channel)
                match self.process_batch().await {
                    Ok(count) if count > 0 => {
                        batch_count += count as u64;
                        debug!("processed {} frames via polling", count);
                    }
                    Ok(_) => {
                        // No frames, sleep before next poll
                        tokio::time::sleep(poll_interval).await;
                    }
                    Err(e) => {
                        error!("batch processing error: {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }

            // Update stats every 10 seconds
            if batch_start.elapsed() >= Duration::from_secs(10) {
                let elapsed_secs = batch_start.elapsed().as_secs_f64();
                let rate = batch_count as f64 / elapsed_secs;

                self.update_stats(rate).await?;

                batch_start = Instant::now();
                batch_count = 0;

                let stats = self.stats.read().await;
                info!(
                    "OCR progress: {}/{} frames processed ({} pending, {:.1} fps)",
                    stats.frames_with_ocr,
                    stats.total_frames,
                    stats.pending_frames,
                    stats.processing_rate
                );
            }
        }

        info!("OCR indexer stopped");
        Ok(())
    }

    /// Stop the indexer gracefully
    pub fn stop(&self) {
        info!("stopping OCR indexer");
        self.running.store(false, Ordering::Relaxed);
    }

    /// Process frames from a specific chunk (event-driven)
    async fn process_chunk_frames(&self, chunk_id: i64) -> Result<usize> {
        // Query frames without OCR for this specific chunk
        let frames = memoire_db::get_frames_for_chunk_without_ocr(
            self.db.connection(),
            chunk_id,
        )?;

        if frames.is_empty() {
            return Ok(0);
        }

        debug!("processing {} frames from chunk {} (event-driven)", frames.len(), chunk_id);

        // Use the same concurrent processing logic as process_batch
        self.process_frame_list(&frames).await
    }

    /// Process a batch of frames without OCR
    async fn process_batch(&self) -> Result<usize> {
        // Query frames without OCR (limit to batch size)
        let frames = memoire_db::get_frames_without_ocr(
            self.db.connection(),
            OCR_BATCH_SIZE as i64,
        )?;

        if frames.is_empty() {
            return Ok(0);
        }

        debug!("processing batch of {} frames concurrently", frames.len());

        self.process_frame_list(&frames).await
    }

    /// Process a list of frames (shared logic for batch and event-driven processing)
    async fn process_frame_list(&self, frames: &[memoire_db::Frame]) -> Result<usize> {

        // Step 1: Extract all frames concurrently using spawn_blocking
        // This is the expensive I/O-bound FFmpeg operation
        let extraction_tasks: Vec<_> = frames.iter().map(|frame| {
            let frame_id = frame.id;
            let video_chunk_id = frame.video_chunk_id;
            let offset_index = frame.offset_index;
            let data_dir = self.data_dir.clone();
            let db_conn = self.db.connection();

            async move {
                // Get video chunk info (cheap database lookup)
                let chunk = match memoire_db::get_video_chunk(db_conn, video_chunk_id) {
                    Ok(Some(c)) => c,
                    Ok(None) => {
                        return (frame_id, Err(anyhow::anyhow!("video chunk {} not found", video_chunk_id)));
                    }
                    Err(e) => {
                        return (frame_id, Err(e));
                    }
                };

                let video_path = data_dir.join(&chunk.file_path);
                let cached_width = chunk.width;
                let cached_height = chunk.height;

                // Run FFmpeg extraction in a blocking task
                let extraction_result = tokio::task::spawn_blocking(move || {
                    Self::extract_frame_from_video_static(&video_path, offset_index, cached_width, cached_height)
                }).await;

                match extraction_result {
                    Ok(Ok(frame_data)) => (frame_id, Ok(frame_data)),
                    Ok(Err(e)) => (frame_id, Err(e)),
                    Err(e) => (frame_id, Err(anyhow::anyhow!("spawn_blocking failed: {}", e))),
                }
            }
        }).collect();

        // Execute extractions concurrently with limited concurrency
        let extracted_frames: Vec<_> = stream::iter(extraction_tasks)
            .buffer_unordered(MAX_CONCURRENT_EXTRACTIONS)
            .collect()
            .await;

        // Step 2: Process OCR sequentially (Windows OCR may not be thread-safe)
        let mut ocr_results = Vec::with_capacity(frames.len());

        for (frame_id, extraction_result) in extracted_frames {
            match extraction_result {
                Ok(frame_data) => {
                    match self.processor.process_frame(frame_data).await {
                        Ok(result) => {
                            ocr_results.push((frame_id, result));
                        }
                        Err(e) => {
                            warn!("OCR failed for frame {}: {}", frame_id, e);
                            ocr_results.push((frame_id, empty_ocr_result()));
                        }
                    }
                }
                Err(e) => {
                    warn!("failed to extract frame {}: {}", frame_id, e);
                    ocr_results.push((frame_id, empty_ocr_result()));
                }
            }
        }

        // Batch insert OCR results
        self.insert_ocr_batch(&ocr_results)?;

        let count = ocr_results.len();
        self.processed_count.fetch_add(count as u64, Ordering::Relaxed);

        Ok(count)
    }

    /// Extract a specific frame from video using FFmpeg command-line tool (static version)
    /// If cached_width/cached_height are provided, skips the ffprobe call for better performance.
    /// This static version allows calling from spawn_blocking without borrowing self.
    fn extract_frame_from_video_static(
        video_path: &PathBuf,
        frame_index: i64,
        cached_width: Option<u32>,
        cached_height: Option<u32>,
    ) -> Result<FrameData> {
        use std::process::{Command, Stdio};
        use std::io::Read;

        // Use ffmpeg to extract a specific frame as raw RGBA data
        // -i input.mp4 -vf "select=eq(n\,FRAME_INDEX)" -vframes 1 -f rawvideo -pix_fmt rgba -

        let frame_filter = format!("select=eq(n\\,{})", frame_index);

        let mut child = Command::new("ffmpeg")
            .arg("-i")
            .arg(video_path)
            .arg("-vf")
            .arg(&frame_filter)
            .arg("-vframes")
            .arg("1")
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("rgba")
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn ffmpeg: {}", e))?;

        // Read frame data from stdout
        let mut frame_data = Vec::new();
        child.stdout.as_mut()
            .ok_or_else(|| anyhow::anyhow!("failed to capture stdout"))?
            .read_to_end(&mut frame_data)?;

        let status = child.wait()?;
        if !status.success() {
            return Err(anyhow::anyhow!("ffmpeg failed with exit code {:?}", status.code()));
        }

        // Use cached dimensions if available, otherwise fall back to ffprobe
        let (width, height) = match (cached_width, cached_height) {
            (Some(w), Some(h)) => (w, h),
            _ => {
                // Fall back to ffprobe for legacy chunks without cached dimensions
                let probe_output = Command::new("ffprobe")
                    .arg("-v")
                    .arg("error")
                    .arg("-select_streams")
                    .arg("v:0")
                    .arg("-show_entries")
                    .arg("stream=width,height")
                    .arg("-of")
                    .arg("csv=p=0")
                    .arg(video_path)
                    .output()
                    .map_err(|e| anyhow::anyhow!("failed to run ffprobe: {}", e))?;

                let dimensions = String::from_utf8_lossy(&probe_output.stdout);
                let parts: Vec<&str> = dimensions.trim().split(',').collect();
                if parts.len() != 2 {
                    return Err(anyhow::anyhow!("invalid ffprobe output: {}", dimensions));
                }

                let w: u32 = parts[0].parse()?;
                let h: u32 = parts[1].parse()?;
                (w, h)
            }
        };

        // Validate frame data size
        let expected_size = (width * height * 4) as usize;
        if frame_data.len() != expected_size {
            return Err(anyhow::anyhow!(
                "unexpected frame data size: got {}, expected {}",
                frame_data.len(),
                expected_size
            ));
        }

        Ok(FrameData {
            width,
            height,
            data: frame_data,
        })
    }

    /// Insert OCR results in a batch
    fn insert_ocr_batch(&self, results: &[(i64, memoire_ocr::OcrFrameResult)]) -> Result<()> {
        if results.is_empty() {
            return Ok(());
        }

        debug!("inserting {} OCR results", results.len());

        for (frame_id, result) in results {
            let text_json = serde_json::to_string(&result.lines)?;

            let new_ocr = memoire_db::NewOcrText {
                frame_id: *frame_id,
                text: result.text.clone(),
                text_json: Some(text_json),
                confidence: Some(result.confidence as f64),
            };

            memoire_db::insert_ocr_text(self.db.connection(), &new_ocr)?;
        }

        Ok(())
    }

    /// Update statistics
    async fn update_stats(&self, processing_rate: f64) -> Result<()> {
        let total = memoire_db::get_frame_count(self.db.connection())?;
        let with_ocr = memoire_db::get_ocr_count(self.db.connection())?;

        let mut stats = self.stats.write().await;
        stats.total_frames = total as u64;
        stats.frames_with_ocr = with_ocr as u64;
        stats.pending_frames = (total.saturating_sub(with_ocr)) as u64;
        stats.processing_rate = processing_rate;
        stats.last_updated = Utc::now();

        Ok(())
    }
}

/// Create an empty OCR result for frames that fail extraction or OCR
fn empty_ocr_result() -> memoire_ocr::OcrFrameResult {
    memoire_ocr::OcrFrameResult {
        text: String::new(),
        lines: Vec::new(),
        confidence: 0.0,
    }
}
