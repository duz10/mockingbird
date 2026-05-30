//! KG filing-pipeline latency bench — Phase 1C.0 (`mb-plz9`, ADR 0051).
//!
//! Drives [`run_pipeline`] + [`apply_filed_outcome`] against a real
//! local Ollama daemon over a small fixed set of representative
//! parity-fixture dictations, and prints per-fixture wall-clock
//! timings as CSV to stdout (plus a summary block). Consumed by
//! `src/bin/kg_latency_bench.rs`. Discharges `mb-b3jy` (empirical
//! latency budget measurement for ADR 0049 §6 "~1 min per dictation"
//! target).
//!
//! ## Why a bench and not a `#[test]`?
//!
//! Same reason `kg_parity` is a bin: LESSONS PINNED P2 — `cargo test
//! --release` exits with `STATUS_ENTRYPOINT_NOT_FOUND` on this box.
//! Living in a `[[bin]]` sidesteps the test-runner bug AND the bench
//! is a one-shot measurement tool that legitimately doesn't belong
//! in the regression test loop (it costs ~minutes and needs an
//! Ollama daemon).
//!
//! ## Why call `run_pipeline` directly instead of the worker?
//!
//! The worker reads the dictation text via `transcripts.final` and
//! pulls `started_at` from the `sessions` row — DB indirection that
//! doesn't change what we're measuring (the 5-pass pipeline + the
//! store apply). Bypassing the worker means the bench needs:
//!
//! - a `sessions` row (FK target for `apply_filed_outcome`'s mention rows),
//!
//! and that's it — no transcripts row, no enqueue/dequeue round-trip,
//! no worker thread. Keeps the bench focused on pipeline + store ms.
//!
//! ## Output format
//!
//! CSV to stdout. Header row first; one row per fixture:
//!
//! ```text
//! fixture_id,segment_count,segment_ms,classify_ms_total,extract_ms_total,extract_entities_ms_total,normalize_ms_total,store_apply_ms,total_pipeline_ms
//! persona-03-case-01,1,1234,5678,...,...
//! ...
//! # summary
//! # mean_total_pipeline_ms = ...
//! # p50_total_pipeline_ms  = ...
//! # p95_total_pipeline_ms  = ...
//! # max_total_pipeline_ms  = ...
//! ```
//!
//! ## Graceful degradation
//!
//! Reaching out to a missing/down Ollama daemon prints a single
//! `# ollama-unreachable` line to stdout, a friendly hint to stderr,
//! and exits 2. The instrumentation half of the wave (per-pass
//! `PassTimings` + structured worker tracing) is still on disk; only
//! the empirical numbers are pending.

use std::path::PathBuf;
use std::time::Instant;

use rusqlite::{params, Connection};
use serde::Deserialize;
use tempfile::NamedTempFile;

use super::ollama::{GenerateOptions, OllamaClient, OllamaDispatcher, OllamaError};
use super::pipeline::{run_pipeline, PassTimings};
use super::schema_loader::Schema;
use super::store::apply_filed_outcome;
use super::worker::build_segment_outputs;
use crate::db::migrations::apply_all;

/// Model the production worker files against (mirrors
/// `kg::worker::DEFAULT_FILING_MODEL`). Kept as a local const so the
/// bench is honest about what it's measuring — swap-the-model studies
/// just edit this line.
const BENCH_MODEL: &str = "qwen2.5:7b-instruct-q4_K_M";

/// Deterministic timestamp; matches the parity probe so fixtures
/// stay comparable across runs.
const BENCH_CAPTURED_ISO: &str = "2026-06-14T08:00:00Z";

/// Fixture file path resolved relative to `CARGO_MANIFEST_DIR`
/// (= `src-tauri/`).
const FIXTURE_REL: &str = "../docs/knowledge-graph/parity/wave-0.5.4-seed-42.json";

