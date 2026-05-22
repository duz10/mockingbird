// Phase 10 Wave 5 — Settings UI for the activity-capture hardening
// surface (ADR 0042 retention, ADR 0043 exclusion rules, ADR 0044
// PDF export — though the PDF export is invoked from ActivityBlocks,
// not here).
//
// Split into two visually-grouped sub-sections:
//
//   1. Retention TTLs + on-demand sweep button.
//   2. Exclusion rules list (toggle / delete / add).
//
// Both subsystems are opt-in (TTLs default to 0 = forever; the
// built-in exclusion rules are pre-seeded but the user can disable
// them). The row is rendered inside the General tab of Settings,
// after the existing Activity audio toggle.

import { useEffect, useState } from "react";

import { activityApi } from "../lib/activity";
import type {
  ExclusionRule,
  ExclusionRuleKind,
  RetentionPolicy,
  RetentionSweepResult,
} from "../lib/activity";

import styles from "./Settings.module.css";

const KINDS: ExclusionRuleKind[] = ["app_glob", "title_regex", "system"];

/** Wave 5 hardening row. Two stacked panels: retention + exclusion. */
export function SettingsActivityHardeningRow() {
  return (
    <>
      <RetentionPanel />
      <ExclusionPanel />
    </>
  );
}

// ---------------------------------------------------------------------------
// Retention panel
// ---------------------------------------------------------------------------

