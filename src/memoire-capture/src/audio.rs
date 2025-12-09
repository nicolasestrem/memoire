//! WASAPI audio capture for Windows
//!
//! Supports input device (microphone) capture.
//! Loopback capture will be added in a future update.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use tracing::{debug, error, info, warn};
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode};

/// Audio device information
#[derive(Debug, Clone)]
pub struct AudioDeviceInfo {
    pub id: String,
    pub name: String,
    pub is_input: bool,
    pub is_default: bool,
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
}

/// Captured audio chunk with metadata
#[derive(Debug, Clone)]
pub struct CapturedAudio {
    /// Audio samples as f32 (normalized to [-1.0, 1.0])
    pub samples: Vec<f32>,
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u16,
    /// Timestamp when capture started
    pub timestamp: DateTime<Utc>,
    /// Duration in seconds
    pub duration_secs: f32,
    /// Device name that captured this audio
    pub device_name: String,
    /// Whether this is from an input device (mic) or output device (loopback)
    pub is_input_device: bool,
}

/// Audio capture configuration
#[derive(Debug, Clone)]
pub struct AudioCaptureConfig {
    /// Specific device ID to capture from (None = default device)
    pub device_id: Option<String>,
    /// Whether to capture in loopback mode (system audio) - NOT YET IMPLEMENTED
    pub is_loopback: bool,
    /// Chunk duration in seconds
    pub chunk_duration_secs: u32,
    /// Target sample rate for output (will resample if needed)
    pub target_sample_rate: u32,
    /// Target channels (1 = mono, 2 = stereo)
    pub target_channels: u16,
}

impl Default for AudioCaptureConfig {
    fn default() -> Self {
        Self {
            device_id: None,
            is_loopback: false,
            chunk_duration_secs: 30,
            target_sample_rate: 16000, // Required by Parakeet STT
            target_channels: 1,        // Mono for STT
        }
    }
}

/// Audio capture using WASAPI
pub struct AudioCapture {
    config: AudioCaptureConfig,
    running: Arc<AtomicBool>,
    device_name: String,
    sample_rate: u32,
    channels: u16,
}

impl AudioCapture {
    /// Enumerate all available audio devices
    pub fn enumerate_devices() -> Result<Vec<AudioDeviceInfo>> {
        // Initialize COM for this thread
        let _ = wasapi::initialize_mta();

        let enumerator = DeviceEnumerator::new()
            .context("Failed to create device enumerator")?;

        let mut devices = Vec::new();

        // Get default input device
        if let Ok(device) = enumerator.get_default_device(&Direction::Capture) {
            if let Ok(info) = Self::device_to_info(&device, true, true) {
                devices.push(info);
            }
        }

        // Get default output device (for loopback - info only)
        if let Ok(device) = enumerator.get_default_device(&Direction::Render) {
            if let Ok(info) = Self::device_to_info(&device, false, true) {
                devices.push(info);
            }
        }

        // Enumerate all input devices
        if let Ok(collection) = enumerator.get_device_collection(&Direction::Capture) {
            for i in 0..collection.get_nbr_devices().unwrap_or(0) {
                if let Ok(device) = collection.get_device_at_index(i) {
                    if let Ok(info) = Self::device_to_info(&device, true, false) {
                        // Avoid duplicating default device
                        if !devices.iter().any(|d| d.id == info.id) {
                            devices.push(info);
                        }
                    }
                }
            }
        }

        // Enumerate all output devices (for loopback - info only)
        if let Ok(collection) = enumerator.get_device_collection(&Direction::Render) {
            for i in 0..collection.get_nbr_devices().unwrap_or(0) {
                if let Ok(device) = collection.get_device_at_index(i) {
                    if let Ok(info) = Self::device_to_info(&device, false, false) {
                        // Avoid duplicating default device
                        if !devices.iter().any(|d| d.id == info.id) {
                            devices.push(info);
                        }
                    }
                }
            }
        }

        Ok(devices)
    }

