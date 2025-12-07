//! Background OCR indexer for processing captured frames

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use memoire_db::{Database, Frame};
use memoire_ocr::{FrameData, Processor as OcrProcessor};

/// OCR batch settings
const OCR_BATCH_SIZE: usize = 30;
const DEFAULT_OCR_FPS: u32 = 10;

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
}

impl Indexer {
    /// Create a new indexer
    pub fn new(data_dir: PathBuf, ocr_fps: Option<u32>) -> Result<Self> {
        info!("initializing OCR indexer");

        let db_path = data_dir.join("memoire.db");
        let db = Database::open(&db_path)?;
        info!("database opened at {:?}", db_path);

        let processor = OcrProcessor::new()?;
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
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(RwLock::new(stats)),
            processed_count: Arc::new(AtomicU64::new(0)),
        })
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
    pub async fn run(&mut self) -> Result<()> {
        info!("starting OCR indexer at {} fps", self.ocr_fps);
        self.running.store(true, Ordering::Relaxed);

        let frame_interval = Duration::from_secs_f64(1.0 / self.ocr_fps as f64);
        let mut last_frame_time = Instant::now();
        let mut batch_start = Instant::now();
        let mut batch_count = 0u64;

        while self.running.load(Ordering::Relaxed) {
            // Rate limiting
            let elapsed = last_frame_time.elapsed();
            if elapsed < frame_interval {
                tokio::time::sleep(frame_interval - elapsed).await;
            }
            last_frame_time = Instant::now();

            // Process next batch of frames
            match self.process_batch().await {
                Ok(count) => {
                    if count > 0 {
                        batch_count += count as u64;
                        debug!("processed {} frames", count);
                    } else {
                        // No frames to process, sleep longer
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
                Err(e) => {
                    error!("batch processing error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
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

        debug!("processing batch of {} frames", frames.len());

        // Extract frames from video chunks
        let mut ocr_results = Vec::new();

        for frame in &frames {
            match self.extract_and_process_frame(frame).await {
                Ok(result) => {
                    ocr_results.push((frame.id, result));
                }
                Err(e) => {
                    warn!("failed to process frame {}: {}", frame.id, e);
                    // Insert empty OCR result to mark as processed
                    ocr_results.push((
                        frame.id,
                        memoire_ocr::OcrFrameResult {
                            text: String::new(),
                            lines: Vec::new(),
                            confidence: 0.0,
                        },
                    ));
                }
            }
        }

        // Batch insert OCR results
        self.insert_ocr_batch(&ocr_results)?;

        let count = ocr_results.len();
        self.processed_count.fetch_add(count as u64, Ordering::Relaxed);

        Ok(count)
    }

    /// Extract frame from video and perform OCR
    async fn extract_and_process_frame(&self, frame: &Frame) -> Result<memoire_ocr::OcrFrameResult> {
        // Get video chunk info
        let chunk = memoire_db::get_video_chunk(self.db.connection(), frame.video_chunk_id)?
            .ok_or_else(|| anyhow::anyhow!("video chunk {} not found", frame.video_chunk_id))?;

        // Build full path to video file
        let video_path = self.data_dir.join(&chunk.file_path);

        if !video_path.exists() {
            return Err(anyhow::anyhow!("video file not found: {:?}", video_path));
        }

        // Extract frame using FFmpeg
        let frame_data = self.extract_frame_from_video(&video_path, frame.offset_index)?;

        // Perform OCR
        let result = self.processor.process_frame(frame_data).await?;

        Ok(result)
    }

    /// Extract a specific frame from video using FFmpeg command-line tool
    fn extract_frame_from_video(&self, video_path: &PathBuf, frame_index: i64) -> Result<FrameData> {
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

        // Get video dimensions using ffprobe
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

        let width: u32 = parts[0].parse()?;
        let height: u32 = parts[1].parse()?;

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
