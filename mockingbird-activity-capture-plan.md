# Activity Capture & Session Summary — Feature Plan

*A comprehensive, implementation-agnostic plan for a local-first activity-recording feature.*

---

## 1. Purpose & Vision

The feature lets a user start a recording session, work normally on their computer, and end the session with a **chronological, human-readable summary of what they did** — accurate enough to review later or send as a work report. It is the digital equivalent of a colleague quietly taking notes over your shoulder, then handing you a clean timeline at the end of the day.

The central design commitment is this: **the deliverable is a structured event log, not a video.** Screen recording is one possible *source* of that log, and an expensive, lossy one. A local-first architecture lets the feature read the operating system's accessibility layer directly, producing a richer log at a fraction of the cost. Video is therefore demoted to an optional fallback, never the primary mechanism.

### What "done" looks like

A user clicks **Start Session**, writes an email, edits a spreadsheet, joins a call, and clicks **Stop**. Within seconds they see:

> **9:14–9:38 — Email** · Drafted and sent a reply to a client regarding the Q3 invoice.
> **9:38–10:05 — Spreadsheet** · Updated the budget projections tab; added three line items.
> **10:05–10:32 — Video call** · Discussed timeline with the design team. *(Includes transcript.)*

…and they can edit, export, or share that summary.

---

## 2. Goals & Non-Goals

### Goals

- **Capture a faithful record** of application use, document context, and on-screen activity with timestamps.
- **Capture spoken context** via the microphone, transcribed locally.
- **Produce a chronological summary** that abstracts raw events into meaningful activity blocks.
- **Run entirely on-device** — no network dependency for any core function.
- **Be production-ready**: stable over multi-hour sessions, low overhead, resilient to crashes, respectful of user control.
- **Be exportable** — the summary can leave the app as Markdown, PDF, or plain text.

### Non-Goals (explicitly out of scope for v1)

- Continuous 24/7 background capture (this is *session-scoped* recording).
- Cloud sync, multi-device timelines, or team dashboards.
- Surveillance or employee-monitoring use cases — this is a personal tool the user runs on themselves.
- Pixel-perfect screen replay or video scrubbing.
- Real-time live summarization during the session (summary is generated at session end; live can come later).
- Mobile or browser-extension capture.

---

## 3. The Local-First Advantage

The architecture choice is not a privacy footnote — it changes what is technically and ethically possible.

Cloud-based activity tools must *minimize* what they capture, because every captured byte is a storage cost, a bandwidth cost, and a liability. That is why the entire user-activity-monitoring industry treats live screen capture as the invasive last resort.

A local-only design **inverts that calculus**:

| Dimension | Cloud approach | Local-first approach |
|---|---|---|
| Capture richness | Minimized to reduce liability | Can be deep — full window context, field-level detail |
| Sensitive data | Must be redacted before transmission | Never leaves the device; redaction optional |
| Bandwidth | Constant constraint | Irrelevant |
| User consent | Broad, anxious, legalistic | Specific, calm, honest |
| Latency | Round-trip to server | Limited only by local hardware |
| Failure mode | Network outage = data loss | Works on a plane |

The practical consequence: the feature can request broad accessibility permissions with a clean conscience and a clean consent dialog, because the honest claim — *"this data physically cannot leave your machine"* — is true.

---

## 4. Capture Architecture — Three Layers

The system is built as three independent capture layers feeding one merge-and-summarize pipeline. Each layer can fail or be disabled without breaking the others.

### Layer 1 — Activity Events (the primary signal)

This is the heart of the feature and the part that replaces video.

A background process samples the operating system's **accessibility layer** — the same APIs screen readers use — on a polling interval *and* on event triggers (window focus changes, application switches). From this it derives structured, timestamped events:

- **Active application** — which program is in the foreground.
- **Active window / document title** — e.g. the filename, the email subject, the browser tab.
- **Accessibility snapshot** — the text content and UI structure of the foreground window: which field is focused, visible button and menu labels, headings, and visible text.
- **State transitions** — app switches, window changes, idle ↔ active transitions.
- **Optional input signals** — coarse keyboard/mouse *activity level* (not keystroke content) to distinguish active work from idle time.

Each of these is one timestamped JSON row. This is lightweight, structured, and dramatically more useful to a language model than a screenshot, because it is already *text*.