function RetentionPanel() {
  const [policy, setPolicy] = useState<RetentionPolicy | null>(null);
  const [events, setEvents] = useState<string>("0");
  const [segments, setSegments] = useState<string>("0");
  const [blocks, setBlocks] = useState<string>("0");
  const [saving, setSaving] = useState<boolean>(false);
  const [sweepResult, setSweepResult] = useState<RetentionSweepResult | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const p = await activityApi.retention.get();
        if (cancelled) return;
        setPolicy(p);
        setEvents(String(p.eventsDays));
        setSegments(String(p.segmentsDays));
        setBlocks(String(p.blocksDays));
      } catch (err) {
        if (!cancelled) setError(String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const onSave = async () => {
    setSaving(true);
    setError(null);
    try {
      await activityApi.retention.set(
        clampInt(events),
        clampInt(segments),
        clampInt(blocks),
      );
      const p = await activityApi.retention.get();
      setPolicy(p);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  const onSweepNow = async () => {
    setSaving(true);
    setError(null);
    try {
      const r = await activityApi.retention.sweepNow();
      setSweepResult(r);
      const p = await activityApi.retention.get();
      setPolicy(p);
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className={styles.row}>
      <div className={styles.rowMain}>
        <span className={styles.rowLabel}>Activity retention</span>
        <span className={styles.rowHelp}>
          How long to keep activity capture data. <strong>0 = forever</strong>.
          Defaults to forever (privacy by default; you opt in to purges).
          The sweep runs daily after boot and on demand. Block summaries
          survive their underlying events being purged — they get a
          "raw events purged" annotation in the UI.
        </span>
        <div className={styles.retentionGrid}>
          <RetentionInput
            label="Raw events (days)"
            value={events}
            onChange={setEvents}
          />
          <RetentionInput
            label="Audio transcript segments (days)"
            value={segments}
            onChange={setSegments}
          />
          <RetentionInput
            label="Block summaries (days)"
            value={blocks}
            onChange={setBlocks}
          />
        </div>
        {policy && policy.lastSweepMs > 0 && (
          <span className={styles.rowHelp}>
            Last sweep: {new Date(policy.lastSweepMs).toLocaleString()}
          </span>
        )}
        {sweepResult && (
          <span className={styles.rowHelp}>
            Sweep removed {sweepResult.eventsDeleted} events,{" "}
            {sweepResult.segmentsDeleted} segments, {sweepResult.blocksDeleted}{" "}
            blocks; marked {sweepResult.blocksMarkedPurged} blocks as
            raw-purged.
          </span>
        )}
        {error && (
          <span className={styles.rowHelp} role="alert">
            {error}
          </span>
        )}
      </div>
      <div className={styles.rowControl}>
        <button type="button" disabled={saving} onClick={() => void onSave()}>
          Save
        </button>
        <button
          type="button"
          disabled={saving}
          onClick={() => void onSweepNow()}
        >
          Sweep now
        </button>
      </div>
    </div>
  );
}

function RetentionInput(props: {
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <label className={styles.retentionLabel}>
      <span>{props.label}</span>
      <input
        type="number"
        min="0"
        step="1"
        value={props.value}
        onChange={(e) => props.onChange(e.target.value)}
      />
    </label>
  );
}

function clampInt(s: string): number {
  const n = Number.parseInt(s, 10);
  if (!Number.isFinite(n) || n < 0) return 0;
  return n;
}

// ---------------------------------------------------------------------------
// Exclusion panel
// ---------------------------------------------------------------------------

function ExclusionPanel() {
  const [rules, setRules] = useState<ExclusionRule[]>([]);
  const [loading, setLoading] = useState<boolean>(true);
  const [error, setError] = useState<string | null>(null);
  const [showAdd, setShowAdd] = useState<boolean>(false);

  const reload = async () => {
    setLoading(true);
    setError(null);
    try {
      const r = await activityApi.exclusion.list();
      setRules(r);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  const onToggle = async (id: string, enabled: boolean) => {
    try {
      await activityApi.exclusion.setEnabled(id, enabled);
      await reload();
    } catch (err) {
      setError(String(err));
    }
  };

  const onDelete = async (id: string) => {
    try {
      await activityApi.exclusion.delete(id);
      await reload();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <div className={styles.row}>
      <div className={styles.rowMain}>
        <span className={styles.rowLabel}>Activity exclusion rules</span>
        <span className={styles.rowHelp}>
          Rules that drop matching events <em>at capture</em> — the data
          never touches disk. Built-in rules can be disabled but not
          deleted; you can add your own. Matching is case-insensitive.
        </span>
        {error && (
          <span className={styles.rowHelp} role="alert">
            {error}
          </span>
        )}
        {loading && <span className={styles.rowHelp}>Loading…</span>}
        {!loading && (
          <ul className={styles.exclusionList}>
            {rules.map((r) => (
              <li key={r.id} className={styles.exclusionItem}>
                <label className={styles.exclusionToggle}>
                  <input
                    type="checkbox"
                    checked={r.enabled}
                    onChange={(e) =>
                      void onToggle(r.id, e.currentTarget.checked)
                    }
                  />
                </label>
                <div className={styles.exclusionMeta}>
                  <code>
                    [{r.kind}] {r.pattern}
                  </code>
                  {r.note && <small>{r.note}</small>}
                  {r.isBuiltin && <small>🔒 built-in</small>}
                </div>
                {!r.isBuiltin && (
                  <button
                    type="button"
                    onClick={() => void onDelete(r.id)}
                    aria-label={`Delete rule ${r.id}`}
                  >
                    Delete
                  </button>
                )}
              </li>
            ))}
          </ul>
        )}
      </div>
      <div className={styles.rowControl}>
        <button type="button" onClick={() => setShowAdd((s) => !s)}>
          {showAdd ? "Cancel" : "Add rule"}
        </button>
      </div>
      {showAdd && (
        <AddExclusionForm
          onSaved={async () => {
            setShowAdd(false);
            await reload();
          }}
          onError={(e) => setError(e)}
        />
      )}
    </div>
  );
}

function AddExclusionForm(props: {
  onSaved: () => Promise<void>;
  onError: (msg: string) => void;
}) {
  const [kind, setKind] = useState<ExclusionRuleKind>("app_glob");
  const [pattern, setPattern] = useState<string>("");
  const [note, setNote] = useState<string>("");
  const [saving, setSaving] = useState<boolean>(false);

  const onSubmit = async () => {
    if (!pattern.trim()) {
      props.onError("pattern cannot be empty");
      return;
    }
    setSaving(true);
    try {
      await activityApi.exclusion.validate(kind, pattern);
      await activityApi.exclusion.upsert(
        null,
        kind,
        pattern,
        true,
        note.trim() || null,
      );
      await props.onSaved();
    } catch (err) {
      props.onError(String(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className={styles.exclusionAddForm}>
      <label>
        Kind
        <select
          value={kind}
          onChange={(e) => setKind(e.currentTarget.value as ExclusionRuleKind)}
        >
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {k}
            </option>
          ))}
        </select>
      </label>
      <label>
        Pattern
        <input
          type="text"
          value={pattern}
          onChange={(e) => setPattern(e.currentTarget.value)}
          placeholder={
            kind === "app_glob"
              ? "1Password*"
              : kind === "title_regex"
                ? "(?i)\\b(bank|login)\\b"
                : "password_field_active"
          }
        />
      </label>
      <label>
        Note (optional)
        <input
          type="text"
          value={note}
          onChange={(e) => setNote(e.currentTarget.value)}
        />
      </label>
      <button type="button" disabled={saving} onClick={() => void onSubmit()}>
        Save
      </button>
    </div>
  );
}
