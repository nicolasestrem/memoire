//! Speech-to-text engine using ONNX Runtime
//!
//! This module provides STT functionality using Parakeet TDT model with:
//! - Encoder: FastConformer (processes mel features)
//! - Decoder: Stateful LSTM predictor
//! - Joiner: Combines encoder and decoder outputs for token prediction
//!
//! TDT (Token-and-Duration Transducer) extends standard transducers by also
//! predicting how many frames to skip, enabling faster decoding.

use anyhow::{Context, Result};
use ort::session::Session;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

use crate::error::SttError;
use crate::mel::{MelSpectrogram, ENCODER_FRAME_DURATION_SEC, SAMPLE_RATE};
use crate::tokenizer::Tokenizer;

/// Configuration for the STT engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SttConfig {
    /// Path to the ONNX model directory
    pub model_dir: PathBuf,
    /// Whether to use GPU acceleration
    pub use_gpu: bool,
    /// Language code (e.g., "en", "fr", "de")
    pub language: Option<String>,
    /// Number of threads for CPU inference
    pub num_threads: usize,
}

impl Default for SttConfig {
    fn default() -> Self {
        Self {
            model_dir: crate::default_model_dir(),
            use_gpu: true,
            language: None, // Auto-detect
            num_threads: 4,
        }
    }
}

/// A segment of transcription with timing information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    /// Start time in seconds
    pub start: f64,
    /// End time in seconds
    pub end: f64,
    /// Transcribed text
    pub text: String,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
}

/// Result of transcription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// Full transcribed text
    pub text: String,
    /// Individual segments with timestamps
    pub segments: Vec<TranscriptionSegment>,
    /// Detected language (if auto-detected)
    pub language: Option<String>,
    /// Processing time in milliseconds
    pub processing_time_ms: u64,
}

/// Parakeet TDT model sessions
struct ParakeetModel {
    encoder: Session,
    decoder: Session,
    joiner: Session,
    /// Number of RNN layers in decoder
    pred_rnn_layers: usize,
    /// Hidden size of decoder RNN
    pred_hidden: usize,
}

/// Speech-to-text engine
pub struct SttEngine {
    config: SttConfig,
    model: Option<ParakeetModel>,
    tokenizer: Option<Tokenizer>,
    mel_extractor: MelSpectrogram,
    is_gpu_enabled: bool,
}

impl SttEngine {
    /// Create a new STT engine with the given configuration
    pub fn new(config: SttConfig) -> Result<Self> {
        info!("initializing STT engine");
        info!("model directory: {:?}", config.model_dir);
        info!("GPU enabled: {}", config.use_gpu);

        // Check if model files exist
        let encoder_path = config.model_dir.join("encoder.onnx");
        let decoder_path = config.model_dir.join("decoder.onnx");
        let joiner_path = config.model_dir.join("joiner.onnx");
        let tokens_path = config.model_dir.join("tokens.txt");

        let has_model = encoder_path.exists()
            && decoder_path.exists()
            && joiner_path.exists()
            && tokens_path.exists();

        if !has_model {
            warn!("model files not found at {:?}", config.model_dir);
            warn!("STT engine will return placeholder results until model is downloaded");
            warn!("Run 'memoire download-models' to download the Parakeet TDT model");

            // Create mel extractor with default 80 bins (will be updated when model loads)
            let mel_extractor = MelSpectrogram::new(80, true);

            return Ok(Self {
                config,
                model: None,
                tokenizer: None,
                mel_extractor,
                is_gpu_enabled: false,
            });
        }

        // Load tokenizer
        let tokenizer = Tokenizer::from_file(&tokens_path)
            .context("failed to load tokenizer")?;
        info!("loaded tokenizer: vocab_size={}", tokenizer.vocab_size());

        // Determine feature dimension from model metadata or use default
        // Parakeet TDT 0.6b-v2 uses 128-dim features
        let num_mels = 128;
        let mel_extractor = MelSpectrogram::new(num_mels, true);

        // Initialize ONNX Runtime sessions
        let mut is_gpu_enabled = false;

        // Create encoder session
        let encoder = Self::create_session(&encoder_path, config.use_gpu, config.num_threads, &mut is_gpu_enabled)
            .context("failed to load encoder model")?;

        // Create decoder session
        let decoder = Self::create_session(&decoder_path, config.use_gpu, config.num_threads, &mut is_gpu_enabled)
            .context("failed to load decoder model")?;

        // Create joiner session
        let joiner = Self::create_session(&joiner_path, config.use_gpu, config.num_threads, &mut is_gpu_enabled)
            .context("failed to load joiner model")?;

        // Get decoder dimensions from model metadata
        // Default values for Parakeet TDT
        let pred_rnn_layers = 2;
        let pred_hidden = 640;

        let model = ParakeetModel {
            encoder,
            decoder,
            joiner,
            pred_rnn_layers,
            pred_hidden,
        };

        // Log model I/O names and shapes for debugging
        info!("Encoder inputs:");
        for inp in &model.encoder.inputs {
            info!("  {}: {:?}", inp.name, inp.input_type);
        }
        info!("Encoder outputs:");
        for out in &model.encoder.outputs {
            info!("  {}: {:?}", out.name, out.output_type);
        }
        info!("Decoder inputs:");
        for inp in &model.decoder.inputs {
            info!("  {}: {:?}", inp.name, inp.input_type);
        }
        info!("Decoder outputs:");
        for out in &model.decoder.outputs {
            info!("  {}: {:?}", out.name, out.output_type);
        }
        info!("Joiner inputs:");
        for inp in &model.joiner.inputs {
            info!("  {}: {:?}", inp.name, inp.input_type);
        }
        info!("Joiner outputs:");
        for out in &model.joiner.outputs {
            info!("  {}: {:?}", out.name, out.output_type);
        }

        info!("STT engine initialized successfully");
        info!("execution provider: {}", if is_gpu_enabled { "CUDA" } else { "CPU" });

        Ok(Self {
            config,
            model: Some(model),
            tokenizer: Some(tokenizer),
            mel_extractor,
            is_gpu_enabled,
        })
    }

