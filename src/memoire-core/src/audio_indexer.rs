//! Background audio indexer for processing captured audio chunks with speech-to-text

use anyhow::Result;
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, error, info, warn};

use memoire_db::Database;
use memoire_stt::{SttConfig, SttEngine};

use crate::recorder::ChunkFinalizedEvent;

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
    chunk_events_rx: Option<broadcast::Receiver<ChunkFinalizedEvent>>,
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
            running: Arc::new(AtomicBool::new(true)), // Start as running
            stats: Arc::new(RwLock::new(stats)),
            processed_count: Arc::new(AtomicU64::new(0)),
            chunk_events_rx: None, // Will be set via set_chunk_events_receiver()
        })
    }

    /// Set the chunk finalization event receiver
    ///
    /// This enables event-driven processing for immediate transcription of finalized audio chunks.
    pub fn set_chunk_events_receiver(&mut self, rx: broadcast::Receiver<ChunkFinalizedEvent>) {
        info!("Audio indexer now using event-driven chunk processing");
        self.chunk_events_rx = Some(rx);
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
    pub async fn run(&mut self, shutdown: Arc<AtomicBool>) -> Result<()> {
        info!("starting audio indexer");
        self.running.store(true, Ordering::Relaxed);

        let poll_interval = Duration::from_secs(30); // Poll every 30 seconds as fallback
        let mut batch_start = Instant::now();
        let mut batch_count = 0u64;

        // Take ownership of the receiver if present
        let mut chunk_rx = self.chunk_events_rx.take();
        let use_events = chunk_rx.is_some();

        if use_events {
            info!("Audio indexer using event-driven mode with {} second fallback polling",
                  poll_interval.as_secs());
        } else {
            info!("Audio indexer using polling mode every {} seconds",
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
                                debug!("received chunk finalized event for chunk {} (audio)",
                                       evt.chunk_id);
                                // Process audio chunks for this video chunk immediately
                                // Note: Audio chunks are associated with video chunks for timing
                                match self.process_batch().await {
                                    Ok(count) if count > 0 => {
                                        batch_count += count as u64;
                                        info!("transcribed {} audio chunks (event-driven)",
                                              count);
                                    }
                                    Ok(_) => {
                                        debug!("no audio chunks pending for processing");
                                    }
                                    Err(e) => {
                                        error!("error processing audio chunks: {}", e);
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                warn!("audio indexer lagged, skipped {} chunk events", skipped);
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                warn!("chunk event channel closed, switching to polling mode");
                                chunk_rx = None;
                            }
                        }
                    }

                    // Branch 2: Timeout - poll for any missed chunks (fallback)
                    _ = tokio::time::sleep(poll_interval) => {
                        match self.process_batch().await {
                            Ok(count) if count > 0 => {
                                batch_count += count as u64;
                                debug!("transcribed {} audio chunks via polling fallback", count);
                            }
                            Ok(_) => {} // No chunks pending
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
                        debug!("transcribed {} audio chunks via polling", count);
                    }
                    Ok(_) => {
                        // No chunks, sleep before next poll
                        tokio::time::sleep(poll_interval).await;
                    }
                    Err(e) => {
                        error!("batch processing error: {}", e);
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
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

            // Transcribe the audio file (blocking operation - run in thread pool)
            let audio_path_clone = audio_path.clone();
            let transcribe_result = tokio::task::spawn_blocking(move || {
                // Create a temporary STT engine for this thread
                // Note: We can't share the engine across threads easily
                let stt_config = SttConfig {
                    model_dir: memoire_stt::default_model_dir(),
                    use_gpu: false, // Use CPU for thread pool tasks
                    language: None,
                    num_threads: 1,
                };
                let engine = SttEngine::new(stt_config)?;
                engine.transcribe_file(&audio_path_clone)
            }).await;

            match transcribe_result {
                Ok(Ok(result)) => {
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
                Ok(Err(e)) => {
                    warn!("failed to transcribe chunk {}: {}", chunk.id, e);
                    // Insert empty transcription to mark as processed
                    self.insert_empty_transcription(chunk.id)?;
                }
                Err(e) => {
                    warn!("task join error transcribing chunk {}: {}", chunk.id, e);
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
