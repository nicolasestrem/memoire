//! Memoire - Screen & Audio Capture CLI
//!
//! Phase 1: Screen capture with video encoding and SQLite storage.

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{error, info, warn, Level};
use tracing_subscriber::FmtSubscriber;

mod recorder;
mod config;
mod tray;
mod indexer;
mod audio_indexer;

use recorder::Recorder;
use config::Config;
use tray::TrayApp;
use indexer::Indexer;

#[derive(Parser)]
#[command(name = "memoire")]
#[command(about = "Screen & audio capture with OCR and speech-to-text")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Configuration file path
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Start recording
    Record {
        /// Data directory for videos and database
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// Recording framerate (FPS)
        #[arg(short, long, default_value = "1")]
        fps: u32,

        /// Disable hardware encoding (use software x264)
        #[arg(long)]
        no_hw: bool,
    },

    /// Run in system tray mode
    Tray {
        /// Data directory for videos and database
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// Recording framerate (FPS)
        #[arg(short, long, default_value = "1")]
        fps: u32,

        /// Disable hardware encoding (use software x264)
        #[arg(long)]
        no_hw: bool,
    },

    /// Show system status
    Status,

    /// List monitors
    Monitors,

    /// Check dependencies (FFmpeg, etc.)
    Check,

    /// Start validation viewer web interface
    Viewer {
        /// Data directory for videos and database
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// Web server port
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },

    /// Run OCR indexer on captured frames
    Index {
        /// Data directory for videos and database
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// OCR processing rate (frames per second)
        #[arg(long, default_value = "10")]
        ocr_fps: u32,

        /// OCR language (BCP47 tag, e.g., "en-US", "fr-FR", "de-DE", "ja-JP")
        #[arg(long)]
        ocr_language: Option<String>,
    },

    /// Search OCR text
    Search {
        /// Search query
        query: String,

        /// Data directory for videos and database
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// Maximum number of results
        #[arg(short, long, default_value = "10")]
        limit: i64,
    },

    /// Reset OCR data (clear empty records for re-indexing)
    ResetOcr {
        /// Data directory for videos and database
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// Clear ALL OCR records, not just empty ones
        #[arg(long)]
        all: bool,
    },

    /// List available audio devices
    AudioDevices,

    /// Record audio only (for testing audio capture)
    RecordAudio {
        /// Data directory for audio files
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// Audio device ID (from audio-devices command)
        #[arg(long)]
        device: Option<String>,

        /// Chunk duration in seconds
        #[arg(long, default_value = "30")]
        chunk_secs: u64,

        /// Enable loopback mode (capture system audio instead of microphone)
        #[arg(long)]
        loopback: bool,
    },

    /// Run audio transcription indexer
    AudioIndex {
        /// Data directory for videos and database
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// Disable GPU acceleration
        #[arg(long)]
        no_gpu: bool,
    },

    /// Download Parakeet TDT speech-to-text models
    DownloadModels {
        /// Data directory for models
        #[arg(short, long)]
        data_dir: Option<PathBuf>,

        /// Force re-download even if models exist
        #[arg(long)]
        force: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let level = if cli.verbose { Level::DEBUG } else { Level::INFO };
    FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .compact()
        .init();

    match cli.command {
        Commands::Record { data_dir, fps, no_hw } => {
            cmd_record(data_dir, fps, !no_hw)?;
        }
        Commands::Tray { data_dir, fps, no_hw } => {
            cmd_tray(data_dir, fps, !no_hw)?;
        }
        Commands::Status => {
            cmd_status()?;
        }
        Commands::Monitors => {
            cmd_monitors()?;
        }
        Commands::Check => {
            cmd_check()?;
        }
        Commands::Viewer { data_dir, port } => {
            cmd_viewer(data_dir, port)?;
        }
        Commands::Index { data_dir, ocr_fps, ocr_language } => {
            cmd_index(data_dir, ocr_fps, ocr_language)?;
        }
        Commands::Search { query, data_dir, limit } => {
            cmd_search(query, data_dir, limit)?;
        }
        Commands::ResetOcr { data_dir, all } => {
            cmd_reset_ocr(data_dir, all)?;
        }
        Commands::AudioDevices => {
            cmd_audio_devices()?;
        }
        Commands::RecordAudio { data_dir, device, chunk_secs, loopback } => {
            cmd_record_audio(data_dir, device, chunk_secs, loopback)?;
        }
        Commands::AudioIndex { data_dir, no_gpu } => {
            cmd_audio_index(data_dir, !no_gpu)?;
        }
        Commands::DownloadModels { data_dir, force } => {
            cmd_download_models(data_dir, force)?;
        }
    }

    Ok(())
}

