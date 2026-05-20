// Typed IPC wrappers for Phase MC (Meeting Capture).
//
// Sister module to `lib/tauri.ts` (the dictation/history IPC). Kept
// separate because:
//   1. The command surface is genuinely independent — Phase MC is a
//      *sibling* subsystem (PLAN.md §invariants #1). Mixing both
//      surfaces into one giant `api.*` object would invite cross-
//      cutting refactors that violate the sibling boundary.
//   2. `tauri.ts` already pushes 14 KB; adding 10 more commands +
//      their fixtures would push it past the 600-line cap.
//
// Same multi-context shim as `lib/tauri.ts`: real Tauri → invoke();
// otherwise → fixtures.
//
// Every command name here must match a `#[tauri::command]` in
// `src-tauri/src/commands/meetings.rs`.

import type {
  LlmPassPromptArg,
  LlmPassResult,
  MeetingDetail,
  MeetingMatch,
  MeetingSourceKind,
  MeetingSourceProbe,
  MeetingSummary,
} from "./types";
import { isTauri } from "./tauri";

/* ------------------------------------------------------------------ */
/* Generic invoke wrapper + fixture override hook.                    */
/* ------------------------------------------------------------------ */

type FixtureMap = Partial<Record<string, unknown>>;
declare global {
  interface Window {
    /** Per-command fixture overrides for the meeting IPC surface.
     *  Lets Playwright specs stage e.g. `meeting_probe_sources` to
     *  return `{ micAvailable: false, systemAvailable: true }`. */
    __MOCKINGBIRD_MEETING_FIXTURES__?: FixtureMap;
  }
}

function fixture<T>(command: string, fallback: T): T {
  const overrides =
    typeof window !== "undefined"
      ? window.__MOCKINGBIRD_MEETING_FIXTURES__
      : undefined;
  if (overrides && command in overrides) {
    return overrides[command] as T;
  }
  return fallback;
}

async function invoke<T>(command: string, args?: object): Promise<T> {
  if (isTauri()) {
    const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
    return tauriInvoke<T>(command, args as Record<string, unknown>);
  }
  return fixtureFor<T>(command, args);
}

/* ------------------------------------------------------------------ */
/* Public API surface — every command the Meetings UI calls.          */
/* ------------------------------------------------------------------ */

export const meetings = {
  /* Lifecycle */

  /** Probe what audio sources are usable on this box right now.
   *  Cheap & idempotent — safe to call on every overlay open. */
  probeSources: () => invoke<MeetingSourceProbe>("meeting_probe_sources"),

  /** Start a meeting. Idempotent: if one's already in flight, returns
   *  its uuid + emits `meeting:state=warn-already-running`. */
  start: (source: MeetingSourceKind) =>
    invoke<{ uuid: string }>("meeting_start", { source }),

  /** Stop the in-flight meeting. Drives the full transcribe→merge→
   *  persist pipeline on the IPC worker; emits `meeting:state=done`
   *  + `meetings:session-saved` on success. */
  stop: (uuid: string) => invoke<void>("meeting_stop", { uuid }),

  /* History (read) */

  list: (limit = 200, offset = 0) =>
    invoke<MeetingSummary[]>("list_meetings", { limit, offset }),

  detail: (uuid: string) => invoke<MeetingDetail>("get_meeting_detail", { uuid }),

  delete: (uuid: string) => invoke<void>("delete_meeting", { uuid }),

  search: (query: string) =>
    invoke<MeetingMatch[]>("search_meeting_transcripts", { query }),

  /* Export */

  /** Write the meeting as a markdown file. If `destPath` is omitted
   *  the Rust side picks `<appdata>/Mockingbird/meetings/exports/<uuid>.md`.
   *  `llmPassId` (when set) injects the previously-cached LLM-pass
   *  text as a trailing section. */
  exportMarkdown: (uuid: string, destPath?: string, llmPassId?: string) =>
    invoke<{ path: string }>("meeting_export_markdown", {
      uuid,
      destPath,
      includeLlmPass: llmPassId ? { id: llmPassId } : undefined,
    }),

  /** Render the meeting markdown and place it on the clipboard.
   *  One-shot `SetClipboardData` — does NOT save/restore (the
   *  user clicked Copy, they expect their clipboard to change). */
  copyToClipboard: (uuid: string, llmPassId?: string) =>
    invoke<void>("meeting_copy_to_clipboard", {
      uuid,
      includeLlmPass: llmPassId ? { id: llmPassId } : undefined,
    }),

  /** Run the optional LLM pass. Output is held in an in-memory cache
   *  keyed by the returned `id`; pass that `id` to `exportMarkdown` /
   *  `copyToClipboard` to include it. NOT persisted to DB —
   *  judge `mc-no-llm-in-critical-path` invariant. */
  runLlmPass: (uuid: string, promptId: LlmPassPromptArg, modelId?: string) =>
    invoke<LlmPassResult>("meeting_run_llm_pass", { uuid, promptId, modelId }),
};

