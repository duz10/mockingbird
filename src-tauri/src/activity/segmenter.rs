//! Stage 1 of the Wave-3 summarization pipeline: **merge + normalize**.
//!
//! Takes the raw [`ActivityEventRow`] stream as it comes out of the
//! repo and produces a [`NormalizedEvent`] stream that the
//! [`super::blocker`] stage can group cleanly.
//!
//! ## What "normalize" means here
//!
//! 1. **Time-order.** Repo returns events `ORDER BY ts ASC, id ASC`
//!    already, but we don't trust that contract here — the assembler
//!    runs at any time, including against hand-fixtured DBs, and
//!    asserting it inside the pipeline is cheap.
//! 2. **Dedupe consecutive `app_switch` rows.** The sampler emits an
//!    `app_switch` only when `(app, title)` changes, but title-only
//!    changes within the same app (`"Inbox (12)"` → `"Inbox (13)"`)
//!    do generate a new row. The blocker wants the *first* such
//!    title for the run; we keep that one and drop the rest.
//! 3. **Pair `idle_start` with `idle_end`.** Raw rows are two flat
//!    events; the blocker needs the duration to decide if the gap
//!    is Block-breaking (≥ 60 s by default). We coalesce into
//!    [`NormalizedEvent::IdleSpan`] with the gap baked in.
//! 4. **Drop control events.** `paused` / `resumed` are FSM
//!    bookkeeping — they don't contribute to the user-facing
//!    timeline. The blocker doesn't care about them.
//! 5. **Drop `layer_error` rows.** These are sampler health
//!    telemetry, not user activity. The session detail UI still
//!    shows them; the summary doesn't.
//!
//! ## Why pure-Rust
//!
//! This stage touches no platform APIs, no DB, no LLM. It's a fold
//! over a slice. That makes it throwaway-testable per LESSONS P2 —
//! no `whisper-rs` / `ort` / `cuda` in the dependency closure.

#![allow(missing_docs)]

use super::persist::ActivityEventRow;

/// A normalized event in the Wave-3 pipeline. One per
/// user-meaningful moment; the blocker consumes a slice of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalizedEvent {
    /// User focused (or stayed in) a window. Carries the snapshot
    /// payload verbatim so the abstractor can mine it for context.
    AppFocus {
        /// Source row id (for `source_event_ids` provenance).
        event_id: String,
        /// `chrome.exe`, `code.exe`, etc.
        app: String,
        /// The window title at the start of this run.
        title: String,
        /// Unix epoch ms.
        ts: i64,
        /// Free-form payload (v2 UIA JSON in Wave 2+; None in pre-W2 rows).
        snapshot_json: Option<String>,
    },
    /// A coalesced idle span. The boundaries below let the blocker
    /// compare against its idle-threshold without re-deriving the
    /// duration. Both `start_ts` and `end_ts` are present even if
    /// they came from separate rows.
    IdleSpan {
        start_event_id: String,
        end_event_id: Option<String>,
        start_ts: i64,
        /// `None` if the session ended while idle. The blocker
        /// treats an open-ended idle as "Block boundary at start_ts".
        end_ts: Option<i64>,
    },
}

impl NormalizedEvent {
    /// Convenience: the timestamp the blocker uses to order events.
    pub fn ts(&self) -> i64 {
        match self {
            Self::AppFocus { ts, .. } => *ts,
            Self::IdleSpan { start_ts, .. } => *start_ts,
        }
    }

    /// Source event ids for `activity_blocks.source_event_ids`
    /// provenance. An `IdleSpan` contributes its start (and end, if
    /// known) row ids.
    pub fn source_event_ids(&self) -> Vec<&str> {
        match self {
            Self::AppFocus { event_id, .. } => vec![event_id.as_str()],
            Self::IdleSpan {
                start_event_id,
                end_event_id,
                ..
            } => {
                let mut v = vec![start_event_id.as_str()];
                if let Some(e) = end_event_id {
                    v.push(e.as_str());
                }
                v
            }
        }
    }
}

