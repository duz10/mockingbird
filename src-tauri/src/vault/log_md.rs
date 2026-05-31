//! Phase 1E amendment `mb-bgpt` (ADR 0054 §E) -- **LOG.md** append-
//! only operations log.
//!
//! LOG.md sits at `<vault>/Knowledge Graph/LOG.md` and records every
//! operation performed against the vault by either agent
//! (Mockingbird + the chat-LLM). Format per ADR 0054 §E:
//!
//! ```markdown
//! ## [YYYY-MM-DD HH:MM] capture | <title>
//! ## [YYYY-MM-DD HH:MM] ingest | <source title>
//! ## [YYYY-MM-DD HH:MM] query | <question>
//! ## [YYYY-MM-DD HH:MM] lint | <summary>
//! ```
//!
//! Mockingbird only ever appends `capture` lines. The chat-LLM
//! authors `ingest`/`query`/`lint`. Both agents share the file --
//! the contract is **append-only**: nobody rewrites historical
//! lines. Crash safety is achieved via atomic temp-sibling-rename
//! of the full file on each append (same shape as
//! [`crate::vault::index_md::rebuild_index_md`]).
//!
//! # Why full-file rewrite (not POSIX `O_APPEND`)
//!
//! Two reasons:
//!
//! 1. **Atomicity contract parity**: the rest of the KG vault uses
//!    temp-sibling-rename for every write
//!    (`vault::writer::write_atomic`, `entity_pages::write_atomic`,
//!    `index_md::write_atomic`). Mixing `O_APPEND` here would
//!    introduce one outlier I/O shape with subtly different
//!    crash-recovery semantics, for no real win at this file size.
//! 2. **Cross-platform**: Windows + Obsidian Sync + iCloud +
//!    Syncthing all behave best with whole-file replacement. Append
//!    semantics over network FS are notoriously inconsistent.
//!
//! The cost (read + concat + atomic write per append) is fine for
//! a file that grows by ~80 bytes per capture and rarely exceeds a
//! few MB even after a year of heavy use.
//!
//! # Idempotency contract
//!
//! Append is **not** idempotent at the byte level — re-calling with
//! the same `(timestamp, kind, subject)` triple will write a
//! duplicate line. The caller (worker phase 5c) is responsible for
//! gating on the seal+mark_done transaction so a single filing
//! produces exactly one append. This matches the
//! `ensure_entity_page` write-once contract (entity pages dedupe by
//! filename; LOG lines dedupe by caller discipline).

use crate::error::{AppError, AppResult};
use crate::vault::kg_layout::kg_root_file_paths;
use chrono::{DateTime, Utc};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The four operation kinds documented in ADR 0054 §E. Exposed so
/// the chat-LLM-side helpers (future, out-of-scope this wave) can
/// share the same wire vocabulary. The worker only ever emits
/// [`LogOp::Capture`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogOp {
    /// Mockingbird filed a new Entry into the vault.
    Capture,
    /// chat-LLM ingested a source into the wiki.
    Ingest,
    /// chat-LLM answered a query against the vault.
    Query,
    /// chat-LLM ran a vault-wide health check.
    Lint,
}

impl LogOp {
    fn as_str(self) -> &'static str {
        match self {
            Self::Capture => "capture",
            Self::Ingest => "ingest",
            Self::Query => "query",
            Self::Lint => "lint",
        }
    }
}

/// Render the bootstrap LOG.md content -- the seed written by
/// [`crate::vault::kg_layout::bootstrap_kg_root_files`] when LOG.md
/// is missing.
///
/// Composes a short header comment + a single
/// `## [<now>] capture | KG activated` line so the file is never
/// empty post-bootstrap (an empty LOG.md would look like
/// "Mockingbird forgot to seed something"). The bootstrap timestamp
/// is captured at call time, not pinned, so determinism here is
/// scoped to "same second produces same bytes" rather than "every
/// install produces same bytes" -- the chat-LLM consumer doesn't
/// care.
///
/// Pure-ish: the only impure bit is `Utc::now()`. Tests inject a
/// fixed time via [`render_bootstrap_log_md_at`].
pub fn render_bootstrap_log_md() -> String {
    render_bootstrap_log_md_at(Utc::now())
}

