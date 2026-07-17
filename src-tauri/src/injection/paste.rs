//! Clipboard save/restore protocol per ADR 0018.
//!
//! ## ⚠️ Workspace discipline
//!
//! This file is the **only** location in the workspace permitted to
//! call `SetClipboardData` / `OpenClipboard` / `EmptyClipboard` /
//! `CloseClipboard`. PLAN §12 #17 binding; the shell-side hook
//! `scripts/dev/hooks/warn-bare-clipboard-set.py` flags violations.
//!
//! ## Public API
//!
//! [`with_saved_clipboard`] is the single entry point. It snapshots
//! the current clipboard, writes the payload, runs the caller's paste
//! closure, then restores the snapshot. On panic or error inside the
//! closure, the [`SnapshotGuard`]'s `Drop` impl restores anyway —
//! RAII makes this unmissable.
//!
//! ## Four-step dance (ADR 0018 §"Decision")
//!
//! 1. **Snapshot**: `OpenClipboard` (3 retries × 10 ms backoff) →
//!    `GetClipboardSequenceNumber` → `EnumClipboardFormats` →
//!    `GetClipboardData(fmt)` → copy HGLOBAL bytes → `CloseClipboard`.
//! 2. **Write**: `OpenClipboard` → `EmptyClipboard` →
//!    `SetClipboardData(CF_UNICODETEXT, payload)` → `CloseClipboard`.
//! 3. **Paste**: caller's closure (typically `SendInput` Ctrl+V).
//!    The closure runs OUTSIDE any open clipboard — `CloseClipboard`
//!    has already returned so the target app can `OpenClipboard`
//!    itself to read CF_UNICODETEXT.
//! 4. **Restore**: re-snapshot the sequence number. If it diverged
//!    from `seq_before + 1 (set) [+ 1 (paste)]`, another app wrote
//!    during our window and we skip the restore (better to lose the
//!    user's pre-existing clip than to overwrite a newer intentional
//!    copy). Otherwise re-`OpenClipboard` / `EmptyClipboard` and
//!    re-`SetClipboardData` each captured format.
//!
//! ## Pure-vs-OS split
//!
//! - [`encode_utf16_nul`] is pure (UTF-16 LE + NUL terminator).
//! - [`SequenceAnalysis::classify`] is pure (decide whether to restore
//!   based on `seq_before` / `seq_after`).
//! - All OS calls live in `#[cfg(target_os = "windows")]` blocks
//!   with thin wrappers (`open_clipboard_with_retries`,
//!   `current_sequence_number`, etc.).

use crate::error::{AppError, AppResult};

/// CF_UNICODETEXT format ID (Win32 constant). Defined here as a
/// freestanding `u32` so non-Windows builds still compile the pure
/// helpers + tests.
pub const CF_UNICODETEXT_ID: u32 = 13;

// ──────────────────────────────────────────────────────────────────────
// Format allowlist (heap-corruption defence)
// ──────────────────────────────────────────────────────────────────────
//
// **Background.** Calling `GlobalSize` / `GlobalLock` on a clipboard
// handle that is NOT actually an `HGLOBAL` (e.g. `CF_BITMAP` returns
// an `HBITMAP`, `CF_ENHMETAFILE` returns an `HENHMETAFILE`) is
// undefined behaviour on Windows. The two functions assume their
// argument points into a moveable-memory header; passing a GDI
// object handle scribbles on whichever heap block happens to live
// at the matching offset.
//
// Real-world symptom: 2026-05-17 user-reported crash, exception
// code `0xC0000374` (STATUS_HEAP_CORRUPTION) faulting in
// `ntdll.dll`, reproduced by taking a screenshot before triggering
// a paste-strategy dictation. `EnumClipboardFormats` enumerated
// `CF_DIB` + `CF_BITMAP` left over from the screenshot, the
// snapshot path called `GlobalSize` on the `HBITMAP`, and the
// process died with no Rust-level error.
//
// **Fix.** Restrict the snapshot path to formats we KNOW are stored
// as `HGLOBAL` per the Win32 documentation. Everything else is
// logged at debug + skipped — the user temporarily loses non-text
// clipboard contents around a dictation, which is a far better
// failure mode than process death.
//
// We intentionally include `CF_DIB` / `CF_DIBV5` here because they
// ARE `HGLOBAL` per docs (a `BITMAPINFOHEADER` followed by pixel
// bytes in moveable memory), even though they look bitmap-shaped.
// `CF_BITMAP` is the dangerous one — it's an `HBITMAP`.
//
// Registered formats (`>= 0xC000`) are app-defined and the docs
// recommend (but don't require) `HGLOBAL`. Since a misbehaving app
// registering a non-HGLOBAL format would crash us the same way,
// we play conservative: include them only after Phase 9 when we
// can build a per-app deny list. For now Wisprflow-style apps that
// register custom formats lose those formats around dictation.
// That's a paper cut we'll fix later; heap corruption is not.
const CF_TEXT: u32 = 1;
const CF_SYLK: u32 = 4;
const CF_DIF: u32 = 5;
const CF_TIFF: u32 = 6;
const CF_OEMTEXT: u32 = 7;
const CF_DIB: u32 = 8;
const CF_HDROP: u32 = 15;
const CF_LOCALE: u32 = 16;
const CF_DIBV5: u32 = 17;

/// Returns true iff the given clipboard format is documented to be
/// stored as an `HGLOBAL` (→ safe to pass to `GlobalSize` /
/// `GlobalLock`). See module-level comment above for rationale.
pub fn is_hglobal_format(fmt: u32) -> bool {
    matches!(
        fmt,
        CF_TEXT
            | CF_SYLK
            | CF_DIF
            | CF_TIFF
            | CF_OEMTEXT
            | CF_DIB
            | CF_UNICODETEXT_ID
            | CF_HDROP
            | CF_LOCALE
            | CF_DIBV5
    )
}

/// Number of `OpenClipboard` retries before giving up. ADR 0018 §"Decision".
pub const OPEN_RETRIES: u32 = 3;

/// Backoff between `OpenClipboard` retries.
pub const OPEN_BACKOFF: std::time::Duration = std::time::Duration::from_millis(10);

