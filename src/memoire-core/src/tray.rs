//! System tray interface for Memoire

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder};
use tray_icon::{
    menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, CheckMenuItem},
    TrayIconBuilder, Icon,
};
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::recorder::Recorder;

/// Menu item IDs
const ID_START_STOP: &str = "start_stop";
const ID_VIDEO_TOGGLE: &str = "video_toggle";
const ID_AUDIO_TOGGLE: &str = "audio_toggle";
const ID_STATUS: &str = "status";
const ID_EXIT: &str = "exit";

/// Recording state shared between tray and recorder
pub struct RecordingState {
    pub is_recording: AtomicBool,
    pub recorder_running: AtomicBool,  // True while recorder thread is active
    pub video_enabled: AtomicBool,
    pub audio_enabled: AtomicBool,
    pub should_exit: AtomicBool,
}

impl Default for RecordingState {
    fn default() -> Self {
        Self {
            is_recording: AtomicBool::new(false),
            recorder_running: AtomicBool::new(false),
            video_enabled: AtomicBool::new(true),
            audio_enabled: AtomicBool::new(false),
            should_exit: AtomicBool::new(false),
        }
    }
}

/// System tray application
pub struct TrayApp {
    config: Config,
    state: Arc<RecordingState>,
}

impl TrayApp {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: Arc::new(RecordingState::default()),
        }
    }

    /// Run the tray application
    pub fn run(&self) -> Result<()> {
        info!("starting system tray");

        let event_loop: EventLoop<()> = EventLoopBuilder::new().build();

        // Create tray menu
        let menu = self.create_menu()?;

        // Create tray icon
        let icon = create_icon(false)?;

        let _tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Memoire - Not Recording")
            .with_icon(icon)
            .build()?;

        let state = self.state.clone();
        let config = self.config.clone();

        // Handle menu events in a separate thread
        let menu_state = state.clone();
        thread::spawn(move || {
            loop {
                if let Ok(event) = MenuEvent::receiver().recv() {
                    handle_menu_event(&event, &menu_state, &config);
                }
            }
        });

        // Run event loop
        event_loop.run(move |_event, _, control_flow| {
            *control_flow = ControlFlow::Wait;

            if state.should_exit.load(Ordering::SeqCst) {
                *control_flow = ControlFlow::Exit;
            }
        });
    }

    fn create_menu(&self) -> Result<Menu> {
        let menu = Menu::new();

        // Status (disabled, just for display)
        let status_item = MenuItem::with_id(ID_STATUS, "Status: Idle", false, None);
        menu.append(&status_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Start/Stop recording
        let start_stop = MenuItem::with_id(ID_START_STOP, "Start Recording", true, None);
        menu.append(&start_stop)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Video toggle (checked by default)
        let video_toggle = CheckMenuItem::with_id(ID_VIDEO_TOGGLE, "Video Capture", true, true, None);
        menu.append(&video_toggle)?;

        // Audio toggle (unchecked - not yet implemented)
        let audio_toggle = CheckMenuItem::with_id(ID_AUDIO_TOGGLE, "Audio Capture (Phase 3)", true, false, None);
        menu.append(&audio_toggle)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // About
        menu.append(&PredefinedMenuItem::about(
            Some("About Memoire"),
            Some(AboutMetadata {
                name: Some("Memoire".to_string()),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                authors: Some(vec!["Memoire Contributors".to_string()]),
                comments: Some("Screen & audio capture with OCR and STT".to_string()),
                ..Default::default()
            }),
        ))?;

        // Exit
        let exit_item = MenuItem::with_id(ID_EXIT, "Exit", true, None);
        menu.append(&exit_item)?;

        Ok(menu)
    }
}

fn handle_menu_event(event: &MenuEvent, state: &Arc<RecordingState>, config: &Config) {
    debug!("menu event: {:?}", event.id.0);

    match event.id.0.as_str() {
        ID_START_STOP => {
            let is_recording = state.is_recording.load(Ordering::SeqCst);
            if is_recording {
                info!("stopping recording via tray");
                state.is_recording.store(false, Ordering::SeqCst);
            } else {
                info!("starting recording via tray");
                state.is_recording.store(true, Ordering::SeqCst);
                state.recorder_running.store(true, Ordering::SeqCst);

                // Start recorder in background thread
                let state_clone = state.clone();
                let config_clone = config.clone();
                thread::spawn(move || {
                    if let Err(e) = run_recorder(&state_clone, config_clone) {
                        error!("recorder error: {}", e);
                    }
                    info!("recorder thread finished");
                    state_clone.is_recording.store(false, Ordering::SeqCst);
                    state_clone.recorder_running.store(false, Ordering::SeqCst);
                });
            }
        }
        ID_VIDEO_TOGGLE => {
            let current = state.video_enabled.load(Ordering::SeqCst);
            state.video_enabled.store(!current, Ordering::SeqCst);
            info!("video capture: {}", if !current { "enabled" } else { "disabled" });
        }
        ID_AUDIO_TOGGLE => {
            let current = state.audio_enabled.load(Ordering::SeqCst);
            state.audio_enabled.store(!current, Ordering::SeqCst);
            info!("audio capture: {}", if !current { "enabled" } else { "disabled" });
        }
        ID_EXIT => {
            info!("exit requested");

            // Stop recording first
            state.is_recording.store(false, Ordering::SeqCst);

            // Wait for recorder to finish (with timeout)
            if state.recorder_running.load(Ordering::SeqCst) {
                info!("waiting for recorder to finalize...");
                let start = std::time::Instant::now();
                let timeout = Duration::from_secs(30);

                while state.recorder_running.load(Ordering::SeqCst) {
                    if start.elapsed() > timeout {
                        warn!("timeout waiting for recorder, forcing exit");
                        break;
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                info!("recorder finished, exiting");
            }

            state.should_exit.store(true, Ordering::SeqCst);
        }
        _ => {}
    }
}

fn run_recorder(state: &Arc<RecordingState>, config: Config) -> Result<()> {
    // Create running flag that mirrors the state
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();
    let state_clone = state.clone();

    // Monitor state changes
    thread::spawn(move || {
        while running_clone.load(Ordering::SeqCst) {
            if !state_clone.is_recording.load(Ordering::SeqCst) {
                running_clone.store(false, Ordering::SeqCst);
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
    });

    let mut recorder = Recorder::new(config)?;
    recorder.run(running)?;

    // Recorder.run() returns after finalizing all chunks
    info!("recorder stopped and finalized");

    Ok(())
}

/// Create a simple colored icon
fn create_icon(is_recording: bool) -> Result<Icon> {
    let size = 32u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);

    // Color: red if recording, gray if not
    let (r, g, b) = if is_recording {
        (244u8, 67u8, 54u8)   // Red for recording
    } else {
        (158u8, 158u8, 158u8) // Gray for idle
    };

    for y in 0..size {
        for x in 0..size {
            // Create a circular icon
            let cx = size as f32 / 2.0;
            let cy = size as f32 / 2.0;
            let dist = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            let radius = size as f32 / 2.0 - 2.0;

            if dist <= radius {
                rgba.push(r);
                rgba.push(g);
                rgba.push(b);
                rgba.push(255);
            } else {
                rgba.push(0);
                rgba.push(0);
                rgba.push(0);
                rgba.push(0);
            }
        }
    }

    Ok(Icon::from_rgba(rgba, size, size)?)
}