/// Same as [`render_bootstrap_log_md`] but with the timestamp
/// injected -- the testable variant.
pub fn render_bootstrap_log_md_at(now: DateTime<Utc>) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("# LOG\n");
    out.push('\n');
    out.push_str("<!--\n");
    out.push_str("Append-only operations log for this Knowledge Graph vault.\n");
    out.push_str("Mockingbird appends `capture` lines after every successful\n");
    out.push_str("KG filing. The chat-LLM appends `ingest` / `query` / `lint`\n");
    out.push_str("lines after each of its workflows. Neither agent rewrites\n");
    out.push_str("historical lines. Format: `## [YYYY-MM-DD HH:MM] kind | subject`.\n");
    out.push_str("See SCHEMA.md.\n");
    out.push_str("-->\n");
    out.push('\n');
    out.push_str(&format_line(now, LogOp::Capture, "KG activated"));
    out.push('\n');
    out
}

/// Format one LOG.md line, including its trailing LF.
///
/// `subject` is folded onto a single line (newlines become spaces)
/// and pipe characters are escaped, so a malformed subject can't
/// corrupt the line grammar. Empty subjects render as
/// `<no subject>` rather than an empty tail, which would look like
/// a truncated line on visual scan.
pub fn format_line(ts: DateTime<Utc>, op: LogOp, subject: &str) -> String {
    let ts = ts.format("%Y-%m-%d %H:%M");
    let folded: String = subject
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let folded = folded.replace('|', r"\|");
    let folded = folded.trim();
    let subject = if folded.is_empty() {
        "<no subject>"
    } else {
        folded
    };
    format!("## [{ts}] {} | {subject}\n", op.as_str())
}

/// Append a single LOG.md line to the vault's LOG.md, creating the
/// file (with a header) if it doesn't yet exist.
///
/// Atomic: reads the existing file (if any), appends the new line,
/// writes the combined bytes to `<LOG.md>.mb-tmp`, and renames over
/// the target. Any crash leaves either the pre-append file or the
/// post-append file -- never a torn middle.
///
/// Caller contract: invoke AFTER the seal + mark_done transaction
/// has committed. A failed append is **non-fatal to the filing
/// queue** — the worker logs at `warn!` level and moves on; the
/// next successful filing's append is independent. Empty LOG.md
/// recovery happens through normal capture flow.
pub fn append_log_line(
    vault_root: &Path,
    ts: DateTime<Utc>,
    op: LogOp,
    subject: &str,
) -> AppResult<AppendOutcome> {
    let paths = kg_root_file_paths(vault_root);
    let target = paths.log_md;
    let line = format_line(ts, op, subject);

    let mut bytes = match fs::read_to_string(&target) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // File vanished between bootstrap and append (user
            // deleted it; vault relocation mid-run; first append on
            // a never-bootstrapped vault path because of a partial
            // bootstrap). Seed a fresh header so the appended line
            // doesn't dangle as the only content.
            render_bootstrap_log_md_at(ts)
        }
        Err(e) => {
            return Err(AppError::Vault(format!(
                "append_log_line: failed to read {} -- {e}",
                target.display()
            )));
        }
    };
    // Ensure exactly one blank line between the last record and
    // the new line, regardless of how the file's tail looks.
    if !bytes.ends_with('\n') {
        bytes.push('\n');
    }
    if !bytes.ends_with("\n\n") {
        bytes.push('\n');
    }
    bytes.push_str(&line);

    write_atomic(&target, bytes.as_bytes()).map_err(|e| {
        AppError::Vault(format!(
            "append_log_line: atomic write to {} -- {e}",
            target.display()
        ))
    })?;

    Ok(AppendOutcome { path: target, line })
}

/// Outcome of an append. Returned so the worker can `tracing::info!`
/// the exact line it just appended (debugging the LOG audit trail
/// is much easier when the worker logs match the file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppendOutcome {
    /// Absolute path of the LOG.md that was appended to.
    pub path: PathBuf,
    /// The line that was appended, including its trailing LF.
    pub line: String,
}

