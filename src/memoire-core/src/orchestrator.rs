//! Orchestrator for running all Memoire components simultaneously
//!
//! Coordinates the lifecycle of recorder, indexers, and web viewer in a single
//! process with unified logging and graceful shutdown.

use anyhow::{Context, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

use crate::config::Config;
use crate::test_config::TestConfig;
use crate::recorder::Recorder;
use crate::indexer::Indexer;
use crate::audio_indexer::AudioIndexer;
use memoire_db::Database;

/// Component health status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentStatus {
    Starting,
    Running,
    Stopped,
    Failed,
}

/// Health monitor for a single component
pub struct ComponentHealth {
    pub name: &'static str,
    pub status: Arc<std::sync::Mutex<ComponentStatus>>,
    pub last_heartbeat: Arc<std::sync::Mutex<Instant>>,
}

impl ComponentHealth {
    fn new(name: &'static str) -> Self {
        Self {
            name,
            status: Arc::new(std::sync::Mutex::new(ComponentStatus::Starting)),
            last_heartbeat: Arc::new(std::sync::Mutex::new(Instant::now())),
        }
    }

    fn update_status(&self, status: ComponentStatus) {
        *self.status.lock().unwrap() = status;
        *self.last_heartbeat.lock().unwrap() = Instant::now();
    }
}

/// Main orchestrator for running all components
pub struct Orchestrator {
    config: TestConfig,
    shutdown: Arc<AtomicBool>,
    components: Vec<ComponentHealth>,
}

impl Orchestrator {
    pub fn new(config: TestConfig) -> Self {
        Self {
            config,
            shutdown: Arc::new(AtomicBool::new(false)),
            components: vec![],
        }
    }

    /// Run all components until shutdown signal
    pub async fn run(self) -> Result<()> {
        info!("🚀 Starting Memoire test orchestrator");

        // Setup Ctrl+C handler
        let shutdown_clone = self.shutdown.clone();
        ctrlc::set_handler(move || {
            info!("🛑 Shutdown signal received");
            shutdown_clone.store(true, Ordering::SeqCst);
        })
        .context("Failed to set Ctrl+C handler")?;

        // Resolve data directory
        let data_dir = self.config.resolve_data_dir();
        info!("📁 Data directory: {}", data_dir.display());
        std::fs::create_dir_all(&data_dir)?;

        // Step 1: Check/download models if needed
        if self.config.audio.enabled && self.config.general.auto_download_models {
            self.ensure_models(&data_dir).await?;
        }

        // Create LocalSet for non-Send futures (Indexer, AudioIndexer use rusqlite)
        let local = tokio::task::LocalSet::new();

        // Step 2: Start viewer first (needs DB to exist)
        let viewer_handle = self.spawn_viewer(&data_dir).await?;

        // Step 3: Create recorder and subscribe to chunk events BEFORE spawning thread
        let (recorder, ocr_events_rx, audio_events_rx) = self.create_recorder_with_subscriptions(&data_dir)?;

        // Step 3b: Spawn recorder thread
        let recorder_handle = self.spawn_recorder_thread(recorder)?;

        // Step 4 & 5: Start indexers in LocalSet (not Send due to rusqlite)
        let data_dir_clone = data_dir.clone();
        let ocr_fps = self.config.index.ocr_fps;
        let ocr_language = self.config.index.ocr_language.clone();
        let audio_enabled = self.config.audio.enabled;
        let shutdown_indexers = self.shutdown.clone();

        let indexers_handle = local.spawn_local(async move {
            // Start OCR indexer
            let data_dir_idx = data_dir_clone.clone();
            let shutdown_ocr = shutdown_indexers.clone();
            let idx_task = tokio::task::spawn_local(async move {
                info!("Starting OCR indexer at {} fps", ocr_fps);
                match Indexer::new(data_dir_idx, Some(ocr_fps), ocr_language) {
                    Ok(mut indexer) => {
                        // Enable event-driven chunk processing
                        indexer.set_chunk_events_receiver(ocr_events_rx);

                        if let Err(e) = indexer.run(shutdown_ocr).await {
                            error!("Indexer error: {}", e);
                        }
                    }
                    Err(e) => error!("Failed to create indexer: {}", e),
                }
                info!("Indexer stopped");
            });

            // Start audio indexer if enabled
            if audio_enabled {
                let data_dir_audio = data_dir_clone;
                let shutdown_audio = shutdown_indexers;
                let audio_task = tokio::task::spawn_local(async move {
                    info!("Starting audio indexer");

                    // Configure ONNX Runtime to use bundled DLL (pattern from main.rs:744-752)
                    let model_dir = data_dir_audio.join("models");
                    if let Err(e) = memoire_stt::configure_onnx_runtime(&model_dir) {
                        error!("Failed to configure ONNX Runtime: {}", e);
                        return;
                    }

                    match AudioIndexer::new(data_dir_audio, false) {
                        Ok(mut indexer) => {
                            // Enable event-driven chunk processing
                            indexer.set_chunk_events_receiver(audio_events_rx);

                            if let Err(e) = indexer.run(shutdown_audio).await {
                                error!("Audio indexer error: {}", e);
                            }
                        }
                        Err(e) => error!("Failed to create audio indexer: {}", e),
                    }
                    info!("Audio indexer stopped");
                });
                let _ = tokio::join!(idx_task, audio_task);
            } else {
                let _ = idx_task.await;
            }
        });

        info!("✅ All components started");

        // Step 6: Wait for shutdown or component failure
        self.wait_for_shutdown_with_local(
            recorder_handle,
            viewer_handle,
            indexers_handle,
            local,
        ).await
    }