/* ------------------------------------------------------------------ */
/* Fixtures — small but representative. Used by `npm run preview` +   */
/* unit tests. Edit here for design work in the browser.              */
/* ------------------------------------------------------------------ */

function fixtureFor<T>(command: string, args?: object): T {
  switch (command) {
    case "meeting_probe_sources":
      return fixture(command, MEETING_FIXTURES.probe) as T;
    case "list_meetings":
      return fixture(command, MEETING_FIXTURES.list) as T;
    case "get_meeting_detail": {
      const uuid = (args as { uuid: string } | undefined)?.uuid;
      const hit = MEETING_FIXTURES.details.find((d) => d.uuid === uuid);
      return fixture(command, hit ?? MEETING_FIXTURES.details[0]!) as T;
    }
    case "search_meeting_transcripts":
      return fixture(command, MEETING_FIXTURES.searchHits) as T;
    case "meeting_start": {
      // Fixture mode: pretend the start succeeded with a fresh uuid.
      const fakeUuid = `fixture-${Date.now().toString(16)}`;
      return fixture(command, { uuid: fakeUuid }) as T;
    }
    case "meeting_export_markdown":
      return fixture(command, {
        path:
          "C:\\Users\\you\\AppData\\Roaming\\Mockingbird\\meetings\\exports\\fixture.md",
      }) as T;
    case "meeting_run_llm_pass":
      return fixture(command, {
        id: "fixture-llm-pass-id",
        text:
          "**Summary** — this is a fixture LLM-pass output, used for browser preview only.",
        latencyMs: 1840,
      }) as T;
    // Void commands — no fixture payload.
    case "meeting_stop":
    case "delete_meeting":
    case "meeting_copy_to_clipboard":
      return fixture(command, undefined as unknown as T);
    default:
      throw new Error(`meetings.fixtureFor: no fixture for command "${command}"`);
  }
}