fn cmd_record(data_dir: Option<PathBuf>, fps: u32, use_hw: bool) -> Result<()> {
    // Resolve data directory
    let data_dir = data_dir.unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Memoire")
    });

    info!("starting memoire recorder");
    info!("data directory: {:?}", data_dir);
    info!("fps: {}, hardware encoding: {}", fps, use_hw);

    // Check FFmpeg
    if !memoire_processing::encoder::check_ffmpeg() {
        error!("ffmpeg not found in PATH - please install FFmpeg");
        return Err(anyhow::anyhow!("FFmpeg not found"));
    }

    if use_hw && !memoire_processing::encoder::check_nvenc() {
        warn!("NVENC not available, will fall back to software encoding");
    }

    // Setup signal handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        info!("received shutdown signal");
        r.store(false, Ordering::SeqCst);
    })?;

    // Create and start recorder
    let config = Config {
        data_dir,
        fps,
        use_hw_encoding: use_hw,
        chunk_duration_secs: 300, // 5 minutes
    };

    let mut recorder = Recorder::new(config)?;
    recorder.run(running)?;

    info!("recorder stopped");
    Ok(())
}

fn cmd_tray(data_dir: Option<PathBuf>, fps: u32, use_hw: bool) -> Result<()> {
    // Resolve data directory
    let data_dir = data_dir.unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Memoire")
    });

    // Create directories
    std::fs::create_dir_all(&data_dir)?;
    std::fs::create_dir_all(data_dir.join("videos"))?;

    info!("starting memoire tray");
    info!("data directory: {:?}", data_dir);

    // Check FFmpeg
    if !memoire_processing::encoder::check_ffmpeg() {
        error!("ffmpeg not found in PATH - please install FFmpeg");
        return Err(anyhow::anyhow!("FFmpeg not found"));
    }

    let config = Config {
        data_dir,
        fps,
        use_hw_encoding: use_hw,
        chunk_duration_secs: 300,
    };

    let app = TrayApp::new(config);
    app.run()?;

    Ok(())
}

fn cmd_status() -> Result<()> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Memoire");

    let db_path = data_dir.join("memoire.db");

    if !db_path.exists() {
        println!("status: not initialized");
        println!("database: not found");
        return Ok(());
    }

    let db = memoire_db::Database::open(&db_path)?;
    let frame_count = memoire_db::get_frame_count(db.connection())?;
    let ocr_count = memoire_db::get_ocr_count(db.connection())?;

    println!("status: ready");
    println!("database: {:?}", db_path);
    println!("total frames: {}", frame_count);
    println!("frames with OCR: {}", ocr_count);

    if frame_count > 0 {
        let percentage = (ocr_count as f64 / frame_count as f64) * 100.0;
        let pending = frame_count - ocr_count;
        println!("OCR progress: {:.1}% ({} pending)", percentage, pending);
    }

    // Get latest chunk
    if let Some(chunk) = memoire_db::get_latest_video_chunk(db.connection())? {
        println!("latest chunk: {}", chunk.file_path);
        println!("recorded at: {}", chunk.created_at);
    }

    Ok(())
}

fn cmd_monitors() -> Result<()> {
    let monitors = memoire_capture::Monitor::enumerate_all()?;

    println!("found {} monitor(s):\n", monitors.len());

    for (i, m) in monitors.iter().enumerate() {
        println!(
            "  [{}] {} - {}x{} {}",
            i,
            m.name,
            m.width,
            m.height,
            if m.is_primary { "(primary)" } else { "" }
        );
    }

    Ok(())
}

