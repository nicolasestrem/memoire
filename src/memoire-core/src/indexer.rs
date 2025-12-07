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

    /// Extract a specific frame from video using FFmpeg
    fn extract_frame_from_video(&self, video_path: &PathBuf, frame_index: i64) -> Result<FrameData> {
        use ffmpeg_next as ffmpeg;

        // Open input file
        let mut ictx = ffmpeg::format::input(video_path)?;

        // Find video stream
        let input = ictx
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or_else(|| anyhow::anyhow!("no video stream found"))?;
        let stream_index = input.index();

        // Create decoder
        let context_decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())?;
        let mut decoder = context_decoder.decoder().video()?;

        let mut current_frame = 0i64;
        let mut target_frame_data: Option<FrameData> = None;

        // Read packets until we find the target frame
        for (stream, packet) in ictx.packets() {
            if stream.index() == stream_index {
                decoder.send_packet(&packet)?;

                let mut decoded = ffmpeg::util::frame::Video::empty();
                while decoder.receive_frame(&mut decoded).is_ok() {
                    if current_frame == frame_index {
                        // Convert to RGBA
                        let mut scaler = ffmpeg::software::scaling::context::Context::get(
                            decoder.format(),
                            decoder.width(),
                            decoder.height(),
                            ffmpeg::format::Pixel::RGBA,
                            decoder.width(),
                            decoder.height(),
                            ffmpeg::software::scaling::Flags::BILINEAR,
                        )?;

                        let mut rgba_frame = ffmpeg::util::frame::Video::empty();
                        scaler.run(&decoded, &mut rgba_frame)?;

                        // Copy data
                        let data = rgba_frame.data(0).to_vec();
                        target_frame_data = Some(FrameData {
                            width: rgba_frame.width(),
                            height: rgba_frame.height(),
                            data,
                        });
                        break;
                    }
                    current_frame += 1;
                }

                if target_frame_data.is_some() {
                    break;
                }
            }
        }

        decoder.send_eof()?;

        target_frame_data.ok_or_else(|| anyhow::anyhow!("frame {} not found in video", frame_index))
    }

    /// Insert OCR results in a batch
    fn insert_ocr_batch(&self, results: &[(i64, memoire_ocr::OcrFrameResult)]) -> Result<()> {
        if results.is_empty() {
            return Ok(());
        }

        debug!("inserting {} OCR results", results.len());

        for (frame_id, result) in results {
            let text_json = serde_json::to_string(&result.lines)?;

            memoire_db::insert_ocr_text(
                self.db.connection(),
                *frame_id,
                &result.text,
                Some(&text_json),
                Some(result.confidence),
            )?;
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