/** Public so tests can import + mutate via the override hook. */
export const MEETING_FIXTURES: {
  probe: MeetingSourceProbe;
  list: MeetingSummary[];
  details: MeetingDetail[];
  searchHits: MeetingMatch[];
} = {
  probe: { micAvailable: true, systemAvailable: true },
  list: [
    {
      uuid: "fixture-meeting-001",
      title: "Q2 launch planning",
      startedAt: new Date(Date.now() - 1000 * 60 * 47).toISOString(),
      totalDurationMs: 28 * 60 * 1000 + 14_000,
      status: "complete",
      source: "both",
    },
    {
      uuid: "fixture-meeting-002",
      title: null,
      startedAt: new Date(Date.now() - 1000 * 60 * 60 * 4).toISOString(),
      totalDurationMs: 9 * 60 * 1000 + 32_000,
      status: "complete",
      source: "mic",
    },
    {
      uuid: "fixture-meeting-003",
      title: "Podcast clip",
      startedAt: new Date(Date.now() - 1000 * 60 * 60 * 22).toISOString(),
      totalDurationMs: 3 * 60 * 1000 + 6_000,
      status: "complete",
      source: "system",
    },
    {
      uuid: "fixture-meeting-004",
      title: "Mid-recording crash",
      startedAt: new Date(Date.now() - 1000 * 60 * 60 * 48).toISOString(),
      totalDurationMs: 14_000,
      status: "interrupted",
      source: "mic",
    },
  ],
  details: [
    {
      uuid: "fixture-meeting-001",
      title: "Q2 launch planning",
      startedAt: new Date(Date.now() - 1000 * 60 * 47).toISOString(),
      endedAt: new Date(Date.now() - 1000 * 60 * 19).toISOString(),
      status: "complete",
      errorMessage: null,
      source: "both",
      totalDurationMs: 28 * 60 * 1000 + 14_000,
      micDurationMs: 28 * 60 * 1000 + 14_000,
      sysDurationMs: 28 * 60 * 1000 + 14_000,
      formatterVersion: "mc.fmt.v1",
      whisperModelId: "whisper-medium-en",
      formattedMic:
        "[You] Okay, kicking off the Q2 launch planning.\n\n[You] First item: the cleanup pipeline status update.\n\n[You] Second item: learning loop nightly run results.",
      formattedSys:
        "[Other(s)] Thanks. On the cleanup pipeline, we're at iteration three of the prompt tuning.\n\n[Other(s)] Learning loop committed six runs in a row.",
      formattedMerged:
        "[You] Okay, kicking off the Q2 launch planning.\n\n[Other(s)] Thanks. On the cleanup pipeline, we're at iteration three of the prompt tuning.\n\n[You] First item: the cleanup pipeline status update.\n\n[Other(s)] Learning loop committed six runs in a row.\n\n[You] Second item: learning loop nightly run results.",
    },
    {
      uuid: "fixture-meeting-002",
      title: null,
      startedAt: new Date(Date.now() - 1000 * 60 * 60 * 4).toISOString(),
      endedAt: new Date(
        Date.now() - 1000 * 60 * 60 * 4 + 9 * 60 * 1000 + 32_000,
      ).toISOString(),
      status: "complete",
      errorMessage: null,
      source: "mic",
      totalDurationMs: 9 * 60 * 1000 + 32_000,
      micDurationMs: 9 * 60 * 1000 + 32_000,
      sysDurationMs: null,
      formatterVersion: "mc.fmt.v1",
      whisperModelId: "whisper-medium-en",
      formattedMic:
        "[You] Quick voice memo: the bug report from this morning was actually a duplicate of the one from last week.\n\n[You] Closing it as duplicate, no action needed.",
      formattedSys: null,
      formattedMerged: null,
    },
    {
      uuid: "fixture-meeting-003",
      title: "Podcast clip",
      startedAt: new Date(Date.now() - 1000 * 60 * 60 * 22).toISOString(),
      endedAt: new Date(
        Date.now() - 1000 * 60 * 60 * 22 + 3 * 60 * 1000 + 6_000,
      ).toISOString(),
      status: "complete",
      errorMessage: null,
      source: "system",
      totalDurationMs: 3 * 60 * 1000 + 6_000,
      micDurationMs: null,
      sysDurationMs: 3 * 60 * 1000 + 6_000,
      formatterVersion: "mc.fmt.v1",
      whisperModelId: "whisper-medium-en",
      formattedMic: null,
      formattedSys:
        "[Other(s)] Welcome back to the show, today we're talking about local-first software.\n\n[Other(s)] The idea has been around since the 80s but only recently became practical.",
      formattedMerged: null,
    },
    {
      uuid: "fixture-meeting-004",
      title: "Mid-recording crash",
      startedAt: new Date(Date.now() - 1000 * 60 * 60 * 48).toISOString(),
      endedAt: new Date(Date.now() - 1000 * 60 * 60 * 48 + 14_000).toISOString(),
      status: "interrupted",
      errorMessage: "process exited mid-recording (drop-finalizer ran)",
      source: "mic",
      totalDurationMs: 14_000,
      micDurationMs: 14_000,
      sysDurationMs: null,
      formatterVersion: "mc.fmt.v1",
      whisperModelId: "whisper-medium-en",
      formattedMic: "[You] Quick note before I head into the meet—",
      formattedSys: null,
      formattedMerged: null,
    },
  ],
  searchHits: [],
};
