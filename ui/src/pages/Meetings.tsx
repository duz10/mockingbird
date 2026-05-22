// Meetings page — history + active-recording control.
//
// Two-pane layout, mirroring History:
//   left  — record bar (source picker + start/stop) + search + list
//   right — selected meeting's detail (transcript + actions + LLM-pass)
//
// Why one page instead of separate Meetings.tsx + MeetingDetail.tsx
// files routed independently:
//   * The user shouldn't lose the live record-bar while reading a
//     transcript. The record bar is always visible at top-left so
//     start/stop is one click from anywhere on the page.
//   * History already proved the two-pane pattern; rebuilding it
//     scattered across two routed pages would violate DRY.
//
// Both `/meetings` and `/meetings/:uuid` resolve to this component.
// `:uuid` selects which detail is shown; absence falls back to the
// most-recent meeting in the list.
//
// Live updates: listens to two Tauri events:
//   * `meeting:state`      — phase changes (started / done / error / …)
//   * `meetings:session-saved` — fires after a new row commits; we
//                                refetch the list so it appears at top.
//
// File-size budget: this page is dense. We split out the
// detail-pane helper functions but keep them in this module so
// imports are minimal. 600-line cap is the hard limit.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";

import {
  EmptyState,
  PageHeader,
  Pill,
  Spinner,
} from "../components/primitives";
import { MeetingsIcon, SearchIcon } from "../design/Icon";
import { t } from "../i18n";
import { formatDuration, formatRelative, truncate } from "../lib/format";
import { meetings } from "../lib/meetings";
import { isTauri } from "../lib/tauri";
import type {
  MeetingDetail,
  MeetingMatch,
  MeetingProgressEvent,
  MeetingSourceKind,
  MeetingSourceProbe,
  MeetingStateEvent,
  MeetingSummary,
} from "../lib/types";

import {
  FRESH_LLM,
  MeetingDetailView,
  SourcePills,
  statusLabel,
  type LlmPassUiState,
} from "./MeetingDetail";
import { MeetingRecordBar } from "./MeetingRecordBar";
import styles from "./Meetings.module.css";

const PAGE_SIZE = 200;
const SEARCH_DEBOUNCE_MS = 200;

/* ------------------------------------------------------------------ */
/* Component                                                           */
/* ------------------------------------------------------------------ */

