//! Mel spectrogram feature extraction for Parakeet TDT models
//!
//! Computes 80-bin or 128-bin mel filterbank features from audio samples.
//! Uses the same parameters as sherpa-onnx/NeMo:
//! - Sample rate: 16kHz
//! - Window: 25ms (400 samples)
//! - Hop: 10ms (160 samples)
//! - FFT size: 512
//! - Mel bins: 80 (default) or 128 (parakeet-tdt-0.6b-v2)
//! - Low freq: 0 Hz
//! - High freq: 8000 Hz (Nyquist for 16kHz)

use std::f32::consts::PI;
use tracing::debug;

/// Default sample rate expected by the model
pub const SAMPLE_RATE: u32 = 16000;

/// Window size in samples (25ms at 16kHz)
const WINDOW_SIZE: usize = 400;

/// Hop size in samples (10ms at 16kHz)
const HOP_SIZE: usize = 160;

/// FFT size (next power of 2 >= window size)
const FFT_SIZE: usize = 512;

/// Frame duration in seconds (based on hop size)
pub const FRAME_DURATION_SEC: f64 = HOP_SIZE as f64 / SAMPLE_RATE as f64; // 0.01s = 10ms

/// Subsampling factor of the encoder (frames to encoder output ratio)
/// For Parakeet TDT, this is typically 8
pub const SUBSAMPLING_FACTOR: usize = 8;

/// Duration per encoder output frame in seconds
pub const ENCODER_FRAME_DURATION_SEC: f64 = FRAME_DURATION_SEC * SUBSAMPLING_FACTOR as f64; // 0.08s = 80ms

/// Mel spectrogram feature extractor
pub struct MelSpectrogram {
    /// Number of mel filter banks
    num_mels: usize,
    /// Precomputed mel filterbank matrix [num_mels, fft_size/2 + 1]
    mel_filters: Vec<Vec<f32>>,
    /// Hann window for STFT
    window: Vec<f32>,
    /// Whether to normalize features
    normalize: bool,
}

impl MelSpectrogram {
    /// Create a new mel spectrogram extractor
    ///
    /// # Arguments
    /// * `num_mels` - Number of mel filter banks (80 or 128)
    /// * `normalize` - Whether to normalize features per utterance
    pub fn new(num_mels: usize, normalize: bool) -> Self {
        let mel_filters = create_mel_filterbank(num_mels, FFT_SIZE, SAMPLE_RATE, 0.0, 8000.0);
        let window = create_hann_window(WINDOW_SIZE);

        debug!(
            "created mel spectrogram extractor: num_mels={}, fft_size={}, window={}, hop={}",
            num_mels, FFT_SIZE, WINDOW_SIZE, HOP_SIZE
        );

        Self {
            num_mels,
            mel_filters,
            window,
            normalize,
        }
    }

    /// Extract mel spectrogram features from audio samples
    ///
    /// # Arguments
    /// * `samples` - Audio samples at 16kHz, normalized to [-1, 1]
    ///
    /// # Returns
    /// Feature matrix of shape [num_frames, num_mels]
    pub fn extract(&self, samples: &[f32]) -> Vec<Vec<f32>> {
        if samples.len() < WINDOW_SIZE {
            return Vec::new();
        }

        let num_frames = (samples.len() - WINDOW_SIZE) / HOP_SIZE + 1;
        let mut features = Vec::with_capacity(num_frames);

        // Compute STFT and mel features for each frame
        for frame_idx in 0..num_frames {
            let start = frame_idx * HOP_SIZE;
            let frame = &samples[start..start + WINDOW_SIZE];

            // Apply window and compute FFT magnitude spectrum
            let spectrum = self.compute_spectrum(frame);

            // Apply mel filterbank
            let mel_frame = self.apply_mel_filterbank(&spectrum);

            features.push(mel_frame);
        }

        // Normalize if requested
        if self.normalize && !features.is_empty() {
            self.normalize_features(&mut features);
        }

        features
    }

    /// Extract features and return as flattened array with shape info
    ///
    /// Returns (features_flat, num_frames, num_mels)
    pub fn extract_flat(&self, samples: &[f32]) -> (Vec<f32>, usize, usize) {
        let features = self.extract(samples);
        let num_frames = features.len();

        let flat: Vec<f32> = features.into_iter().flatten().collect();

        (flat, num_frames, self.num_mels)
    }

    /// Compute magnitude spectrum using real-valued FFT
    fn compute_spectrum(&self, frame: &[f32]) -> Vec<f32> {
        // Apply Hann window
        let mut windowed: Vec<f32> = frame
            .iter()
            .zip(self.window.iter())
            .map(|(s, w)| s * w)
            .collect();

        // Zero-pad to FFT size
        windowed.resize(FFT_SIZE, 0.0);

        // Compute FFT magnitude spectrum
        // Using a simple DFT implementation (for correctness over speed)
        // In production, you'd use rustfft or similar
        let spectrum = compute_magnitude_spectrum(&windowed);

        spectrum
    }

    /// Apply mel filterbank to magnitude spectrum
    fn apply_mel_filterbank(&self, spectrum: &[f32]) -> Vec<f32> {
        let mut mel_energies = vec![0.0f32; self.num_mels];

        for (mel_idx, filter) in self.mel_filters.iter().enumerate() {
            let mut energy = 0.0f32;
            for (bin_idx, &weight) in filter.iter().enumerate() {
                if bin_idx < spectrum.len() && weight > 0.0 {
                    energy += spectrum[bin_idx] * weight;
                }
            }
            // Apply log with floor to avoid -inf
            mel_energies[mel_idx] = (energy.max(1e-10)).ln();
        }

        mel_energies
    }

