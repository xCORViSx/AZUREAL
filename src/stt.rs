//! Speech-to-text engine using cpal (microphone capture) + whisper-rs (transcription).
//!
//! Runs entirely on a background thread. Main thread sends Start/Stop commands
//! via mpsc channel, receives events (RecordingStarted, Transcribed, Error, etc.)
//! back via another channel. Zero CPU when idle — blocks on recv().
//!
//! Audio flow: cpal callback → Arc<Mutex<Vec<f32>>> → resample to 16kHz → whisper.
//! WhisperContext is lazy-loaded on first transcription and cached for reuse.
//! Model lives at ~/.azureal/models/ggml-base.en.bin (~142MB).

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Command sent from main thread to STT background thread
pub enum SttCommand {
    StartRecording,
    StopRecording,
}

/// Event sent from STT background thread back to main thread
pub enum SttEvent {
    /// Microphone stream opened, recording in progress
    RecordingStarted,
    /// Recording stopped, about to transcribe
    RecordingStopped { duration_secs: f32 },
    /// Transcription complete — text ready for insertion
    Transcribed(String),
    /// Something went wrong (device not found, model missing, etc.)
    Error(String),
    /// Whisper model is being loaded from disk (first use only)
    ModelLoading,
    /// Whisper model loaded and ready
    ModelReady,
}

/// Main-thread handle for communicating with the STT background thread.
/// Owns the command sender and event receiver. Thread lives as long as this handle.
pub struct SttHandle {
    /// Send commands (Start/Stop) to the background thread
    cmd_tx: Sender<SttCommand>,
    /// Receive events (Transcribed, Error, etc.) from the background thread
    event_rx: Receiver<SttEvent>,
    /// Keep the thread alive — dropped when SttHandle is dropped
    _handle: thread::JoinHandle<()>,
}

impl SttHandle {
    /// Spawn the STT background thread. Call once — reuse the handle for all recordings.
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();

        let handle = thread::Builder::new()
            .name("stt".into())
            .spawn(move || stt_loop(cmd_rx, event_tx))
            .expect("failed to spawn STT thread");

        Self { cmd_tx, event_rx, _handle: handle }
    }

    /// Send a command to the background thread (non-blocking)
    pub fn send(&self, cmd: SttCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Poll for events from the background thread (non-blocking).
    /// Returns None when no events are pending.
    pub fn try_recv(&self) -> Option<SttEvent> {
        self.event_rx.try_recv().ok()
    }
}

/// Background thread main loop. Blocks on recv() when idle (zero CPU).
/// Manages recording state and WhisperContext lifetime.
fn stt_loop(cmd_rx: Receiver<SttCommand>, event_tx: Sender<SttEvent>) {
    // Recording state — either idle or actively capturing audio
    enum State {
        Idle,
        // cpal stream is live, samples accumulating in the shared buffer
        Recording {
            // Shared buffer between cpal audio callback and this thread
            samples: Arc<Mutex<Vec<f32>>>,
            // Sample rate from the input device (typically 44100 or 48000)
            sample_rate: u32,
            // Keep the cpal stream alive — dropping it stops capture
            _stream: cpal::Stream,
        },
    }

    let mut state = State::Idle;
    // Cached WhisperContext — loaded once on first transcription, reused forever
    let mut whisper_ctx: Option<whisper_rs::WhisperContext> = None;

    // Block on commands — zero CPU when idle
    while let Ok(cmd) = cmd_rx.recv() {
        match cmd {
            SttCommand::StartRecording => {
                // If already recording, ignore (user double-pressed)
                if matches!(state, State::Recording { .. }) { continue; }

                match start_recording() {
                    Ok((stream, samples, sample_rate)) => {
                        state = State::Recording { samples, sample_rate, _stream: stream };
                        let _ = event_tx.send(SttEvent::RecordingStarted);
                    }
                    Err(e) => {
                        let _ = event_tx.send(SttEvent::Error(format!("Mic error: {}", e)));
                    }
                }
            }
            SttCommand::StopRecording => {
                // Extract recording state, dropping _stream stops cpal capture
                let (samples_buf, sample_rate) = match state {
                    State::Recording { samples, sample_rate, _stream } => {
                        // _stream dropped here — stops audio capture
                        drop(_stream);
                        (samples, sample_rate)
                    }
                    State::Idle => continue, // Not recording, ignore
                };
                state = State::Idle;

                // Drain accumulated audio from the shared buffer
                let raw_samples = samples_buf.lock().unwrap().clone();
                let duration_secs = raw_samples.len() as f32 / sample_rate as f32;
                let _ = event_tx.send(SttEvent::RecordingStopped { duration_secs });

                // Need at least 0.5s of audio for meaningful transcription
                if duration_secs < 0.5 {
                    let _ = event_tx.send(SttEvent::Error("Recording too short (< 0.5s)".into()));
                    continue;
                }

                // Lazy-load Whisper model on first transcription
                if whisper_ctx.is_none() {
                    let _ = event_tx.send(SttEvent::ModelLoading);
                    match load_whisper_model() {
                        Ok(ctx) => {
                            whisper_ctx = Some(ctx);
                            let _ = event_tx.send(SttEvent::ModelReady);
                        }
                        Err(e) => {
                            let _ = event_tx.send(SttEvent::Error(e));
                            continue;
                        }
                    }
                }

                // Resample to 16kHz mono (Whisper's required format)
                let samples_16k = resample_to_16k(&raw_samples, sample_rate);

                // Run Whisper transcription
                match transcribe(whisper_ctx.as_ref().unwrap(), &samples_16k) {
                    Ok(text) => {
                        let _ = event_tx.send(SttEvent::Transcribed(text));
                    }
                    Err(e) => {
                        let _ = event_tx.send(SttEvent::Error(format!("Transcription failed: {}", e)));
                    }
                }
            }
        }
    }
}