export function MeetingsPage() {
  const navigate = useNavigate();
  const { uuid: routeUuid } = useParams<{ uuid?: string }>();

  // List state
  const [summaries, setSummaries] = useState<MeetingSummary[] | null>(null);
  const [searchHits, setSearchHits] = useState<MeetingMatch[] | null>(null);
  const [query, setQuery] = useState("");

  // Selected meeting + detail
  const [selectedUuid, setSelectedUuid] = useState<string | null>(routeUuid ?? null);
  const [detail, setDetail] = useState<MeetingDetail | null>(null);

  // Record-bar state
  const [probe, setProbe] = useState<MeetingSourceProbe | null>(null);
  const [source, setSource] = useState<MeetingSourceKind>("mic");
  const [recordingUuid, setRecordingUuid] = useState<string | null>(null);
  const [recordingPhase, setRecordingPhase] = useState<string | null>(null);
  // Per-channel chunk counters — reset on each fresh start. `null`
  // total means the capture side hasn't closed the chunk sender yet
  // (more chunks may still arrive), so the UI renders "4/?" until
  // the driver's `chunks_total` flips to a concrete value.
  const [progress, setProgress] = useState<{
    mic?: { done: number; total: number | null };
    system?: { done: number; total: number | null };
  }>({});
  const [startingOrStopping, setStartingOrStopping] = useState<
    "starting" | "stopping" | null
  >(null);
  const recordStartTimeRef = useRef<number | null>(null);
  const [elapsedSec, setElapsedSec] = useState(0);

  // LLM-pass UI state — keyed per-meeting so switching meetings
  // doesn't leak prompt selection between detail views.
  const [llmByUuid, setLlmByUuid] = useState<Record<string, LlmPassUiState>>({});

  const [toast, setToast] = useState<string | null>(null);

  /* -------- initial probe + list fetch ----------------------------- */

  useEffect(() => {
    void (async () => {
      try {
        const [p, list] = await Promise.all([
          meetings.probeSources(),
          meetings.list(PAGE_SIZE, 0),
        ]);
        setProbe(p);
        // Default-pick a source the box actually supports.
        if (!p.micAvailable && p.systemAvailable) setSource("system");
        setSummaries(list);
        if (list[0] && selectedUuid === null) {
          setSelectedUuid(list[0].uuid);
        }
      } catch (err) {
        // eslint-disable-next-line no-console
        console.warn("Meetings page boot fetch failed", err);
        setProbe({ micAvailable: false, systemAvailable: false });
        setSummaries([]);
      }
    })();
    // selectedUuid intentionally omitted — initial fetch only.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /* -------- route sync: URL `/meetings/:uuid` → selectedUuid ------ */

  useEffect(() => {
    if (routeUuid && routeUuid !== selectedUuid) setSelectedUuid(routeUuid);
  }, [routeUuid, selectedUuid]);

  /* -------- live events from Rust runtime ------------------------- */

  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    const unlisteners: Array<() => void> = [];

    (async () => {
      const { listen } = await import("@tauri-apps/api/event");

      // Phase changes — drive the record-bar UI. The Rust runtime
      // emits "started" on start, "done" / "error" on stop.
      unlisteners.push(
        await listen<MeetingStateEvent>("meeting:state", (e) => {
          if (cancelled) return;
          // mb-z5y wave-2 forensic ping — fire-and-forget IPC so
          // the Rust log has hard evidence the listener ran.
          void (async () => {
            try {
              const { invoke } = await import("@tauri-apps/api/core");
              await invoke("meeting_debug_listener_ping", {
                window: "main",
                event: "meeting:state",
                payloadState: e.payload.state,
              });
            } catch {
              /* swallow — diagnostic only */
            }
          })();
          setRecordingPhase(e.payload.state);
          if (e.payload.state === "started" && e.payload.uuid) {
            setRecordingUuid(e.payload.uuid);
            recordStartTimeRef.current = Date.now();
            // Fresh meeting — clear stale progress from the previous run.
            setProgress({});
            // mb-fc1 hotfix: the Settings → Start button path sets
            // `startingOrStopping="starting"` and used to rely on the
            // `done`/`error` event to clear it — but `done` only
            // fires on stop, not on start. Result: after a
            // successful start, the button stayed in the "starting…"
            // state and the Stop click was a no-op because it was
            // disabled. The chord-start path skipped
            // `handleStart` entirely so the bug was invisible there.
            // Now we clear the latch on the actual started event,
            // which fires from both the button and the chord paths.
            setStartingOrStopping(null);
          } else if (
            e.payload.state === "done" ||
            e.payload.state === "error" ||
            e.payload.state === "interrupted" ||
            e.payload.state === "cancelled"
          ) {
            setRecordingUuid(null);
            recordStartTimeRef.current = null;
            setElapsedSec(0);
            setStartingOrStopping(null);
          }
        }),
      );

      // Per-chunk progress from the long-form STT driver. Updates
      // the record bar's status chip during the "transcribing" phase.
      // Mirrors the per-channel ChannelProgress shape on the Rust side.
      unlisteners.push(
        await listen<MeetingProgressEvent>("meeting:progress", (e) => {
          if (cancelled) return;
          const { channel, chunksDone, chunksTotal } = e.payload;
          setProgress((prev) => ({
            ...prev,
            [channel]: { done: chunksDone, total: chunksTotal },
          }));
        }),
      );

      // Session committed — refresh the list. Mirrors History's
      // `history:session-saved` handler.
      unlisteners.push(
        await listen<{ uuid: string }>("meetings:session-saved", (e) => {
          if (cancelled) return;
          void (async () => {
            const list = await meetings.list(PAGE_SIZE, 0);
            if (cancelled) return;
            setSummaries(list);
            // Auto-focus the just-saved meeting if the user hasn't
            // navigated elsewhere mid-recording.
            setSelectedUuid((prev) => prev ?? e.payload.uuid);
          })();
        }),
      );
    })();

    return () => {
      cancelled = true;
      for (const f of unlisteners) f();
    };
  }, []);

  /* -------- elapsed-time ticker ----------------------------------- */

  useEffect(() => {
    if (recordingUuid === null) return;
    const handle = window.setInterval(() => {
      const t0 = recordStartTimeRef.current;
      if (t0 !== null) setElapsedSec(Math.floor((Date.now() - t0) / 1000));
    }, 1000);
    return () => window.clearInterval(handle);
  }, [recordingUuid]);

  /* -------- debounced search -------------------------------------- */

  useEffect(() => {
    if (query.trim().length === 0) {
      setSearchHits(null);
      return;
    }
    const handle = window.setTimeout(() => {
      void (async () => {
        const hits = await meetings.search(query.trim());
        setSearchHits(hits);
      })();
    }, SEARCH_DEBOUNCE_MS);
    return () => window.clearTimeout(handle);
  }, [query]);

  /* -------- detail fetch on selection change ---------------------- */

  useEffect(() => {
    if (selectedUuid === null) {
      setDetail(null);
      return;
    }
    void (async () => {
      try {
        const d = await meetings.detail(selectedUuid);
        setDetail(d);
      } catch (err) {
        // Most likely the row was just deleted; clear silently.
        // eslint-disable-next-line no-console
        console.warn("get_meeting_detail failed", err);
        setDetail(null);
      }
    })();
  }, [selectedUuid]);

  /* -------- record-bar handlers ----------------------------------- */

  const handleStart = useCallback(async () => {
    if (recordingUuid || startingOrStopping) return;
    setStartingOrStopping("starting");
    try {
      const { uuid } = await meetings.start(source);
      setRecordingUuid(uuid);
      recordStartTimeRef.current = Date.now();
      // mb-z5y defensive clear: we already optimistically set
      // `recordingUuid` above (so the timer shows immediately).
      // For consistency, also optimistically clear the
      // starting/stopping latch here — otherwise the Stop button
      // stays disabled until the `meeting:state="started"` event
      // arrives, and the user is stranded if that event is ever
      // lost in transit (cf. mb-fc1 / mb-z5y). The event listener
      // also clears this latch, which makes both paths idempotent.
      setStartingOrStopping(null);
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("meeting_start failed", err);
      setToast(String(err));
      window.setTimeout(() => setToast(null), 3000);
      setStartingOrStopping(null);
    }
  }, [recordingUuid, source, startingOrStopping]);

  const handleStop = useCallback(async () => {
    if (!recordingUuid || startingOrStopping) return;
    setStartingOrStopping("stopping");
    try {
      await meetings.stop(recordingUuid);
      // The `meeting:state=done` event will clear recordingUuid +
      // startingOrStopping. We don't clear here optimistically
      // because the persist path takes a few seconds on long
      // meetings; users need feedback the request was acknowledged.
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("meeting_stop failed", err);
      setToast(String(err));
      window.setTimeout(() => setToast(null), 3000);
      setStartingOrStopping(null);
    }
  }, [recordingUuid, startingOrStopping]);

  /* -------- list-row click ---------------------------------------- */

  const handleSelect = useCallback(
    (uuid: string) => {
      setSelectedUuid(uuid);
      // Mirror the selection into the URL so the browser back button
      // works as expected. Use `replace` to avoid stacking dozens of
      // history entries as the user clicks through the list.
      navigate(`/meetings/${uuid}`, { replace: true });
    },
    [navigate],
  );

  /* -------- detail-pane action handlers --------------------------- */

  const showToast = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 1800);
  }, []);

  const handleDelete = useCallback(async () => {
    if (!detail) return;
    if (!window.confirm(`${t("meetings.detail.action.delete")}?`)) return;
    await meetings.delete(detail.uuid);
    const list = await meetings.list(PAGE_SIZE, 0);
    setSummaries(list);
    const next = list[0]?.uuid ?? null;
    setSelectedUuid(next);
    if (next) {
      navigate(`/meetings/${next}`, { replace: true });
    } else {
      navigate("/meetings", { replace: true });
    }
    showToast(t("meetings.deleted"));
  }, [detail, navigate, showToast]);

  const handleCopy = useCallback(async () => {
    if (!detail) return;
    const llm = llmByUuid[detail.uuid];
    const id = llm?.includeInExport ? llm.result?.id : undefined;
    await meetings.copyToClipboard(detail.uuid, id);
    showToast(t("meetings.copied"));
  }, [detail, llmByUuid, showToast]);

  const handleRename = useCallback(
    async (next: string | null) => {
      if (!detail) return;
      const uuid = detail.uuid;
      try {
        await meetings.rename(uuid, next);
        // Refresh BOTH the detail (header title) and the list (left-
        // pane row title) so the UI immediately reflects the rename
        // without a manual reload. Run in parallel.
        const [fresh, list] = await Promise.all([
          meetings.detail(uuid),
          meetings.list(PAGE_SIZE, 0),
        ]);
        setDetail(fresh);
        setSummaries(list);
      } catch (err) {
        // eslint-disable-next-line no-console
        console.error("meeting_rename failed", err);
        setToast(String(err));
        window.setTimeout(() => setToast(null), 3000);
      }
    },
    [detail],
  );

  const handleExport = useCallback(async () => {
    if (!detail) return;
    const llm = llmByUuid[detail.uuid];
    const id = llm?.includeInExport ? llm.result?.id : undefined;
    // Phase MC Wave 5 — default-pass `promptUserForPath: true` so the
    // Export button opens a native Save As… dialog. A null `path`
    // comes back when the user cancelled.
    const { path } = await meetings.exportMarkdown(
      detail.uuid,
      undefined,
      id,
      true,
    );
    if (path === null) {
      showToast(t("meetings.exportCancelled"));
      return;
    }
    showToast(t("meetings.exported").replace("{path}", path));
  }, [detail, llmByUuid, showToast]);

  /* -------- LLM-pass handlers ------------------------------------- */

  const updateLlm = useCallback(
    (uuid: string, patch: Partial<LlmPassUiState>) => {
      setLlmByUuid((prev) => ({
        ...prev,
        [uuid]: { ...(prev[uuid] ?? FRESH_LLM), ...patch },
      }));
    },
    [],
  );

  const handleRunLlmPass = useCallback(async () => {
    if (!detail) return;
    const state = llmByUuid[detail.uuid] ?? FRESH_LLM;
    const promptArg =
      state.promptId === "custom"
        ? { custom: state.customBody }
        : state.promptId;
    if (state.promptId === "custom" && state.customBody.trim().length === 0) {
      showToast("Empty custom prompt");
      return;
    }
    updateLlm(detail.uuid, { running: true });
    try {
      const result = await meetings.runLlmPass(detail.uuid, promptArg);
      updateLlm(detail.uuid, { result, running: false });
    } catch (err) {
      // eslint-disable-next-line no-console
      console.error("meeting_run_llm_pass failed", err);
      updateLlm(detail.uuid, { running: false });
      showToast(String(err));
    }
  }, [detail, llmByUuid, showToast, updateLlm]);

  /* -------- derived: which list to render ------------------------- */

  const listContent = useMemo(() => {
    if (summaries === null) return <Spinner />;
    if (summaries.length === 0) {
      return (
        <EmptyState
          icon={<MeetingsIcon size={28} />}
          title={t("meetings.empty.title")}
          subtitle={t("meetings.empty.subtitle")}
        />
      );
    }
    if (searchHits !== null) {
      return (
        <SearchHitsList
          hits={searchHits}
          selectedUuid={selectedUuid}
          onSelect={handleSelect}
        />
      );
    }
    return (
      <MeetingList
        summaries={summaries}
        selectedUuid={selectedUuid}
        onSelect={handleSelect}
      />
    );
  }, [summaries, searchHits, selectedUuid, handleSelect]);

  /* -------- render ------------------------------------------------ */

  return (
    <>
      <PageHeader
        title={t("meetings.title")}
        subtitle={t("meetings.subtitle")}
      />

      <div className={styles.shell}>
        <div className={styles.leftPane}>
          <MeetingRecordBar
            probe={probe}
            source={source}
            onSourceChange={setSource}
            recordingUuid={recordingUuid}
            recordingPhase={recordingPhase}
            startingOrStopping={startingOrStopping}
            elapsedSec={elapsedSec}
            progress={progress}
            onStart={handleStart}
            onStop={handleStop}
          />
          <SearchInput value={query} onChange={setQuery} />
          {listContent}
        </div>

        <div className={styles.rightPane}>
          {detail ? (
            <MeetingDetailView
              detail={detail}
              llm={llmByUuid[detail.uuid] ?? FRESH_LLM}
              onLlmChange={(patch) => updateLlm(detail.uuid, patch)}
              onRunLlmPass={handleRunLlmPass}
              onCopy={handleCopy}
              onExport={handleExport}
              onDelete={handleDelete}
              onRename={handleRename}
            />
          ) : summaries && summaries.length === 0 ? null : selectedUuid ===
            null ? (
            <EmptyState
              icon={<MeetingsIcon size={28} />}
              title={t("meetings.detail.empty")}
            />
          ) : (
            <Spinner />
          )}
        </div>
      </div>

      {toast ? <div className={styles.toast}>{toast}</div> : null}
    </>
  );
}