/// Stage 1 entry point. Owns its output (no borrow against the input
/// slice), so the blocker can drive it without lifetime juggling.
///
/// The input slice MUST be time-ordered (`ts ASC, id ASC`) — the repo's
/// `get_session_detail` guarantees this. We assert defensively in
/// debug builds.
pub fn normalize(events: &[ActivityEventRow]) -> Vec<NormalizedEvent> {
    debug_assert!(
        events.windows(2).all(|w| w[0].ts <= w[1].ts),
        "segmenter::normalize requires time-ordered input"
    );

    let mut out: Vec<NormalizedEvent> = Vec::with_capacity(events.len());
    let mut open_idle: Option<(String, i64)> = None; // (start_event_id, start_ts)
    let mut last_focus_app: Option<String> = None;

    for ev in events {
        match ev.kind.as_str() {
            "idle_start" => {
                // If a previous idle was never closed (shouldn't happen
                // but defensive), flush it as open-ended.
                if let Some((start_id, start_ts)) = open_idle.take() {
                    out.push(NormalizedEvent::IdleSpan {
                        start_event_id: start_id,
                        end_event_id: None,
                        start_ts,
                        end_ts: None,
                    });
                }
                open_idle = Some((ev.id.clone(), ev.ts));
                // An idle gap is a hard reset for the same-app dedupe.
                // A focus event that arrives AFTER an idle deserves to
                // be its own NormalizedEvent so the blocker can decide
                // whether to break the Block (per the idle-duration
                // heuristic).
                last_focus_app = None;
            }
            "idle_end" => {
                if let Some((start_id, start_ts)) = open_idle.take() {
                    out.push(NormalizedEvent::IdleSpan {
                        start_event_id: start_id,
                        end_event_id: Some(ev.id.clone()),
                        start_ts,
                        end_ts: Some(ev.ts),
                    });
                }
                // Lone idle_end with no matching start — skip; nothing
                // sensible to do.
            }
            "app_switch" | "context_snapshot" => {
                // Dedupe: if this row's app matches the previous AppFocus
                // and that previous focus is the most recent emitted item,
                // skip. Title-only changes within the same app get folded;
                // the blocker's title-Levenshtein heuristic will re-break
                // them when they're large enough.
                //
                // We special-case `context_snapshot` here: it always
                // follows an `app_switch` for the same key, but the
                // sampler emits both because Wave 1B sampled titles only.
                // For Wave 3 we treat the *first* of the pair as the
                // focus event (carries the snapshot JSON) and drop the
                // app_switch row if it's the duplicate.
                //
                // Strategy: if the previous emitted item is an AppFocus
                // with the same app+ts (within 50ms), upgrade it in place
                // with the snapshot JSON when this row has one; otherwise
                // emit a fresh AppFocus.
                let app = ev.app_name.clone().unwrap_or_default();
                let title = ev.window_title.clone().unwrap_or_default();
                if app.is_empty() && title.is_empty() {
                    // Neither field — degenerate row, skip.
                    continue;
                }

                // Same-app, same-ts (or within 50 ms) → upgrade the last AppFocus.
                if let Some(NormalizedEvent::AppFocus {
                    app: prev_app,
                    ts: prev_ts,
                    snapshot_json: prev_snap,
                    ..
                }) = out.last_mut()
                {
                    let same_app = prev_app == &app;
                    let close_in_time = (ev.ts - *prev_ts).abs() <= 50;
                    if same_app && close_in_time {
                        // Pull a richer payload onto the existing focus.
                        if prev_snap.is_none() && ev.snapshot_json.is_some() {
                            *prev_snap = ev.snapshot_json.clone();
                        }
                        // Note the app for the next dedupe pass.
                        last_focus_app = Some(app);
                        continue;
                    }
                }

                // Same app as the most-recent focus run, just a different
                // title? Keep the FIRST title for the run; the LLM only
                // sees one (Block summaries are 1 sentence, not a
                // title-by-title narration).
                if last_focus_app.as_deref() == Some(app.as_str()) {
                    // Within the same focus run — fold this row's
                    // provenance into the last focus.
                    if let Some(NormalizedEvent::AppFocus { snapshot_json, .. }) = out.last_mut() {
                        if snapshot_json.is_none() && ev.snapshot_json.is_some() {
                            *snapshot_json = ev.snapshot_json.clone();
                        }
                    }
                    continue;
                }

                out.push(NormalizedEvent::AppFocus {
                    event_id: ev.id.clone(),
                    app: app.clone(),
                    title,
                    ts: ev.ts,
                    snapshot_json: ev.snapshot_json.clone(),
                });
                last_focus_app = Some(app);
            }
            // Drop control + telemetry rows: not user-facing activity.
            "paused" | "resumed" | "layer_error" => {}
            // Unknown kinds (future event types): tolerate; just don't
            // surface them in the pipeline. The drill-down UI still
            // shows them.
            _ => {}
        }
    }

    // Flush any dangling open idle as open-ended.
    if let Some((start_id, start_ts)) = open_idle.take() {
        out.push(NormalizedEvent::IdleSpan {
            start_event_id: start_id,
            end_event_id: None,
            start_ts,
            end_ts: None,
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(
        id: &str,
        ts: i64,
        kind: &str,
        app: Option<&str>,
        title: Option<&str>,
    ) -> ActivityEventRow {
        ActivityEventRow {
            id: id.into(),
            session_id: "s1".into(),
            ts,
            kind: kind.into(),
            app_name: app.map(str::to_string),
            window_title: title.map(str::to_string),
            snapshot_json: None,
            created_at: ts,
        }
    }

    fn ev_with_snap(
        id: &str,
        ts: i64,
        kind: &str,
        app: &str,
        title: &str,
        snap: &str,
    ) -> ActivityEventRow {
        let mut e = ev(id, ts, kind, Some(app), Some(title));
        e.snapshot_json = Some(snap.into());
        e
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(normalize(&[]).is_empty());
    }

    #[test]
    fn single_app_switch_becomes_one_app_focus() {
        let rows = vec![ev("e1", 100, "app_switch", Some("a.exe"), Some("T1"))];
        let out = normalize(&rows);
        assert_eq!(out.len(), 1);
        match &out[0] {
            NormalizedEvent::AppFocus { app, title, ts, .. } => {
                assert_eq!(app, "a.exe");
                assert_eq!(title, "T1");
                assert_eq!(*ts, 100);
            }
            _ => panic!("expected AppFocus"),
        }
    }

    #[test]
    fn app_switch_then_context_snapshot_same_ts_fold_into_one_with_snapshot() {
        // The sampler in Wave 2 emits BOTH for every focus change:
        // AppSwitch (no payload) immediately followed by ContextSnapshot
        // (with payload), both at the same `now_ms`.
        let rows = vec![
            ev("e1", 100, "app_switch", Some("chrome.exe"), Some("Gmail")),
            ev_with_snap(
                "e2",
                100,
                "context_snapshot",
                "chrome.exe",
                "Gmail",
                "{\"schema\":\"v2\"}",
            ),
        ];
        let out = normalize(&rows);
        assert_eq!(out.len(), 1, "should fold to one AppFocus");
        match &out[0] {
            NormalizedEvent::AppFocus { snapshot_json, .. } => {
                assert!(snapshot_json.is_some(), "snapshot should be merged in");
            }
            _ => panic!("expected AppFocus"),
        }
    }

    #[test]
    fn title_only_change_within_same_app_keeps_first_title() {
        let rows = vec![
            ev(
                "e1",
                100,
                "app_switch",
                Some("chrome.exe"),
                Some("Inbox (12)"),
            ),
            ev(
                "e2",
                200,
                "app_switch",
                Some("chrome.exe"),
                Some("Inbox (13)"),
            ),
            ev(
                "e3",
                300,
                "app_switch",
                Some("chrome.exe"),
                Some("Inbox (14)"),
            ),
        ];
        let out = normalize(&rows);
        assert_eq!(out.len(), 1, "title spam within one app dedupes");
        match &out[0] {
            NormalizedEvent::AppFocus { title, ts, .. } => {
                assert_eq!(title, "Inbox (12)", "first title is kept");
                assert_eq!(*ts, 100);
            }
            _ => panic!("expected AppFocus"),
        }
    }

    #[test]
    fn distinct_apps_each_get_their_own_app_focus() {
        let rows = vec![
            ev("e1", 100, "app_switch", Some("a.exe"), Some("T1")),
            ev("e2", 200, "app_switch", Some("b.exe"), Some("T2")),
            ev("e3", 300, "app_switch", Some("a.exe"), Some("T3")),
        ];
        let out = normalize(&rows);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn idle_start_end_coalesce_into_idle_span() {
        let rows = vec![
            ev("e1", 100, "app_switch", Some("a.exe"), Some("T")),
            ev("e2", 200, "idle_start", None, None),
            ev("e3", 80_000, "idle_end", None, None),
            ev("e4", 80_100, "app_switch", Some("a.exe"), Some("T2")),
        ];
        let out = normalize(&rows);
        assert_eq!(out.len(), 3);
        match &out[1] {
            NormalizedEvent::IdleSpan {
                start_ts,
                end_ts,
                end_event_id,
                ..
            } => {
                assert_eq!(*start_ts, 200);
                assert_eq!(*end_ts, Some(80_000));
                assert!(end_event_id.is_some());
            }
            _ => panic!("expected IdleSpan at index 1"),
        }
    }

    #[test]
    fn open_ended_idle_at_session_end_yields_idle_span_with_none_end() {
        let rows = vec![
            ev("e1", 100, "app_switch", Some("a.exe"), Some("T")),
            ev("e2", 200, "idle_start", None, None),
        ];
        let out = normalize(&rows);
        assert_eq!(out.len(), 2);
        match &out[1] {
            NormalizedEvent::IdleSpan { end_ts, .. } => assert!(end_ts.is_none()),
            _ => panic!("expected IdleSpan"),
        }
    }

    #[test]
    fn lone_idle_end_is_dropped() {
        let rows = vec![ev("e1", 200, "idle_end", None, None)];
        let out = normalize(&rows);
        assert!(out.is_empty());
    }

    #[test]
    fn paused_resumed_layer_error_are_dropped() {
        let rows = vec![
            ev("e1", 100, "app_switch", Some("a.exe"), Some("T")),
            ev("e2", 200, "paused", None, None),
            ev("e3", 300, "resumed", None, None),
            ev("e4", 400, "layer_error", None, None),
        ];
        let out = normalize(&rows);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], NormalizedEvent::AppFocus { .. }));
    }

    #[test]
    fn unknown_kind_is_tolerated() {
        let rows = vec![
            ev("e1", 100, "app_switch", Some("a.exe"), Some("T")),
            ev("e2", 200, "future_kind_we_dont_know_yet", None, None),
        ];
        let out = normalize(&rows);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn dangling_open_idle_is_flushed_when_new_idle_starts() {
        // Defensive: shouldn't happen in real data, but if a buggy
        // sampler emits two idle_start in a row, we don't want to
        // silently lose the first.
        let rows = vec![
            ev("e1", 100, "idle_start", None, None),
            ev("e2", 200, "idle_start", None, None),
            ev("e3", 300, "idle_end", None, None),
        ];
        let out = normalize(&rows);
        assert_eq!(
            out.len(),
            2,
            "first idle is flushed open-ended; second closes"
        );
        match &out[0] {
            NormalizedEvent::IdleSpan {
                start_ts, end_ts, ..
            } => {
                assert_eq!(*start_ts, 100);
                assert!(end_ts.is_none());
            }
            _ => panic!("expected open-ended IdleSpan"),
        }
        match &out[1] {
            NormalizedEvent::IdleSpan {
                start_ts, end_ts, ..
            } => {
                assert_eq!(*start_ts, 200);
                assert_eq!(*end_ts, Some(300));
            }
            _ => panic!("expected closed IdleSpan"),
        }
    }

    #[test]
    fn source_event_ids_returns_all_contributing_rows() {
        let focus = NormalizedEvent::AppFocus {
            event_id: "e1".into(),
            app: "a".into(),
            title: "T".into(),
            ts: 100,
            snapshot_json: None,
        };
        assert_eq!(focus.source_event_ids(), vec!["e1"]);

        let idle = NormalizedEvent::IdleSpan {
            start_event_id: "e2".into(),
            end_event_id: Some("e3".into()),
            start_ts: 200,
            end_ts: Some(300),
        };
        assert_eq!(idle.source_event_ids(), vec!["e2", "e3"]);

        let open_idle = NormalizedEvent::IdleSpan {
            start_event_id: "e4".into(),
            end_event_id: None,
            start_ts: 400,
            end_ts: None,
        };
        assert_eq!(open_idle.source_event_ids(), vec!["e4"]);
    }

    #[test]
    fn ts_accessor_returns_the_anchor_timestamp() {
        let f = NormalizedEvent::AppFocus {
            event_id: "e1".into(),
            app: "a".into(),
            title: "T".into(),
            ts: 100,
            snapshot_json: None,
        };
        assert_eq!(f.ts(), 100);

        let i = NormalizedEvent::IdleSpan {
            start_event_id: "e2".into(),
            end_event_id: None,
            start_ts: 200,
            end_ts: None,
        };
        assert_eq!(i.ts(), 200);
    }

    #[test]
    fn degenerate_row_with_no_app_and_no_title_is_dropped() {
        let rows = vec![ev("e1", 100, "app_switch", None, None)];
        let out = normalize(&rows);
        assert!(out.is_empty());
    }
}