**Per-platform reality:** the accessibility layer is different on each OS, and this is the single largest engineering cost of the feature.

| OS | API | Notes |
|---|---|---|
| macOS | Accessibility API (AX) | Requires the user to grant Accessibility permission in System Settings. |
| Windows | UI Automation (UIA) | Generally available without special elevation. |
| Linux | AT-SPI | Quality varies by toolkit (GTK/Qt good, others weaker). |

A clean internal abstraction — *"give me the current foreground context"* — should sit on top of all three so the rest of the system is platform-agnostic.

### Layer 2 — Audio (the narration track)

The microphone is captured for the session's duration and transcribed locally with the existing speech-to-text integration. Output is a set of **timestamped transcript segments** that interleave with Layer 1 events on the same clock.

Audio is what turns "edited a document" into "edited the document *while talking through the reasoning with a colleague*." It is optional per-session — some users will want a silent activity log only.

Considerations:
- Voice-activity detection so silence does not generate empty transcript noise.
- Optional speaker hinting (at minimum, distinguishing the user from others on a call).
- Clear, persistent indication that the mic is live.

### Layer 3 — Screenshot Fallback (optional, deferred)

Some applications expose almost nothing through accessibility — canvas-based design tools, certain games, some poorly-built Electron apps. For these, an *optional* periodic screenshot can be captured and run through local OCR to recover visible text.

This is deliberately a **fallback, not a default**. It is more expensive (CPU, disk), more privacy-sensitive, and lower-quality than the accessibility tree. It should be off by default, enabled per-app or per-session by the user, and never the system's first choice.

> **Design rule:** if an app yields good accessibility data, never screenshot it.

---

## 5. The Summarization Pipeline

At session end, the three layers have produced a merged, time-ordered stream of events and transcript segments. Turning that into a readable summary is a multi-stage process — not a single model call.

### Stage 1 — Merge & normalize
Combine all layers onto one timeline. Deduplicate redundant snapshots (ten identical "still in Gmail" events become one span). Resolve overlaps. Output: a clean, ordered event stream.

### Stage 2 — Segment into activity blocks
Group contiguous events into coherent **blocks** — periods of doing one thing. A block has a start time, end time, primary application, and a context bundle (titles, key text fragments, relevant transcript). Segmentation is driven by app switches, document changes, and idle gaps.

### Stage 3 — Abstract each block
The local language model receives one block's context bundle and produces a one- or two-sentence human description: *"Drafted a reply to a client about the Q3 invoice."* The model's job here is **abstraction and naming**, not transcription — it is told what happened and asked to phrase it well.

### Stage 4 — Assemble the session summary
Blocks are assembled into the final chronological document: a timeline, optional totals (time per application, per project), and optional highlights. This is what the user sees and exports.

> **Why staged, not one prompt:** a single giant prompt over hours of raw events produces vague, hallucination-prone output and is hard to debug. Staging keeps each model call small, scoped, and inspectable — and lets you fix segmentation independently of phrasing.

### Model strategy
All inference runs through the on-device model runtime. Practical notes:
- Small, fast local models are sufficient for Stage 3 — it is a constrained rewriting task.
- Long sessions must be processed in chunks; never assume the whole day fits in one context window.
- The pipeline must degrade gracefully: if the model is slow or unavailable, the user should still get the **raw structured timeline** (app names, titles, times) without AI prose. The AI layer is an enhancement, not a dependency.

---

## 6. Data Model (conceptual)

Storage is local, structured, and queryable — a local database, not loose files.

- **Session** — id, label, start time, end time, status, layer-enabled flags, summary reference.
- **Event** — id, session id, timestamp, type (`app_switch`, `context_snapshot`, `idle_start`, `idle_end`, …), application, window title, structured context payload.
- **TranscriptSegment** — id, session id, start time, end time, text, speaker hint, confidence.
- **Screenshot** *(if Layer 3 used)* — id, session id, timestamp, image reference, OCR text.
- **Block** — id, session id, start, end, primary app, generated description, source event ids.
- **SessionSummary** — id, session id, generated timeline, totals, model/version metadata, user-edited flag.

