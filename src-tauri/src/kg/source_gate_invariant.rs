//! Source-gate invariant probe — ADR 0052 §"Acceptance gates" J1
//! (`kg-source-gate-invariant`); Wave 1D.6 (`mb-q2p1`).
//!
//! Deterministic, non-LLM-graded judge that asserts the **principal**
//! ADR 0052 invariant:
//!
//! > The KG filing queue receives exactly the rows that originate
//! > from a KG capture surface (`capture_kind IN ('kg-note',
//! > 'kg-note-text')`) AND are filed while the global
//! > `KgGraphEnabled` toggle is on. Every other combination of
//! > capture kind × toggle state MUST result in an empty queue
//! > contribution from that combination.
//!
//! This is the "trigger-direction" invariant — the drift Phase 1D
//! corrects (per ADR 0052 §"Context" Drift 1). The sibling
//! [`crate::kg::graph_off_invariant`] probe already covers the
//! audio-path source-gate at all eight [`InjectionOutcome`] variants;
//! this probe extends coverage to the **text-note ingest path**
//! ([`crate::kg::ingest_text::ingest_text_note`]), which is a
//! distinct entry point (no dictation pipeline, no
//! [`InjectionOutcome`], its own toggle-only gate). The two probes
//! together prove the cross-path invariant.
//!
//! ## Why a separate probe (and not an extension of `graph_off_invariant`)
//!
//! The two probes share a deterministic posture but exercise
//! different surfaces:
//!
//! | Probe | Entry points | Matrix |
//! |---|---|---|
//! | `kg_graph_off_invariant` | `try_enqueue_for_kg_filing` only | 8 outcomes × 2 capture_kinds × off, + source-gate negative control + positive control |
//! | `kg_source_gate_invariant` (this) | `try_enqueue_for_kg_filing` AND `ingest_text_note` | 3 capture_kinds × 2 toggle states, full corpus |
//!
//! Conflating them would force `graph_off_invariant` to stand up
//! the text-note path's `SessionsEventBus` + `OrchestratorConfig`
//! dependencies and would muddy the off-mode-only assertion the
//! original probe was authored for. Sibling probes per ADR 0050 §D8
//! gate 2 / ADR 0052 §"Acceptance gates" J1 keep each contract
//! readable in isolation.
//!
//! ## Why deterministic and not LLM-graded
//!
//! AGENTS.md §"Judges — when" authorizes a single one-off judge for
//! a narrow invariant. The property is binary (queue row counts per
//! corpus cell); a language model cannot grade it better than
//! `SELECT COUNT(*) WHERE capture_kind = ?`. Same precedent as
//! `mc-dictation-untouched` (Phase MC) and `kg_graph_off_invariant`
//! (Phase 1B).
//!
//! ## Why a binary probe and not a `#[test]`
//!
//! LESSONS PINNED P2 — `cargo test --release` exits with
//! `STATUS_ENTRYPOINT_NOT_FOUND` on this box. Binary probes sidestep
//! the test-runner bug entirely (same reason
//! `kg_graph_off_invariant` and `kg_parity --persist` are structured
//! this way).

use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use tempfile::NamedTempFile;

use crate::db::migrations::apply_all;
use crate::db::sessions::CaptureKind;
use crate::dictation::events::SessionsEventBus;
use crate::dictation::runtime::bootstrap_provenance_rows;
use crate::dictation::{try_enqueue_for_kg_filing, OrchestratorConfig};
use crate::error::{AppError, AppResult};
use crate::injection::InjectionOutcome;
use crate::kg::ingest_text::ingest_text_note;
use crate::settings::model::SettingKey;
use crate::settings::Settings;

const PROBE_CAPTURED_ISO: &str = "2026-06-04T08:00:00Z";

/// A no-op [`SessionsEventBus`] for the text-note path. The probe
/// does not assert on the React refetch event — only on the SQLite
/// row counts — so a swallowing impl matches the contract without
/// adding a dependency on the real `RecordingWindow`.
struct NoopBus;

impl SessionsEventBus for NoopBus {
    fn emit_session_saved(&self, _session_id: i64) {
        // Deliberate no-op. The contract under test is queue
        // contents; the event bus is best-effort by design (see
        // `SessionsEventBus` docs) so swallowing here matches the
        // production "UI hiccup must not block ingest" invariant.
    }
}