    fn device_to_info(device: &wasapi::Device, is_input: bool, is_default: bool) -> Result<AudioDeviceInfo> {
        let id = device.get_id().unwrap_or_else(|_| "unknown".to_string());
        let name = device.get_friendlyname().unwrap_or_else(|_| "Unknown Device".to_string());

        // Get audio format
        let audio_client = device.get_iaudioclient()?;
        let format = audio_client.get_mixformat()?;

        Ok(AudioDeviceInfo {
            id,
            name,
            is_input,
            is_default,
            sample_rate: format.get_samplespersec() as u32,
            channels: format.get_nchannels() as u16,
            bits_per_sample: format.get_bitspersample() as u16,
        })
    }

    /// Create a new audio capture instance
    pub fn new(config: AudioCaptureConfig) -> Result<Self> {
        if config.is_loopback {
            return Err(anyhow::anyhow!("Loopback capture is not yet implemented"));
        }

        // Initialize COM for this thread
        let _ = wasapi::initialize_mta();

        let enumerator = DeviceEnumerator::new()
            .context("Failed to create device enumerator")?;

        let device = if let Some(ref device_id) = config.device_id {
            // Find specific device by ID
            enumerator.get_device(device_id)
                .context(format!("Device not found: {}", device_id))?
        } else {
            // Use default capture device
            enumerator.get_default_device(&Direction::Capture)
                .context("Failed to get default capture device")?
        };

        let device_name = device.get_friendlyname().unwrap_or_else(|_| "Unknown".to_string());

        // Get audio format
        let audio_client = device.get_iaudioclient()?;
        let format = audio_client.get_mixformat()?;
        let sample_rate = format.get_samplespersec() as u32;
        let channels = format.get_nchannels() as u16;

        info!(
            "audio capture initialized: {} ({} Hz, {} ch)",
            device_name, sample_rate, channels
        );

        Ok(Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            device_name,
            sample_rate,
            channels,
        })
    }

    /// Get device name
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Get source sample rate
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Get source channels
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Check if capture is running
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get the running flag for external control
    pub fn running_flag(&self) -> Arc<AtomicBool> {
        self.running.clone()
    }

    /// Start capturing audio, returning a receiver for audio chunks
    pub fn start(&mut self) -> Result<tokio::sync::mpsc::Receiver<CapturedAudio>> {
        if self.running.load(Ordering::Relaxed) {
            return Err(anyhow::anyhow!("Capture already running"));
        }

        self.running.store(true, Ordering::Relaxed);
        let (tx, rx) = tokio::sync::mpsc::channel(16);

        let config = self.config.clone();
        let device_name = self.device_name.clone();
        let source_sample_rate = self.sample_rate;
        let source_channels = self.channels;
        let running = self.running.clone();

        // Spawn capture thread
        thread::spawn(move || {
            if let Err(e) = Self::capture_loop(
                config,
                device_name,
                source_sample_rate,
                source_channels,
                running.clone(),
                tx,
            ) {
                error!("audio capture error: {}", e);
            }
            running.store(false, Ordering::Relaxed);
            info!("audio capture thread stopped");
        });

        Ok(rx)
    }

    /// Stop capturing
    pub fn stop(&self) {
        info!("stopping audio capture");
        self.running.store(false, Ordering::Relaxed);
    }

    fn capture_loop(
        config: AudioCaptureConfig,
        device_name: String,
        source_sample_rate: u32,
        source_channels: u16,
        running: Arc<AtomicBool>,
        tx: tokio::sync::mpsc::Sender<CapturedAudio>,
    ) -> Result<()> {
        // Initialize COM for this thread
        let hr = wasapi::initialize_mta();
        if hr.is_err() {
            return Err(anyhow::anyhow!("COM initialization failed: {:?}", hr));
        }

        let enumerator = DeviceEnumerator::new()?;

        let device = if let Some(ref device_id) = config.device_id {
            enumerator.get_device(device_id)?
        } else {
            enumerator.get_default_device(&Direction::Capture)?
        };

        let mut audio_client = device.get_iaudioclient()?;
        let device_format = audio_client.get_mixformat()?;
        let blockalign = device_format.get_blockalign() as usize;
        let bits_per_sample = device_format.get_bitspersample() as u16;
        let sample_type = device_format.get_subformat().ok();

        debug!(
            "device format: {} Hz, {} ch, {} bits, blockalign={}",
            source_sample_rate, source_channels, bits_per_sample, blockalign
        );

        // Use event-driven shared mode
        let stream_mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 1_000_000, // 100ms in 100ns units
        };

        // Initialize audio client
        audio_client.initialize_client(&device_format, &Direction::Capture, &stream_mode)?;

        let capture_client = audio_client.get_audiocaptureclient()?;
        let event_handle = audio_client.set_get_eventhandle()?;

        audio_client.start_stream()?;
        info!("audio capture started");

        // Calculate samples per chunk
        let samples_per_chunk = (config.chunk_duration_secs as usize * source_sample_rate as usize) as usize;
        let mut chunk_buffer: Vec<f32> = Vec::with_capacity(samples_per_chunk * source_channels as usize);
        let mut chunk_start_time = Utc::now();
        let mut raw_buffer: VecDeque<u8> = VecDeque::new();

        while running.load(Ordering::Relaxed) {
            // Wait for audio data (with timeout)
            if event_handle.wait_for_event(100).is_err() {
                continue;
            }

            // Read available frames into deque
            match capture_client.read_from_device_to_deque(&mut raw_buffer) {
                Ok(_buffer_info) => {
                    // Convert raw bytes to f32 samples
                    while raw_buffer.len() >= blockalign {
                        let bytes: Vec<u8> = raw_buffer.drain(..blockalign).collect();
                        let samples = bytes_to_f32(&bytes, bits_per_sample, &sample_type);
                        chunk_buffer.extend(samples);
                    }
                }
                Err(e) => {
                    let err_str = e.to_string();
                    // Ignore "no data" errors
                    if !err_str.contains("AUDCLNT_S_BUFFER_EMPTY") && !err_str.contains("0x08890001") {
                        debug!("capture read error: {}", e);
                    }
                }
            }

            // Check if chunk is complete
            let samples_collected = chunk_buffer.len() / source_channels as usize;
            if samples_collected >= samples_per_chunk {
                let chunk_samples = chunk_buffer.drain(..(samples_per_chunk * source_channels as usize)).collect::<Vec<_>>();

                // Convert to target format (mono, target sample rate)
                let processed_samples = process_audio(
                    &chunk_samples,
                    source_sample_rate,
                    source_channels,
                    config.target_sample_rate,
                    config.target_channels,
                );

                let captured = CapturedAudio {
                    samples: processed_samples,
                    sample_rate: config.target_sample_rate,
                    channels: config.target_channels,
                    timestamp: chunk_start_time,
                    duration_secs: config.chunk_duration_secs as f32,
                    device_name: device_name.clone(),
                    is_input_device: true,
                };

                // Send chunk
                if tx.blocking_send(captured).is_err() {
                    warn!("audio channel closed, stopping capture");
                    break;
                }

                info!("captured audio chunk: {} seconds", config.chunk_duration_secs);
                chunk_start_time = Utc::now();
            }
        }

        audio_client.stop_stream()?;
        Ok(())
    }
}

