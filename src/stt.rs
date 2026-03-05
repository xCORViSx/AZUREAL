//! Speech-to-text engine using cpal (microphone capture) + whisper-rs (transcription).
//!
//! Runs entirely on a background thread. Main thread sends Start/Stop commands
//! via mpsc channel, receives events (RecordingStarted, Transcribed, Error, etc.)
//! back via another channel. Zero CPU when idle — blocks on recv().
//!
//! Audio flow: cpal callback → Arc<Mutex<Vec<f32>>> → resample to 16kHz → whisper.
//! WhisperContext is lazy-loaded on first transcription and cached for reuse.
//! Model lives at ~/.azureal/speech/ggml-small.en.bin (~466MB).

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

/// Load the Whisper model from ~/.azureal/speech/ggml-small.en.bin.
/// Returns a descriptive error message with download instructions if the file is missing.
fn load_whisper_model() -> Result<whisper_rs::WhisperContext, String> {
    let model_dir = dirs::home_dir()
        .ok_or("Cannot find home directory")?
        .join(".azureal")
        .join("speech");
    let model_path = model_dir.join("ggml-small.en.bin");

    if !model_path.exists() {
        return Err(format!(
            "Whisper model not found. Download it:\n\
             mkdir -p ~/.azureal/speech && curl -L -o {} \
             https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin",
            model_path.display()
        ));
    }

    // Suppress all whisper.cpp + GGML debug output (model params, Metal info, system info).
    // Without log_backend/tracing_backend features, logs are silently dropped.
    whisper_rs::install_logging_hooks();

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
#[allow(dead_code)]
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
    // Multi-segment mode: allows Whisper to handle pauses and longer recordings
    // by splitting audio into multiple segments internally. Single-segment mode
    // treats the whole clip as one utterance and drops content after pauses.
    params.set_single_segment(false);
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── resample_to_16k: passthrough at 16kHz ──

    #[test]
    fn test_resample_16k_passthrough() {
        let samples = vec![0.1, 0.2, 0.3, 0.4, 0.5];
        let result = resample_to_16k(&samples, 16000);
        assert_eq!(result, samples);
    }

    #[test]
    fn test_resample_16k_passthrough_empty() {
        let samples: Vec<f32> = vec![];
        let result = resample_to_16k(&samples, 16000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_resample_16k_passthrough_single_sample() {
        let samples = vec![0.42];
        let result = resample_to_16k(&samples, 16000);
        assert_eq!(result, vec![0.42]);
    }

    // ── resample_to_16k: downsampling from 48kHz ──

    #[test]
    fn test_resample_48k_to_16k_reduces_length() {
        let samples: Vec<f32> = (0..48000).map(|i| (i as f32 / 48000.0).sin()).collect();
        let result = resample_to_16k(&samples, 48000);
        // 48000 / 3 = 16000 samples expected
        assert_eq!(result.len(), 16000);
    }

    #[test]
    fn test_resample_44100_to_16k_reduces_length() {
        let samples: Vec<f32> = (0..44100).map(|i| (i as f32 / 44100.0).sin()).collect();
        let result = resample_to_16k(&samples, 44100);
        // 44100 / (44100/16000) ≈ 16000
        let expected_len = (44100.0 / (44100.0 / 16000.0)) as usize;
        assert_eq!(result.len(), expected_len);
    }

    #[test]
    fn test_resample_32k_to_16k() {
        let samples: Vec<f32> = vec![1.0; 32000];
        let result = resample_to_16k(&samples, 32000);
        // 32000 / 2 = 16000
        assert_eq!(result.len(), 16000);
    }

    #[test]
    fn test_resample_48k_empty() {
        let samples: Vec<f32> = vec![];
        let result = resample_to_16k(&samples, 48000);
        assert!(result.is_empty());
    }

    #[test]
    fn test_resample_preserves_dc_signal() {
        // A constant signal should remain approximately constant after resampling
        let dc_value = 0.75f32;
        let samples: Vec<f32> = vec![dc_value; 48000];
        let result = resample_to_16k(&samples, 48000);
        for &s in &result {
            assert!((s - dc_value).abs() < 1e-5, "DC signal should be preserved: got {}", s);
        }
    }

    #[test]
    fn test_resample_zero_signal() {
        let samples: Vec<f32> = vec![0.0; 48000];
        let result = resample_to_16k(&samples, 48000);
        for &s in &result {
            assert_eq!(s, 0.0);
        }
    }

    #[test]
    fn test_resample_linear_interpolation() {
        // Two samples: [0.0, 1.0] at 32kHz → one sample at 16kHz
        // ratio = 32000/16000 = 2.0
        // For i=0: src_idx = 0.0, idx=0, frac=0.0, s0=0.0, s1=1.0 → 0.0
        let samples = vec![0.0, 1.0];
        let result = resample_to_16k(&samples, 32000);
        assert_eq!(result.len(), 1);
        assert!((result[0] - 0.0).abs() < 1e-5);
    }

    #[test]
    fn test_resample_output_in_valid_range() {
        // Input in [-1.0, 1.0] → output should also be in [-1.0, 1.0]
        let samples: Vec<f32> = (0..48000)
            .map(|i| (i as f32 * 2.0 * std::f32::consts::PI / 48000.0).sin())
            .collect();
        let result = resample_to_16k(&samples, 48000);
        for &s in &result {
            assert!(s >= -1.0 && s <= 1.0, "sample {} out of range", s);
        }
    }

    #[test]
    fn test_resample_negative_values() {
        let samples: Vec<f32> = vec![-1.0; 32000];
        let result = resample_to_16k(&samples, 32000);
        for &s in &result {
            assert!((s - (-1.0)).abs() < 1e-5);
        }
    }

    #[test]
    fn test_resample_alternating_signal() {
        let samples: Vec<f32> = (0..48000).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let result = resample_to_16k(&samples, 48000);
        assert_eq!(result.len(), 16000);
    }

    #[test]
    fn test_resample_very_short_signal() {
        let samples = vec![0.5, -0.5, 0.5];
        let result = resample_to_16k(&samples, 48000);
        // 3 / 3.0 = 1 sample
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_resample_ratio_correctness_48k() {
        let n = 96000;
        let samples: Vec<f32> = vec![0.0; n];
        let result = resample_to_16k(&samples, 48000);
        // expected: 96000 / 3.0 = 32000
        assert_eq!(result.len(), 32000);
    }

    #[test]
    fn test_resample_ratio_correctness_44100() {
        let n = 44100;
        let samples: Vec<f32> = vec![0.0; n];
        let result = resample_to_16k(&samples, 44100);
        let ratio = 44100.0_f64 / 16000.0;
        let expected = (n as f64 / ratio) as usize;
        assert_eq!(result.len(), expected);
    }

    // ── resample_to_16k: upsampling (rare but valid) ──

    #[test]
    fn test_resample_8k_to_16k_doubles_length() {
        let samples: Vec<f32> = vec![0.5; 8000];
        let result = resample_to_16k(&samples, 8000);
        // ratio = 8000/16000 = 0.5, out_len = 8000/0.5 = 16000
        assert_eq!(result.len(), 16000);
    }

    #[test]
    fn test_resample_8k_dc_preserved() {
        let dc = 0.33f32;
        let samples: Vec<f32> = vec![dc; 8000];
        let result = resample_to_16k(&samples, 8000);
        for &s in &result {
            assert!((s - dc).abs() < 1e-5);
        }
    }

    // ── SttCommand enum ──

    #[test]
    fn test_stt_command_start_variant() {
        let cmd = SttCommand::StartRecording;
        assert!(matches!(cmd, SttCommand::StartRecording));
    }

    #[test]
    fn test_stt_command_stop_variant() {
        let cmd = SttCommand::StopRecording;
        assert!(matches!(cmd, SttCommand::StopRecording));
    }

    // ── SttEvent enum ──

    #[test]
    fn test_stt_event_recording_started() {
        let event = SttEvent::RecordingStarted;
        assert!(matches!(event, SttEvent::RecordingStarted));
    }

    #[test]
    fn test_stt_event_recording_stopped() {
        let event = SttEvent::RecordingStopped { duration_secs: 2.5 };
        if let SttEvent::RecordingStopped { duration_secs } = event {
            assert!((duration_secs - 2.5).abs() < 1e-5);
        } else {
            panic!("expected RecordingStopped");
        }
    }

    #[test]
    fn test_stt_event_transcribed() {
        let event = SttEvent::Transcribed("hello world".to_string());
        if let SttEvent::Transcribed(text) = event {
            assert_eq!(text, "hello world");
        } else {
            panic!("expected Transcribed");
        }
    }

    #[test]
    fn test_stt_event_transcribed_empty() {
        let event = SttEvent::Transcribed(String::new());
        if let SttEvent::Transcribed(text) = event {
            assert!(text.is_empty());
        } else {
            panic!("expected Transcribed");
        }
    }

    #[test]
    fn test_stt_event_error() {
        let event = SttEvent::Error("mic not found".to_string());
        if let SttEvent::Error(msg) = event {
            assert_eq!(msg, "mic not found");
        } else {
            panic!("expected Error");
        }
    }

    #[test]
    fn test_stt_event_model_loading() {
        let event = SttEvent::ModelLoading;
        assert!(matches!(event, SttEvent::ModelLoading));
    }

    #[test]
    fn test_stt_event_model_ready() {
        let event = SttEvent::ModelReady;
        assert!(matches!(event, SttEvent::ModelReady));
    }

    // ── SttHandle channel communication ──

    #[test]
    fn test_stt_handle_channel_basics() {
        // Test that mpsc channel sends and receives SttCommand
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(SttCommand::StartRecording).unwrap();
        let cmd = rx.recv().unwrap();
        assert!(matches!(cmd, SttCommand::StartRecording));
    }

    #[test]
    fn test_stt_handle_event_channel() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(SttEvent::RecordingStarted).unwrap();
        tx.send(SttEvent::RecordingStopped { duration_secs: 1.0 }).unwrap();
        tx.send(SttEvent::Transcribed("test".to_string())).unwrap();
        assert!(matches!(rx.recv().unwrap(), SttEvent::RecordingStarted));
        assert!(matches!(rx.recv().unwrap(), SttEvent::RecordingStopped { .. }));
        assert!(matches!(rx.recv().unwrap(), SttEvent::Transcribed(_)));
    }

    #[test]
    fn test_stt_try_recv_empty_channel() {
        let (_tx, rx) = std::sync::mpsc::channel::<SttEvent>();
        assert!(rx.try_recv().is_err());
    }

    // ── resample_to_16k: edge cases ──

    #[test]
    fn test_resample_single_sample_high_rate() {
        let samples = vec![0.9];
        let result = resample_to_16k(&samples, 48000);
        // 1 / 3.0 = 0 samples
        assert!(result.is_empty() || result.len() == 1);
    }

    #[test]
    fn test_resample_two_samples_48k() {
        let samples = vec![0.0, 1.0];
        let result = resample_to_16k(&samples, 48000);
        // 2/3.0 = 0 samples (truncated)
        assert!(result.len() <= 1);
    }

    #[test]
    fn test_resample_three_samples_48k() {
        let samples = vec![0.0, 0.5, 1.0];
        let result = resample_to_16k(&samples, 48000);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_resample_deterministic() {
        let samples: Vec<f32> = (0..4800).map(|i| (i as f32 / 100.0).sin()).collect();
        let r1 = resample_to_16k(&samples, 48000);
        let r2 = resample_to_16k(&samples, 48000);
        assert_eq!(r1, r2);
    }

    #[test]
    fn test_resample_96k_to_16k() {
        let samples: Vec<f32> = vec![0.5; 96000];
        let result = resample_to_16k(&samples, 96000);
        // 96000 / 6.0 = 16000
        assert_eq!(result.len(), 16000);
    }

    // ── SttEvent: error message variations ──

    #[test]
    fn test_stt_event_error_with_format() {
        let event = SttEvent::Error(format!("Mic error: {}", "device not found"));
        if let SttEvent::Error(msg) = event {
            assert!(msg.contains("Mic error"));
            assert!(msg.contains("device not found"));
        }
    }

    #[test]
    fn test_stt_event_recording_stopped_zero_duration() {
        let event = SttEvent::RecordingStopped { duration_secs: 0.0 };
        if let SttEvent::RecordingStopped { duration_secs } = event {
            assert_eq!(duration_secs, 0.0);
        }
    }

    #[test]
    fn test_stt_event_recording_stopped_long_duration() {
        let event = SttEvent::RecordingStopped { duration_secs: 300.0 };
        if let SttEvent::RecordingStopped { duration_secs } = event {
            assert_eq!(duration_secs, 300.0);
        }
    }

    #[test]
    fn test_stt_event_transcribed_unicode() {
        let event = SttEvent::Transcribed("日本語テスト".to_string());
        if let SttEvent::Transcribed(text) = event {
            assert_eq!(text, "日本語テスト");
        }
    }

    #[test]
    fn test_stt_event_transcribed_multiline() {
        let event = SttEvent::Transcribed("line one\nline two\nline three".to_string());
        if let SttEvent::Transcribed(text) = event {
            assert!(text.contains('\n'));
            assert_eq!(text.lines().count(), 3);
        }
    }

    // ── Command/Event matching ──

    #[test]
    fn test_stt_command_matches_start() {
        let cmd = SttCommand::StartRecording;
        assert!(matches!(cmd, SttCommand::StartRecording));
        assert!(!matches!(cmd, SttCommand::StopRecording));
    }

    #[test]
    fn test_stt_command_matches_stop() {
        let cmd = SttCommand::StopRecording;
        assert!(matches!(cmd, SttCommand::StopRecording));
        assert!(!matches!(cmd, SttCommand::StartRecording));
    }

    #[test]
    fn test_all_stt_event_variants_constructable() {
        let events: Vec<SttEvent> = vec![
            SttEvent::RecordingStarted,
            SttEvent::RecordingStopped { duration_secs: 1.0 },
            SttEvent::Transcribed("text".into()),
            SttEvent::Error("err".into()),
            SttEvent::ModelLoading,
            SttEvent::ModelReady,
        ];
        assert_eq!(events.len(), 6);
    }

    // ── Additional resample tests ──

    #[test]
    fn test_resample_22050_to_16k() {
        let samples: Vec<f32> = vec![0.0; 22050];
        let result = resample_to_16k(&samples, 22050);
        let ratio = 22050.0_f64 / 16000.0;
        let expected = (22050.0 / ratio) as usize;
        assert_eq!(result.len(), expected);
    }

    #[test]
    fn test_resample_preserves_max_amplitude() {
        let samples: Vec<f32> = vec![1.0; 48000];
        let result = resample_to_16k(&samples, 48000);
        for &s in &result {
            assert!((s - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn test_resample_preserves_min_amplitude() {
        let samples: Vec<f32> = vec![-1.0; 48000];
        let result = resample_to_16k(&samples, 48000);
        for &s in &result {
            assert!((s - (-1.0)).abs() < 1e-5);
        }
    }

    #[test]
    fn test_resample_4k_to_16k_quadruples() {
        let samples: Vec<f32> = vec![0.5; 4000];
        let result = resample_to_16k(&samples, 4000);
        // 4000 / 0.25 = 16000
        assert_eq!(result.len(), 16000);
    }

    #[test]
    fn test_resample_output_capacity() {
        let samples: Vec<f32> = vec![0.0; 48000];
        let result = resample_to_16k(&samples, 48000);
        // Should have exactly the right number of samples
        assert!(result.capacity() >= result.len());
    }

    #[test]
    fn test_stt_event_error_empty_message() {
        let event = SttEvent::Error(String::new());
        if let SttEvent::Error(msg) = event {
            assert!(msg.is_empty());
        }
    }

    #[test]
    fn test_stt_command_channel_multiple() {
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(SttCommand::StartRecording).unwrap();
        tx.send(SttCommand::StopRecording).unwrap();
        tx.send(SttCommand::StartRecording).unwrap();
        assert!(matches!(rx.recv().unwrap(), SttCommand::StartRecording));
        assert!(matches!(rx.recv().unwrap(), SttCommand::StopRecording));
        assert!(matches!(rx.recv().unwrap(), SttCommand::StartRecording));
    }

    #[test]
    fn test_resample_16k_large_passthrough() {
        let samples: Vec<f32> = vec![0.5; 160000]; // 10 seconds at 16kHz
        let result = resample_to_16k(&samples, 16000);
        assert_eq!(result.len(), 160000);
        assert_eq!(result, samples);
    }
}