/// One cell of the 3×2 corpus matrix. `expected_enqueues` is the
/// number of `kg_filing_queue` rows this cell should contribute
/// when its entry point is driven once.
struct CorpusCell {
    label: &'static str,
    capture_kind: CaptureKind,
    toggle_on: bool,
    expected_enqueues: i64,
}

const CORPUS: [CorpusCell; 6] = [
    // Standard dictation -- the drift Phase 1D corrects. Must NEVER
    // enqueue, regardless of toggle state.
    CorpusCell {
        label: "dictation + toggle off",
        capture_kind: CaptureKind::Dictation,
        toggle_on: false,
        expected_enqueues: 0,
    },
    CorpusCell {
        label: "dictation + toggle on",
        capture_kind: CaptureKind::Dictation,
        toggle_on: true,
        expected_enqueues: 0,
    },
    // KG-screen audio note -- the dictation-tail source-gate is the
    // discriminator. Enqueues only when toggle is on.
    CorpusCell {
        label: "kg-note (audio) + toggle off",
        capture_kind: CaptureKind::KgNote,
        toggle_on: false,
        expected_enqueues: 0,
    },
    CorpusCell {
        label: "kg-note (audio) + toggle on",
        capture_kind: CaptureKind::KgNote,
        toggle_on: true,
        expected_enqueues: 1,
    },
    // KG-screen text note -- bypasses the dictation tail entirely;
    // the text-note ingest function has its own toggle-only gate.
    // Source is implicit (the function is only called from the
    // KG-screen text input IPC), so it only checks the toggle.
    CorpusCell {
        label: "kg-note-text + toggle off",
        capture_kind: CaptureKind::KgNoteText,
        toggle_on: false,
        expected_enqueues: 0,
    },
    CorpusCell {
        label: "kg-note-text + toggle on",
        capture_kind: CaptureKind::KgNoteText,
        toggle_on: true,
        expected_enqueues: 1,
    },
];

/// Run the source-gate invariant probe. Returns the process exit
/// code: `0` on green (every corpus cell matches its expected
/// enqueue count), `1` on any breach.
pub fn run_source_gate_invariant_probe() -> i32 {
    println!(
        "🐶 KG source-gate invariant probe — ADR 0052 §\"Acceptance gates\" J1 (kg-source-gate-invariant)"
    );

    match run_inner() {
        Ok(()) => {
            println!();
            println!(
                "✅ SOURCE-GATE INVARIANT GREEN: all 6 corpus cells (3 capture_kinds × 2 toggle states) match expected enqueues; only kg-note + on AND kg-note-text + on produced queue rows"
            );
            0
        }
        Err(e) => {
            eprintln!();
            eprintln!("❌ SOURCE-GATE INVARIANT FAILED");
            eprintln!("    reason: {e}");
            1
        }
    }
}

fn run_inner() -> AppResult<()> {
    // Each cell drives a *fresh* DB so that cells cannot leak
    // queue rows into each other (and so the per-cell assertion is
    // unambiguous about which cell would have caused a breach).
    // This is more expensive than a single shared DB but matches
    // the "one assertion per cell" diagnostic shape the kickoff
    // wants when a regression lands.
    let mut total_enqueues_observed = 0i64;
    for cell in &CORPUS {
        let observed = drive_cell(cell)?;
        if observed != cell.expected_enqueues {
            return Err(io_err(&format!(
                "corpus cell `{}` produced {} kg_filing_queue rows (expected {})",
                cell.label, observed, cell.expected_enqueues
            )));
        }
        total_enqueues_observed += observed;
        println!(
            "  ✓ {} → {} queue row(s) (expected {})",
            cell.label, observed, cell.expected_enqueues
        );
    }

    // Defense in depth: total across all cells must equal the sum
    // of expected enqueues. Catches a regression where a cell's
    // observed count masks another cell's miscount via arithmetic
    // coincidence (theoretically impossible with per-cell fresh
    // DBs, but the assertion is cheap and the failure-mode story
    // it tells is unambiguous).
    let expected_total: i64 = CORPUS.iter().map(|c| c.expected_enqueues).sum();
    if total_enqueues_observed != expected_total {
        return Err(io_err(&format!(
            "corpus total mismatch: observed {total_enqueues_observed}, expected {expected_total}"
        )));
    }
    println!(
        "  ✓ corpus total: {total_enqueues_observed} queue row(s) across {} cells (expected {expected_total})",
        CORPUS.len()
    );

    Ok(())
}

