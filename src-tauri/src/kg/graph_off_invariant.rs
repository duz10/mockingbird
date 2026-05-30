//! Graph-off invariant probe — ADR 0050 §D8 gate 2 (`kg-graph-off-untouched`).
//!
//! Deterministic, non-LLM-graded judge that asserts the **principal**
//! ADR 0050 invariant:
//!
//! > With `SettingKey::KgGraphEnabled = false` (the default), the
//! > dictation-tail KG hook MUST NOT write to any `kg_*` table for
//! > ANY `InjectionOutcome` variant, AND the hook MUST NOT return an
//! > error to the caller (ignore-error semantics, ADR 0050
//! > `kg-graph-failure-non-regressing`).
//!
//! Why deterministic and not LLM-graded: AGENTS.md §"Judges — when"
//! authorizes a single one-off judge for a narrow invariant. The
//! property is binary (row count == 0 across five tables × eight
//! variants); no language model can grade it better than
//! `SELECT COUNT(*)`. LLM judges add noise + nondeterminism without
//! buying anything for an invariant of this shape. The Phase MC
//! `mc-dictation-untouched` precedent for this exact shape was also
//! deterministic.
//!
//! Why a binary probe and not a `#[test]`: LESSONS PINNED P2 —
//! `cargo test --release` exits with `STATUS_ENTRYPOINT_NOT_FOUND`
//! on this box. Binary probes sidestep the test-runner bug entirely
//! (same reason the parity probe is structured this way).
//!
//! The probe runs a positive-control flip at the end (enable graph,
//! re-fire one Ok outcome, assert the queue does receive a row) so
//! that a false negative (assertion vacuously passing because the
//! helper is no-op against an unreachable code path) is caught.
//!
//! Charter: ADR 0050 §"Invariants" + §D8 gate 2 + the Chunk 5
//! kickoff (`mb-k17a`).

use rusqlite::{params, Connection};
use tempfile::NamedTempFile;

use crate::db::migrations::apply_all;
use crate::dictation::try_enqueue_for_kg_filing;
use crate::error::{AppError, AppResult};
use crate::injection::InjectionOutcome;
use crate::settings::model::SettingKey;
use crate::settings::Settings;

const PROBE_CAPTURED_ISO: &str = "2026-06-03T08:00:00Z";

/// The eight `InjectionOutcome` variants — every one must be a
/// no-op against the `kg_*` tables when the graph is off. Listed
/// explicitly here (rather than reflected) so the exhaustiveness is
/// load-bearing: if a new variant is added to `InjectionOutcome` and
/// not also added here, the array length stops at 8 and the gate
/// stops covering the new variant — the author has to think about
/// whether it should enqueue.
const ALL_OUTCOMES: [InjectionOutcome; 8] = [
    InjectionOutcome::Ok,
    InjectionOutcome::OkClipboardNotRestored,
    InjectionOutcome::AbortedSecure,
    InjectionOutcome::AbortedUserOptOut,
    InjectionOutcome::AbortedFocusChanged,
    InjectionOutcome::FailedClipboardLocked,
    InjectionOutcome::FailedSendInput,
    InjectionOutcome::InAppNoInject,
];

/// The five `kg_*` tables whose row counts must remain `0` for every
/// off-mode outcome variant. `kg_canonical_tags` is the actual
/// table name (the kickoff's "kg_tags" was shorthand for it).
const KG_TABLES: [&str; 5] = [
    "kg_filing_queue",
    "kg_entity_mentions",
    "kg_tag_mentions",
    "kg_entities",
    "kg_canonical_tags",
];

/// Run the graph-off invariant probe. Returns the process exit code:
/// `0` on green (all eight variants leave every `kg_*` table empty +
/// the positive-control flip succeeds), `1` on any assertion failure.
pub fn run_graph_off_invariant_probe() -> i32 {
    println!("🐶 KG graph-off invariant probe — ADR 0050 §D8 gate 2 (kg-graph-off-untouched)");

    match run_inner() {
        Ok(()) => {
            println!();
            println!(
                "✅ GRAPH-OFF INVARIANT GREEN: all 8 InjectionOutcome variants left every kg_* table empty + positive control flip filed correctly"
            );
            0
        }
        Err(e) => {
            eprintln!();
            eprintln!("❌ GRAPH-OFF INVARIANT FAILED");
            eprintln!("    reason: {e}");
            1
        }
    }
}