/// Five representative fixtures of increasing segment count, chosen
/// from the Wave 0.5.4 seed-42 set (see `docs/knowledge-graph/parity/`
/// for the full inventory). The choice covers the 1..=5 segment-count
/// range available in the fixture set (no 7-segment dictation exists
/// — `persona-05-case-03` at 5 segments is the longest available).
const BENCH_FIXTURE_IDS: &[&str] = &[
    "persona-03-case-01", // 1 segment, 132 chars
    "persona-01-case-01", // 2 segments, 204 chars
    "persona-04-case-02", // 3 segments, 294 chars
    "persona-05-case-02", // 4 segments, 467 chars
    "persona-05-case-03", // 5 segments, 605 chars (longest)
];

#[derive(Debug, Deserialize)]
struct Fixture {
    dictations: Vec<FixtureDictation>,
}

/// Minimal projection of the parity fixture — only the two fields the
/// bench needs. Every other key in the fixture file is ignored by
/// `serde_json` per the default deny-unknown-fields-off behavior.
#[derive(Debug, Deserialize)]
struct FixtureDictation {
    dictation_id: String,
    dictation_text: String,
}

/// One row of the bench's CSV output.
struct BenchRow {
    fixture_id: String,
    segment_count: usize,
    timings: PassTimings,
    store_apply_ms: u64,
    total_pipeline_ms: u64,
}