/// Atomic file write -- mirrors the other vault writers (same
/// `.mb-tmp` suffix so a future reconcile sweep catches crash-
/// leaked temps across all sites).
fn write_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".mb-tmp");
    let tmp_path = PathBuf::from(tmp);
    fs::write(&tmp_path, bytes)?;
    fs::rename(&tmp_path, target)?;
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn fixed_ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 15, 14, 32, 1).unwrap()
    }

    #[test]
    fn bootstrap_render_is_lf_only_and_deterministic_at_fixed_time() {
        let a = render_bootstrap_log_md_at(fixed_ts());
        let b = render_bootstrap_log_md_at(fixed_ts());
        assert_eq!(a, b);
        assert!(!a.contains('\r'), "LOG.md must be LF-only");
        assert!(
            a.contains("## [2026-06-15 14:32] capture | KG activated"),
            "bootstrap line missing or malformed: {a}"
        );
    }

    #[test]
    fn format_line_uses_ymd_hm_utc_and_escapes_pipes() {
        let line = format_line(fixed_ts(), LogOp::Capture, "buy | milk");
        assert_eq!(line, "## [2026-06-15 14:32] capture | buy \\| milk\n");
    }

    #[test]
    fn format_line_folds_newlines_and_emits_placeholder_for_empty() {
        let line = format_line(fixed_ts(), LogOp::Capture, "first\nsecond");
        assert!(line.contains("first second"));
        let empty = format_line(fixed_ts(), LogOp::Capture, "   ");
        assert!(empty.contains("<no subject>"));
    }

    #[test]
    fn format_line_renders_each_op_distinctly() {
        for (op, expected) in [
            (LogOp::Capture, "capture"),
            (LogOp::Ingest, "ingest"),
            (LogOp::Query, "query"),
            (LogOp::Lint, "lint"),
        ] {
            let line = format_line(fixed_ts(), op, "subject");
            assert!(
                line.contains(&format!("] {expected} |")),
                "expected `{expected}` in: {line}"
            );
        }
    }

    #[test]
    fn append_creates_file_with_header_when_missing() {
        let td = tempfile::TempDir::new().unwrap();
        crate::vault::kg_layout::bootstrap_kg_subtree(td.path()).unwrap();
        // Note: LOG.md NOT bootstrapped on purpose.

        let outcome =
            append_log_line(td.path(), fixed_ts(), LogOp::Capture, "first capture").unwrap();

        let bytes = fs::read_to_string(&outcome.path).unwrap();
        assert!(bytes.contains("# LOG"), "header missing: {bytes}");
        assert!(bytes.contains("first capture"));
        assert!(!bytes.contains('\r'));
    }

    #[test]
    fn append_preserves_existing_content_and_only_appends() {
        let td = tempfile::TempDir::new().unwrap();
        crate::vault::kg_layout::bootstrap_kg_subtree(td.path()).unwrap();
        crate::vault::kg_layout::bootstrap_kg_root_files(td.path()).unwrap();

        let r = kg_root_file_paths(td.path());
        let original = fs::read_to_string(&r.log_md).unwrap();

        let ts = Utc.with_ymd_and_hms(2026, 6, 15, 15, 0, 0).unwrap();
        append_log_line(td.path(), ts, LogOp::Capture, "second capture").unwrap();

        let after = fs::read_to_string(&r.log_md).unwrap();
        assert!(
            after.starts_with(&original) || after.contains(original.trim()),
            "original content must be preserved verbatim:\n--- before ---\n{original}\n--- after ---\n{after}"
        );
        assert!(after.contains("second capture"));
    }

    #[test]
    fn append_is_atomic_no_tmp_leftover() {
        let td = tempfile::TempDir::new().unwrap();
        crate::vault::kg_layout::bootstrap_kg_subtree(td.path()).unwrap();
        crate::vault::kg_layout::bootstrap_kg_root_files(td.path()).unwrap();

        append_log_line(td.path(), fixed_ts(), LogOp::Capture, "x").unwrap();

        let r = kg_root_file_paths(td.path());
        let mut tmp = r.log_md.as_os_str().to_owned();
        tmp.push(".mb-tmp");
        assert!(
            !PathBuf::from(tmp).exists(),
            "atomic write must clean up .mb-tmp"
        );
    }

    #[test]
    fn append_recovers_when_file_deleted_mid_life() {
        // User wiped LOG.md (or vault was relocated to a fresh root
        // mid-session). Next append must seed a header + the line,
        // not crash.
        let td = tempfile::TempDir::new().unwrap();
        crate::vault::kg_layout::bootstrap_kg_subtree(td.path()).unwrap();
        // No bootstrap_kg_root_files; emulate post-delete state.

        let outcome =
            append_log_line(td.path(), fixed_ts(), LogOp::Capture, "post-delete capture").unwrap();
        let body = fs::read_to_string(&outcome.path).unwrap();
        assert!(body.contains("# LOG"));
        assert!(body.contains("post-delete capture"));
    }
}