/* ------------------------------------------------------------------ */
/* Search input + list variants                                        */
/* ------------------------------------------------------------------ */

function SearchInput({
  value,
  onChange,
}: {
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <label className={styles.searchBox}>
      <span className={styles.searchIcon}>
        <SearchIcon size={16} />
      </span>
      <input
        type="search"
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={t("meetings.search.placeholder")}
        className={styles.searchInput}
        aria-label={t("meetings.search.placeholder")}
      />
    </label>
  );
}

function MeetingList({
  summaries,
  selectedUuid,
  onSelect,
}: {
  summaries: MeetingSummary[];
  selectedUuid: string | null;
  onSelect: (uuid: string) => void;
}) {
  return (
    <div className={styles.list} role="listbox" aria-label="Meetings">
      {summaries.map((m) => (
        <MeetingRow
          key={m.uuid}
          summary={m}
          active={m.uuid === selectedUuid}
          onClick={() => onSelect(m.uuid)}
        />
      ))}
    </div>
  );
}

function MeetingRow({
  summary,
  active,
  onClick,
}: {
  summary: MeetingSummary;
  active: boolean;
  onClick: () => void;
}) {
  const title = summary.title?.trim() || t("meetings.detail.untitled");
  return (
    <div
      className={`${styles.row} ${active ? styles.rowActive : ""}`}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onClick();
        }
      }}
      role="option"
      aria-selected={active}
      tabIndex={0}
    >
      <div className={styles.rowHeader}>
        <span className={styles.rowTitle}>{truncate(title, 60)}</span>
        <span className={styles.rowTime}>
          {formatRelative(summary.startedAt)}
        </span>
      </div>
      <div className={styles.rowMeta}>
        <SourcePills source={summary.source} />
        <span>{formatDuration(summary.totalDurationMs)}</span>
        {summary.status !== "complete" ? (
          <Pill tone="status-error">{statusLabel(summary.status)}</Pill>
        ) : null}
      </div>
    </div>
  );
}