/// Open the default input device and start capturing f32 audio samples.
/// Returns the live cpal stream, a shared sample buffer, and the device sample rate.
/// The stream must be kept alive (not dropped) for capture to continue.
fn start_recording() -> Result<(cpal::Stream, Arc<Mutex<Vec<f32>>>, u32), String> {
    let host = cpal::default_host();
    let device = host.default_input_device()
        .ok_or("No microphone found")?;

    // Use the device's default input config (usually 44100 or 48000 Hz, mono or stereo)
    let config = device.default_input_config()
        .map_err(|e| format!("Input config error: {}", e))?;
    // cpal 0.17: SampleRate is just u32, no tuple wrapper
    let sample_rate = config.sample_rate();
    let channels = config.channels() as usize;

    // Pre-allocate for ~60s of audio at the device's sample rate (mono).
    // This avoids repeated reallocations during recording.
    let samples = Arc::new(Mutex::new(Vec::with_capacity(sample_rate as usize * 60)));
    let samples_clone = Arc::clone(&samples);

    // Build the input stream. The callback runs on a high-priority CoreAudio thread.
    // Only operation: lock mutex + extend_from_slice (~10 microseconds per callback).
    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            // If stereo (or more), mix down to mono by averaging channels
            let mut buf = samples_clone.lock().unwrap();
            if channels == 1 {
                buf.extend_from_slice(data);
            } else {
                // Mix N channels to mono: average every `channels` samples
                for chunk in data.chunks(channels) {
                    let sum: f32 = chunk.iter().sum();
                    buf.push(sum / channels as f32);
                }
            }
        },
        |err| {
            eprintln!("cpal stream error: {}", err);
        },
        None, // no timeout
    ).map_err(|e| format!("Build stream error: {}", e))?;

    // Start the capture
    stream.play().map_err(|e| format!("Play error: {}", e))?;

    Ok((stream, samples, sample_rate))
}

/// Load the Whisper model from ~/.azureal/models/ggml-base.en.bin.
/// Returns a descriptive error message with download instructions if the file is missing.
fn load_whisper_model() -> Result<whisper_rs::WhisperContext, String> {
    let model_dir = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".azureal")
        .join("models");
    let model_path = model_dir.join("ggml-base.en.bin");

    if !model_path.exists() {
        return Err(format!(
            "Whisper model not found. Download it:\n\
             mkdir -p ~/.azureal/models && curl -L -o {} \
             https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
            model_path.display()
        ));
    }

    let params = whisper_rs::WhisperContextParameters::default();
    whisper_rs::WhisperContext::new_with_params(
        model_path.to_str().unwrap_or(""),
        params,
    ).map_err(|e| format!("Failed to load Whisper model: {}", e))
}

/// Resample audio from source_rate to 16000 Hz using linear interpolation.
/// Whisper requires 16kHz mono f32 input. This is a simple but effective
/// downsampler — good enough for speech (no high-frequency content to alias).
fn resample_to_16k(samples: &[f32], source_rate: u32) -> Vec<f32> {
    if source_rate == 16000 { return samples.to_vec(); }

    let ratio = source_rate as f64 / 16000.0;
    let out_len = (samples.len() as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;
        let s0 = samples.get(idx).copied().unwrap_or(0.0);
        let s1 = samples.get(idx + 1).copied().unwrap_or(s0);
        out.push((s0 as f64 * (1.0 - frac) + s1 as f64 * frac) as f32);
    }
    out
}

/// Run Whisper transcription on 16kHz mono f32 audio samples.
/// Returns the concatenated text from all recognized segments.
fn transcribe(ctx: &whisper_rs::WhisperContext, samples_16k: &[f32]) -> Result<String, String> {
    let mut state = ctx.create_state()
        .map_err(|e| format!("Create state error: {}", e))?;

    // Configure Whisper parameters for speech-to-text dictation
    let mut params = whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    // Disable printing to stdout — we capture text programmatically
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    // Single segment mode for short dictation clips
    params.set_single_segment(true);
    // Suppress non-speech tokens (reduce hallucinations on silence)
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);

    state.full(params, samples_16k)
        .map_err(|e| format!("Transcription error: {}", e))?;

    // Collect text from all segments using the iterator API
    let n_segments = state.full_n_segments();
    let mut text = String::new();
    for i in 0..n_segments {
        if let Some(segment) = state.get_segment(i) {
            if let Ok(s) = segment.to_str_lossy() {
                text.push_str(&s);
            }
        }
    }

    Ok(text)
}