fn run_inner() -> AppResult<()> {
    let tmpfile = NamedTempFile::new().map_err(|e| io_err(&format!("tempfile: {e}")))?;
    let conn =
        Connection::open(tmpfile.path()).map_err(|e| io_err(&format!("open tempfile DB: {e}")))?;

    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| io_err(&format!("enable FKs: {e}")))?;
    let fk_on: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .map_err(|e| io_err(&format!("read fk pragma: {e}")))?;
    if fk_on != 1 {
        return Err(io_err(&format!(
            "PRAGMA foreign_keys returned {fk_on} (wanted 1)"
        )));
    }
    apply_all(&conn)?;

    // The migration 024 seed must have planted `kg_graph_enabled = false`
    // as the durable default. We verify the SEED row directly (not via
    // the Settings facade) so a missing seed surfaces here instead of
    // being papered over by `default_value()`.
    let seeded: String = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'kg_graph_enabled'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| {
            io_err(&format!(
                "migration 024 did not seed kg_graph_enabled row: {e}"
            ))
        })?;
    if seeded != "false" {
        return Err(io_err(&format!(
            "migration 024 seed for kg_graph_enabled = `{seeded}` (wanted `false`)"
        )));
    }
    let settings = Settings::new(&conn);
    let read_back: bool = settings.get(SettingKey::KgGraphEnabled)?;
    if read_back {
        return Err(io_err(
            "Settings facade reads KgGraphEnabled = true despite seed (default-off contract broken)",
        ));
    }
    println!("  ✓ migration 024 seeded kg_graph_enabled = false");

    // Insert ONE sessions row — we'll reuse session_id = 1 across all
    // eight off-mode variants. The hook's outcome gate doesn't care
    // about uniqueness per-call (UNIQUE(entry_id) on kg_filing_queue
    // would collapse any duplicates anyway), and the assertion is on
    // ROW COUNTS, so reusing one row keeps the test data minimal.
    conn.execute(
        "INSERT INTO sessions (id, uuid, mode_id, hotkey_pressed, started_at,
            recording_ended_at, status, audio_duration_ms)
         VALUES (1, 'graph-off-invariant', 1, 'gate', ?1, ?1, 'persist_probe', 0)",
        params![PROBE_CAPTURED_ISO],
    )
    .map_err(|e| io_err(&format!("insert sessions row: {e}")))?;

    // ── Off-mode sweep ───────────────────────────────────────────
    println!(
        "  → sweeping {} InjectionOutcome variants × KgGraphEnabled=false",
        ALL_OUTCOMES.len()
    );
    for outcome in ALL_OUTCOMES {
        // The helper is ignore-error by design; if it ever returned
        // an error directly to the caller, that would be the
        // kg-graph-failure-non-regressing breach. Since the
        // signature is `-> ()`, we instead assert (a) the call
        // doesn't panic (it returns) and (b) row counts stay zero.
        try_enqueue_for_kg_filing(&conn, 1, outcome, PROBE_CAPTURED_ISO);

        for table in KG_TABLES {
            let count = row_count(&conn, table)?;
            if count != 0 {
                return Err(io_err(&format!(
                    "graph-off breach: outcome {outcome:?} produced {count} rows in {table} (wanted 0)"
                )));
            }
        }
        println!("    ✓ {outcome:?}: all 5 kg_* tables empty");
    }

    // ── Positive control ─────────────────────────────────────────
    //
    // Flip the toggle ON and re-fire ONE `Ok` outcome. The queue
    // must now have exactly one pending row — confirming the
    // upstream assertion isn't vacuous (i.e. the helper IS
    // structurally able to write; it just doesn't when the toggle
    // is off). Without this control, a regression that broke the
    // helper entirely (e.g. early-return before the settings read)
    // would still pass the off-mode sweep.
    settings.set(SettingKey::KgGraphEnabled, &true)?;
    let read_back: bool = settings.get(SettingKey::KgGraphEnabled)?;
    if !read_back {
        return Err(io_err(
            "Settings facade failed to flip KgGraphEnabled to true (positive control setup)",
        ));
    }

    try_enqueue_for_kg_filing(&conn, 1, InjectionOutcome::Ok, PROBE_CAPTURED_ISO);

    let queue_count = row_count(&conn, "kg_filing_queue")?;
    if queue_count != 1 {
        return Err(io_err(&format!(
            "positive control failed: KgGraphEnabled=true + Ok outcome produced {queue_count} kg_filing_queue rows (wanted 1)"
        )));
    }
    // Mention/entity tables still 0 — the helper only ENQUEUES;
    // the worker materializes mentions on drain. We're asserting
    // the hook's contract, not the worker's.
    for table in ["kg_entity_mentions", "kg_tag_mentions", "kg_entities"] {
        let c = row_count(&conn, table)?;
        if c != 0 {
            return Err(io_err(&format!(
                "positive control: {table} should be 0 (worker hasn't run), got {c}"
            )));
        }
    }
    println!("  ✓ positive control: KgGraphEnabled=true + Ok enqueued 1 row");

    Ok(())
}

fn row_count(conn: &Connection, table: &str) -> AppResult<i64> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    let n: i64 = conn
        .query_row(&sql, [], |r| r.get(0))
        .map_err(|e| io_err(&format!("count {table}: {e}")))?;
    Ok(n)
}

fn io_err(msg: &str) -> AppError {
    AppError::Other(msg.to_string())
}