fn cmd_check() -> Result<()> {
    println!("checking dependencies...\n");

    // FFmpeg
    let ffmpeg_ok = memoire_processing::encoder::check_ffmpeg();
    println!(
        "  ffmpeg: {}",
        if ffmpeg_ok { "OK" } else { "NOT FOUND" }
    );

    // NVENC
    if ffmpeg_ok {
        let nvenc_ok = memoire_processing::encoder::check_nvenc();
        println!(
            "  nvenc:  {}",
            if nvenc_ok { "OK" } else { "not available (will use software encoding)" }
        );
    }

    // Monitors
    let monitors = memoire_capture::Monitor::enumerate_all()?;
    println!("  monitors: {} found", monitors.len());

    println!();

    if !ffmpeg_ok {
        println!("WARNING: FFmpeg is required for video encoding.");
        println!("Please install FFmpeg and ensure it's in your PATH.");
        println!("Download: https://ffmpeg.org/download.html");
    } else {
        println!("all checks passed!");
    }

    Ok(())
}

#[tokio::main]
async fn cmd_viewer(data_dir: Option<PathBuf>, port: u16) -> Result<()> {
    // Resolve data directory
    let data_dir = data_dir.unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Memoire")
    });

    let db_path = data_dir.join("memoire.db");

    if !db_path.exists() {
        error!("database not found at {:?}", db_path);
        error!("please run 'memoire record' first to initialize the database");
        return Err(anyhow::anyhow!("database not found"));
    }

    info!("starting memoire validation viewer");
    info!("data directory: {:?}", data_dir);
    info!("database: {:?}", db_path);
    info!("web interface: http://localhost:{}", port);

    // Open database connection
    let db = memoire_db::Database::open(&db_path)?;
    let connection = db.into_connection();

    // Start web server
    memoire_web::serve(connection, data_dir, port).await?;

    Ok(())
}

#[tokio::main]
async fn cmd_index(data_dir: Option<PathBuf>, ocr_fps: u32, ocr_language: Option<String>) -> Result<()> {
    // Resolve data directory
    let data_dir = data_dir.unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Memoire")
    });

    let db_path = data_dir.join("memoire.db");

    if !db_path.exists() {
        error!("database not found at {:?}", db_path);
        error!("please run 'memoire record' first to initialize the database");
        return Err(anyhow::anyhow!("database not found"));
    }

    info!("starting OCR indexer");
    info!("data directory: {:?}", data_dir);
    info!("OCR rate: {} fps", ocr_fps);
    if let Some(ref lang) = ocr_language {
        info!("OCR language: {}", lang);
    } else {
        info!("OCR language: en-US (default)");
    }

    // Create indexer
    let mut indexer = Indexer::new(data_dir, Some(ocr_fps), ocr_language)?;

    // Set up signal handler for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        warn!("received shutdown signal, stopping indexer...");
        r.store(false, Ordering::Relaxed);
    })?;

    // Run indexer until Ctrl+C
    indexer.run().await?;

    info!("indexer stopped");
    Ok(())
}

fn cmd_search(query: String, data_dir: Option<PathBuf>, limit: i64) -> Result<()> {
    // Resolve data directory
    let data_dir = data_dir.unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Memoire")
    });

    let db_path = data_dir.join("memoire.db");

    if !db_path.exists() {
        error!("database not found at {:?}", db_path);
        error!("please run 'memoire record' first to initialize the database");
        return Err(anyhow::anyhow!("database not found"));
    }

    info!("searching for: '{}'", query);

    // Open database
    let db = memoire_db::Database::open(&db_path)?;

    // Perform search
    let results = memoire_db::search_ocr(db.connection(), &query, limit, 0)?;

    if results.is_empty() {
        println!("no results found for query: '{}'", query);
        return Ok(());
    }

    println!("found {} result(s):\n", results.len());

    for (i, (ocr, frame)) in results.iter().enumerate() {
        println!("{}. Frame ID: {}", i + 1, frame.id);
        println!("   Timestamp: {}", frame.timestamp);

        // Get video chunk for device name
        if let Ok(Some(chunk)) = memoire_db::get_video_chunk(db.connection(), frame.video_chunk_id) {
            println!("   Device: {}", chunk.device_name);
        }

        // Show snippet of text (first 150 chars)
        let snippet = if ocr.text.len() > 150 {
            format!("{}...", &ocr.text[..150])
        } else {
            ocr.text.clone()
        };
        println!("   Text: {}", snippet.replace('\n', " "));

        if let Some(conf) = ocr.confidence {
            println!("   Confidence: {:.2}%", conf * 100.0);
        }

        println!();
    }

    Ok(())
}