Design principles:
- **Provenance preserved.** Keep raw events even after summarization, so the user can drill down and so summaries can be regenerated with a better model later.
- **Editable derived layer.** Blocks and summaries are user-editable; raw events are immutable.
- **Schema versioned** from day one — this data is meant to persist and outlive early versions of the app.

---

## 7. User Experience

### Lifecycle controls
- **Start / Stop / Pause** a session. Pause is essential — users will step away or do something private.
- A clear, **always-visible recording indicator** whenever any layer is active. Never capture silently.
- Per-session toggles before starting: audio on/off, screenshot-fallback on/off.

### Reviewing a session
- The generated **chronological timeline** as the primary view.
- **Drill-down** from any block to its underlying events and transcript.
- **Inline editing** — rename, merge, split, delete, or rewrite blocks. User corrections are valuable and should be easy.
- Optional **totals view** — time per app, per project.

### Exporting & sharing
- Export the summary as Markdown, PDF, or plain text.
- A "work report" mode that strips internal detail and produces a clean client- or manager-facing summary.
- Export is **explicit and user-initiated** — nothing is ever sent anywhere automatically.

### Trust & control surfaces
- A visible, honest **privacy statement** in the UI: what is captured, where it lives, that it never leaves the device.
- An **exclusion list** — applications or window-title patterns that are never captured (password managers, banking, personal apps).
- **Automatic pause** when an excluded app comes to the foreground.
- One-click **delete** for any session, and a **delete-everything** option that genuinely wipes all capture data.

---

## 8. Privacy, Security & Trust

Even though data stays local, this feature captures genuinely sensitive material. Local is not a license to be careless.

- **Capture exclusions.** Ship a sensible default exclusion list (password managers, banking sites, system credential dialogs) and let users extend it. Honor it at *capture* time, not just display time — excluded content should never be written to disk.
- **Encryption at rest.** The local capture database should be encrypted. A lost or stolen laptop should not mean a readable activity log.
- **No silent capture.** A recording indicator is mandatory whenever any layer is live.
- **Input content boundaries.** Capture keyboard/mouse *activity level*, never keystroke content. The accessibility tree already provides field context safely; raw keylogging is both a privacy hazard and unnecessary.
- **Microphone honesty.** OS-level mic indicators plus an in-app indicator. Audio capture is opt-in per session.
- **Retention controls.** Let users set automatic deletion of sessions older than N days. Make manual deletion immediate and complete — including derived blocks and summaries.
- **Consent that matches reality.** The permission request and onboarding should make the true claim plainly: this data physically cannot leave the machine. Do not over-promise (e.g. avoid absolute claims about what a future cloud feature might do) — describe only what the current version does.
- **Supply-chain awareness.** A locally-running app with accessibility access is a high-trust component. Keep dependencies minimal and audited; this access in the wrong hands is dangerous.

---

## 9. Performance & Reliability

The feature must survive a real eight-hour workday without becoming a problem.

- **Event-driven over polling where possible.** React to focus-change events; poll only as a backstop. This is what keeps overhead low — running OCR on every frame is exactly the cost to avoid.
- **CPU budget.** Target single-digit percent CPU during steady-state capture. The accessibility-first design makes this achievable.
- **Bounded memory.** Flush events to the local database continuously; never accumulate a whole session in memory.
- **Crash resilience.** Because events are persisted as they happen, an app or system crash should lose at most the last few seconds. On restart, the app should detect an interrupted session and offer to recover and summarize it.
- **Disk management.** Activity events are tiny. Audio is moderate. Screenshots are the heavy item — another reason Layer 3 is opt-in. Surface current storage use and offer cleanup.
- **Graceful degradation.** If a layer fails (permission revoked mid-session, mic disconnected), the session continues with the remaining layers and the failure is noted in the summary rather than crashing.
- **Idle handling.** Detect idle periods and represent them as gaps, not fabricated activity.

---

## 10. Edge Cases to Design For

- **Multiple monitors** — capture context across all displays, not just the active one.
- **Apps with no accessibility data** — detect this and either fall back to screenshots (if enabled) or honestly log "activity in [app], details unavailable."
- **Very long sessions** — chunked summarization; no assumption the timeline fits one model context.
- **Very short sessions** — a two-minute session should still produce something sensible, not an error.
- **Rapid app-switching** — debounce so flicking between two windows does not generate dozens of micro-blocks.
- **Sensitive content appearing mid-session** — exclusion list plus auto-pause must catch this.
- **Permission revoked mid-session** — detect, notify, continue with remaining layers.
- **System sleep / lid close** — cleanly pause and resume; do not log the sleep gap as work.
- **Time zone / clock changes** — store timestamps in a stable absolute form so a travel-related clock change does not scramble the timeline.