/// Convert raw bytes to f32 samples based on format
fn bytes_to_f32(data: &[u8], bits_per_sample: u16, sample_type: &Option<SampleType>) -> Vec<f32> {
    let is_float = matches!(sample_type, Some(SampleType::Float));

    match (bits_per_sample, is_float) {
        (32, true) => {
            // 32-bit float
            data.chunks_exact(4)
                .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
                .collect()
        }
        (32, false) => {
            // 32-bit int
            data.chunks_exact(4)
                .map(|chunk| {
                    let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    sample as f32 / i32::MAX as f32
                })
                .collect()
        }
        (16, _) => {
            // 16-bit int
            data.chunks_exact(2)
                .map(|chunk| {
                    let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                    sample as f32 / i16::MAX as f32
                })
                .collect()
        }
        (24, _) => {
            // 24-bit int
            data.chunks_exact(3)
                .map(|chunk| {
                    let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]) >> 8;
                    sample as f32 / (1 << 23) as f32
                })
                .collect()
        }
        _ => {
            warn!("unsupported format: {} bits, float={}", bits_per_sample, is_float);
            Vec::new()
        }
    }
}

/// Process audio: convert to mono and resample if needed
fn process_audio(
    samples: &[f32],
    source_rate: u32,
    source_channels: u16,
    target_rate: u32,
    target_channels: u16,
) -> Vec<f32> {
    // First convert to mono if needed
    let mono_samples = if source_channels > 1 && target_channels == 1 {
        to_mono(samples, source_channels)
    } else {
        samples.to_vec()
    };

    // Then resample if needed
    if source_rate != target_rate {
        resample(&mono_samples, source_rate, target_rate)
    } else {
        mono_samples
    }
}