function SearchHitsList({
  hits,
  selectedUuid,
  onSelect,
}: {
  hits: MeetingMatch[];
  selectedUuid: string | null;
  onSelect: (uuid: string) => void;
}) {
  if (hits.length === 0) {
    return (
      <EmptyState
        icon={<SearchIcon size={28} />}
        title={t("meetings.search.noResults")}
      />
    );
  }
  return (
    <div className={styles.list} role="listbox" aria-label="Search results">
      {hits.map((h) => (
        <div
          key={`${h.uuid}-${h.channel}`}
          className={`${styles.row} ${
            h.uuid === selectedUuid ? styles.rowActive : ""
          }`}
          onClick={() => onSelect(h.uuid)}
          onKeyDown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              onSelect(h.uuid);
            }
          }}
          role="option"
          aria-selected={h.uuid === selectedUuid}
          tabIndex={0}
        >
          <div className={styles.rowHeader}>
            <span className={styles.rowTitle}>
              {h.title?.trim() || t("meetings.detail.untitled")}
            </span>
            <span className={styles.rowTime}>{formatRelative(h.startedAt)}</span>
          </div>
          <div
            className={styles.snippet}
            // FTS5 snippets contain `<mark>...</mark>` tags. They come
            // straight out of SQLite so the trust boundary is the same
            // as the rest of the transcript (the user's own dictation).
            // eslint-disable-next-line react/no-danger
            dangerouslySetInnerHTML={{ __html: h.snippet }}
          />
          <div className={styles.rowMeta}>
            <Pill tone="status-info">{h.channel}</Pill>
          </div>
        </div>
      ))}
    </div>
  );
}