---

## 11. Suggested Build Phases

A staged delivery that produces something useful early and defers the expensive, optional parts.

**Phase 1 — Activity log skeleton.**
Layer 1 capture on the primary development OS only. Active app, window title, app-switch and idle events. Session start/stop. Raw timeline view. No AI, no audio. *This alone is a usable feature.*

**Phase 2 — Summarization.**
Add the merge → segment → abstract → assemble pipeline using the local model. Editable blocks. Markdown export. The accessibility *snapshot* (not just titles) is added here to give the model real context.

**Phase 3 — Audio.**
Integrate microphone capture and local transcription as Layer 2. Interleave transcript with events. Audio-aware summaries.

**Phase 4 — Cross-platform.**
Bring Layer 1 to the remaining operating systems behind the shared "foreground context" abstraction. This is mostly per-platform accessibility work.

**Phase 5 — Hardening & polish.**
Encryption at rest, exclusion lists and auto-pause, retention controls, crash recovery, PDF export, work-report mode, storage management.

**Phase 6 (optional / future) — Screenshot fallback.**
Layer 3 for accessibility-blind apps, opt-in per app, with local OCR.

> Audio (Phase 3) can move earlier if spoken context is considered core to the product rather than an enhancement — the phases are about dependency order, not fixed priority.

---

## 12. Open Questions to Resolve Before Building

These need a project-specific answer and will shape the implementation:

1. **Session-scoped or always-on?** This plan assumes explicit start/stop sessions. An always-on background mode is a meaningfully different product with different consent and storage implications.
2. **Is audio core or optional?** This determines whether Phase 3 moves up.
3. **Which OS ships first**, and is cross-platform a v1 requirement or a fast-follow?
4. **How much accessibility detail to capture** — titles only (lighter, safer) versus full window text (richer, more sensitive)? This may itself be a user setting.
5. **Default exclusion list** — what ships excluded out of the box?
6. **Summary editing model** — are user edits purely cosmetic, or are they captured as a feedback signal to improve future segmentation?
7. **Multi-project tagging** — should a session be attributable to a project/client, and should that drive the totals and the work-report export?

---

## 13. Reference Implementations to Study

Existing tools that illuminate parts of this design (study them; do not assume their licenses or code are drop-in compatible without checking):

- **screenpipe** — open-source, MIT-licensed; the closest existing analog to this feature's full scope. Its core lesson is the **accessibility-tree-first, OCR-as-fallback** capture model and the resulting low overhead. The most valuable reference for Layer 1.
- **ActivityWatch** — open-source; the gold standard for the *lightweight* end of the spectrum: app/window/duration tracking with no OCR and no video. Closest model for Phase 1.
- **Windrecorder** — Windows-only, open-source; instructive as a **counter-example** — it relies on continuous video plus OCR, and the cost of constantly compressing and managing that video is exactly what the accessibility-first approach is designed to avoid.

---

## 14. Summary of Key Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Primary capture mechanism | OS accessibility layer | Structured, cheap, text-native; better LLM input than pixels |
| Screen video | Not used | Expensive, lossy; the goal is a log, not a replay |
| Screenshots | Optional fallback only | Only for accessibility-blind apps; opt-in |
| Audio | Separate optional layer | Adds narration context; not all sessions need it |
| Summarization | Staged pipeline, on-device | Small scoped model calls; debuggable; degrades gracefully |
| Data residency | Fully local, encrypted at rest | Enables rich capture with honest consent |
| Capture scope | Session-based (start/stop) | Bounded, intentional, simpler consent than always-on |
| AI dependency | Enhancement, not requirement | Raw timeline still delivered if the model is unavailable |

---

*This plan is intentionally implementation-agnostic. It defines what to build and why, and the constraints any implementation must satisfy, without prescribing specific libraries, file structures, or framework bindings — those decisions belong with the codebase and the team.*
