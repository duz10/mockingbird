//! Tracing initialization with daily-rotated file output + PII scrubbing.
//!
//! Layer composition (bottom-up):
//!   1. EnvFilter (RUST_LOG, default "info")
//!   2. stdout fmt (ANSI on, target off — terse for dev)
//!   3. file fmt (ANSI off, target on, written through the scrubbing
//!      `MakeWriter` into a daily-rotating appender)
//!
//! PII scrubbing runs at the bytes-being-written boundary so all
//! formatter output passes through it.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use regex::Regex;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::error::{AppError, AppResult};

/// Initialize the tracing subscriber. Returns a `WorkerGuard` that
/// MUST be held by the caller for the duration of the app — dropping
/// it shuts down the background appender and silently loses logs.
///
/// Logs land at `<app_data_dir>/logs/mockingbird.YYYY-MM-DD.log` with
/// daily rotation.
pub fn init(app_data_dir: &Path) -> AppResult<WorkerGuard> {
    let logs_dir = app_data_dir.join("logs");
    std::fs::create_dir_all(&logs_dir)?;

    // Use the simple `daily` constructor for cross-version compatibility.
    // Phase-7 polish may revisit for retention (`max_log_files`).
    let file_appender = tracing_appender::rolling::daily(&logs_dir, "mockingbird.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let scrubbers = Arc::new(ScrubberSet::new(user_profile_dir()));
    let scrubbing_writer = ScrubbingMakeWriter {
        inner: non_blocking,
        scrubbers: scrubbers.clone(),
    };

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_writer(scrubbing_writer)
        .with_target(true);

    let stdout_layer = fmt::layer().with_ansi(true).with_target(false);

    tracing_subscriber::registry()
        .with(env_filter)
        .with(file_layer)
        .with(stdout_layer)
        .try_init()
        .map_err(|e| AppError::Tracing(format!("subscriber init: {e}")))?;

    Ok(guard)
}

/// Resolve `%USERPROFILE%` (or `$HOME` on Unix) at startup so we can
/// strip it from log output. Empty string if unavailable — scrubber
/// then becomes a no-op for paths.
fn user_profile_dir() -> String {
    #[cfg(windows)]
    {
        std::env::var("USERPROFILE").unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        std::env::var("HOME").unwrap_or_default()
    }
}

/// Compiled scrubber set. Construct once, share via `Arc`.
struct ScrubberSet {
    api_key: Regex,
    email: Regex,
    user_profile_path: String,
}

impl ScrubberSet {
    fn new(user_profile_path: String) -> Self {
        Self {
            // `sk-` covers Anthropic (`sk-ant-...`), OpenAI (`sk-...`, `sk-proj-...`),
            // and any future provider using the same shape. The {20,} guard avoids
            // false positives on short tokens.
            api_key: Regex::new(r"sk-[A-Za-z0-9_\-]{20,}").expect("compile api_key regex"),
            email: Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}")
                .expect("compile email regex"),
            user_profile_path,
        }
    }

    fn scrub(&self, s: &str) -> String {
        let s = self.api_key.replace_all(s, "sk-<REDACTED>");
        let s = self.email.replace_all(&s, "<EMAIL>");
        if self.user_profile_path.is_empty() {
            s.into_owned()
        } else {
            s.replace(&self.user_profile_path, "<HOME>")
        }
    }
}

/// `MakeWriter` adapter that wraps each per-event writer in a
/// scrubbing pass.
#[derive(Clone)]
struct ScrubbingMakeWriter<W: Clone> {
    inner: W,
    scrubbers: Arc<ScrubberSet>,
}

impl<'a, W> MakeWriter<'a> for ScrubbingMakeWriter<W>
where
    W: MakeWriter<'a> + Clone,
{
    type Writer = ScrubbingWriter<<W as MakeWriter<'a>>::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        ScrubbingWriter {
            inner: self.inner.make_writer(),
            scrubbers: self.scrubbers.clone(),
        }
    }
}

struct ScrubbingWriter<W: Write> {
    inner: W,
    scrubbers: Arc<ScrubberSet>,
}

impl<W: Write> Write for ScrubbingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let as_str = std::str::from_utf8(buf);
        match as_str {
            Ok(s) => {
                let scrubbed = self.scrubbers.scrub(s);
                self.inner.write_all(scrubbed.as_bytes())?;
                // Account for the original byte count — the writer
                // contract is "I consumed `buf.len()` bytes from your
                // side", not "I emitted N bytes downstream".
                Ok(buf.len())
            }
            Err(_) => self.inner.write(buf),
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

// Allow constructing PathBufs from string roots — used by tests.
#[allow(dead_code)]
fn ensure_path(p: impl AsRef<Path>) -> PathBuf {
    p.as_ref().to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrub(s: &str) -> String {
        let set = ScrubberSet::new("C:\\Users\\dustin".into());
        set.scrub(s)
    }

    #[test]
    fn scrubber_redacts_api_keys() {
        assert_eq!(
            scrub("auth=sk-ant-abcdefghij0123456789KLMN"),
            "auth=sk-<REDACTED>"
        );
        assert_eq!(
            scrub("openai=sk-proj-xyz0123456789abcdef0123"),
            "openai=sk-<REDACTED>"
        );
        // Short tokens shouldn't trigger.
        assert_eq!(scrub("not-a-key sk-short"), "not-a-key sk-short");
    }

    #[test]
    fn scrubber_redacts_emails() {
        assert_eq!(scrub("user dustin@example.com is in"), "user <EMAIL> is in");
        assert_eq!(scrub("contact: a.b+c@d.co.uk."), "contact: <EMAIL>.");
    }

    #[test]
    fn scrubber_redacts_user_profile_paths() {
        assert_eq!(
            scrub("loading C:\\Users\\dustin\\AppData\\Roaming\\Mockingbird"),
            "loading <HOME>\\AppData\\Roaming\\Mockingbird"
        );
    }

    #[test]
    fn scrubber_passes_innocent_text_unchanged() {
        assert_eq!(scrub("hello world"), "hello world");
        assert_eq!(scrub(""), "");
    }

    #[test]
    fn scrubber_with_empty_profile_path_is_noop_for_paths() {
        let set = ScrubberSet::new(String::new());
        assert_eq!(
            set.scrub("path C:\\Users\\dustin"),
            "path C:\\Users\\dustin"
        );
    }

    #[test]
    fn scrubbing_writer_round_trips_short_strings() {
        let mut buf = Vec::new();
        let scrubbers = Arc::new(ScrubberSet::new(String::new()));
        let mut w = ScrubbingWriter {
            inner: &mut buf,
            scrubbers,
        };
        let payload = b"user=alice@example.com key=sk-ant-abcdefghij0123456789KLMN";
        let n = w.write(payload).unwrap();
        assert_eq!(n, payload.len());
        let written = String::from_utf8(buf).unwrap();
        assert!(written.contains("<EMAIL>"));
        assert!(written.contains("sk-<REDACTED>"));
        assert!(!written.contains("alice@"));
    }

    /// Note: `init()` itself is not directly tested here because
    /// `tracing_subscriber::try_init` errors on second call (process-wide),
    /// and Cargo's default test runner shares a binary across tests.
    /// The Wave-5 manual smoke (`cargo tauri dev` + tail the log)
    /// covers init.
    #[test]
    fn init_creates_logs_dir_if_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let app_data = tmp.path();
        let _ = init(app_data); // ignore error; second test call into try_init
        assert!(app_data.join("logs").exists());
    }
}