/// Convert stereo (or multi-channel) audio to mono by averaging channels
pub fn to_mono(samples: &[f32], channels: u16) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }

    samples
        .chunks_exact(channels as usize)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Resample audio from source rate to target rate using rubato
pub fn resample(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if source_rate == target_rate || samples.is_empty() {
        return samples.to_vec();
    }

    use rubato::{FftFixedInOut, Resampler};

    // Use chunk size that divides evenly
    let chunk_size = 1024;
    let resampler_result = FftFixedInOut::<f32>::new(
        source_rate as usize,
        target_rate as usize,
        chunk_size,
        1, // mono
    );

    match resampler_result {
        Ok(mut resampler) => {
            let mut output = Vec::new();
            let input_frames = resampler.input_frames_next();

            // Process in chunks
            for chunk in samples.chunks(input_frames) {
                if chunk.len() < input_frames {
                    // Pad last chunk with zeros
                    let mut padded = chunk.to_vec();
                    padded.resize(input_frames, 0.0);
                    let input = vec![padded];
                    if let Ok(result) = resampler.process(&input, None) {
                        if !result.is_empty() {
                            output.extend(&result[0]);
                        }
                    }
                } else {
                    let input = vec![chunk.to_vec()];
                    if let Ok(result) = resampler.process(&input, None) {
                        if !result.is_empty() {
                            output.extend(&result[0]);
                        }
                    }
                }
            }

            output
        }
        Err(e) => {
            warn!("failed to create resampler: {}", e);
            samples.to_vec()
        }
    }
}

/// Save audio samples to a WAV file
pub fn save_wav(audio: &CapturedAudio, path: &PathBuf) -> Result<()> {
    use hound::{WavSpec, WavWriter};

    let spec = WavSpec {
        channels: audio.channels,
        sample_rate: audio.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    // Create parent directories if needed
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut writer = WavWriter::create(path, spec)
        .context("Failed to create WAV file")?;

    // Convert f32 samples to i16
    for sample in &audio.samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_sample = (clamped * i16::MAX as f32) as i16;
        writer.write_sample(i16_sample)?;
    }

    writer.finalize()?;

    debug!("saved WAV file: {:?}", path);
    Ok(())
}

/// Load audio samples from a WAV file
pub fn load_wav(path: &PathBuf) -> Result<CapturedAudio> {
    use hound::WavReader;

    let reader = WavReader::open(path)
        .context("Failed to open WAV file")?;

    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            reader.into_samples::<i16>()
                .filter_map(|s| s.ok())
                .map(|s| s as f32 / i16::MAX as f32)
                .collect()
        }
        hound::SampleFormat::Float => {
            reader.into_samples::<f32>()
                .filter_map(|s| s.ok())
                .collect()
        }
    };

    let duration_secs = samples.len() as f32 / spec.sample_rate as f32 / spec.channels as f32;

    Ok(CapturedAudio {
        samples,
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        timestamp: Utc::now(),
        duration_secs,
        device_name: path.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        is_input_device: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_mono_stereo() {
        let stereo = vec![1.0, 0.0, 0.5, 0.5, -1.0, 1.0];
        let mono = to_mono(&stereo, 2);
        assert_eq!(mono.len(), 3);
        assert!((mono[0] - 0.5).abs() < 0.001);
        assert!((mono[1] - 0.5).abs() < 0.001);
        assert!((mono[2] - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_to_mono_already_mono() {
        let mono = vec![1.0, 0.5, 0.0, -0.5, -1.0];
        let result = to_mono(&mono, 1);
        assert_eq!(result.len(), mono.len());
    }

    #[test]
    fn test_bytes_to_f32_16bit() {
        // i16::MAX as bytes
        let bytes = vec![0xFF, 0x7F, 0x00, 0x00];
        let samples = bytes_to_f32(&bytes, 16, &None);
        assert_eq!(samples.len(), 2);
        assert!((samples[0] - 1.0).abs() < 0.001);
        assert!((samples[1] - 0.0).abs() < 0.001);
    }
}
