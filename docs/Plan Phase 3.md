Phase 3 Implementation Plan: Audio Capture + Speech-to-Text

 Overview

 Implement Phase 3 of Memoire: WASAPI audio capture (loopback + microphone) with Parakeet TDT speech-to-text
 integration, storing transcriptions in SQLite with FTS5 search.

 User's Audio Device: HD Pro Webcam C920 (2ch, 16-bit, 16000 Hz)

 ---
 Architecture Summary

 Audio Capture → WAV Chunks → STT Indexer → SQLite + FTS5
      ↓              ↓             ↓
   WASAPI        30-second      Parakeet TDT
 (loopback+mic)    files        via ONNX

 ---
 Implementation Phases

 Phase 3.1: Audio Capture Module

 New File: src/memoire-capture/src/audio.rs

 Dependencies to add:
 # memoire-capture/Cargo.toml
 wasapi = "2.0"    # Direct WASAPI for reliable loopback
 hound = "3.5"     # WAV file writing
 rubato = "0.15"   # High-quality resampling

 Core Components:
 - AudioDeviceInfo - Device enumeration struct
 - AudioCaptureConfig - Configuration (device, loopback mode, sample rate)
 - AudioCapture - Main capture struct with WASAPI backend
 - CapturedAudio - Captured audio chunk with metadata
 - Helper functions: save_wav(), resample(), to_mono()

 Key Features:
 - Support loopback (system audio) via AUDCLNT_STREAMFLAGS_LOOPBACK
 - Support input devices (microphone)
 - Resample to 16kHz mono (Parakeet requirement)
 - 30-second chunk duration
 - Channel communication via tokio::sync::mpsc

 ---
 Phase 3.2: Audio Processing Module

 New File: src/memoire-processing/src/audio_encoder.rs

 Core Components:
 - AudioEncoderConfig - Output directory, chunk duration, sample rate
 - AudioEncoder - Manages audio chunks similar to VideoEncoder

 File Storage Pattern:
 audio/{device_name}/{date}/chunk_{time}_{index}.wav

 ---
 Phase 3.3: New STT Crate

 New Crate: src/memoire-stt/

 Structure:
 src/memoire-stt/
   Cargo.toml
   src/
     lib.rs
     engine.rs      # ONNX model loading + inference
     processor.rs   # Audio preprocessing
     error.rs

 Dependencies:
 ort = { version = "2.0", features = ["cuda", "load-dynamic"] }
 hound = "3.5"
 rubato = "0.15"

 Core API:
 - SttEngine - Load Parakeet TDT ONNX model, transcribe audio
 - SttConfig - Model path, GPU enable, language
 - TranscriptionResult - Text, segments with timestamps, confidence
 - GPU/CPU fallback via ONNX Runtime execution providers

 Model: Parakeet TDT 0.6B v2 from https://huggingface.co/onnx-community/parakeet-tdt-0.6b-v2-ONNX
 - Location: %LOCALAPPDATA%\Memoire\models\
 - RTFx: 3386x (68x faster than Whisper)

 ---
 Phase 3.4: Audio Indexer

 New File: src/memoire-core/src/audio_indexer.rs

 Mirrors OCR indexer pattern:
 - AudioIndexer - Main struct with DB, STT engine, stats
 - AudioIndexerStats - Progress tracking
 - Batch processing: 5 chunks at a time
 - Rate limiting: 2 chunks/second max
 - Uses existing audio_chunks and audio_transcriptions tables

 ---
 Phase 3.5: Database Updates

 File: src/memoire-db/src/schema.rs - Add insertion types:
 - NewAudioChunk
 - NewAudioTranscription
 - AudioStats
 - SearchResult (unified OCR + audio)

 File: src/memoire-db/src/queries.rs - Add functions:
 - insert_audio_chunk()
 - get_audio_chunks_without_transcription()
 - insert_audio_transcription()
 - search_transcriptions()
 - search_all() - Unified search
 - get_audio_stats()

 ---
 Phase 3.6: CLI Commands

 File: src/memoire-core/src/main.rs

 New commands:
 memoire record --audio [--audio-device ID] [--loopback]
 memoire audio-devices          # List available devices
 memoire audio-index [--no-gpu] # Run STT indexer
 memoire search-all "query"     # Unified search
 memoire download-models        # Download Parakeet model

 ---
 Phase 3.7: Web API

 File: src/memoire-web/src/routes/api.rs

 New endpoints:
 GET /api/audio/chunks           # List audio chunks
 GET /api/audio/chunks/:id       # Chunk with transcription
 GET /api/audio/search?q=text    # Search transcriptions
 GET /api/stats/audio            # Audio indexing progress
 GET /api/search/all?q=text      # Unified OCR + audio search
 GET /audio/:filename            # Audio file streaming

 ---
 Files to Create

 | File                                        | Purpose                  |
 |---------------------------------------------|--------------------------|
 | src/memoire-capture/src/audio.rs            | WASAPI audio capture     |
 | src/memoire-processing/src/audio_encoder.rs | Audio chunk encoding     |
 | src/memoire-stt/Cargo.toml                  | New crate manifest       |
 | src/memoire-stt/src/lib.rs                  | STT crate exports        |
 | src/memoire-stt/src/engine.rs               | ONNX model inference     |
 | src/memoire-stt/src/processor.rs            | Audio preprocessing      |
 | src/memoire-stt/src/error.rs                | Error types              |
 | src/memoire-core/src/audio_indexer.rs       | Background transcription |
 | src/memoire-web/src/routes/audio.rs         | Audio streaming          |

 Files to Modify

 | File                              | Changes                                  |
 |-----------------------------------|------------------------------------------|
 | Cargo.toml (workspace)            | Add memoire-stt member                   |
 | src/memoire-capture/Cargo.toml    | Add wasapi, hound, rubato                |
 | src/memoire-capture/src/lib.rs    | Export audio module                      |
 | src/memoire-processing/Cargo.toml | Add hound                                |
 | src/memoire-processing/src/lib.rs | Export audio_encoder                     |
 | src/memoire-db/src/schema.rs      | Add NewAudioChunk, NewAudioTranscription |
 | src/memoire-db/src/queries.rs     | Add audio query functions                |
 | src/memoire-core/Cargo.toml       | Add memoire-stt dependency               |
 | src/memoire-core/src/lib.rs       | Export audio_indexer                     |
 | src/memoire-core/src/main.rs      | Add audio CLI commands                   |
 | src/memoire-core/src/config.rs    | Add audio config fields                  |
 | src/memoire-web/src/routes/mod.rs | Export audio routes                      |
 | src/memoire-web/src/server.rs     | Register audio routes                    |

 ---
 Performance Targets

 | Metric               | Target            |
 |----------------------|-------------------|
 | STT Latency          | <2s per 30s audio |
 | Search Latency       | <100ms            |
 | Audio Chunk Duration | 30 seconds        |
 | Sample Rate          | 16000 Hz (mono)   |

 ---
 Fallback Strategy

 1. Primary: Parakeet TDT via ort + CUDA
 2. Fallback 1: Parakeet via ort + CPU
 3. Fallback 2: Cloud STT (Deepgram/AssemblyAI) - future

 ---
 Implementation Order

 1. Audio capture (audio.rs) + device enumeration
 2. Audio encoder + WAV chunk storage
 3. Database query functions for audio
 4. CLI audio-devices and record --audio commands
 5. Test audio capture end-to-end
 6. Create memoire-stt crate with ONNX integration
 7. Model download command
 8. Audio indexer implementation
 9. Web API endpoints
 10. Unified search across OCR + transcriptions
 11. Integration testing

 ---
 User Decisions

 | Decision     | Choice                               |
 |--------------|--------------------------------------|
 | Capture Mode | Both simultaneously (mic + loopback) |
 | GPU Mode     | GPU with CPU fallback                |
 | STT Model    | Parakeet TDT v3 (25 languages)       |

 ---
 Verification Checklist

 After implementation, verify:
 - memoire audio-devices lists HD Pro Webcam C920
 - memoire record --audio --loopback captures both streams
 - Audio chunks saved as 30-second WAV files at 16kHz mono
 - memoire audio-index transcribes chunks with GPU acceleration
 - memoire search-all "query" returns results from both OCR and audio
 - Web API /api/search/all returns unified results
 - Build passes: cargo build --release
 - Commands work: memoire.exe record, memoire.exe index, memoire.exe viewer



 Phase 3 Implementation Summary

  I've successfully implemented the foundation for Phase 3 (Audio Capture + Speech-to-Text) of the Memoire project.
  Here's what was completed:

  New Files Created:

  - src/memoire-capture/src/audio.rs - WASAPI audio capture module (615 lines)
  - src/memoire-processing/src/audio_encoder.rs - Audio chunk encoder (230 lines)
  - src/memoire-stt/ - New crate for STT with ONNX Runtime integration
  - src/memoire-core/src/audio_indexer.rs - Background transcription indexer

  Modified Files:

  - Workspace Cargo.toml - Added memoire-stt crate and audio dependencies
  - memoire-capture/Cargo.toml - Added wasapi, hound, rubato dependencies
  - memoire-processing/Cargo.toml - Added hound dependency
  - memoire-db/src/schema.rs - Added audio schema types
  - memoire-db/src/queries.rs - Added audio query functions (325 lines)
  - memoire-core/src/main.rs - Added audio CLI commands

  New CLI Commands:

  1. memoire audio-devices - Lists all audio input/output devices (tested and working - found your HD Pro Webcam
  C920)
  2. memoire record-audio - Records audio to 30-second WAV chunks
  3. memoire audio-index - Runs background STT transcription

  What's Working:

  - Audio device enumeration via WASAPI
  - Audio capture configuration and setup
  - Database schema and queries for audio/transcriptions
  - STT engine framework (placeholder inference until model is downloaded)
  - All existing commands still work (record, index, viewer, search, etc.)

  Pending Items (for future work):

  1. Loopback capture - Returns error if requested (deferred)
  2. Parakeet TDT model download - Need to add download-models command
  3. Full STT inference - Currently returns placeholder results
  4. Web API endpoints for audio - Can be added in a follow-up commit