/// Entry point used by the `[[bin]] kg_latency_bench` shim. Returns
/// a process exit code:
///
/// - `0` — all fixtures ran cleanly; CSV + summary on stdout.
/// - `2` — Ollama unreachable; partial output on stdout, hint on stderr.
/// - `1` — any other failure (fixture parse, schema load, etc.).
pub fn run_latency_bench() -> i32 {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir.join(FIXTURE_REL);

    let fixture_bytes = match std::fs::read(&fixture_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!(
                "kg_latency_bench: failed to read fixture {}: {e}",
                fixture_path.display()
            );
            return 1;
        }
    };
    let fixture: Fixture = match serde_json::from_slice(&fixture_bytes) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("kg_latency_bench: fixture parse failed: {e}");
            return 1;
        }
    };

    let selected = match select_bench_fixtures(&fixture) {
        Ok(v) => v,
        Err(missing) => {
            eprintln!(
                "kg_latency_bench: requested fixture id(s) not present in fixture file: {}",
                missing.join(", ")
            );
            return 1;
        }
    };

    let schema = match Schema::load_default() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("kg_latency_bench: schema load failed: {e}");
            return 1;
        }
    };

    let ollama = OllamaClient::new();

    // Reachability ping — a 1-token prompt against the daemon. First
    // contact also triggers model load if the daemon hasn't seen
    // BENCH_MODEL since last restart; that latency is included in
    // the per-fixture numbers, not the ping (the ping uses a
    // throwaway tiny prompt with a deliberately tight context).
    let ping_opts = GenerateOptions {
        temperature: 0.0,
        seed: Some(0),
        num_ctx: 256,
    };
    match ollama.generate(BENCH_MODEL, "ok", None, &ping_opts) {
        Ok(_) => {}
        Err(OllamaError::Transport { url, source }) => {
            println!("# ollama-unreachable");
            eprintln!("kg_latency_bench: Ollama not reachable at {url}: {source}");
            eprintln!(
                "kg_latency_bench: start Ollama (`ollama serve`) and retry, OR confirm \
                 model `{BENCH_MODEL}` is pulled. Wave 1C.0's instrumentation half \
                 (PassTimings + worker tracing) is already on disk."
            );
            return 2;
        }
        Err(other) => {
            // Non-transport error (BadStatus, missing field, etc.)
            // is still a daemon-side problem we can't fix from here.
            // Surface and bail.
            eprintln!("kg_latency_bench: Ollama ping failed (non-transport): {other}");
            return 1;
        }
    }

    // Open the tempfile-backed SQLite + apply all migrations + FK on.
    let tmpfile = match NamedTempFile::new() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("kg_latency_bench: tempfile creation failed: {e}");
            return 1;
        }
    };
    let mut conn = match Connection::open(tmpfile.path()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("kg_latency_bench: sqlite open failed: {e}");
            return 1;
        }
    };
    if let Err(e) = conn.pragma_update(None, "foreign_keys", "ON") {
        eprintln!("kg_latency_bench: PRAGMA foreign_keys failed: {e}");
        return 1;
    }
    if let Err(e) = apply_all(&conn) {
        eprintln!("kg_latency_bench: migration apply failed: {e}");
        return 1;
    }

    // CSV header.
    println!(
        "fixture_id,segment_count,segment_ms,classify_ms_total,extract_ms_total,\
         extract_entities_ms_total,normalize_ms_total,store_apply_ms,total_pipeline_ms"
    );

    let mut rows: Vec<BenchRow> = Vec::with_capacity(selected.len());
    let mut next_entry_id: i64 = 1;

    for dict in &selected {
        let entry_id = next_entry_id;
        next_entry_id += 1;

        if let Err(e) = seed_session_row(&conn, entry_id, &dict.dictation_id) {
            eprintln!(
                "kg_latency_bench: seed sessions row failed for {}: {e}",
                dict.dictation_id
            );
            return 1;
        }

        let options = GenerateOptions {
            temperature: 0.2,
            seed: Some(entry_id),
            num_ctx: 4096,
        };

        let pipeline_t0 = Instant::now();
        let result = run_pipeline(
            &ollama,
            &schema,
            None, // no synonym map (production worker also passes None)
            BENCH_MODEL,
            &dict.dictation_id,
            &dict.dictation_text,
            BENCH_CAPTURED_ISO,
            &options,
            None, // no artifact dir
        );
        let total_pipeline_ms = pipeline_t0.elapsed().as_millis() as u64;

        // Even on per-pass failures we still want to time the store
        // apply step if there's anything to file — that's the same
        // contract the production worker honors.
        let segments = build_segment_outputs(&result);
        let segment_count = segments.len();

        let store_apply_ms = match apply_store(&mut conn, entry_id, &segments) {
            Ok(ms) => ms,
            Err(e) => {
                eprintln!(
                    "kg_latency_bench: store apply failed for {}: {e}",
                    dict.dictation_id
                );
                return 1;
            }
        };

        let row = BenchRow {
            fixture_id: dict.dictation_id.clone(),
            segment_count,
            timings: result.pass_timings.clone(),
            store_apply_ms,
            total_pipeline_ms,
        };

        println!(
            "{},{},{},{},{},{},{},{},{}",
            row.fixture_id,
            row.segment_count,
            row.timings.segment_ms,
            row.timings.classify_ms_total,
            row.timings.extract_ms_total,
            row.timings.extract_entities_ms_total,
            row.timings.normalize_ms_total,
            row.store_apply_ms,
            row.total_pipeline_ms,
        );

        rows.push(row);
    }

    print_summary(&rows);
    0
}

/// Look the requested `BENCH_FIXTURE_IDS` up in the loaded fixture
/// file. Preserves the canonical order of `BENCH_FIXTURE_IDS` so the
/// CSV emits short → long. Returns the missing IDs (if any) so the
/// caller can fail loudly when the fixture has drifted.
fn select_bench_fixtures(fixture: &Fixture) -> Result<Vec<&FixtureDictation>, Vec<String>> {
    let mut out: Vec<&FixtureDictation> = Vec::with_capacity(BENCH_FIXTURE_IDS.len());
    let mut missing: Vec<String> = Vec::new();
    for id in BENCH_FIXTURE_IDS {
        match fixture.dictations.iter().find(|d| d.dictation_id == *id) {
            Some(d) => out.push(d),
            None => missing.push((*id).to_string()),
        }
    }
    if missing.is_empty() {
        Ok(out)
    } else {
        Err(missing)
    }
}

