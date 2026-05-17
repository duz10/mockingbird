// Insights dashboard — single fetch of `insights_snapshot`, rendered
// as a three-row grid of tiles + cards. No chart library: a tiny
// canvas sparkline + a CSS segmented bar are all we need.
//
// Empty-state heuristic: if today has zero sessions AND there's no
// activity in the 7-day sparkline, we render an EmptyState instead
// of a grid of zeros. The threshold is intentionally generous —
// users who skipped a day still see their streak + recent learning
// runs, which is more useful than "no data".

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

/** Latency thresholds (ms) for color-coding. Generous because local
 * Whisper varies by hardware; the goal is to flag obvious outliers,
 * not to grade every run. */
const LATENCY_FAST_MS = 800;
const LATENCY_SLOW_MS = 2500;

export function InsightsPage() {
  const [snap, setSnap] = useState<InsightsSnapshot | null>(null);
  const [err, setErr] = useState<string | null>(null);

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

  const hasActivity =
    snap.today.sessions > 0 || snap.sparkline7d.some((v) => v > 0);

  return (
    <>
      <PageHeader title={t("insights.title")} />

      {!hasActivity ? (
        <EmptyState
          icon={<SparklesIcon size={32} />}
          title={t("insights.empty.title")}
          subtitle={t("insights.empty.subtitle")}
        />
      ) : (
        <div className={styles.shell}>
          <TodayTiles snap={snap} />

          <div className={styles.row2}>
            <Card title={t("insights.last7")}>
              <Sparkline data={snap.sparkline7d} />
            </Card>
            <Card title={t("insights.modeMix")}>
              <ModeMix mix={snap.modeMix} />
            </Card>
          </div>

          <div className={styles.row3}>
            <Card title={t("insights.topApps")}>
              <TopApps apps={snap.topApps} />
            </Card>
            <Card title={t("insights.latency")}>
              <LatencyBlock latency={snap.latency} />
            </Card>
            <Card title={t("insights.learning")}>
              <LearningBlock learning={snap.learning} />
            </Card>
          </div>
        </div>
      )}
    </>
  );
}

/* ------------------------------------------------------------------ */
/* Row 1 — tiles                                                       */
/* ------------------------------------------------------------------ */

function TodayTiles({ snap }: { snap: InsightsSnapshot }) {
  return (
    <div className={styles.tiles}>
      <Tile label={t("insights.words")} value={formatCount(snap.today.words)} />
      <Tile label={t("insights.sessions")} value={formatCount(snap.today.sessions)} />
      <Tile
        label={t("insights.recording")}
        value={formatDuration(snap.today.recordingMs)}
      />
      <Tile
        label={t("insights.timeSaved")}
        value={formatDuration(snap.today.timeSavedMs)}
        hint="vs typing @ 40 wpm"
      />
      <Tile
        label={t("insights.streakDays")}
        value={String(snap.streakDays)}
        accent="streak"
      />
    </div>
  );
}

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

/* ------------------------------------------------------------------ */
/* Sparkline — tiny canvas, no chart library.                           */
/* Why canvas + not SVG: we get device-pixel-ratio scaling for free     */
/* via setTransform, and a 7-point bar chart isn't worth a chart dep.   */
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

    // Bar color sampled from CSS custom property so theme switches
    // apply without a re-render — we read on each effect run.
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
      // Rounded-top bars look softer; 3px radius is small enough not
      // to need a polyfill on Firefox.
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

  // 7 ticks: -6d, -5d, ... today. Two visible (oldest / today) keep
  // it visually tidy.
  return (
    <div className={styles.sparkWrap}>
      <canvas
        ref={ref}
        className={styles.sparkCanvas}
        // Accessible alt: list the 7 values.
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
/* Mode mix — segmented bar + legend                                    */
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
/* Top apps                                                             */
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
/* Latency                                                              */
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
/* Learning loop                                                        */
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