fn cmd_reset_ocr(data_dir: Option<PathBuf>, clear_all: bool) -> Result<()> {
    // Resolve data directory
    let data_dir = data_dir.unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Memoire")
    });

    let db_path = data_dir.join("memoire.db");
    let db = memoire_db::Database::open(&db_path)?;

    if clear_all {
        println!("clearing ALL OCR records...");
        memoire_db::reset_all_ocr(db.connection())?;
        println!("✓ all OCR records cleared");
    } else {
        println!("clearing empty OCR records...");
        let deleted = memoire_db::reset_empty_ocr(db.connection())?;
        println!("✓ cleared {} empty OCR records", deleted);
    }

    Ok(())
}

fn cmd_audio_devices() -> Result<()> {
    println!("enumerating audio devices...\n");

    let devices = memoire_capture::AudioCapture::enumerate_devices()?;

    if devices.is_empty() {
        println!("no audio devices found");
        return Ok(());
    }

    println!("found {} audio device(s):\n", devices.len());

    for device in &devices {
        let default_marker = if device.is_default { " (default)" } else { "" };
        let type_str = if device.is_input { "input" } else { "output" };

        println!("  [{}] {}{}", type_str, device.name, default_marker);
        println!("      ID: {}", device.id);
        println!("      Channels: {}, Sample Rate: {} Hz, Bits: {}",
            device.channels, device.sample_rate, device.bits_per_sample);
        println!();
    }

    Ok(())
}

#[tokio::main]
async fn cmd_record_audio(data_dir: Option<PathBuf>, device_id: Option<String>, chunk_secs: u64, loopback: bool) -> Result<()> {
    // Resolve data directory
    let data_dir = data_dir.unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Memoire")
    });

    // Create audio output directory
    let audio_dir = data_dir.join("audio");
    std::fs::create_dir_all(&audio_dir)?;

    info!("starting audio capture (loopback={})", loopback);
    info!("data directory: {:?}", data_dir);
    info!("chunk duration: {} seconds", chunk_secs);

    // Set up signal handler for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        warn!("received shutdown signal, stopping audio capture...");
        r.store(false, Ordering::Relaxed);
    })?;

    // Configure audio capture
    let config = memoire_capture::AudioCaptureConfig {
        device_id: device_id.clone(),
        is_loopback: loopback,
        target_sample_rate: 16000, // 16kHz for STT
        target_channels: 1,        // mono for STT
        chunk_duration_secs: chunk_secs as u32,
    };

    // Start audio capture
    let mut capture = memoire_capture::AudioCapture::new(config)?;
    let mut rx = capture.start()?;

    info!("audio capture started, press Ctrl+C to stop");

    // Open database for storing audio chunks
    let db_path = data_dir.join("memoire.db");
    let db = memoire_db::Database::open(&db_path)?;

    // Create audio encoder
    let encoder_config = memoire_processing::AudioEncoderConfig {
        output_dir: audio_dir,
        chunk_duration_secs: chunk_secs as u32,
        sample_rate: 16000,
        channels: 1,
    };
    let device_name_for_encoder = device_id.as_deref().unwrap_or("default");
    let mut encoder = memoire_processing::AudioEncoder::new(encoder_config, device_name_for_encoder)?;

    // Receive and process audio chunks
    let mut chunk_count = 0;
    while running.load(Ordering::Relaxed) {
        match tokio::time::timeout(
            std::time::Duration::from_millis(500),
            rx.recv()
        ).await {
            Ok(Some(audio)) => {
                chunk_count += 1;
                info!("received audio chunk {}: {} samples, {:.1}s",
                    chunk_count, audio.samples.len(), audio.duration_secs);

                // Save to file using encoder
                if let Some(file_path) = encoder.add_samples(&audio.samples, audio.timestamp)? {
                    info!("saved audio chunk: {:?}", file_path);

                    // Insert into database
                    let new_chunk = memoire_db::NewAudioChunk {
                        file_path: file_path.to_string_lossy().to_string(),
                        device_name: Some(audio.device_name.clone()),
                        is_input_device: Some(true),
                    };
                    memoire_db::insert_audio_chunk(db.connection(), &new_chunk)?;
                }
            }
            Ok(None) => {
                // Channel closed
                break;
            }
            Err(_) => {
                // Timeout - check if still running
                continue;
            }
        }
    }

    // Finalize any remaining audio
    if let Some(file_path) = encoder.finalize_chunk()? {
        info!("saved final audio chunk: {:?}", file_path);

        let new_chunk = memoire_db::NewAudioChunk {
            file_path: file_path.to_string_lossy().to_string(),
            device_name: None,
            is_input_device: Some(true),
        };
        memoire_db::insert_audio_chunk(db.connection(), &new_chunk)?;
    }

    capture.stop();
    info!("audio capture stopped, {} chunks recorded", chunk_count);

    Ok(())
}

