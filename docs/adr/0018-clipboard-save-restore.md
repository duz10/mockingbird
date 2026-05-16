# ADR-0018: Clipboard save/restore protocol

- **Status:** Accepted
- **Date:** 2026-05-17
- **Deciders:** Dustin (project lead), code-puppy (implementor), planning-agent

## Context

The default Layer-3 injection strategy (ADR 0016) is "put the cleaned
transcript on the clipboard, send Ctrl+V". PLAN §12 #17 is binding:
**clipboard save/restore wraps every paste.** The user's pre-existing
clipboard contents must be intact after Mockingbird is done.

Naïve implementations of this dance ("save text, paste, restore text")
fail in three predictable ways:

1. **Format loss.** User copied an image, an Excel range, or a Word
   selection with rich formatting. Saving only `CF_UNICODETEXT` and
   restoring it would destroy the image / formatting.
2. **Ownership race.** Another app writes the clipboard between our
   paste and our restore (especially under clipboard-history /
   sync-tool environments like Win+V or Ditto). Restoring blindly
   would clobber the user's most recent intentional copy.
3. **Lock contention.** A clipboard-manager (Ditto, ClipboardFusion)
   holds `OpenClipboard` briefly. Our `OpenClipboard(NULL)` returns
   `false` and a naïve impl drops the entire injection.

Additionally, the clipboard API is fiddly: `SetClipboardData` takes
ownership of the `HGLOBAL`; restoring requires re-allocating and
re-handing-off; `EnumClipboardFormats` walks formats in a non-obvious
order; large data must use handle-based formats.

## Decision

The protocol is implemented exactly once, in
`src-tauri/src/injection/paste.rs`. The hook `block-bare-paste` (Phase
0) statically forbids calls to `SetClipboardData` outside that file.

### Four-step dance

```text
1. SNAPSHOT
   - OpenClipboard(NULL)  [3 retries × 10 ms backoff on failure]
   - GetClipboardSequenceNumber()  → seq_before
   - For each format reported by EnumClipboardFormats():
       - GetClipboardData(format) → HANDLE
       - Capture (format_id, format_name_if_registered, handle_bytes)
   - CloseClipboard()

2. WRITE PAYLOAD
   - OpenClipboard(NULL)
   - EmptyClipboard()
   - SetClipboardData(CF_UNICODETEXT, payload_hglobal)
   - CloseClipboard()

3. PASTE
   - SendInput Ctrl+V
   - Poll GetClipboardSequenceNumber() at 5 ms intervals:
       - If it advances past (seq_before + 1) → paste consumed.
         Break.
       - If 250 ms elapses → timeout. Log warning. Continue to
         restore anyway.

4. RESTORE
   - GetClipboardSequenceNumber() → seq_after_paste
   - If seq_after_paste != (seq_before + 1) AND
        seq_after_paste != (seq_before + 2):
       # Someone else wrote the clipboard during our paste — they
       # win. Skip restore. Emit tray toast "Clipboard changed
       # during dictation — not restored".
   - Else:
       - OpenClipboard(NULL)
       - EmptyClipboard()
       - For each captured (format_id, bytes) from step 1:
           - Re-allocate HGLOBAL, copy bytes, SetClipboardData
       - CloseClipboard()
```

### Snapshot scope

We snapshot **every format the system enumerates** (via
`EnumClipboardFormats`), not just a fixed list. This handles:

- `CF_UNICODETEXT`, `CF_TEXT`, `CF_OEMTEXT`
- `CF_BITMAP`, `CF_DIB`, `CF_DIBV5`
- `CF_HDROP` (file lists)
- Registered formats: `"HTML Format"`, `"Rich Text Format"`,
  `"image/png"`, `"Star Object Descriptor"`, etc.

Large blobs (snapshot bytes >4 MB) are still snapshotted by copying
the underlying memory — Phase 3 does not optimize this. A future ADR
may switch to handle-reference for huge clips if a real workload
shows it matters.

### Failure handling

