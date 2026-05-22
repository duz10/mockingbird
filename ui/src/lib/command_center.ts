// Command Center IPC types + helpers (Phase 10 Wave 1A, ADR 0037).
//
// The Rust side lives in `src-tauri/src/command_center/`. State
// transitions are pure-Rust; this file is the TS mirror of the IPC
// shapes plus a handful of thin invoke wrappers. Anything that
// changes here must change in lockstep on the Rust side — the IPC
// command shapes are the contract.

import { invoke } from "./tauri";

/** Discriminator for which recording subsystem a session belongs to. */
export type RecordingKind = "dictation" | "meeting" | "activity";

/** The four shapes the Command Center overlay can render. */
export type CcStateName = "closed" | "modePicker" | "sessionCard" | "launching";

/**
 * The serialized state shape we get back from `cc_get_state` and
 * via the `command_center:state` event. Mirrors
 * `src-tauri/src/command_center/mod.rs::CcStatePayload`.
 */
export interface CcStateSnapshot {
  state: CcStateName;
  firstRun: boolean;
  /** Present when `state === "sessionCard" | "launching"`. */
  kind?: RecordingKind;
}

/**
 * Argument shape for `cc_update_session`. `"none"` clears the
 * current-session bookkeeping (and emits a SessionEnded input on
 * the Rust side if we were displaying that kind).
 */
export type CcSessionKindArg = RecordingKind | "none";

/**
 * Open the Command Center via the tray (or wherever a programmatic
 * open makes sense). The chord hotkey path doesn't go through this —
 * it fires the same effect on the Rust side without an IPC round-trip.
 */
export const openCommandCenterFromTray = (): Promise<void> =>
  invoke("cc_open_via_tray");

/** Dismiss the Command Center. Bound to Esc + outside-click + tray re-click. */
export const dismissCommandCenter = (): Promise<void> => invoke("cc_dismiss");

/**
 * Tell the Rust orchestrator the user picked a mode tile. The Rust
 * side dispatches to the appropriate runtime and emits a state
 * update; the UI just listens.
 */
export const pickCommandCenterMode = (kind: RecordingKind): Promise<void> =>
  invoke("cc_pick_mode", { kind });

/** Stop the live recording from the SessionCard's Stop button. */
export const stopActiveCommandCenterSession = (): Promise<void> =>
  invoke("cc_stop_active_session");

/**
 * Tell the orchestrator that an external observer (the dictation /
 * meeting overlay React component) saw a state change. Used to keep
 * the SessionCard accurate when the user opens the CC mid-recording.
 */
export const updateCommandCenterSession = (
  kind: CcSessionKindArg,
): Promise<void> => invoke("cc_update_session", { kind });

/** Synchronous snapshot fetch. Used at first paint + as a fallback. */
export const getCommandCenterState = (): Promise<CcStateSnapshot> =>
  invoke<CcStateSnapshot>("cc_get_state");

/**
 * Event name the Rust side emits on every state transition. The
 * payload shape is [`CcStateSnapshot`] above.
 */
export const COMMAND_CENTER_STATE_EVENT = "command_center:state";