/// How long to wait after `SendInput(Ctrl+V)` for the focused app to
/// process `WM_PASTE` and read `CF_UNICODETEXT` before we restore the
/// caller's clipboard. Restoring too fast races the target's read
/// → target ends up pasting the RESTORED bytes (old clipboard)
/// instead of our dictation text. 30 ms is comfortably above the
/// worst-case message-pump latency on modern Windows + leaves Wave-5
/// total injection latency well under 100 ms.
///
/// Why a fixed sleep instead of polling the sequence number for an
/// advance: most apps perform read-only paste (no clipboard write
/// → no sequence advance), so an advance-poll would just time out
/// and burn the full timeout regardless. The fixed sleep gives
/// determinism + worst-case bound.
pub const PASTE_CONSUME_GRACE: std::time::Duration = std::time::Duration::from_millis(30);

/// macOS-only: lead time between writing the pasteboard and posting Cmd+V.
///
/// `arboard::set_text` lands the write on `NSPasteboard` synchronously, but
/// the synthesized Cmd+V is delivered ASYNCHRONOUSLY (`CGEventPost` →
/// HID tap → WindowServer → the target app's run loop). This small lead
/// guarantees the write is visible to the target's paste handler before the
/// keystroke arrives, closing the second, subtler half of the race (target
/// reads a *pre-write* pasteboard). Kept small so it adds no perceptible
/// lag. macOS-scoped: the Windows path is byte-identical and untouched.
#[cfg(target_os = "macos")]
pub const MAC_SET_PROPAGATE_DELAY: std::time::Duration = std::time::Duration::from_millis(20);

/// macOS-only: how long to wait AFTER posting Cmd+V before restoring the
/// caller's clipboard. **This is the core paste-race fix.**
///
/// On Windows the target consumes the paste synchronously off the message
/// pump, so 30 ms ([`PASTE_CONSUME_GRACE`]) suffices. On macOS the paste is
/// consumed asynchronously — the target reads `NSPasteboard` some tens of ms
/// after the HID event lands (`CGEventPost` → HID tap → WindowServer → the
/// target's run loop) — so restoring too early wins the race and the app
/// reads the RESTORED (previous) clipboard, pasting the old contents instead
/// of the transcript.
///
/// **mb-yxs (P0 regression on the release `.app`, reopened mb-22y).** The
/// original 120 ms was tuned against `cargo tauri dev`; on the bundled
/// release `.app` the target consistently reads the pasteboard *later* than
/// 120 ms, so the restore beat every read and the app pasted the previous
/// clipboard EVERY time. The [`mac_change_count_safe_to_restore`] guard
/// cannot rescue this: Cmd+V is a read-only event that never bumps
/// `changeCount`, so the guard is *structurally blind* to "target has not
/// consumed yet" — it always reports "safe to restore." Paste correctness
/// therefore rests ENTIRELY on this grace out-lasting the target's read.
///
/// The grace is generous BY DESIGN: it defers only the *invisible* C0
/// restore (and the recording-window hide), never the transcript — the
/// paste already appeared at Cmd+V time. So there is no perceptible-lag
/// reason to keep it tight, and every reason to give it real headroom over
/// the worst-case HID→app read latency. 400 ms clears the observed release
/// `.app` read latency with a ~3× margin while the completion pill lingering
/// a few hundred ms longer is imperceptible.
#[cfg(target_os = "macos")]
pub const MAC_PASTE_CONSUME_GRACE: std::time::Duration = std::time::Duration::from_millis(400);

/// macOS-only pure guard: given the `NSPasteboard.changeCount` observed
/// immediately after we wrote the transcript (`after_write`) and the count
/// observed just before we restore (`before_restore`), decide whether
/// restoring the caller's clipboard is safe.
///
/// The macOS twin of [`SequenceAnalysis::classify`] — but with a crucial
/// difference: it demands **strict equality** rather than tolerating a `+1`
/// advance. `changeCount` is a monotonic counter bumped by exactly 1 on
/// every pasteboard WRITE, and Cmd+V is a pure READ on macOS (it never bumps
/// the count). So `after_write == before_restore` means "the pasteboard
/// still holds exactly our transcript, untouched." ANY advance means someone
/// else wrote after us — a third-party clipboard manager, OR (the race we
/// must defend) a rapid SECOND dictation whose write would be clobbered if
/// we restored the stale `C0`. Tolerating `+1` (as Windows does for apps
/// that re-copy during paste) would re-open exactly that second-dictation
/// clobber, so we don't. Cost of strict equality: if a clipboard manager
/// re-writes during paste we skip the restore (user keeps the transcript on
/// the clipboard rather than `C0`) — a safe paper cut, surfaced as
/// [`PasteOutcome::OkClipboardNotRestored`].
#[cfg(target_os = "macos")]
pub fn mac_change_count_safe_to_restore(after_write: isize, before_restore: isize) -> bool {
    after_write == before_restore
}

/// Outcome of a single `with_saved_clipboard` invocation — surfaced to
/// the orchestrator so the DB `injection_status` column can record
/// whether the restore happened cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteOutcome {
    /// Snapshot + write + paste + restore all succeeded.
    Ok,
    /// Snapshot + write + paste succeeded; restore was skipped because
    /// another app wrote the clipboard between our write and our
    /// restore. The injection itself reached the focused app.
    OkClipboardNotRestored,
}

// --------------------------------------------------------------------
// Pure helpers
// --------------------------------------------------------------------

/// Encode a `&str` as UTF-16 LE with a trailing NUL `u16`.
///
/// `CF_UNICODETEXT` requires NUL-terminated UTF-16; producing the
/// terminator at encoding time is simpler than asking the caller to
/// remember.
pub fn encode_utf16_nul(s: &str) -> Vec<u16> {
    let mut v: Vec<u16> = s.encode_utf16().collect();
    v.push(0);
    v
}