fn seed_session_row(conn: &Connection, entry_id: i64, dictation_id: &str) -> rusqlite::Result<()> {
    // Mirrors `kg::parity::run_persist_round_trip`'s session-seed
    // call — only the NOT-NULL columns get values; provenance FKs are
    // nullable and irrelevant to the bench.
    conn.execute(
        "INSERT INTO sessions (\
            id, uuid, mode_id, hotkey_pressed, started_at, recording_ended_at,\
            status, audio_duration_ms\
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            entry_id,
            format!("latency-bench-{entry_id}-{dictation_id}"),
            1_i64, // modes(id) = 1 seeded by migration 003
            "latency-bench",
            BENCH_CAPTURED_ISO,
            BENCH_CAPTURED_ISO,
            "bench",
            0_i64,
        ],
    )?;
    Ok(())
}

/// Time the wall-clock cost of `apply_filed_outcome` inside its own
/// transaction. Returns elapsed ms.
fn apply_store(
    conn: &mut Connection,
    entry_id: i64,
    segments: &[super::store::SegmentOutput],
) -> rusqlite::Result<u64> {
    let tx = conn.transaction()?;
    let t0 = Instant::now();
    apply_filed_outcome(&tx, entry_id, segments, BENCH_CAPTURED_ISO).map_err(|e| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(BenchAppErr(e.to_string())))
    })?;
    let elapsed_ms = t0.elapsed().as_millis() as u64;
    tx.commit()?;
    Ok(elapsed_ms)
}

/// Tiny error-wrapping shim so `apply_filed_outcome`'s `AppError` can
/// surface through `rusqlite::Error` without pulling in a richer
/// error type just for the bench.
#[derive(Debug)]
struct BenchAppErr(String);
impl std::fmt::Display for BenchAppErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for BenchAppErr {}

fn print_summary(rows: &[BenchRow]) {
    if rows.is_empty() {
        println!("# (no rows; bench produced nothing to summarize)");
        return;
    }
    let mut totals: Vec<u64> = rows.iter().map(|r| r.total_pipeline_ms).collect();
    totals.sort_unstable();

    let n = totals.len() as u64;
    let sum: u64 = totals.iter().sum();
    let mean = sum / n;
    let p50 = totals[(rows.len() - 1) / 2];
    // p95 with `nearest-rank` method per NIST: ceil(0.95 * n).
    let p95_idx = (((0.95_f64) * rows.len() as f64).ceil() as usize)
        .saturating_sub(1)
        .min(rows.len() - 1);
    let p95 = totals[p95_idx];
    let max = *totals.last().expect("rows non-empty");

    println!("# summary");
    println!("# samples              = {}", rows.len());
    println!("# mean_total_pipeline_ms = {mean}");
    println!("# p50_total_pipeline_ms  = {p50}");
    println!("# p95_total_pipeline_ms  = {p95}");
    println!("# max_total_pipeline_ms  = {max}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_bench_fixtures_preserves_order_and_reports_missing() {
        let fix = Fixture {
            dictations: vec![
                FixtureDictation {
                    dictation_id: "persona-05-case-03".into(),
                    dictation_text: "z".into(),
                },
                FixtureDictation {
                    dictation_id: "persona-03-case-01".into(),
                    dictation_text: "a".into(),
                },
                // intentionally missing persona-01-case-01, persona-04-case-02, persona-05-case-02
            ],
        };
        match select_bench_fixtures(&fix) {
            Err(missing) => {
                assert_eq!(missing.len(), 3);
                assert!(missing.contains(&"persona-01-case-01".to_string()));
                assert!(missing.contains(&"persona-04-case-02".to_string()));
                assert!(missing.contains(&"persona-05-case-02".to_string()));
            }
            Ok(_) => panic!("expected missing-id error"),
        }
    }

    #[test]
    fn print_summary_handles_single_row() {
        // Smoke test: just confirm it doesn't panic with n=1
        // (p95-idx math has a saturating_sub that needs to behave).
        let rows = vec![BenchRow {
            fixture_id: "x".into(),
            segment_count: 1,
            timings: PassTimings::default(),
            store_apply_ms: 0,
            total_pipeline_ms: 42,
        }];
        print_summary(&rows);
    }
}
