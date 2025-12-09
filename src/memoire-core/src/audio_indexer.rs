//! Background audio indexer for processing captured audio chunks with speech-to-text

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use memoire_db::Database;
use memoire_stt::{SttConfig, SttEngine};

/// Audio indexer batch settings
const AUDIO_BATCH_SIZE: i64 = 5;
/// Default maximum chunks to process per second
const DEFAULT_CHUNKS_PER_SEC: f64 = 2.0;

/// Statistics for audio transcription processing
#[derive(Debug, Clone)]
pub struct AudioIndexerStats {
    pub total_chunks: u64,
    pub chunks_with_transcription: u64,
    pub pending_chunks: u64,
    pub processing_rate: f64, // chunks/sec
    pub last_updated: DateTime<Utc>,
}

/// Audio Indexer that transcribes audio chunks in background
pub struct AudioIndexer {
    db: Database,
    stt_engine: SttEngine,
    data_dir: PathBuf,
    chunks_per_sec: f64,
    running: Arc<AtomicBool>,
    stats: Arc<RwLock<AudioIndexerStats>>,
    processed_count: Arc<AtomicU64>,
}

impl AudioIndexer {
    /// Create a new audio indexer
    pub fn new(data_dir: PathBuf, use_gpu: bool) -> Result<Self> {
        info!("initializing audio indexer");

        let db_path = data_dir.join("memoire.db");
        let db = Database::open(&db_path)?;
        info!("database opened at {:?}", db_path);

        // Create STT engine
        let stt_config = SttConfig {
            model_dir: memoire_stt::default_model_dir(),
            use_gpu,
            language: None, // Auto-detect
            num_threads: 4,
        };

        let stt_engine = SttEngine::new(stt_config)?;
        info!(
            "STT engine initialized (GPU: {}, model loaded: {})",
            stt_engine.is_gpu_enabled(),
            stt_engine.is_model_loaded()
        );

        let stats = AudioIndexerStats {
            total_chunks: 0,
            chunks_with_transcription: 0,
            pending_chunks: 0,
            processing_rate: 0.0,
            last_updated: Utc::now(),
        };

        Ok(Self {
            db,
            stt_engine,
            data_dir,
            chunks_per_sec: DEFAULT_CHUNKS_PER_SEC,
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(RwLock::new(stats)),
            processed_count: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Get current statistics
    pub async fn get_stats(&self) -> AudioIndexerStats {
        self.stats.read().await.clone()
    }

    /// Check if indexer is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Run the indexer (blocks until stopped)
    pub async fn run(&mut self) -> Result<()> {
        info!("starting audio indexer at {} chunks/sec", self.chunks_per_sec);
        self.running.store(true, Ordering::Relaxed);

        let chunk_interval = Duration::from_secs_f64(1.0 / self.chunks_per_sec);
        let mut last_chunk_time = Instant::now();
        let mut batch_start = Instant::now();
        let mut batch_count = 0u64;

        while self.running.load(Ordering::Relaxed) {
            // Rate limiting
            let elapsed = last_chunk_time.elapsed();
            if elapsed < chunk_interval {
                tokio::time::sleep(chunk_interval - elapsed).await;
            }
            last_chunk_time = Instant::now();

            // Process next batch of chunks
            match self.process_batch().await {
                Ok(count) => {
                    if count > 0 {
                        batch_count += count as u64;
                        debug!("transcribed {} audio chunks", count);
                    } else {
                        // No chunks to process, sleep longer
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
                Err(e) => {
                    error!("batch processing error: {}", e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }

            // Update stats every 30 seconds
            if batch_start.elapsed() >= Duration::from_secs(30) {
                let elapsed_secs = batch_start.elapsed().as_secs_f64();
                let rate = batch_count as f64 / elapsed_secs;

                self.update_stats(rate).await?;

                batch_start = Instant::now();
                batch_count = 0;

                let stats = self.stats.read().await;
                info!(
                    "audio transcription progress: {}/{} chunks processed ({} pending, {:.2} chunks/sec)",
                    stats.chunks_with_transcription,
                    stats.total_chunks,
                    stats.pending_chunks,
                    stats.processing_rate
                );
            }
        }

        info!("audio indexer stopped");
        Ok(())
    }

    /// Stop the indexer gracefully
    pub fn stop(&self) {
        info!("stopping audio indexer");
        self.running.store(false, Ordering::Relaxed);
    }

    /// Process a batch of audio chunks without transcription
    async fn process_batch(&mut self) -> Result<usize> {
        // Query audio chunks without transcription
        let chunks = memoire_db::get_audio_chunks_without_transcription(
            self.db.connection(),
            AUDIO_BATCH_SIZE,
        )?;

        if chunks.is_empty() {
            return Ok(0);
        }

        info!("processing batch of {} audio chunks", chunks.len());

        let mut processed_count = 0;

        for chunk in &chunks {
            // Resolve the audio file path
            let audio_path = self.data_dir.join(&chunk.file_path);

            if !audio_path.exists() {
                warn!("audio file not found: {:?}", audio_path);
                // Insert empty transcription to mark as processed
                self.insert_empty_transcription(chunk.id)?;
                processed_count += 1;
                continue;
            }

            // Transcribe the audio file
            match self.stt_engine.transcribe_file(&audio_path) {
                Ok(result) => {
                    // Insert transcription segments
                    for segment in &result.segments {
                        let new_transcription = memoire_db::NewAudioTranscription {
                            audio_chunk_id: chunk.id,
                            transcription: segment.text.clone(),
                            timestamp: chunk.timestamp,
                            speaker_id: None,
                            start_time: Some(segment.start),
                            end_time: Some(segment.end),
                        };
                        memoire_db::insert_audio_transcription(
                            self.db.connection(),
                            &new_transcription,
                        )?;
                    }

                    // If no segments, insert the full text as a single transcription
                    if result.segments.is_empty() && !result.text.is_empty() {
                        let new_transcription = memoire_db::NewAudioTranscription {
                            audio_chunk_id: chunk.id,
                            transcription: result.text.clone(),
                            timestamp: chunk.timestamp,
                            speaker_id: None,
                            start_time: None,
                            end_time: None,
                        };
                        memoire_db::insert_audio_transcription(
                            self.db.connection(),
                            &new_transcription,
                        )?;
                    } else if result.segments.is_empty() {
                        // Insert empty transcription to mark as processed
                        self.insert_empty_transcription(chunk.id)?;
                    }

                    info!(
                        "transcribed chunk {}: '{}' ({} chars, {} segments, {}ms)",
                        chunk.id,
                        if result.text.len() > 100 { &result.text[..100] } else { &result.text },
                        result.text.len(),
                        result.segments.len(),
                        result.processing_time_ms
                    );
                }
                Err(e) => {
                    warn!("failed to transcribe chunk {}: {}", chunk.id, e);
                    // Insert empty transcription to mark as processed
                    self.insert_empty_transcription(chunk.id)?;
                }
            }

            processed_count += 1;
        }

        self.processed_count.fetch_add(processed_count as u64, Ordering::Relaxed);

        Ok(processed_count)
    }

    /// Insert an empty transcription to mark a chunk as processed
    fn insert_empty_transcription(&self, chunk_id: i64) -> Result<()> {
        let new_transcription = memoire_db::NewAudioTranscription {
            audio_chunk_id: chunk_id,
            transcription: String::new(),
            timestamp: Utc::now(),
            speaker_id: None,
            start_time: None,
            end_time: None,
        };
        memoire_db::insert_audio_transcription(self.db.connection(), &new_transcription)?;
        Ok(())
    }

    /// Update statistics
    async fn update_stats(&self, processing_rate: f64) -> Result<()> {
        let stats = memoire_db::get_audio_stats(self.db.connection())?;

        let mut s = self.stats.write().await;
        s.total_chunks = stats.total_chunks as u64;
        s.chunks_with_transcription = stats.chunks_with_transcription as u64;
        s.pending_chunks = stats.pending_chunks as u64;
        s.processing_rate = processing_rate;
        s.last_updated = Utc::now();

        Ok(())
    }
}