    /// Create an ONNX session with optional GPU acceleration
    fn create_session(
        path: &Path,
        use_gpu: bool,
        num_threads: usize,
        is_gpu_enabled: &mut bool,
    ) -> Result<Session> {
        let builder = Session::builder()?
            .with_intra_threads(num_threads)?;

        let builder = if use_gpu {
            match builder.with_execution_providers([
                ort::execution_providers::CUDAExecutionProvider::default().build(),
            ]) {
                Ok(b) => {
                    *is_gpu_enabled = true;
                    info!("CUDA execution provider enabled");
                    b
                }
                Err(e) => {
                    warn!("failed to enable CUDA, falling back to CPU: {}", e);
                    Session::builder()?.with_intra_threads(num_threads)?
                }
            }
        } else {
            builder
        };

        builder.commit_from_file(path).map_err(Into::into)
    }

    /// Check if GPU acceleration is enabled
    pub fn is_gpu_enabled(&self) -> bool {
        self.is_gpu_enabled
    }

    /// Check if the model is loaded
    pub fn is_model_loaded(&self) -> bool {
        self.model.is_some()
    }

    /// Transcribe audio from a WAV file
    pub fn transcribe_file(&mut self, path: impl AsRef<Path>) -> Result<TranscriptionResult> {
        let path = path.as_ref();
        debug!("transcribing file: {:?}", path);

        // Load audio
        let audio = self.load_audio(path)?;

        // Transcribe
        self.transcribe_samples(&audio.samples, audio.sample_rate)
    }

    /// Transcribe audio samples directly
    pub fn transcribe_samples(&mut self, samples: &[f32], sample_rate: u32) -> Result<TranscriptionResult> {
        let start_time = std::time::Instant::now();

        // If no model is loaded, return placeholder
        if self.model.is_none() || self.tokenizer.is_none() {
            warn!("no model loaded, returning placeholder result");
            return Ok(TranscriptionResult {
                text: "[Model not loaded - run 'memoire download-models' first]".to_string(),
                segments: vec![TranscriptionSegment {
                    start: 0.0,
                    end: samples.len() as f64 / sample_rate as f64,
                    text: "[Model not loaded]".to_string(),
                    confidence: 0.0,
                }],
                language: None,
                processing_time_ms: start_time.elapsed().as_millis() as u64,
            });
        }

        // Preprocess audio (ensure 16kHz mono)
        let processed_samples = self.preprocess_audio(samples, sample_rate)?;

        // Run inference
        let result = self.run_inference(&processed_samples)?;

        let processing_time_ms = start_time.elapsed().as_millis() as u64;
        debug!("transcription completed in {}ms", processing_time_ms);

        Ok(TranscriptionResult {
            text: result.text,
            segments: result.segments,
            language: result.language,
            processing_time_ms,
        })
    }

