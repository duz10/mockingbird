// Tiny, dependency-light formatters. Anything date-fns can't do in
// one call gets a helper here.

import { formatDistanceToNowStrict, format as formatDate } from "date-fns";

/** "2h 14m" / "47s" — for a short duration display. */
export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return "—";
  const totalSec = Math.round(ms / 1000);
  const h = Math.floor(totalSec / 3600);
  const m = Math.floor((totalSec % 3600) / 60);
  const s = totalSec % 60;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

/** "12:34" — for the recording-window stopwatch. */
export function formatStopwatch(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "0:00";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/** "1,842" — locale-aware thousands separator. */
export function formatCount(n: number): string {
  return new Intl.NumberFormat().format(Math.round(n));
}

/** "9:14 AM" or "Yesterday at 11:02 PM" — depending on recency. */
export function formatTimestamp(iso: string): string {
  const date = new Date(iso);
  const now = new Date();
  const sameDay =
    date.getFullYear() === now.getFullYear() &&
    date.getMonth() === now.getMonth() &&
    date.getDate() === now.getDate();
  if (sameDay) return formatDate(date, "h:mm a");
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  const isYesterday =
    date.getFullYear() === yesterday.getFullYear() &&
    date.getMonth() === yesterday.getMonth() &&
    date.getDate() === yesterday.getDate();
  if (isYesterday) return `Yesterday at ${formatDate(date, "h:mm a")}`;
  const sameYear = date.getFullYear() === now.getFullYear();
  return formatDate(date, sameYear ? "MMM d, h:mm a" : "MMM d, yyyy");
}

/** "3 minutes ago" — for live-updating relative timestamps. */
export function formatRelative(iso: string): string {
  try {
    return formatDistanceToNowStrict(new Date(iso), { addSuffix: true });
  } catch {
    return "";
  }
}

/** "slack.exe" → "Slack". Best-effort prettifier for the foreground-app field. */
export function prettyAppName(app: string | null | undefined): string {
  if (!app) return "Unknown";
  const stem = app.replace(/\.exe$/i, "");
  // Title-case first character to handle "code", "slack", etc.
  return stem.charAt(0).toUpperCase() + stem.slice(1);
}

/** Truncate to `n` chars with a trailing ellipsis when needed. */
export function truncate(s: string, n: number): string {
  if (s.length <= n) return s;
  return s.slice(0, Math.max(0, n - 1)).trimEnd() + "…";
}

/** Count whitespace-separated words. */
export function countWords(s: string): string {
  // Returns a string so we don't pretend we have semantic-word
  // segmentation. Callers that need a number should use the numeric
  // sibling below.
  return formatCount(numericWordCount(s));
}

export function numericWordCount(s: string): number {
  const trimmed = s.trim();
  if (!trimmed) return 0;
  return trimmed.split(/\s+/).length;
}