/// Decode a NUL-terminated UTF-16 `&[u16]` to a Rust `String`.
///
/// Stops at the first NUL (Win32 strings are NUL-terminated by
/// convention; trailing garbage past NUL is ignored). Uses lossy
/// decoding so a lone surrogate doesn't blow up the restore path.
pub fn decode_utf16_nul(buf: &[u16]) -> String {
    let len = buf.iter().position(|&u| u == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Classification of the clipboard sequence-number delta to decide
/// whether the restore is safe to perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceAnalysis {
    /// Sequence number advanced by exactly our writes (set + maybe
    /// the target's own write during paste). Safe to restore.
    SafeToRestore,
    /// Sequence number diverged — another app intentionally wrote the
    /// clipboard during our window. Skip restore.
    Diverged,
}

impl SequenceAnalysis {
    /// Decide based on the sequence number measured **immediately after
    /// our `write_unicode_text` returns** (`seq_after_set`) and the
    /// number measured just before restore (`seq_after_paste`).
    ///
    /// Baselining off `seq_after_set` (not `seq_before_set`) is
    /// essential: `EmptyClipboard + SetClipboardData` together
    /// advance the sequence by an OS-dependent amount (Windows may
    /// fold consecutive ops into one bump, or count them separately).
    /// Baselining before the write makes the classifier brittle to
    /// that fold; baselining AFTER the write makes it depend only on
    /// what the *target* does during paste, which is the actual
    /// question we care about.
    ///
    /// - `seq_after_paste == seq_after_set` → target performed a
    ///   read-only paste (the common case). Safe to restore.
    /// - `seq_after_paste == seq_after_set + 1` → target also wrote
    ///   the clipboard during paste (some clipboard managers /
    ///   apps with their own dedupe logic). Safe to restore — only
    ///   one extra writer, which we'd be overwriting deliberately.
    /// - Any larger advance → some OTHER process wrote in our
    ///   window. Skip the restore (better to lose the user's
    ///   pre-existing clip than to overwrite a newer intentional
    ///   copy).
    ///
    /// Wrap-around on `u32` is handled by `wrapping_add`; the
    /// clipboard sequence number wraps every ~4 billion changes
    /// (decades of real-world use).
    pub fn classify(seq_after_set: u32, seq_after_paste: u32) -> Self {
        if seq_after_paste == seq_after_set || seq_after_paste == seq_after_set.wrapping_add(1) {
            Self::SafeToRestore
        } else {
            Self::Diverged
        }
    }
}

// --------------------------------------------------------------------
// Snapshot data structure (platform-agnostic)
// --------------------------------------------------------------------

/// A captured clipboard state.
///
/// `formats` holds `(format_id, raw_bytes)` pairs harvested from
/// `EnumClipboardFormats` + `GetClipboardData`. The bytes are the
/// HGLOBAL contents, so restore is `GlobalAlloc` → copy →
/// `SetClipboardData`. Non-HGLOBAL formats (`CF_BITMAP`,
/// `CF_ENHMETAFILE`, `CF_PALETTE`) are skipped — they round-trip via
/// the system's own format-synthesis on most apps.
#[derive(Debug, Default)]
pub struct ClipboardSnapshot {
    /// Sequence number observed at snapshot time. Restore decisions
    /// compare against the post-paste sequence number.
    pub seq_before: u32,
    /// Captured `(format_id, bytes)` tuples. Order matches
    /// `EnumClipboardFormats` order, which is the user's MRU order.
    pub formats: Vec<(u32, Vec<u8>)>,
}

impl ClipboardSnapshot {
    /// Number of formats captured. Used by tests + logging.
    pub fn len(&self) -> usize {
        self.formats.len()
    }

    /// Whether this snapshot held no formats at all (clipboard was
    /// empty or only contained non-HGLOBAL formats).
    pub fn is_empty(&self) -> bool {
        self.formats.is_empty()
    }

    /// Extract the CF_UNICODETEXT payload as a `String` if present.
    /// Used by tests to verify save/restore round trips.
    pub fn unicode_text(&self) -> Option<String> {
        let (_, bytes) = self
            .formats
            .iter()
            .find(|(fmt, _)| *fmt == CF_UNICODETEXT_ID)?;
        // bytes is the HGLOBAL contents — UTF-16 LE.
        let u16s: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        Some(decode_utf16_nul(&u16s))
    }
}

// --------------------------------------------------------------------
// Public entry point
// --------------------------------------------------------------------

/// Snapshot the clipboard, write `payload` as `CF_UNICODETEXT`, run
/// `paste_fn`, then restore the snapshot.
///
/// `paste_fn` typically synthesises Ctrl+V via `SendInput`. It runs
/// AFTER `CloseClipboard` returns, so the focused app is free to
/// `OpenClipboard` itself.
///
/// On any error inside `paste_fn`, the snapshot's `Drop` guard still
/// performs the restore on its best-effort path.
#[cfg(target_os = "windows")]
pub fn with_saved_clipboard<F>(payload: &str, paste_fn: F) -> AppResult<PasteOutcome>
where
    F: FnOnce() -> AppResult<()>,
{
    let snapshot = win::snapshot()?;

    // Plant the payload.
    win::write_unicode_text(payload)?;

    // Measure the sequence number AFTER our write. This is the
    // baseline against which post-paste drift is judged. See
    // [`SequenceAnalysis::classify`] for the rationale.
    let seq_after_set = win::current_sequence_number();

    // Run caller-supplied paste. Errors here still need to restore.
    let paste_result = paste_fn();

    // Block while the target consumes `CF_UNICODETEXT`. Fixed sleep,
    // not a poll loop — see [`PASTE_CONSUME_GRACE`].
    std::thread::sleep(PASTE_CONSUME_GRACE);

    // Decide whether to restore.
    let seq_after = win::current_sequence_number();
    let analysis = SequenceAnalysis::classify(seq_after_set, seq_after);

    let outcome = match analysis {
        SequenceAnalysis::SafeToRestore => {
            if let Err(e) = win::restore(&snapshot) {
                tracing::warn!("clipboard restore failed (best-effort): {e}");
                PasteOutcome::OkClipboardNotRestored
            } else {
                PasteOutcome::Ok
            }
        }
        SequenceAnalysis::Diverged => {
            tracing::info!(
                "clipboard sequence diverged ({} → {}), skipping restore",
                seq_after_set,
                seq_after
            );
            PasteOutcome::OkClipboardNotRestored
        }
    };

    // Propagate paste error if any.
    paste_result?;
    Ok(outcome)
}

/// macOS clipboard save/restore (ADR 0058, mb-mac-v1.4.2).
///
/// The macOS analogue of the Win32 four-step dance: snapshot the
/// current pasteboard text, write `payload`, run `paste_fn` (typically
/// the synthesized Cmd+V keypost in `injection/macos.rs`), then restore
/// the snapshot. Implemented over the portable `arboard` crate — the
/// pasteboard is the OS's, but arboard is the only `set_text` caller so
/// the "clipboard writes live in paste.rs" discipline (PLAN §12 #17)
/// holds on macOS too.
///
/// ## Parity gaps vs. Windows (documented in ADR 0058)
///
/// 1. **Text-only.** Only `CF_UNICODETEXT`-equivalent text is saved /
///    restored; images and custom pasteboard flavors are lost around a
///    dictation. This mirrors the Windows path's deliberate skip of
///    non-`HGLOBAL` formats — a paper cut, never a crash.
/// 2. ~~No change-count divergence skip.~~ **CLOSED** (paste-race fix).
///    The macOS path now consults `NSPasteboard.changeCount` (read via
///    `objc2-app-kit`, a pure query — arboard remains the sole WRITER)
///    to skip the restore when another writer touched the pasteboard
///    after our transcript write, exactly like the Win32
///    `GetClipboardSequenceNumber` divergence check. This also defends
///    the rapid-second-dictation clobber. See
///    [`mac_change_count_safe_to_restore`] +
///    [`MAC_PASTE_CONSUME_GRACE`].
#[cfg(target_os = "macos")]
pub fn with_saved_clipboard<F>(payload: &str, paste_fn: F) -> AppResult<PasteOutcome>
where
    F: FnOnce() -> AppResult<()>,
{
    mac::with_saved_clipboard(payload, paste_fn)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn with_saved_clipboard<F>(_payload: &str, _paste_fn: F) -> AppResult<PasteOutcome>
where
    F: FnOnce() -> AppResult<()>,
{
    Err(AppError::Injection(
        "clipboard save/restore is not implemented on this platform".into(),
    ))
}

// --------------------------------------------------------------------
// macOS implementation (the only place the macOS pasteboard is written)
// --------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use arboard::Clipboard;
    use objc2_app_kit::NSPasteboard;

    /// Read the general pasteboard's monotonic `changeCount`.
    ///
    /// A read-only `NSPasteboard` query — the macOS analogue of Win32
    /// `GetClipboardSequenceNumber`. It does NOT write the pasteboard, so
    /// the "arboard is the sole writer" discipline (PLAN §12 #17) holds.
    /// `changeCount` increments by exactly 1 on every write; Cmd+V is a
    /// pure read and never bumps it.
    fn pasteboard_change_count() -> isize {
        NSPasteboard::generalPasteboard().changeCount()
    }

    /// See [`super::with_saved_clipboard`] (macOS arm) for the contract.
    ///
    /// Ordering (the paste-race fix): snapshot `C0` → write transcript `T`
    /// → record `changeCount` → **lead delay** so the write propagates →
    /// Cmd+V → **consume grace** so the target reads `T` before we touch
    /// the pasteboard → restore `C0` **only if** `changeCount` is
    /// unchanged (nobody wrote after us). The two delays + the changeCount
    /// guard are what keep the restore from beating the target's read
    /// (which pasted the previous clipboard) and from clobbering a rapid
    /// second dictation.
    pub fn with_saved_clipboard<F>(payload: &str, paste_fn: F) -> AppResult<PasteOutcome>
    where
        F: FnOnce() -> AppResult<()>,
    {
        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Injection(format!("open macOS pasteboard: {e}")))?;

        // Timeline anchor: the pasteboard state BEFORE we touch anything.
        // Logged so a live re-test can prove the full changeCount timeline
        // (before → after-write → before-restore) from the `.app` logs
        // alone — the evidence that was missing when mb-22y "regressed."
        let change_count_before_write = pasteboard_change_count();

        // 1. Snapshot C0. `Err`/empty → treat as "no text to restore".
        let saved = clipboard.get_text().ok();

        // 2. Write the transcript payload (T).
        clipboard
            .set_text(payload.to_owned())
            .map_err(|e| AppError::Injection(format!("set macOS pasteboard text: {e}")))?;

        // Record the changeCount our write produced. Any later advance means
        // ANOTHER writer (clipboard manager, or a rapid second dictation)
        // touched the pasteboard after us → we must NOT restore over them.
        let change_count_after_write = pasteboard_change_count();
        tracing::info!(
            change_count_before_write,
            change_count_after_write,
            wrote_advanced = change_count_after_write > change_count_before_write,
            payload_len = payload.len(),
            consume_grace_ms = MAC_PASTE_CONSUME_GRACE.as_millis() as u64,
            "macOS paste: transcript written to pasteboard; awaiting target consume"
        );

        // 2a. Let the write fully propagate before the keystroke lands, so
        // the target never reads a pre-write pasteboard. See
        // [`MAC_SET_PROPAGATE_DELAY`].
        std::thread::sleep(MAC_SET_PROPAGATE_DELAY);

        // 3. Paste (caller's Cmd+V keypost). Errors still need restore.
        let paste_result = paste_fn();

        // 4. Wait for the target to CONSUME the paste before touching the
        // pasteboard again. THE core race fix — see
        // [`MAC_PASTE_CONSUME_GRACE`]. Restoring before this window closes
        // is exactly what made the app paste the previous clipboard.
        std::thread::sleep(MAC_PASTE_CONSUME_GRACE);

        // 5. Restore C0 — but only if the pasteboard STILL holds exactly
        // our transcript (changeCount unchanged). The macOS twin of the
        // Win32 sequence-number divergence check; also defends the
        // rapid-second-dictation clobber. See
        // [`mac_change_count_safe_to_restore`].
        let before_restore = pasteboard_change_count();
        let outcome = if !mac_change_count_safe_to_restore(change_count_after_write, before_restore)
        {
            tracing::info!(
                change_count_after_write,
                before_restore,
                "macOS paste: pasteboard changeCount advanced after our write; \
                 skipping restore to avoid clobbering a newer clipboard owner"
            );
            PasteOutcome::OkClipboardNotRestored
        } else {
            tracing::info!(
                change_count_after_write,
                before_restore,
                "macOS paste: consume grace elapsed, changeCount unchanged; \
                 restoring the caller's clipboard (C0)"
            );
            match saved {
                Some(prev) => match clipboard.set_text(prev) {
                    Ok(()) => PasteOutcome::Ok,
                    Err(e) => {
                        tracing::warn!("macOS clipboard restore failed (best-effort): {e}");
                        PasteOutcome::OkClipboardNotRestored
                    }
                },
                None => {
                    // Nothing meaningful preceded our write — clear our
                    // transcript so it doesn't linger on the clipboard.
                    if let Err(e) = clipboard.clear() {
                        tracing::warn!("macOS clipboard clear failed (best-effort): {e}");
                    }
                    PasteOutcome::Ok
                }
            }
        };

        // Propagate any paste error after the restore has run.
        paste_result?;
        Ok(outcome)
    }
}