| Failure                                | Behaviour                                                                                                                                                                  |
|----------------------------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `OpenClipboard` fails 3× in row        | Return `AppError::Injection("clipboard locked")`; orchestrator records `injection_status = "failed_clipboard_locked"`; raw transcript still persisted; tray toast.        |
| Sequence number wrong on restore       | Skip restore; tray toast; log; `injection_status = "ok_clipboard_not_restored"`. **Better to lose the user's pre-existing clip than overwrite a newer intentional copy.** |
| `SetClipboardData` returns NULL during restore | Log per-format failure; continue with remaining formats. Don't abort the whole restore on one bad format.                                                          |
| Paste sentinel never advances (250 ms) | Log warning; treat as "paste possibly didn't take" but proceed to restore anyway. The orchestrator may still report `injection_status = "ok"` — we can't tell from here.   |

### Win+V clipboard history

Unaffected. Win+V observes `SetClipboardData` calls through the
standard API. Our payload + the restored content both appear in
history. This is a known UX wrinkle; documented in CONTRIBUTING for
the privacy-paranoid user (they can disable Win+V system-wide, or set
Mockingbird to Keystroke strategy globally).

### Where save/restore lives in the code

```text
src-tauri/src/injection/paste.rs
  pub fn with_saved_clipboard<F>(payload: &str, paste_fn: F) -> AppResult<PasteOutcome>
  where F: FnOnce() -> AppResult<()>
```

The `paste_fn` callback is the SendInput Ctrl+V call. By inverting
control here, `paste.rs` owns the dance and the callers cannot
forget a step.

## Consequences

- **Positive:**
  - Total-format fidelity on restore (images, RTF, file lists).
  - Race-aware: if someone else writes the clipboard mid-paste, we
    yield to them.
  - Single point of clipboard mutation in the codebase — hook
    `block-bare-paste` enforces.
- **Negative:**
  - 100+ lines of Win32 boilerplate. Mitigated by isolating in one
    file with tight scope.
  - Snapshot+restore takes ~5–20 ms depending on clipboard contents.
    Imperceptible in dictation flow (Whisper transcription already
    dominates).
  - Large clipboard contents (a 50 MB screenshot) cause a 50 MB
    memory copy. Acceptable in v1; ADR-worthy if real users hit it.
- **Neutral:**
  - We don't preserve clipboard *owner* — the new owner of the
    pre-existing data is Mockingbird, not whatever app originally
    owned it. That's invisible to most apps; some clipboard managers
    show "(unknown)" in their history.

## Alternatives considered

- **`arboard` crate.** Wraps cross-platform clipboard; handles
  text only or text+image; does not preserve unknown formats. Loses
  the user's RTF / HDROP / HTML. Rejected as the substrate for the
  save half. (Possible future use for the simple set-text half.)
- **OLE `IDataObject`.** Richer model; required for snapshotting
  delay-rendered formats. Real apps rarely use delay-rendered
  formats for content the user actively interacts with. Deferred.
- **Don't restore at all.** PLAN §12 #17 binding — rejected.
- **Always-Keystroke fallback for "complex" clipboards.** Forces the
  user to wait 5+ seconds for a long paragraph to type. Rejected as
  default; available as user override.
- **Use Win+V to "undo" the paste.** Race-prone and unsupported
  programmatically. Rejected.

## Cross-references

- PLAN §3 (Layer-3 injection), §12 #17 (binding: clipboard
  save/restore)
- ADR 0007 (Tier-0 paste default — this ADR is the substrate)
- ADR 0016 (injection strategy — Paste branch calls this protocol)
- ADR 0017 (secure-input guard — runs **before** any of this; if
  triggered, none of this code runs)
- `docs/phases/phase3.md` Wave 4 (`injection/paste.rs`), Wave 5
  judge `clipboard-restored`
- Microsoft docs:
  <https://learn.microsoft.com/windows/win32/dataxchg/clipboard-formats>
  <https://learn.microsoft.com/windows/win32/api/winuser/nf-winuser-getclipboardsequencenumber>
