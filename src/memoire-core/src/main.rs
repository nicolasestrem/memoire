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

use recorder::Recorder;
use config::Config;
use tray::TrayApp;

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

    println!("status: ready");
    println!("database: {:?}", db_path);
    println!("total frames: {}", frame_count);

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