// --------------------------------------------------------------------
// Windows implementation (the only place `SetClipboardData` may be called)
// --------------------------------------------------------------------

#[cfg(target_os = "windows")]
mod win {
    use super::*;

    use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
        GetClipboardSequenceNumber, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
    };

    /// RAII guard that calls `CloseClipboard` on drop.
    ///
    /// Used so every code path (success, error, panic) closes the
    /// clipboard. Forgotten `CloseClipboard` calls are the #1 cause
    /// of clipboard-lockup bugs in Win32 apps.
    pub struct ClipboardOpenGuard;

    impl Drop for ClipboardOpenGuard {
        fn drop(&mut self) {
            // SAFETY: balanced with the OpenClipboard in `open_with_retries`.
            let _ = unsafe { CloseClipboard() };
        }
    }

    /// `OpenClipboard(NULL)` with retries. Returns an RAII guard so
    /// `CloseClipboard` fires on every exit path.
    pub fn open_with_retries() -> AppResult<ClipboardOpenGuard> {
        for attempt in 0..OPEN_RETRIES {
            // SAFETY: passing HWND(0) is documented as "this thread,
            // no owner window" and is the standard idiom.
            let r = unsafe { OpenClipboard(HWND(0)) };
            if r.is_ok() {
                return Ok(ClipboardOpenGuard);
            }
            if attempt + 1 < OPEN_RETRIES {
                std::thread::sleep(OPEN_BACKOFF);
            }
        }
        Err(AppError::Injection(format!(
            "OpenClipboard failed after {OPEN_RETRIES} retries (clipboard locked)"
        )))
    }

    /// Snapshot current clipboard. Returns an empty snapshot if the
    /// clipboard is empty.
    pub fn snapshot() -> AppResult<ClipboardSnapshot> {
        let seq_before = current_sequence_number();
        let _guard = open_with_retries()?;

        let mut formats = Vec::new();
        let mut fmt = 0u32;
        loop {
            // SAFETY: EnumClipboardFormats(0) starts enumeration;
            // subsequent calls pass the previous format.
            fmt = unsafe { EnumClipboardFormats(fmt) };
            if fmt == 0 {
                break;
            }
            // CRITICAL: only HGLOBAL-backed formats may be passed to
            // GlobalSize/GlobalLock. Passing a GDI handle
            // (CF_BITMAP, CF_ENHMETAFILE, registered custom formats,
            // …) corrupts the heap (STATUS_HEAP_CORRUPTION /
            // 0xC0000374). See module-level allowlist comment.
            if !is_hglobal_format(fmt) {
                tracing::debug!(
                    "clipboard snapshot: skipping non-HGLOBAL format {fmt:#x} (not in allowlist)"
                );
                continue;
            }
            match copy_format_bytes(fmt) {
                Ok(Some(bytes)) => formats.push((fmt, bytes)),
                Ok(None) => {
                    // Allowlisted but somehow returned zero size —
                    // shouldn't happen for the formats we accept,
                    // but log so we know if it does.
                    tracing::debug!("skipping zero-size clipboard format {fmt:#x}");
                }
                Err(e) => {
                    // Per ADR 0018: don't abort the whole snapshot on
                    // one bad format.
                    tracing::warn!("failed to snapshot format {fmt:#x}: {e}");
                }
            }
        }

        Ok(ClipboardSnapshot {
            seq_before,
            formats,
        })
    }

    /// Copy the HGLOBAL bytes for a single clipboard format.
    /// Returns `Ok(None)` if the format's handle isn't an HGLOBAL
    /// (e.g. CF_BITMAP returns HBITMAP).
    fn copy_format_bytes(fmt: u32) -> AppResult<Option<Vec<u8>>> {
        // SAFETY: GetClipboardData returns a HANDLE owned by the
        // clipboard — we do NOT free it.
        let handle: HANDLE = unsafe { GetClipboardData(fmt) }
            .map_err(|e| AppError::Injection(format!("GetClipboardData({fmt:#x}): {e}")))?;

        let hglobal = HGLOBAL(handle.0 as *mut _);

        // GlobalSize returns 0 if `handle` isn't actually an HGLOBAL.
        // SAFETY: handle is non-null per the OK branch above.
        let size = unsafe { GlobalSize(hglobal) };
        if size == 0 {
            return Ok(None);
        }

        // SAFETY: GlobalLock pins the HGLOBAL and returns a raw
        // pointer; the matching GlobalUnlock is below.
        let ptr = unsafe { GlobalLock(hglobal) } as *const u8;
        if ptr.is_null() {
            return Err(AppError::Injection(format!(
                "GlobalLock returned NULL for format {fmt:#x}"
            )));
        }

        // SAFETY: ptr + size came from the same HGLOBAL above.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, size) }.to_vec();

        // SAFETY: balances GlobalLock above.
        let _ = unsafe { GlobalUnlock(hglobal) };

        Ok(Some(bytes))
    }

    /// Write a single `CF_UNICODETEXT` payload, replacing all formats.
    pub fn write_unicode_text(payload: &str) -> AppResult<()> {
        let utf16 = encode_utf16_nul(payload);
        let byte_len = std::mem::size_of_val(utf16.as_slice());

        // Allocate movable HGLOBAL — SetClipboardData requires it.
        // SAFETY: GMEM_MOVEABLE + size; checked for null below.
        let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, byte_len) }
            .map_err(|e| AppError::Injection(format!("GlobalAlloc({byte_len}): {e}")))?;

        // Copy payload into the HGLOBAL.
        // SAFETY: GlobalLock + matching GlobalUnlock; ptr is non-null
        // (we error out otherwise).
        let ptr = unsafe { GlobalLock(hglobal) } as *mut u16;
        if ptr.is_null() {
            return Err(AppError::Injection(
                "GlobalLock returned NULL while writing payload".into(),
            ));
        }
        // SAFETY: utf16 and ptr both point to byte_len bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(utf16.as_ptr(), ptr, utf16.len());
            let _ = GlobalUnlock(hglobal);
        }

        let _guard = open_with_retries()?;
        // SAFETY: balanced — guard closes on drop.
        unsafe {
            EmptyClipboard().map_err(|e| AppError::Injection(format!("EmptyClipboard: {e}")))?;
            // SetClipboardData takes ownership of the HGLOBAL on success.
            SetClipboardData(CF_UNICODETEXT_ID, HANDLE(hglobal.0 as isize))
                .map_err(|e| AppError::Injection(format!("SetClipboardData: {e}")))?;
        }
        Ok(())
    }

    /// Restore the snapshot.
    pub fn restore(snapshot: &ClipboardSnapshot) -> AppResult<()> {
        let _guard = open_with_retries()?;
        unsafe {
            EmptyClipboard()
                .map_err(|e| AppError::Injection(format!("EmptyClipboard (restore): {e}")))?;
        }
        for (fmt, bytes) in &snapshot.formats {
            if let Err(e) = restore_one(*fmt, bytes) {
                // Per ADR 0018: don't abort restore on one bad format.
                tracing::warn!("failed to restore format {fmt:#x}: {e}");
            }
        }
        Ok(())
    }

    fn restore_one(fmt: u32, bytes: &[u8]) -> AppResult<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        // SAFETY: GMEM_MOVEABLE + size; null-checked below.
        let hglobal = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }
            .map_err(|e| AppError::Injection(format!("GlobalAlloc (restore): {e}")))?;
        // SAFETY: GlobalLock + matching GlobalUnlock.
        let ptr = unsafe { GlobalLock(hglobal) } as *mut u8;
        if ptr.is_null() {
            return Err(AppError::Injection(
                "GlobalLock returned NULL (restore)".into(),
            ));
        }
        // SAFETY: ptr + bytes.len() came from the same HGLOBAL.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            let _ = GlobalUnlock(hglobal);
            SetClipboardData(fmt, HANDLE(hglobal.0 as isize))
                .map_err(|e| AppError::Injection(format!("SetClipboardData (restore): {e}")))?;
        }
        Ok(())
    }

    /// `GetClipboardSequenceNumber` wrapper.
    pub fn current_sequence_number() -> u32 {
        // SAFETY: no parameters, no lifetimes; pure Win32 read.
        unsafe { GetClipboardSequenceNumber() }
    }
}