    /// Ensure models are downloaded
    async fn ensure_models(&self, data_dir: &std::path::Path) -> Result<()> {
        let model_dir = data_dir.join("models");
        let downloader = memoire_stt::ModelDownloader::new(model_dir.clone());

        if downloader.is_fully_complete() {
            info!("✓ Models already downloaded");
            return Ok(());
        }

        info!("📦 Downloading required models...");

        // Download ONNX Runtime first
        if !downloader.has_ort_dll() {
            info!("Downloading ONNX Runtime 1.22.0...");
            downloader.download_onnx_runtime(false).await
                .context("Failed to download ONNX Runtime")?;
        }

        // Download Parakeet TDT models
        if !downloader.is_complete() {
            info!("Downloading Parakeet TDT models (~630 MB)...");
            downloader.download_all(false).await
                .context("Failed to download STT models")?;
        }

        info!("✓ All models downloaded");
        Ok(())
    }

    /// Create recorder and subscribe to chunk finalization events
    fn create_recorder_with_subscriptions(
        &self,
        data_dir: &std::path::Path,
    ) -> Result<(Recorder, tokio::sync::broadcast::Receiver<crate::recorder::ChunkFinalizedEvent>, tokio::sync::broadcast::Receiver<crate::recorder::ChunkFinalizedEvent>)> {
        let config = Config {
            data_dir: data_dir.to_path_buf(),
            fps: self.config.record.fps.max(1.0) as u32,
            use_hw_encoding: self.config.record.use_hw_encoding,
            chunk_duration_secs: self.config.record.chunk_duration_secs,
        };

        let recorder = Recorder::new(config)?;

        // Subscribe to chunk finalization events (one for OCR, one for audio)
        let ocr_events_rx = recorder.subscribe_to_chunk_events();
        let audio_events_rx = recorder.subscribe_to_chunk_events();

        Ok((recorder, ocr_events_rx, audio_events_rx))
    }

    /// Spawn recorder in blocking thread
    fn spawn_recorder_thread(&self, mut recorder: Recorder) -> Result<thread::JoinHandle<()>> {
        let shutdown = self.shutdown.clone();

        Ok(thread::spawn(move || {
            info!("Starting recorder");

            if let Err(e) = recorder.run(shutdown) {
                error!("Recorder error: {}", e);
            }

            info!("Recorder stopped");
        }))
    }

    /// Spawn recorder in blocking thread (pattern from tray.rs:163-170)
    fn spawn_recorder(&self, data_dir: &std::path::Path) -> Result<thread::JoinHandle<()>> {
        let config = Config {
            data_dir: data_dir.to_path_buf(),
            fps: self.config.record.fps.max(1.0) as u32, // Clamp to minimum 1 FPS to avoid division by zero
            use_hw_encoding: self.config.record.use_hw_encoding,
            chunk_duration_secs: self.config.record.chunk_duration_secs,
        };

        let shutdown = self.shutdown.clone();

        Ok(thread::spawn(move || {
            info!("Starting recorder at {} fps", config.fps);

            match Recorder::new(config) {
                Ok(mut recorder) => {
                    if let Err(e) = recorder.run(shutdown) {
                        error!("Recorder error: {}", e);
                    }
                }
                Err(e) => error!("Failed to create recorder: {}", e),
            }

            info!("Recorder stopped");
        }))
    }

    /// Spawn viewer as async task
    async fn spawn_viewer(&self, data_dir: &std::path::Path) -> Result<JoinHandle<()>> {
        let db_path = data_dir.join("memoire.db");
        let data_dir = data_dir.to_path_buf();
        let port = self.config.viewer.port;

        Ok(tokio::spawn(async move {
            // Wait for DB to exist (created by first recorder chunk)
            for _ in 0..30 {
                if db_path.exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }

            info!("Starting viewer on port {}", port);

            match Database::open(&db_path) {
                Ok(db) => {
                    let connection = db.into_connection();
                    if let Err(e) = memoire_web::serve(connection, data_dir, port).await {
                        error!("Viewer error: {}", e);
                    }
                }
                Err(e) => error!("Failed to open database: {}", e),
            }

            info!("Viewer stopped");
        }))
    }

    /// Wait for shutdown and cleanup (pattern from tray.rs:189-203)
    async fn wait_for_shutdown_with_local(
        &self,
        recorder: thread::JoinHandle<()>,
        viewer: JoinHandle<()>,
        indexers: tokio::task::JoinHandle<()>,
        local: tokio::task::LocalSet,
    ) -> Result<()> {
        // Run LocalSet concurrently with shutdown wait using tokio::select!
        // This ensures indexers actually execute instead of waiting until shutdown
        let shutdown_flag = self.shutdown.clone();

        tokio::select! {
            // Run the LocalSet (this makes indexers actually start!)
            _ = local.run_until(indexers) => {
                info!("Indexers completed");
            }
            // Wait for shutdown signal
            _ = async {
                while !shutdown_flag.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            } => {
                info!("🛑 Shutdown signal received");
            }
        }

        info!("🔄 Shutting down components...");

        // Wait for recorder thread with timeout (pattern from tray.rs:189-203)
        let start = Instant::now();
        let timeout = Duration::from_secs(30);

        info!("Waiting for recorder to finalize...");
        tokio::task::spawn_blocking(move || {
            if recorder.join().is_err() {
                warn!("Recorder thread panicked");
            }
        }).await?;

        // Wait for viewer
        info!("Waiting for async components...");
        tokio::time::timeout(timeout.saturating_sub(start.elapsed()), viewer).await.ok();

        info!("✅ All components stopped");
        Ok(())
    }
}