/// Drive one corpus cell against a freshly migrated tempfile DB.
/// Returns the observed `kg_filing_queue` row count.
fn drive_cell(cell: &CorpusCell) -> AppResult<i64> {
    let tmpfile = NamedTempFile::new().map_err(|e| io_err(&format!("tempfile: {e}")))?;
    let conn =
        Connection::open(tmpfile.path()).map_err(|e| io_err(&format!("open tempfile DB: {e}")))?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| io_err(&format!("enable FKs: {e}")))?;
    apply_all(&conn)?;

    // The migration 025 seed defaults `kg_graph_enabled = 'false'`;
    // flip only when the cell needs it.
    if cell.toggle_on {
        Settings::new(&conn).set(SettingKey::KgGraphEnabled, &true)?;
    }

    match cell.capture_kind {
        CaptureKind::Dictation | CaptureKind::KgNote => {
            drive_dictation_tail(&conn, cell.capture_kind)?;
        }
        CaptureKind::KgNoteText => {
            drive_text_note_ingest(conn)?;
            // `conn` was consumed by `drive_text_note_ingest` so we
            // re-open the same tempfile DB to inspect the queue.
            let conn2 = Connection::open(tmpfile.path())
                .map_err(|e| io_err(&format!("re-open tempfile DB: {e}")))?;
            return row_count(&conn2, "kg_filing_queue");
        }
    }
    row_count(&conn, "kg_filing_queue")
}

/// Drive the dictation-tail path: seed one sessions row, fire the
/// helper with `InjectionOutcome::Ok` (the canonical "successful
/// dictation" outcome that the outcome-gate accepts).
fn drive_dictation_tail(conn: &Connection, capture_kind: CaptureKind) -> AppResult<()> {
    conn.execute(
        "INSERT INTO sessions (id, uuid, mode_id, hotkey_pressed, started_at,
            recording_ended_at, status, audio_duration_ms)
         VALUES (1, 'source-gate-probe', 1, 'gate', ?1, ?1, 'persist_probe', 0)",
        params![PROBE_CAPTURED_ISO],
    )
    .map_err(|e| io_err(&format!("insert sessions row: {e}")))?;

    try_enqueue_for_kg_filing(
        conn,
        1,
        InjectionOutcome::Ok,
        capture_kind,
        PROBE_CAPTURED_ISO,
    );
    Ok(())
}

/// Drive the text-note ingest path: bootstrap provenance rows + a
/// minimal `OrchestratorConfig`, then fire one `ingest_text_note`
/// call. The function takes ownership of the `Connection` (wrapped
/// in `Arc<Mutex<_>>`) so the caller re-opens the tempfile DB to
/// assert.
fn drive_text_note_ingest(conn: Connection) -> AppResult<()> {
    let (dict_id, example_id) = bootstrap_provenance_rows(&conn)?;
    let (mode_id, slug, prompt_id) = {
        let (m, s): (i64, String) = conn
            .query_row("SELECT id, slug FROM modes ORDER BY id LIMIT 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(|e| io_err(&format!("lookup default mode: {e}")))?;
        let p: i64 = conn
            .query_row(
                "SELECT prompt_id FROM modes WHERE id = ?1",
                params![m],
                |r| r.get(0),
            )
            .map_err(|e| io_err(&format!("lookup default prompt_id: {e}")))?;
        (m, s, p)
    };
    let cfg = OrchestratorConfig {
        mode_id,
        mode_slug: slug,
        prompt_id,
        dictionary_snapshot_id: dict_id,
        example_set_id: example_id,
        hotkey_label: "source-gate-probe".into(),
    };

    let db = Arc::new(Mutex::new(conn));
    let bus = NoopBus;
    let _session_id = ingest_text_note(&db, &bus, &cfg, "source-gate probe text note")?;
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