    /// Normalize features to zero mean and unit variance per dimension
    fn normalize_features(&self, features: &mut [Vec<f32>]) {
        if features.is_empty() {
            return;
        }

        let num_frames = features.len();
        let num_dims = features[0].len();

        // Compute mean and variance per dimension
        for dim in 0..num_dims {
            let sum: f32 = features.iter().map(|f| f[dim]).sum();
            let mean = sum / num_frames as f32;

            let var_sum: f32 = features.iter().map(|f| (f[dim] - mean).powi(2)).sum();
            let std = (var_sum / num_frames as f32).sqrt().max(1e-10);

            // Normalize
            for frame in features.iter_mut() {
                frame[dim] = (frame[dim] - mean) / std;
            }
        }
    }
}

/// Create a Hann window of specified length
fn create_hann_window(length: usize) -> Vec<f32> {
    (0..length)
        .map(|n| 0.5 * (1.0 - (2.0 * PI * n as f32 / (length - 1) as f32).cos()))
        .collect()
}

/// Convert frequency to mel scale
fn hz_to_mel(hz: f32) -> f32 {
    2595.0 * (1.0 + hz / 700.0).log10()
}

/// Convert mel to frequency
fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0_f32.powf(mel / 2595.0) - 1.0)
}

/// Create mel filterbank matrix
fn create_mel_filterbank(
    num_mels: usize,
    fft_size: usize,
    sample_rate: u32,
    low_freq: f32,
    high_freq: f32,
) -> Vec<Vec<f32>> {
    let num_bins = fft_size / 2 + 1;
    let sample_rate = sample_rate as f32;

    // Convert frequency bounds to mel
    let low_mel = hz_to_mel(low_freq);
    let high_mel = hz_to_mel(high_freq);

    // Create equally spaced mel points
    let mel_points: Vec<f32> = (0..=num_mels + 1)
        .map(|i| low_mel + (high_mel - low_mel) * i as f32 / (num_mels + 1) as f32)
        .collect();

    // Convert mel points to FFT bin indices
    let bin_points: Vec<usize> = mel_points
        .iter()
        .map(|&mel| {
            let hz = mel_to_hz(mel);
            let bin = ((fft_size as f32 + 1.0) * hz / sample_rate).floor() as usize;
            bin.min(num_bins - 1)
        })
        .collect();

    // Create triangular filters
    let mut filters = Vec::with_capacity(num_mels);

    for m in 0..num_mels {
        let mut filter = vec![0.0f32; num_bins];

        let left = bin_points[m];
        let center = bin_points[m + 1];
        let right = bin_points[m + 2];

        // Rising edge
        for k in left..center {
            if center > left {
                filter[k] = (k - left) as f32 / (center - left) as f32;
            }
        }

        // Falling edge
        for k in center..=right {
            if right > center {
                filter[k] = (right - k) as f32 / (right - center) as f32;
            }
        }

        filters.push(filter);
    }

    filters
}

/// Compute magnitude spectrum using DFT
/// This is a simple implementation - use rustfft for better performance
/// TODO: Replace with rustfft for production (this is O(n²) vs O(n log n))
fn compute_magnitude_spectrum(samples: &[f32]) -> Vec<f32> {
    let n = samples.len();

    // Validate input size to prevent excessive computation
    if n > 8192 {
        // For samples > 8192, this naive DFT becomes prohibitively slow
        // Return empty spectrum - caller should use smaller chunks or rustfft
        return vec![0.0f32; n / 2 + 1];
    }

    let num_bins = n / 2 + 1;
    let mut spectrum = vec![0.0f32; num_bins];
    let n_f32 = n as f32;

    for k in 0..num_bins {
        let mut real = 0.0f32;
        let mut imag = 0.0f32;
        let k_f32 = k as f32;

        for (n_idx, &sample) in samples.iter().enumerate() {
            // Use f32 arithmetic throughout to avoid overflow
            let angle = -2.0 * PI * k_f32 * (n_idx as f32) / n_f32;
            real += sample * angle.cos();
            imag += sample * angle.sin();
        }

        // Magnitude spectrum (power spectrum would be real^2 + imag^2)
        spectrum[k] = (real * real + imag * imag).sqrt();
    }

    spectrum
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mel_spectrogram_basic() {
        let mel = MelSpectrogram::new(80, true);

        // Create a simple test signal (1 second of sine wave)
        let samples: Vec<f32> = (0..16000)
            .map(|i| (2.0 * PI * 440.0 * i as f32 / 16000.0).sin())
            .collect();

        let features = mel.extract(&samples);

        // Should have approximately (16000 - 400) / 160 + 1 = 98 frames
        assert!(!features.is_empty());
        assert_eq!(features[0].len(), 80);

        // Features should be normalized (mean ~0, std ~1)
        let mean: f32 = features.iter().map(|f| f[0]).sum::<f32>() / features.len() as f32;
        assert!(mean.abs() < 0.1, "mean should be near 0, got {}", mean);
    }

    #[test]
    fn test_hz_to_mel() {
        // Test known values
        assert!((hz_to_mel(0.0) - 0.0).abs() < 0.01);
        assert!((hz_to_mel(1000.0) - 1000.0).abs() < 10.0); // ~1000 mel at 1000 Hz
    }

    #[test]
    fn test_empty_input() {
        let mel = MelSpectrogram::new(80, true);
        let features = mel.extract(&[]);
        assert!(features.is_empty());
    }

    #[test]
    fn test_short_input() {
        let mel = MelSpectrogram::new(80, true);
        // Input shorter than window size
        let features = mel.extract(&[0.0; 100]);
        assert!(features.is_empty());
    }
}
