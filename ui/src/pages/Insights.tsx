// Insights dashboard.
//
// Two-tab layout — "Your usage" (lifetime totals + heatmap + activity
// shape) vs. "Your voice" (WPM, peak hours, top terms, latency,
// learning loop). Single fetch of `insights_snapshot`; both tabs
// render from the same payload so we never see stale data on tab
// switch.
//
// Empty-state heuristic: if the user has zero lifetime sessions AND
// no activity in the 7-day sparkline, we render an EmptyState
// instead of a grid of zeros. Once they've ever dictated, the full
// dashboard always renders — yesterday's gap shouldn't hide the
// streak summary.

import { useEffect, useRef, useState } from "react";

import { Card, EmptyState, PageHeader, Spinner } from "../components/primitives";
import { SparklesIcon } from "../design/Icon";
import { t } from "../i18n";
import {
  formatCount,
  formatDuration,
  formatRelative,
  prettyAppName,
} from "../lib/format";
import { api } from "../lib/tauri";
import type { InsightsSnapshot } from "../lib/types";

import styles from "./Insights.module.css";

type TabId = "usage" | "voice";

/** Latency thresholds (ms) for color-coding. */
const LATENCY_FAST_MS = 800;
const LATENCY_SLOW_MS = 2500;

export function InsightsPage() {
  const [snap, setSnap] = useState<InsightsSnapshot | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [tab, setTab] = useState<TabId>("usage");

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const s = await api.insights_snapshot();
        if (!cancelled) setSnap(s);
      } catch (e) {
        if (!cancelled) setErr(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  if (err) {
    return (
      <>
        <PageHeader title={t("insights.title")} />
        <EmptyState
          icon={<SparklesIcon size={32} />}
          title={t("insights.empty.title")}
          subtitle={err}
        />
      </>
    );
  }
  if (!snap) {
    return (
      <>
        <PageHeader title={t("insights.title")} />
        <Spinner />
      </>
    );
  }

  const hasEverDictated = snap.lifetime.dictationSessions > 0;
  const has7dActivity = snap.sparkline7d.some((v) => v > 0);
  if (!hasEverDictated && !has7dActivity) {
    return (
      <>
        <PageHeader title={t("insights.title")} />
        <EmptyState
          icon={<SparklesIcon size={32} />}
          title={t("insights.empty.title")}
          subtitle={t("insights.empty.subtitle")}
        />
      </>
    );
  }

  return (
    <>
      <PageHeader title={t("insights.title")} />
      <Tabs current={tab} onChange={setTab} />
      <div className={styles.shell}>
        {tab === "usage" ? (
          <UsageTab snap={snap} />
        ) : (
          <VoiceTab snap={snap} />
        )}
      </div>
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Tab strip                                                          */
/* ------------------------------------------------------------------ */

function Tabs({
  current,
  onChange,
}: {
  current: TabId;
  onChange: (id: TabId) => void;
}) {
  // Two tabs only — a select dropdown would be overkill. Plain
  // buttons with an animated underline via CSS `aria-selected`.
  const tabs: Array<{ id: TabId; label: string }> = [
    { id: "usage", label: t("insights.tab.usage") },
    { id: "voice", label: t("insights.tab.voice") },
  ];
  return (
    <div role="tablist" aria-label="Insights tabs" className={styles.tabs}>
      {tabs.map((tabDef) => (
        <button
          key={tabDef.id}
          type="button"
          role="tab"
          aria-selected={current === tabDef.id}
          className={styles.tab}
          onClick={() => onChange(tabDef.id)}
        >
          {tabDef.label}
        </button>
      ))}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Tab 1 — "Your usage"                                              */
/* ------------------------------------------------------------------ */

function UsageTab({ snap }: { snap: InsightsSnapshot }) {
  return (
    <>
      <LifetimeTiles snap={snap} />
      <Card title={t("insights.heatmap.title")}>
        <Heatmap days={snap.heatmap365d} />
      </Card>
      <div className={styles.row2}>
        <Card title={t("insights.last7")}>
          <Sparkline data={snap.sparkline7d} />
        </Card>
        <Card title={t("insights.modeMix")}>
          <ModeMix mix={snap.modeMix} />
        </Card>
      </div>
      <div className={styles.row2}>
        <Card title={t("insights.topApps")}>
          <TopApps apps={snap.topApps} />
        </Card>
        <Card title={t("insights.today.title")}>
          <TodayBlock snap={snap} />
        </Card>
      </div>
    </>
  );
}

function LifetimeTiles({ snap }: { snap: InsightsSnapshot }) {
  const lt = snap.lifetime;
  return (
    <div className={styles.tiles}>
      <Tile
        label={t("insights.lifetime.words")}
        value={formatCount(lt.dictationWords)}
        hint={`${formatCount(lt.dictationSessions)} sessions`}
      />
      <Tile
        label={t("insights.lifetime.dictation")}
        value={formatDuration(lt.dictationRecordingMs)}
      />
      <Tile
        label={t("insights.lifetime.meetings")}
        value={formatDuration(lt.meetingsTotalMs)}
        hint={t("insights.lifetime.meetingsCount").replace(
          "{count}",
          String(lt.meetingsCount),
        )}
      />
      <Tile
        label={t("insights.streak.current")}
        value={
          snap.streakDays === 1
            ? t("insights.streak.day")
            : t("insights.streak.days").replace("{count}", String(snap.streakDays))
        }
        accent="streak"
      />
      <Tile
        label={t("insights.streak.longest")}
        value={t("insights.streak.days").replace(
          "{count}",
          String(snap.longestStreakDays),
        )}
      />
    </div>
  );
}

function TodayBlock({ snap }: { snap: InsightsSnapshot }) {
  const td = snap.today;
  return (
    <div className={styles.todayGrid}>
      <MiniStat label={t("insights.words")} value={formatCount(td.words)} />
      <MiniStat label={t("insights.sessions")} value={formatCount(td.sessions)} />
      <MiniStat
        label={t("insights.recording")}
        value={formatDuration(td.recordingMs)}
      />
      <MiniStat
        label={t("insights.timeSaved")}
        value={formatDuration(td.timeSavedMs)}
      />
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Tab 2 — "Your voice"                                              */
/* ------------------------------------------------------------------ */

function VoiceTab({ snap }: { snap: InsightsSnapshot }) {
  return (
    <>
      <div className={styles.row2}>
        <Card title={t("insights.wpm.title")}>
          <WpmBlock wpm={snap.wpm} />
        </Card>
        <Card title={t("insights.peakHours.title")}>
          <PeakHours hours={snap.peakHours} />
        </Card>
      </div>
      <div className={styles.row2}>
        <Card title={t("insights.topTerms.title")}>
          <TopTerms terms={snap.topDictTerms} />
        </Card>
        <Card title={t("insights.topCorrections.title")}>
          <TopCorrections items={snap.topCorrections} />
        </Card>
      </div>
      <div className={styles.row2}>
        <Card title={t("insights.latency")}>
          <LatencyBlock latency={snap.latency} />
        </Card>
        <Card title={t("insights.learning")}>
          <LearningBlock learning={snap.learning} />
        </Card>
      </div>
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Shared tile                                                          */
/* ------------------------------------------------------------------ */

interface TileProps {
  label: string;
  value: string;
  hint?: string;
  accent?: "streak";
}

function Tile({ label, value, hint, accent }: TileProps) {
  return (
    <div
      className={`${styles.tile} ${accent === "streak" ? styles.tileStreak : ""}`}
    >
      <div className={styles.tileLabel}>{label}</div>
      <div className={styles.tileValue}>{value}</div>
      {hint ? <div className={styles.tileHint}>{hint}</div> : null}
    </div>
  );
}

function MiniStat({ label, value }: { label: string; value: string }) {
  return (
    <div className={styles.miniStat}>
      <div className={styles.tileLabel}>{label}</div>
      <div className={styles.miniStatValue}>{value}</div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Heatmap — GitHub-style contribution grid (53 weeks × 7 days).      */
/*                                                                    */
/* The 365-day series we get from the backend has *today* at the end. */
/* We pad the LEADING edge to align the first column to Sunday so all */
/* week-columns are uniform and the day-of-week row labels match.     */
/* ------------------------------------------------------------------ */

const HEATMAP_LEVELS = 5; // 0 = none, 1..4 = increasing intensity

function Heatmap({ days }: { days: InsightsSnapshot["heatmap365d"] }) {
  if (days.length === 0) return null;

  // Compute level thresholds by scanning the max — keep it simple:
  // [1..max/4, max/4..max/2, max/2..3max/4, >3max/4]. A logarithmic
  // bucketing would compress heavy users, but for a personal tool the
  // linear split matches expectations.
  const max = Math.max(1, ...days.map((d) => d.sessions));
  const levelFor = (n: number): number => {
    if (n <= 0) return 0;
    const ratio = n / max;
    if (ratio <= 0.25) return 1;
    if (ratio <= 0.5) return 2;
    if (ratio <= 0.75) return 3;
    return 4;
  };

  // Pad leading days so column 0 starts on Sunday (matches GitHub).
  // `getDay()` returns 0=Sun..6=Sat.
  const firstDow = new Date(days[0]!.date).getDay();
  const padded: Array<InsightsSnapshot["heatmap365d"][number] | null> = [
    ...Array(firstDow).fill(null),
    ...days,
  ];

  // Bucket into week-columns. Round up to keep the trailing partial
  // week visible; we'll render `null` padding cells as invisible.
  const columns: Array<typeof padded> = [];
  for (let i = 0; i < padded.length; i += 7) {
    columns.push(padded.slice(i, i + 7));
  }

  // Month labels — show a label above each week column whose first
  // day-of-month <= 7 falls inside the column (i.e. the column where
  // a new month begins).
  const monthLabels: Array<{ col: number; label: string }> = [];
  let lastMonth = -1;
  columns.forEach((col, colIdx) => {
    for (const cell of col) {
      if (!cell) continue;
      const date = new Date(cell.date);
      const month = date.getMonth();
      if (month !== lastMonth) {
        lastMonth = month;
        monthLabels.push({
          col: colIdx,
          label: date.toLocaleString("en-US", { month: "short" }),
        });
        break;
      }
    }
  });

  return (
    <div className={styles.heatmap}>
      <div className={styles.heatmapMonths}>
        {monthLabels.map((m) => (
          <span
            key={`${m.col}-${m.label}`}
            style={{ gridColumn: m.col + 2 /* +2: 1-based + dow-label col */ }}
          >
            {m.label}
          </span>
        ))}
      </div>
      <div className={styles.heatmapBody}>
        <div className={styles.heatmapDows}>
          <span>Mon</span>
          <span>Wed</span>
          <span>Fri</span>
        </div>
        <div
          className={styles.heatmapGrid}
          style={{ gridTemplateColumns: `repeat(${columns.length}, 1fr)` }}
        >
          {columns.map((col, colIdx) => (
            <div key={colIdx} className={styles.heatmapCol}>
              {col.map((cell, rowIdx) => {
                if (!cell) {
                  return (
                    <span key={rowIdx} className={styles.heatmapPad} aria-hidden />
                  );
                }
                const lvl = levelFor(cell.sessions);
                return (
                  <span
                    key={rowIdx}
                    className={`${styles.heatmapCell} ${styles[`hm${lvl}`]}`}
                    title={t("insights.heatmap.tooltip")
                      .replace("{date}", cell.date)
                      .replace("{sessions}", String(cell.sessions))
                      .replace("{words}", String(cell.words))}
                    role="img"
                    aria-label={`${cell.date}: ${cell.sessions} sessions`}
                  />
                );
              })}
            </div>
          ))}
        </div>
      </div>
      <div className={styles.heatmapLegend}>
        <span>{t("insights.heatmap.legend.less")}</span>
        {Array.from({ length: HEATMAP_LEVELS }, (_, i) => (
          <span key={i} className={`${styles.heatmapCell} ${styles[`hm${i}`]}`} />
        ))}
        <span>{t("insights.heatmap.legend.more")}</span>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* WPM big number                                                       */
/* ------------------------------------------------------------------ */

function WpmBlock({ wpm }: { wpm: InsightsSnapshot["wpm"] }) {
  if (wpm.avgWpm == null) {
    return (
      <p style={{ color: "var(--on-surf-muted)" }}>
        {t("insights.wpm.noData")}
      </p>
    );
  }
  return (
    <div className={styles.wpmBlock}>
      <div className={styles.wpmValue}>{Math.round(wpm.avgWpm)}</div>
      <div className={styles.wpmUnit}>wpm</div>
      <div className={styles.tileHint}>
        {t("insights.wpm.subtitle").replace(
          "{samples}",
          String(wpm.samples),
        )}
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Peak hours — 24-bucket bar chart                                    */
/* ------------------------------------------------------------------ */

function PeakHours({ hours }: { hours: number[] }) {
  const max = Math.max(1, ...hours);
  return (
    <>
      <div
        className={styles.peakBars}
        role="img"
        aria-label={`Hourly session counts: ${hours.join(", ")}`}
      >
        {hours.map((n, h) => (
          <span
            key={h}
            className={styles.peakBar}
            style={{ height: `${(n / max) * 100}%` }}
            title={`${h}:00 — ${n} sessions`}
          />
        ))}
      </div>
      <div className={styles.peakScale}>
        <span>12a</span>
        <span>6a</span>
        <span>12p</span>
        <span>6p</span>
      </div>
      <div className={styles.tileHint} style={{ marginTop: 6 }}>
        {t("insights.peakHours.subtitle")}
      </div>
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Top dictionary terms / top corrections — bar lists                  */
/* ------------------------------------------------------------------ */

function TopTerms({ terms }: { terms: InsightsSnapshot["topDictTerms"] }) {
  if (terms.length === 0) {
    return (
      <p style={{ color: "var(--on-surf-muted)" }}>
        {t("insights.topTerms.empty")}
      </p>
    );
  }
  const max = Math.max(1, ...terms.map((t) => t.useCount));
  return (
    <div className={styles.appsList}>
      {terms.map((tEntry) => (
        <div className={styles.appRow} key={tEntry.term}>
          <span className={styles.appName}>{tEntry.term}</span>
          <span className={styles.appBar}>
            <span
              className={styles.appBarFill}
              style={{ width: `${(tEntry.useCount / max) * 100}%` }}
            />
          </span>
          <span className={styles.appCount}>{tEntry.useCount}</span>
        </div>
      ))}
    </div>
  );
}

function TopCorrections({
  items,
}: {
  items: InsightsSnapshot["topCorrections"];
}) {
  if (items.length === 0) {
    return (
      <p style={{ color: "var(--on-surf-muted)" }}>
        {t("insights.topCorrections.empty")}
      </p>
    );
  }
  const max = Math.max(1, ...items.map((i) => i.count));
  return (
    <div className={styles.appsList}>
      {items.map((it) => (
        <div className={styles.appRow} key={it.before}>
          <span className={styles.appName}>{it.before}</span>
          <span className={styles.appBar}>
            <span
              className={styles.appBarFill}
              style={{ width: `${(it.count / max) * 100}%` }}
            />
          </span>
          <span className={styles.appCount}>{it.count}</span>
        </div>
      ))}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* 7-day sparkline (kept — small canvas, no chart lib).                */
/* ------------------------------------------------------------------ */

function Sparkline({ data }: { data: number[] }) {
  const ref = useRef<HTMLCanvasElement | null>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    canvas.width = Math.floor(rect.width * dpr);
    canvas.height = Math.floor(rect.height * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, rect.width, rect.height);

    const max = Math.max(1, ...data);
    const barCount = data.length;
    const gap = 6;
    const totalGap = gap * (barCount - 1);
    const barW = Math.max(2, (rect.width - totalGap) / barCount);

    const accent =
      getComputedStyle(document.documentElement)
        .getPropertyValue("--mode-normal")
        .trim() || "#7ab8ff";

    ctx.fillStyle = accent;
    for (let i = 0; i < barCount; i++) {
      const v = data[i] ?? 0;
      const h = (v / max) * (rect.height - 8);
      const x = i * (barW + gap);
      const y = rect.height - h;
      ctx.beginPath();
      const r = Math.min(3, barW / 2);
      ctx.moveTo(x, y + r);
      ctx.arcTo(x, y, x + r, y, r);
      ctx.lineTo(x + barW - r, y);
      ctx.arcTo(x + barW, y, x + barW, y + r, r);
      ctx.lineTo(x + barW, rect.height);
      ctx.lineTo(x, rect.height);
      ctx.closePath();
      ctx.fill();
    }
  }, [data]);

  return (
    <div className={styles.sparkWrap}>
      <canvas
        ref={ref}
        className={styles.sparkCanvas}
        aria-label={`7-day word counts: ${data.join(", ")}`}
        role="img"
      />
      <div className={styles.sparkScale}>
        <span>6d ago</span>
        <span>Today</span>
      </div>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Mode mix — segmented bar + legend (unchanged)                      */
/* ------------------------------------------------------------------ */

function ModeMix({ mix }: { mix: InsightsSnapshot["modeMix"] }) {
  const total = mix.reduce((sum, m) => sum + m.count, 0) || 1;
  return (
    <>
      <div
        className={styles.mixBar}
        role="img"
        aria-label={
          "Mode breakdown: " +
          mix.map((m) => `${m.label} ${m.count}`).join(", ")
        }
      >
        {mix.map((m) => (
          <span
            key={m.slug}
            className={styles.mixSeg}
            style={{
              width: `${(m.count / total) * 100}%`,
              background: `var(--mode-${m.slug}, var(--mode-normal))`,
            }}
            title={`${m.label}: ${m.count}`}
          />
        ))}
      </div>
      <div className={styles.mixLegend}>
        {mix.map((m) => (
          <span key={m.slug} className={styles.mixLegendItem}>
            <span
              className={styles.mixSwatch}
              style={{ background: `var(--mode-${m.slug}, var(--mode-normal))` }}
            />
            {m.label}
            <span className={styles.mixCount}>{m.count}</span>
          </span>
        ))}
      </div>
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Top apps (unchanged)                                                */
/* ------------------------------------------------------------------ */

function TopApps({ apps }: { apps: InsightsSnapshot["topApps"] }) {
  if (apps.length === 0) {
    return <p style={{ color: "var(--on-surf-muted)" }}>No apps tracked yet.</p>;
  }
  const max = Math.max(1, ...apps.map((a) => a.count));
  return (
    <div className={styles.appsList}>
      {apps.map((a) => (
        <div className={styles.appRow} key={a.app}>
          <span className={styles.appName}>{prettyAppName(a.app)}</span>
          <span className={styles.appBar}>
            <span
              className={styles.appBarFill}
              style={{ width: `${(a.count / max) * 100}%` }}
            />
          </span>
          <span className={styles.appCount}>{a.count}</span>
        </div>
      ))}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Latency (unchanged)                                                 */
/* ------------------------------------------------------------------ */

function LatencyBlock({ latency }: { latency: InsightsSnapshot["latency"] }) {
  return (
    <div className={styles.latencyRow}>
      <LatencyItem label={t("insights.latencyStt")} ms={latency.sttMs} />
      <LatencyItem label={t("insights.latencyCleanup")} ms={latency.cleanupMs} />
      <LatencyItem label={t("insights.latencyInject")} ms={latency.injectMs} />
    </div>
  );
}

function LatencyItem({ label, ms }: { label: string; ms: number }) {
  const cls =
    ms <= LATENCY_FAST_MS
      ? styles.fast
      : ms >= LATENCY_SLOW_MS
        ? styles.slow
        : "";
  return (
    <div className={styles.latencyItem}>
      <span className={styles.latencyLabel}>{label}</span>
      <span className={`${styles.latencyValue} ${cls}`}>{Math.round(ms)} ms</span>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Learning loop (unchanged)                                            */
/* ------------------------------------------------------------------ */

function LearningBlock({ learning }: { learning: InsightsSnapshot["learning"] }) {
  return (
    <>
      <div className={styles.learnGrid}>
        <div>
          <div className={styles.tileLabel}>{t("insights.learningLastRun")}</div>
          <div style={{ marginTop: 4 }}>
            {learning.lastRunAt ? (
              <span title={learning.lastRunAt}>
                {formatRelative(learning.lastRunAt)}
              </span>
            ) : (
              <span style={{ color: "var(--on-surf-faint)" }}>—</span>
            )}
          </div>
          {learning.lastRolledBack ? (
            <div className={styles.learnRolledBack}>rolled back</div>
          ) : null}
        </div>
        <div>
          <div className={styles.tileLabel}>
            {t("insights.learningCommittedStreak")}
          </div>
          <div style={{ marginTop: 4, fontVariantNumeric: "tabular-nums" }}>
            {learning.committedStreak} runs
          </div>
        </div>
      </div>
      {learning.recentTerms.length > 0 ? (
        <div>
          <div className={styles.tileLabel}>
            {t("insights.learningRecentTerms")}
          </div>
          <div className={styles.learnTerms} style={{ marginTop: 6 }}>
            {learning.recentTerms.map((term) => (
              <span key={term} className={styles.learnTerm}>
                {term}
              </span>
            ))}
          </div>
        </div>
      ) : null}
    </>
  );
}