    /// Load audio from a WAV file
    fn load_audio(&self, path: &Path) -> Result<AudioData> {
        let reader = hound::WavReader::open(path)
            .context("failed to open WAV file")?;

        let spec = reader.spec();
        let sample_rate = spec.sample_rate;
        let channels = spec.channels as usize;

        debug!("loading audio: {} Hz, {} channels", sample_rate, channels);

        let samples: Vec<f32> = match spec.sample_format {
            hound::SampleFormat::Float => {
                reader.into_samples::<f32>()
                    .filter_map(|s| s.ok())
                    .collect()
            }
            hound::SampleFormat::Int => {
                let bits = spec.bits_per_sample;
                let max_val = (1 << (bits - 1)) as f32;
                reader.into_samples::<i32>()
                    .filter_map(|s| s.ok())
                    .map(|s| s as f32 / max_val)
                    .collect()
            }
        };

        // Convert to mono if stereo
        let mono_samples = if channels > 1 {
            samples
                .chunks(channels)
                .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
                .collect()
        } else {
            samples
        };

        Ok(AudioData {
            samples: mono_samples,
            sample_rate,
        })
    }

    /// Preprocess audio for the model (resample to 16kHz if needed)
    fn preprocess_audio(&self, samples: &[f32], sample_rate: u32) -> Result<Vec<f32>> {
        if sample_rate == SAMPLE_RATE {
            return Ok(samples.to_vec());
        }

        // Resample to 16kHz
        debug!("resampling from {} Hz to {} Hz", sample_rate, SAMPLE_RATE);

        use rubato::{FftFixedIn, Resampler};

        let mut resampler = FftFixedIn::<f32>::new(
            sample_rate as usize,
            SAMPLE_RATE as usize,
            samples.len(),
            1, // chunk size
            1, // channels (mono)
        )?;

        let input = vec![samples.to_vec()];
        let output = resampler.process(&input, None)?;

        Ok(output.into_iter().flatten().collect())
    }

