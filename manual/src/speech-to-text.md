# Speech-to-Text

AZUREAL includes a fully local speech-to-text engine that lets you dictate
prompts instead of typing them. Audio is captured from your default input device,
transcribed on-device via whisper.cpp, and inserted at the cursor position in the
prompt or edit buffer. No audio ever leaves your machine.

---

## How It Works

The speech pipeline runs through four stages:

1. **Capture** -- Audio is recorded from the default input device via the `cpal`
   library (CoreAudio on macOS). Raw samples arrive as f32 at the device's native
   sample rate and channel count.
2. **Preprocessing** -- Multi-channel audio is mixed down to mono, then
   resampled to 16kHz (Whisper's expected input rate).
3. **Transcription** -- The accumulated audio buffer is fed to whisper.cpp with
   `Greedy { best_of: 1 }` decoding. On macOS, Metal GPU acceleration is used
   automatically.
4. **Insertion** -- The transcribed text is inserted at the current cursor
   position with smart spacing: a space is prepended if the cursor is not at the
   start of a line or immediately after a space.

---

## Toggle Recording

Press **`Ctrl+S`** while in prompt mode or edit mode to start recording. Press
**`Ctrl+S`** again to stop recording and trigger transcription.

The stop keybinding resolves from **any** focus state or mode while recording is
active. You do not need to navigate back to the prompt to stop -- `Ctrl+S` will
always stop an active recording regardless of where focus currently sits.

---

## Visual Feedback

While recording is active, two visual indicators appear:

- **Magenta border** -- The prompt or edit buffer border turns magenta to signal
  that the microphone is live.
- **REC / ... prefix** -- The status area shows `REC` while audio is being
  captured and `...` while transcription is in progress.

A progress indicator also appears in the status bar during the transcription
phase, since Whisper processing takes a moment depending on the length of the
recording.

---

## Resource Efficiency

The speech subsystem is designed to consume zero resources when not in use:

- **Background thread** -- The audio processing thread blocks on
  `mpsc::recv()` when idle. It consumes no CPU until a recording is started.
- **Lazy model loading** -- The `WhisperContext` is not created at startup.
  It is loaded on the first use of speech-to-text, so users who never dictate
  pay no memory cost.

Once loaded, the Whisper context remains in memory for the duration of the
session to avoid repeated model load times.

---

## Whisper Model

AZUREAL uses the `ggml-small.en` Whisper model, stored at:

```text
~/.azureal/speech/ggml-small.en.bin
```

This file is approximately **466 MB**. It is downloaded automatically on first
use if not already present. The `small.en` model provides a good balance between
transcription accuracy and speed for English-language input.

---

## Quick Reference

```text
Ctrl+S    Toggle recording on/off (prompt mode or edit mode)
Ctrl+S    Stop recording from ANY focus/mode (always resolves)
```

| Detail | Value |
|--------|-------|
| Audio library | cpal (CoreAudio on macOS) |
| Transcription engine | whisper.cpp (Metal GPU on macOS) |
| Sample pipeline | f32 -> mono mixdown -> 16kHz resample |
| Decoding strategy | Greedy { best_of: 1 } |
| Model file | `~/.azureal/speech/ggml-small.en.bin` (~466 MB) |
| Idle CPU usage | Zero (thread blocks on mpsc::recv) |