#[tokio::main]
async fn cmd_audio_index(data_dir: Option<PathBuf>, use_gpu: bool) -> Result<()> {
    // Resolve data directory
    let data_dir = data_dir.unwrap_or_else(|| {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Memoire")
    });

    let db_path = data_dir.join("memoire.db");

    if !db_path.exists() {
        error!("database not found at {:?}", db_path);
        error!("please run 'memoire record' first to initialize the database");
        return Err(anyhow::anyhow!("database not found"));
    }

    info!("starting audio transcription indexer");
    info!("data directory: {:?}", data_dir);
    info!("GPU enabled: {}", use_gpu);

    // Configure ONNX Runtime to use bundled DLL (required for ort 2.0.0-rc.10)
    // This must be done BEFORE creating the STT engine
    let model_dir = data_dir.join("models");
    if memoire_stt::has_bundled_onnx_runtime(&model_dir) {
        memoire_stt::configure_onnx_runtime(&model_dir)?;
    } else {
        warn!("bundled ONNX Runtime not found, using system DLL");
        warn!("if you get version errors, run 'memoire download-models' first");
    }

    // Create indexer
    let mut indexer = audio_indexer::AudioIndexer::new(data_dir, use_gpu)?;

    // Set up signal handler for graceful shutdown
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        warn!("received shutdown signal, stopping audio indexer...");
        r.store(false, Ordering::Relaxed);
    })?;

    // Run indexer until Ctrl+C
    indexer.run().await?;

    info!("audio indexer stopped");
    Ok(())
}

#[tokio::main]
async fn cmd_download_models(data_dir: Option<PathBuf>, force: bool) -> Result<()> {
    // Resolve model directory
    let model_dir = data_dir
        .map(|d| d.join("models"))
        .unwrap_or_else(memoire_stt::default_model_dir);

    info!("model directory: {:?}", model_dir);

    // Create downloader
    let downloader = memoire_stt::ModelDownloader::new(model_dir.clone());

    // Check if everything already exists
    if !force && downloader.is_fully_complete() {
        println!("All models and ONNX Runtime already downloaded at {:?}", model_dir);
        println!("Use --force to re-download");
        return Ok(());
    }

    // Show what's missing
    let missing = downloader.missing_files();
    if !missing.is_empty() && !force {
        println!("Missing model files: {:?}", missing);
    }
    if !downloader.has_ort_dll() {
        println!("ONNX Runtime DLL not found, will download v1.22.0");
    }

    // Download ONNX Runtime first (required for model inference)
    // This is needed because the system may have an incompatible version (e.g., 1.17.1)
    println!("Downloading ONNX Runtime 1.22.0 (~50 MB)...\n");
    downloader.download_onnx_runtime(force).await?;

    // Download models
    println!("\nDownloading Parakeet TDT models (~630 MB total)...\n");
    downloader.download_all(force).await?;

    println!("\nDownload complete! You can now run 'memoire audio-index' to transcribe audio.");
    Ok(())
}