    /// Run TDT model inference
    fn run_inference(&mut self, samples: &[f32]) -> Result<TranscriptionResult> {
        // Extract mel spectrogram features first (doesn't need model)
        let (features_flat, num_frames, num_mels) = self.mel_extractor.extract_flat(samples);

        if num_frames == 0 {
            return Ok(TranscriptionResult {
                text: String::new(),
                segments: Vec::new(),
                language: self.config.language.clone(),
                processing_time_ms: 0,
            });
        }

        debug!("extracted {} frames of {} mel features", num_frames, num_mels);

        // Get tokenizer info before we borrow model mutably
        let tokenizer = self.tokenizer.as_ref()
            .ok_or_else(|| SttError::ModelLoadError("tokenizer not loaded".to_string()))?;
        let vocab_size = tokenizer.vocab_size();
        let blank_id = tokenizer.blank_id();

        // Run encoder - extract data into owned Vec before further processing
        let (encoder_data, encoder_len, encoder_dim) = {
            let model = self.model.as_mut()
                .ok_or_else(|| SttError::ModelLoadError("model not loaded".to_string()))?;

            // Input shape: [batch=1, time, features] but NeMo expects [batch, features, time]
            // We need to transpose
            let mut features_transposed = vec![0.0f32; num_frames * num_mels];
            for t in 0..num_frames {
                for f in 0..num_mels {
                    features_transposed[f * num_frames + t] = features_flat[t * num_mels + f];
                }
            }

            let encoder_input = ort::value::Tensor::from_array((
                [1, num_mels, num_frames],
                features_transposed.into_boxed_slice(),
            ))?;

            let features_length = ort::value::Tensor::from_array((
                [1],
                vec![num_frames as i64].into_boxed_slice(),
            ))?;

            let encoder_outputs = model.encoder.run(ort::inputs![
                "audio_signal" => encoder_input,
                "length" => features_length,
            ])?;

            // Encoder output shape: [batch, hidden_dim=1024, time]
            // Note: NeMo/Parakeet uses [batch, hidden, time] convention, NOT [batch, time, hidden]
            let encoder_out = encoder_outputs.get("outputs").or_else(|| encoder_outputs.get("logits"))
                .ok_or_else(|| anyhow::anyhow!("encoder output not found"))?;

            let (encoder_shape, data) = encoder_out.try_extract_tensor::<f32>()?;
            let encoder_dim = encoder_shape[1] as usize;  // hidden dimension (1024)
            let encoder_len = encoder_shape[2] as usize;  // time dimension

            debug!("encoder raw shape: {:?}", encoder_shape);

            // Copy data to owned Vec to release the borrow
            (data.to_vec(), encoder_len, encoder_dim)
        };

        debug!("encoder output: {} frames x {} dim", encoder_len, encoder_dim);

        // Run TDT greedy decoding with fresh model borrow
        let model = self.model.as_mut()
            .ok_or_else(|| SttError::ModelLoadError("model not loaded".to_string()))?;

        let (tokens, timestamps) = Self::decode_tdt_static(
            model,
            &encoder_data,
            encoder_len,
            encoder_dim,
            vocab_size,
            blank_id,
        )?;

        // Convert tokens to text (reborrow tokenizer)
        let tokenizer = self.tokenizer.as_ref()
            .ok_or_else(|| SttError::ModelLoadError("tokenizer not loaded".to_string()))?;
        let text = tokenizer.decode(&tokens);

        // Create segments with word-level timestamps
        let word_segments = tokenizer.decode_with_timestamps(
            &tokens,
            &timestamps,
            ENCODER_FRAME_DURATION_SEC * 1000.0, // ms per frame
        );

        let segments: Vec<TranscriptionSegment> = word_segments
            .into_iter()
            .map(|(word, start, end)| TranscriptionSegment {
                start,
                end,
                text: word,
                confidence: 1.0, // TDT doesn't provide confidence scores directly
            })
            .collect();

        Ok(TranscriptionResult {
            text,
            segments,
            language: self.config.language.clone(),
            processing_time_ms: 0, // Will be set by caller
        })
    }

