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
///
/// Patterns are applied in order; an earlier hit consumes the bytes and
/// prevents a later pattern from over-matching the redacted placeholder.
/// Specifically `api_key` runs before `hex40` so an `sk-ant-...` token
/// never gets re-classified as a hex blob, and `bearer` runs before
/// `hex40` so a `Bearer <40-char-hex>` token gets the bearer placeholder
/// (which carries more context for log readers) instead of the hex one.
struct ScrubberSet {
    api_key: Regex,
    github_pat: Regex,
    bearer: Regex,
    hex40: Regex,
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
            // GitHub PAT shapes: `ghp_`, `ghs_` (server), with the documented
            // 36-char base62 body. We exclude `gho_`, `ghu_`, `ghr_` because
            // those are OAuth/user/refresh tokens not normally pasted into
            // log lines, but the same shape would still trip `api_key` only
            // if they started with `sk-` (they do not), so adding them later
            // is purely a coverage decision.
            github_pat: Regex::new(r"gh[ps]_[A-Za-z0-9]{36}").expect("compile github_pat regex"),
            // Generic `Bearer <token>` shape. RFC 6750 token68 alphabet plus
            // `=` padding. {20,} keeps it off of stub words like `Bearer x`.
            bearer: Regex::new(r"Bearer\s+[A-Za-z0-9\-._~+/]{20,}=*")
                .expect("compile bearer regex"),
            // 40-char hex blob. Matches Unsplash access-key shape (and Git
            // SHA-1 commit hashes, which we do not log intentionally, so the
            // false-positive surface is negligible).
            hex40: Regex::new(r"(?i)\b[a-f0-9]{40}\b").expect("compile hex40 regex"),
            email: Regex::new(r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}")
                .expect("compile email regex"),
            user_profile_path,
        }
    }

    fn scrub(&self, s: &str) -> String {
        let s = self.api_key.replace_all(s, "sk-<REDACTED>");
        let s = self.github_pat.replace_all(&s, "gh<REDACTED>");
        let s = self.bearer.replace_all(&s, "Bearer <REDACTED>");
        let s = self.hex40.replace_all(&s, "<HEX40_REDACTED>");
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
    fn scrubber_redacts_github_pat() {
        // 36-char body after `ghp_` / `ghs_` per GitHub PAT docs.
        assert_eq!(
            scrub("PAT=ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"),
            "PAT=gh<REDACTED>"
        );
        // 36-char body after `ghs_` per GitHub PAT docs (a..z = 26,
        // 0..9 = 10). (mb-mac-v1.9: the prior input had two trailing
        // `AB` chars -> a 38-char body the 36-char regex correctly
        // leaves, so the assertion was wrong, not the scrubber.)
        assert_eq!(
            scrub("srv=ghs_abcdefghijklmnopqrstuvwxyz0123456789"),
            "srv=gh<REDACTED>"
        );
        // Not-a-PAT cases: wrong prefix, too short.
        assert_eq!(
            scrub("gho_unsupportedprefix000000000000000000"),
            "gho_unsupportedprefix000000000000000000"
        );
        assert_eq!(scrub("ghp_tooshort"), "ghp_tooshort");
    }

    #[test]
    fn scrubber_redacts_bearer_tokens() {
        // RFC-6750-shape token, base64-ish with `=` padding.
        assert_eq!(
            scrub("auth: Bearer abcdefghijklmnopqrstuvwx=="),
            "auth: Bearer <REDACTED>"
        );
        // Short bearer should not match (token body < 20 chars).
        assert_eq!(scrub("x = Bearer abc"), "x = Bearer abc");
        // Tab whitespace between `Bearer` and the token still scrubs.
        assert_eq!(
            scrub("h:\tBearer\tAAAAAAAAAAAAAAAAAAAAAAAA"),
            "h:\tBearer <REDACTED>"
        );
        // Bearer ahead of a 40-hex blob: `bearer` runs first and wins.
        // The placeholder text contains `<REDACTED>` not `<HEX40_REDACTED>`.
        assert_eq!(
            scrub("h: Bearer 0123456789abcdef0123456789abcdef01234567"),
            "h: Bearer <REDACTED>"
        );
    }

    #[test]
    fn scrubber_redacts_hex40_blobs() {
        // Unsplash access-key shape: 40 lowercase hex.
        assert_eq!(
            scrub("key=0123456789abcdef0123456789abcdef01234567 done"),
            "key=<HEX40_REDACTED> done"
        );
        // Mixed case still hits (case-insensitive).
        assert_eq!(
            scrub("DEADBEEFDEADBEEFDEADBEEFDEADBEEFDEADBEEF"),
            "<HEX40_REDACTED>"
        );
        // 39-char hex must NOT hit; 41-char hex must NOT hit on the boundary.
        // 39 chars (one short of the 40-hex trigger). (mb-mac-v1.9:
        // the prior literal was actually 40 chars -- a miscount that
        // the hex40 regex correctly redacted; the boundary test, not
        // the scrubber, was wrong.)
        let nine = "123456789abcdef0123456789abcdef01234567"; // 39 chars
        assert_eq!(scrub(nine), nine);
        let one = "123456789abcdef0123456789abcdef0123456789a"; // 41 chars
        assert_eq!(scrub(one), one);
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
