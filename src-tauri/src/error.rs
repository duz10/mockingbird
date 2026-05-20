//! Top-level application error type.
//!
//! Every fallible operation in `mockingbird_lib` returns
//! `Result<T, AppError>`. Module-specific error variants are added
//! here as the modules land in later Phase-1 waves.

use thiserror::Error;

/// The single error type returned from public API surfaces.
///
/// Variants are added per module as Phase 1 progresses. Keeping them
/// concentrated in one enum (rather than per-module sub-errors that
/// nest via `#[from]`) trades a slight loss of granularity for a much
/// simpler public surface and a single conversion point at the Tauri
/// command boundary.
#[derive(Error, Debug)]
pub enum AppError {
    /// Wrap a `std::io::Error`.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Wrap a Tauri-side error.
    #[error("tauri error: {0}")]
    Tauri(#[from] tauri::Error),

    /// Wrap a `rusqlite::Error` from the DB layer.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// Logging / tracing-subscriber initialization failures.
    #[error("tracing error: {0}")]
    Tracing(String),

    /// Audio capture / VAD failures (cpal, ort, ringbuf overflow).
    #[error("audio error: {0}")]
    Audio(String),

    /// Speech-to-text failures (whisper-rs init, transcribe, model load).
    #[error("stt error: {0}")]
    Stt(String),

    /// Hotkey subsystem failures.
    ///
    /// Sources: `SetWindowsHookEx` install failure, message-pump thread
    /// death, conflict-probe failures, watchdog timeouts. Phase 3 +.
    #[error("hotkey error: {0}")]
    Hotkey(String),

    /// Text-injection failures.
    ///
    /// Sources: clipboard lock contention, `SetClipboardData` failures,
    /// `SendInput` failures, secure-input abort (when treated as an
    /// error path rather than a normal abort), strategy resolution.
    /// Phase 3 +.
    #[error("injection error: {0}")]
    Injection(String),

    /// Cleanup-LLM failures.
    ///
    /// Sources: HTTP to Ollama / Claude (timeout, 5xx, connection
    /// refused), provider-side model errors, token budget exceeded
    /// before the raw transcript can fit, prompt assembly failures.
    /// Phase 4 +.
    #[error("cleanup error: {0}")]
    Cleanup(String),

    /// Secrets-store failures (DPAPI on Windows, Keychain on macOS).
    ///
    /// Sources: DPAPI protect/unprotect errors, missing key, file I/O.
    /// Phase 4 +.
    #[error("secrets error: {0}")]
    Secrets(String),

    /// Meeting-capture lifecycle failures.
    ///
    /// Sources: chord-hook install failure, twin-stream coordinator
    /// startup (mic or loopback unavailable when required), persist
    /// transaction failures, export I/O, overlay-window crashes. The
    /// chunker writes WAVs and reports its own `Io` errors via the
    /// existing variant; this one fires for higher-level meeting
    /// lifecycle. Phase MC +.
    #[error("meeting capture error: {0}")]
    MeetingCapture(String),

    /// Long-form chunked STT failures.
    ///
    /// Sources: chunk-WAV checksum mismatch (`crc32fast` verification),
    /// `transcribe_segments` per-chunk errors that abort the whole
    /// long-form pass, overlap-stitch boundary mis-alignment. Distinct
    /// from `Stt` so the meeting pipeline can demote a partial-success
    /// run without confusing dictation's per-utterance error path.
    /// Phase MC +.
    #[error("long-form stt error: {0}")]
    LongFormStt(String),

    /// Deterministic formatter failures.
    ///
    /// Sources: invariant violations the formatter detects in its own
    /// input (e.g. a `TimedSegment` with `t1 < t0`). The formatter is
    /// pure and deterministic; an error here is a contract bug in the
    /// caller (`long_form_stt`), not a transient failure. Phase MC +.
    #[error("formatter error: {0}")]
    Formatter(String),

    /// Generic catch-all for early Phase 1; replaced by typed variants
    /// as concrete modules surface their errors.
    #[error("{0}")]
    Other(String),
}

/// Convenience alias.
pub type AppResult<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_round_trips_via_from() {
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: AppError = inner.into();
        assert!(matches!(err, AppError::Io(_)));
    }

    #[test]
    fn other_displays_payload() {
        let err = AppError::Other("explanation".to_string());
        assert_eq!(err.to_string(), "explanation");
    }

    #[test]
    fn hotkey_displays_with_prefix() {
        let err = AppError::Hotkey("hook install failed".to_string());
        assert_eq!(err.to_string(), "hotkey error: hook install failed");
    }

    #[test]
    fn injection_displays_with_prefix() {
        let err = AppError::Injection("clipboard locked".to_string());
        assert_eq!(err.to_string(), "injection error: clipboard locked");
    }

    #[test]
    fn cleanup_displays_with_prefix() {
        let err = AppError::Cleanup("ollama refused connection".to_string());
        assert_eq!(err.to_string(), "cleanup error: ollama refused connection");
    }

    #[test]
    fn secrets_displays_with_prefix() {
        let err = AppError::Secrets("DPAPI unprotect failed".to_string());
        assert_eq!(err.to_string(), "secrets error: DPAPI unprotect failed");
    }

    #[test]
    fn meeting_capture_displays_with_prefix() {
        let err = AppError::MeetingCapture("loopback endpoint unavailable".to_string());
        assert_eq!(
            err.to_string(),
            "meeting capture error: loopback endpoint unavailable"
        );
    }

    #[test]
    fn long_form_stt_displays_with_prefix() {
        let err = AppError::LongFormStt("chunk 4/12 crc32 mismatch".to_string());
        assert_eq!(
            err.to_string(),
            "long-form stt error: chunk 4/12 crc32 mismatch"
        );
    }

    #[test]
    fn formatter_displays_with_prefix() {
        let err = AppError::Formatter("segment t1 < t0".to_string());
        assert_eq!(err.to_string(), "formatter error: segment t1 < t0");
    }
}