    /// TDT greedy decoding algorithm (static method to avoid borrow conflicts)
    fn decode_tdt_static(
        model: &mut ParakeetModel,
        encoder_data: &[f32],
        encoder_len: usize,
        encoder_dim: usize,
        vocab_size: usize,
        blank_id: i32,
    ) -> Result<(Vec<i32>, Vec<i32>)> {
        let mut tokens = Vec::new();
        let mut timestamps = Vec::new();

        // Initialize decoder states: [num_layers, batch=1, hidden_dim]
        let state_shape = [model.pred_rnn_layers, 1, model.pred_hidden];
        let state_size = model.pred_rnn_layers * model.pred_hidden;

        let mut h_state = vec![0.0f32; state_size];
        let mut c_state = vec![0.0f32; state_size];

        // Start with blank token
        let mut prev_token = blank_id;

        let max_tokens_per_frame = 5;
        let mut tokens_this_frame = 0;
        let mut t = 0i32;

        while (t as usize) < encoder_len {
            // Get encoder output at time t
            // Shape is [batch=1, hidden_dim=1024, time], so data is stored as:
            // [h0_t0, h0_t1, ..., h0_tN, h1_t0, h1_t1, ..., h1_tN, ...]
            // To get all hidden dims at time t: encoder_data[d * encoder_len + t]
            let mut cur_encoder = vec![0.0f32; encoder_dim];
            for d in 0..encoder_dim {
                cur_encoder[d] = encoder_data[d * encoder_len + (t as usize)];
            }

            // Run decoder
            let decoder_input = ort::value::Tensor::from_array((
                [1, 1],
                vec![prev_token].into_boxed_slice(),
            ))?;

            let decoder_length = ort::value::Tensor::from_array((
                [1],
                vec![1i32].into_boxed_slice(),
            ))?;

            let h_input = ort::value::Tensor::from_array((
                state_shape,
                h_state.clone().into_boxed_slice(),
            ))?;

            let c_input = ort::value::Tensor::from_array((
                state_shape,
                c_state.clone().into_boxed_slice(),
            ))?;

            // Note: The Parakeet TDT decoder model input/output names from sherpa-onnx:
            // Inputs: "targets", "target_length", "states.1", "onnx::Slice_3"
            // Outputs: "outputs", "prednet_lengths", "states", "162"
            let decoder_outputs = model.decoder.run(ort::inputs![
                "targets" => decoder_input,
                "target_length" => decoder_length,
                "states.1" => h_input,
                "onnx::Slice_3" => c_input,
            ])?;

            // Get decoder output and updated states
            let decoder_out = decoder_outputs.get("outputs")
                .ok_or_else(|| anyhow::anyhow!("decoder output not found"))?;
            let (decoder_shape, decoder_data) = decoder_out.try_extract_tensor::<f32>()?;

            // Decoder output shape: [batch=1, hidden_dim=640, seq=1]
            let decoder_dim = decoder_shape[1] as usize;  // 640

            // Update states if present (output names from model)
            if let Some(new_h) = decoder_outputs.get("states") {
                let (_, h_data) = new_h.try_extract_tensor::<f32>()?;
                h_state = h_data.to_vec();
            }
            if let Some(new_c) = decoder_outputs.get("162") {
                let (_, c_data) = new_c.try_extract_tensor::<f32>()?;
                c_state = c_data.to_vec();
            }

            // Get decoder output for joiner - all hidden dims at seq=0
            // Shape is [batch=1, hidden=640, seq=1], so just take all 640 values
            let cur_decoder: Vec<f32> = decoder_data[..decoder_dim].to_vec();

            // Run joiner
            // Joiner expects encoder_outputs shape [batch, hidden, 1] and decoder_outputs [batch, hidden, 1]
            let joiner_encoder = ort::value::Tensor::from_array((
                [1, encoder_dim, 1],
                cur_encoder.into_boxed_slice(),
            ))?;

            let joiner_decoder = ort::value::Tensor::from_array((
                [1, decoder_dim, 1],
                cur_decoder.into_boxed_slice(),
            ))?;

            let joiner_outputs = model.joiner.run(ort::inputs![
                "encoder_outputs" => joiner_encoder,
                "decoder_outputs" => joiner_decoder,
            ])?;

            let logits = joiner_outputs.get("outputs")
                .ok_or_else(|| anyhow::anyhow!("joiner output not found"))?;
            let (logits_shape, logits_data) = logits.try_extract_tensor::<f32>()?;

            // Shape is [batch=1, vocab_size + num_durations, 1]
            let output_size = logits_shape[1] as usize;
            let num_durations = output_size.saturating_sub(vocab_size);

            // Split into token and duration logits
            // Token prediction: argmax over [0, vocab_size)
            let mut best_token = 0i32;
            let mut best_token_score = f32::NEG_INFINITY;
            for v in 0..vocab_size {
                let score = logits_data[v];
                if score > best_token_score {
                    best_token_score = score;
                    best_token = v as i32;
                }
            }

            // Duration prediction: argmax over [vocab_size, output_size)
            let mut skip = 1i32;
            if num_durations > 0 {
                let mut best_dur_score = f32::NEG_INFINITY;
                for d in 0..num_durations {
                    let score = logits_data[vocab_size + d];
                    if score > best_dur_score {
                        best_dur_score = score;
                        skip = d as i32;
                    }
                }
            }

            // Process prediction
            if best_token != blank_id {
                tokens.push(best_token);
                timestamps.push(t);
                prev_token = best_token;
                tokens_this_frame += 1;
            }

            // Handle skip logic
            if skip > 0 {
                tokens_this_frame = 0;
            }

            if tokens_this_frame >= max_tokens_per_frame {
                tokens_this_frame = 0;
                skip = 1;
            }

            if best_token == blank_id && skip == 0 {
                tokens_this_frame = 0;
                skip = 1;
            }

            t += skip.max(1);
        }

        debug!("decoded {} tokens", tokens.len());

        Ok((tokens, timestamps))
    }
}

/// Internal struct for loaded audio data
struct AudioData {
    samples: Vec<f32>,
    sample_rate: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = SttConfig::default();
        assert!(config.use_gpu);
        assert_eq!(config.num_threads, 4);
    }
}