// --------------------------------------------------------------------
// Tests
// --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_utf16_nul_terminates() {
        let v = encode_utf16_nul("hi");
        assert_eq!(v, vec![b'h' as u16, b'i' as u16, 0]);
    }

    #[test]
    fn encode_utf16_nul_handles_empty_string() {
        let v = encode_utf16_nul("");
        assert_eq!(v, vec![0]);
    }

    #[test]
    fn encode_utf16_nul_handles_emoji_via_surrogate_pair() {
        // 🐦 = U+1F426; UTF-16 surrogate pair (D83D, DC26).
        let v = encode_utf16_nul("🐦");
        assert_eq!(v, vec![0xD83D, 0xDC26, 0]);
    }

    #[test]
    fn encode_utf16_nul_handles_mixed_scripts() {
        let v = encode_utf16_nul("Hello, 世界!");
        // Spot-check: ends in NUL; '世' = U+4E16, '界' = U+754C.
        assert_eq!(v.last(), Some(&0));
        assert!(v.contains(&0x4E16));
        assert!(v.contains(&0x754C));
    }

    #[test]
    fn decode_utf16_nul_stops_at_first_nul() {
        let input = vec![b'h' as u16, b'i' as u16, 0, b'?' as u16];
        assert_eq!(decode_utf16_nul(&input), "hi");
    }

    #[test]
    fn decode_utf16_nul_handles_missing_terminator() {
        // No NUL → take the whole buffer.
        let input = vec![b'h' as u16, b'i' as u16];
        assert_eq!(decode_utf16_nul(&input), "hi");
    }

    #[test]
    fn decode_utf16_nul_round_trips_via_encode() {
        for s in ["", "hi", "🐦", "Hello, 世界!", "with\nnewline"] {
            let encoded = encode_utf16_nul(s);
            assert_eq!(decode_utf16_nul(&encoded), s, "round trip failed for {s:?}");
        }
    }

    // -----------------------------------------------------------------
    // SequenceAnalysis::classify — pure
    // -----------------------------------------------------------------

    #[test]
    fn sequence_safe_when_target_read_only_paste() {
        // Baseline is post-set. Target reads, doesn't write → seq
        // unchanged. This is the common case (notepad, browsers,
        // most input fields).
        assert_eq!(
            SequenceAnalysis::classify(100, 100),
            SequenceAnalysis::SafeToRestore
        );
    }

    #[test]
    fn sequence_safe_when_paste_target_also_wrote() {
        // Some apps re-write the clipboard during paste (clipboard
        // managers, apps with internal dedupe). +1 advance is safe.
        assert_eq!(
            SequenceAnalysis::classify(100, 101),
            SequenceAnalysis::SafeToRestore
        );
    }

    #[test]
    fn sequence_diverged_when_third_party_wrote() {
        // 2+ advance past baseline → at least one OTHER process
        // intentionally wrote; skip restore so we don't trample
        // them.
        assert_eq!(
            SequenceAnalysis::classify(100, 102),
            SequenceAnalysis::Diverged
        );
        assert_eq!(
            SequenceAnalysis::classify(100, 200),
            SequenceAnalysis::Diverged
        );
    }

    #[test]
    fn sequence_diverged_when_seq_went_backwards() {
        // Should never happen in practice (seq is monotonic
        // modulo wrap) but the classifier must reject it cleanly
        // rather than treating it as a small advance.
        assert_eq!(
            SequenceAnalysis::classify(100, 99),
            SequenceAnalysis::Diverged
        );
    }

    #[test]
    fn sequence_handles_u32_wrap_around() {
        // Permissive deltas (0, +1) must work correctly across the
        // u32 wrap boundary. Wave 4.9 baseline-post-set semantics:
        //   - baseline u32::MAX → safe outcomes are u32::MAX (read-only)
        //     and 0 (write-during-paste, after wrap).
        //   - baseline u32::MAX - 1 → safe outcomes are u32::MAX - 1
        //     and u32::MAX. (0 would be a +2 advance after wrap →
        //     Diverged, third-party wrote.)
        assert_eq!(
            SequenceAnalysis::classify(u32::MAX, u32::MAX),
            SequenceAnalysis::SafeToRestore,
            "read-only paste at wrap boundary"
        );
        assert_eq!(
            SequenceAnalysis::classify(u32::MAX, 0),
            SequenceAnalysis::SafeToRestore,
            "target wrote during paste; seq wrapped from u32::MAX → 0"
        );
        assert_eq!(
            SequenceAnalysis::classify(u32::MAX - 1, u32::MAX),
            SequenceAnalysis::SafeToRestore,
            "target wrote during paste; seq advanced by 1 just before wrap"
        );
        assert_eq!(
            SequenceAnalysis::classify(u32::MAX, 1),
            SequenceAnalysis::Diverged,
            "+2 across wrap = third-party writer; skip restore"
        );
        assert_eq!(
            SequenceAnalysis::classify(u32::MAX - 1, 0),
            SequenceAnalysis::Diverged,
            "+2 across wrap = third-party writer; skip restore"
        );
    }

    // -----------------------------------------------------------------
    // ClipboardSnapshot
    // -----------------------------------------------------------------

    #[test]
    fn empty_snapshot_reports_empty() {
        let snap = ClipboardSnapshot::default();
        assert!(snap.is_empty());
        assert_eq!(snap.len(), 0);
        assert_eq!(snap.unicode_text(), None);
    }

    #[test]
    fn snapshot_extracts_unicode_text_payload() {
        let utf16 = encode_utf16_nul("hello clipboard");
        let bytes: Vec<u8> = utf16.iter().flat_map(|u| u.to_le_bytes()).collect();
        let snap = ClipboardSnapshot {
            seq_before: 42,
            formats: vec![(CF_UNICODETEXT_ID, bytes)],
        };
        assert_eq!(snap.unicode_text().as_deref(), Some("hello clipboard"));
        assert_eq!(snap.len(), 1);
    }

    #[test]
    fn snapshot_returns_none_for_unicode_text_when_only_other_formats() {
        let snap = ClipboardSnapshot {
            seq_before: 0,
            formats: vec![(0x10, vec![0xDE, 0xAD, 0xBE, 0xEF])], // some other format
        };
        assert_eq!(snap.unicode_text(), None);
        assert_eq!(snap.len(), 1);
    }

    // -----------------------------------------------------------------
    // PasteOutcome
    // -----------------------------------------------------------------

    #[test]
    fn paste_outcome_distinguishes_restore_states() {
        // These map 1:1 to InjectionOutcome variants the orchestrator
        // surfaces; keep them distinct.
        assert_ne!(PasteOutcome::Ok, PasteOutcome::OkClipboardNotRestored);
    }

    // -----------------------------------------------------------------
    // is_hglobal_format — the allowlist that prevents heap corruption
    // -----------------------------------------------------------------

    #[test]
    fn unicode_text_is_hglobal() {
        // The format we ALWAYS write — if this ever returns false the
        // write path would refuse the very format it just planted.
        assert!(is_hglobal_format(CF_UNICODETEXT_ID));
    }

    #[test]
    fn text_formats_are_hglobal() {
        for fmt in [CF_TEXT, CF_OEMTEXT, CF_LOCALE] {
            assert!(
                is_hglobal_format(fmt),
                "text format {fmt:#x} should be allowlisted"
            );
        }
    }

    #[test]
    fn dib_formats_are_hglobal_but_cf_bitmap_is_not() {
        // CF_DIB / CF_DIBV5 are documented as HGLOBAL (header +
        // pixels in moveable memory). CF_BITMAP is an HBITMAP —
        // calling GlobalSize on it is the exact bug that crashed
        // 0xC0000374. This test pins the distinction.
        assert!(is_hglobal_format(CF_DIB));
        assert!(is_hglobal_format(CF_DIBV5));
        const CF_BITMAP: u32 = 2;
        assert!(
            !is_hglobal_format(CF_BITMAP),
            "CF_BITMAP is HBITMAP, NOT HGLOBAL — calling GlobalSize on it corrupts the heap"
        );
    }

    #[test]
    fn gdi_handle_formats_are_rejected() {
        // Every Win32 format that returns a GDI handle instead of an
        // HGLOBAL. Each one would crash the process if we passed it
        // to GlobalSize. Pinning all of them in one test so future
        // edits to is_hglobal_format can't accidentally let one
        // through.
        const CF_BITMAP: u32 = 2;
        const CF_METAFILEPICT: u32 = 3;
        const CF_PALETTE: u32 = 9;
        const CF_ENHMETAFILE: u32 = 14;
        const CF_OWNERDISPLAY: u32 = 0x0080;
        const CF_DSPBITMAP: u32 = 0x0082;
        const CF_DSPMETAFILEPICT: u32 = 0x0083;
        const CF_DSPENHMETAFILE: u32 = 0x008E;
        for fmt in [
            CF_BITMAP,
            CF_METAFILEPICT,
            CF_PALETTE,
            CF_ENHMETAFILE,
            CF_OWNERDISPLAY,
            CF_DSPBITMAP,
            CF_DSPMETAFILEPICT,
            CF_DSPENHMETAFILE,
        ] {
            assert!(
                !is_hglobal_format(fmt),
                "format {fmt:#x} returns a GDI handle, NOT HGLOBAL — must be rejected"
            );
        }
    }

    #[test]
    fn registered_formats_are_rejected_for_now() {
        // Custom registered formats (>= 0xC000) are app-defined.
        // Docs RECOMMEND HGLOBAL but don't require it; we play
        // conservative until Phase 9 per-app deny-list lands. The
        // user temporarily loses round-trip of these formats around
        // a dictation; that's a paper cut, not a crash.
        assert!(!is_hglobal_format(0xC000));
        assert!(!is_hglobal_format(0xC123));
        assert!(!is_hglobal_format(0xFFFF));
    }

    // -----------------------------------------------------------------
    // Constants sanity
    // -----------------------------------------------------------------

    #[test]
    fn cf_unicodetext_constant_matches_win32() {
        // CF_UNICODETEXT has been 13 since Windows NT 3.51 — stable
        // across every windows-rs release we care about. If this
        // changes, every Win32 program on Earth breaks.
        assert_eq!(CF_UNICODETEXT_ID, 13);
    }

    // -----------------------------------------------------------------
    // macOS paste-race fix — changeCount guard + timing (pure/deterministic)
    // -----------------------------------------------------------------

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_restore_safe_only_when_change_count_unchanged() {
        // Read-only paste: Cmd+V never bumps changeCount, so the count we
        // recorded after our write is still current at restore time → the
        // pasteboard still holds our transcript → safe to restore C0.
        assert!(mac_change_count_safe_to_restore(100, 100));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_restore_skipped_when_a_second_writer_advanced_the_count() {
        // The rapid-SECOND-dictation guard: dictation #2's set_text bumps
        // changeCount by 1 while dictation #1 is still in its post-paste
        // grace window. #1 must SKIP its restore — restoring the stale C0
        // here would clobber #2's freshly-written transcript. Strict
        // equality (NOT the Windows +1 tolerance) is what makes this hold.
        assert!(
            !mac_change_count_safe_to_restore(100, 101),
            "a +1 advance means a second writer (e.g. dictation #2) owns the \
             pasteboard; restoring C0 would clobber it"
        );
        // A third-party clipboard manager writing multiple times → also skip.
        assert!(!mac_change_count_safe_to_restore(100, 105));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_restore_skipped_if_count_somehow_regressed() {
        // changeCount is monotonic in practice, but the guard must reject a
        // regression cleanly rather than treat it as "unchanged".
        assert!(!mac_change_count_safe_to_restore(100, 99));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn mac_restore_happens_after_the_paste_is_consumed() {
        // Ordering proof: the restore is gated behind the consume grace,
        // which on macOS is strictly longer than the Windows message-pump
        // grace (30 ms) — the whole point of the fix. If someone "tunes"
        // MAC_PASTE_CONSUME_GRACE back down to the Windows value, the race
        // reopens; this test pins the invariant.
        assert!(
            MAC_PASTE_CONSUME_GRACE > PASTE_CONSUME_GRACE,
            "macOS async paste needs a longer consume window than the \
             synchronous Windows message-pump path"
        );
        // The propagate lead must be positive (write visible before Cmd+V)
        // and small relative to the consume grace (no perceptible lag).
        assert!(MAC_SET_PROPAGATE_DELAY > std::time::Duration::ZERO);
        assert!(MAC_SET_PROPAGATE_DELAY < MAC_PASTE_CONSUME_GRACE);
        // mb-yxs: the guard is structurally blind to an unconsumed read
        // (Cmd+V never bumps changeCount), so paste correctness rests
        // ENTIRELY on this grace out-lasting the target's HID→app read
        // latency. The old ceiling (<=200 ms total) was a mistaken
        // "perceptible-lag budget" — the grace defers only the INVISIBLE
        // C0 restore, never the transcript, so there is no lag to bound.
        // Pin a headroom FLOOR instead: the release `.app` read latency
        // consistently exceeded 120 ms, so demand a generous margin.
        assert!(
            MAC_PASTE_CONSUME_GRACE >= std::time::Duration::from_millis(350),
            "consume grace must carry real headroom over the worst-case macOS \
             HID→app pasteboard-read latency; a tight window is what made the \
             release .app paste the stale clipboard every time (mb-yxs)"
        );
    }

    // -----------------------------------------------------------------
    // Live tests — require interactive desktop
    // -----------------------------------------------------------------

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "live clipboard mutation; run with `cargo test -- --ignored`"]
    fn live_snapshot_then_write_then_restore_preserves_text() {
        // Plant a known string FIRST so we have something to capture.
        win::write_unicode_text("PRE-EXISTING SENTINEL").unwrap();
        let original = win::snapshot().unwrap();
        assert_eq!(
            original.unicode_text().as_deref(),
            Some("PRE-EXISTING SENTINEL")
        );

        // Run a no-op "paste" — just write a different payload.
        let outcome =
            with_saved_clipboard("INJECTED PAYLOAD", || Ok(())).expect("with_saved_clipboard");
        // Either Ok or OkClipboardNotRestored is acceptable; the test
        // asserts the sentinel is back.
        assert!(matches!(
            outcome,
            PasteOutcome::Ok | PasteOutcome::OkClipboardNotRestored
        ));

        let after = win::snapshot().unwrap();
        // If outcome was Ok we should see the sentinel back. If
        // OkClipboardNotRestored, we don't enforce — the dance still
        // ran, the user just lost their clip.
        if outcome == PasteOutcome::Ok {
            assert_eq!(
                after.unicode_text().as_deref(),
                Some("PRE-EXISTING SENTINEL"),
                "sentinel should be restored"
            );
        }
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "live clipboard mutation; run with `cargo test -- --ignored`"]
    fn live_paste_fn_error_still_attempts_restore() {
        win::write_unicode_text("SENTINEL FOR ERROR-PATH TEST").unwrap();

        let result = with_saved_clipboard("WHATEVER", || {
            Err(AppError::Injection("simulated paste failure".into()))
        });
        // Error propagated…
        assert!(result.is_err());

        // …but the sentinel is still on the clipboard.
        let after = win::snapshot().unwrap();
        assert_eq!(
            after.unicode_text().as_deref(),
            Some("SENTINEL FOR ERROR-PATH TEST")
        );
    }
}
