//! ADR 0046 Iter 1 / `mb-jbf7` — programmatic smoke for the desktop
//! file-import path.
//!
//! Mirrors `lib.rs::run()`'s setup (open DB, build [`OrchestratorConfig`],
//! spawn [`DictationRuntime`]) and then mirrors the
//! `dictation_import_file` IPC handler (decode via symphonia + enqueue a
//! [`HeadlessIngestRequest`] through `headless_ingest_sender()`). Awaits
//! the orchestrator's reply, prints the resulting `sessions` row +
//! transcripts, drops the runtime.
//!
//! Exercises the SAME orchestrator path the production IPC uses — VAD
//! reset, whisper-rs CUDA STT, cleanup-pass dispatch, `SessionsEventBus`
//! emit, two-row `transcripts` insert (raw + cleaned, no final).
//!
//! ## Usage
//!
//! ```text
//! powershell -File scripts\cargo-with-cuda.ps1 run --release \
//!     --example smoke_iter1_ingest -- \
//!     "C:\Users\<you>\Downloads\New Recording 38.m4a"
//! ```
//!
//! The wrapper script gives us MSVC + CUDA env; this binary additionally
//! sets `ORT_DYLIB_PATH` + prepends CUDA bin to PATH the same way
//! `scripts/run-mockingbird.ps1` does, so it works without a separate
//! env-prep step.
//!
//! ## Not a test
//!
//! Lives under `examples/` (NOT `tests/`) so it never enters CI. It is
//! deliberately a one-shot diagnostic — it talks to the production DB at
//! `%APPDATA%\com.dustin.mockingbird\mockingbird.db`, so a successful
//! run leaves a real `sessions` row visible in the Dictations page.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::Connection;

use mockingbird_lib::audio::decode::decode_to_pcm16_mono_16k;
use mockingbird_lib::db::Database;
use mockingbird_lib::dictation::ingest::IngestProvenance;
use mockingbird_lib::dictation::ingest_channel::HeadlessIngestRequest;
use mockingbird_lib::dictation::runtime::{default_normal_config, DictationRuntime};
use mockingbird_lib::error::AppResult;
use mockingbird_lib::vault::export_job::VaultRuntime;

const REPLY_TIMEOUT: Duration = Duration::from_secs(300);

fn main() {
    init_tracing();
    setup_env();

    let fixture: PathBuf = env::args()
        .nth(1)
        .unwrap_or_else(|| {
            eprintln!("usage: smoke_iter1_ingest <audio-file>");
            std::process::exit(2);
        })
        .into();
    if !fixture.exists() {
        eprintln!("fixture does not exist: {}", fixture.display());
        std::process::exit(2);
    }

    println!("\n🐶 smoke_iter1_ingest");
    println!("  fixture: {}", fixture.display());

    let db_path = locate_db();
    println!("  db:      {}", db_path.display());

    // --- Step 1: open DB, resolve OrchestratorConfig (mirrors lib.rs).
    let database = Database::open(&db_path).expect("Database::open");
    let config = default_normal_config(&database.conn).expect("default_normal_config");
    println!(
        "  active mode: {} (mode_id={}, prompt_id={}, dict_id={}, example_id={})",
        config.mode_slug,
        config.mode_id,
        config.prompt_id,
        config.dictionary_snapshot_id,
        config.example_set_id,
    );
    let shared = Arc::new(Mutex::new(database.conn));

    // --- Step 2: spawn DictationRuntime (loads whisper-rs CUDA + Silero VAD + cleaner).
    println!("\n[1/4] Spawning DictationRuntime (whisper-rs CUDA + Silero VAD load) ...");
    let t0 = std::time::Instant::now();
    // ADR 0046 Iter 2 / mb-lvzw — vault runtime, disabled by default
    // on a brand-new DB so `trigger()` is a no-op for this smoke.
    let vault = Arc::new(VaultRuntime::new(&shared).expect("VaultRuntime::new"));
    let runtime = DictationRuntime::spawn(shared.clone(), config, HashMap::new(), vault)
        .expect("DictationRuntime::spawn");
    println!("      runtime up in {:?}", t0.elapsed());

    // --- Step 3: decode fixture off the runtime thread (same as the IPC handler does).
    println!("\n[2/4] Decoding fixture via symphonia ...");
    let t1 = std::time::Instant::now();
    let samples = decode_to_pcm16_mono_16k(&fixture).expect("decode_to_pcm16_mono_16k");
    let approx_secs = samples.len() as f64 / 16_000.0;
    println!(
        "      decoded {} samples (~{:.2}s @ 16 kHz mono) in {:?}",
        samples.len(),
        approx_secs,
        t1.elapsed(),
    );

    // --- Step 4: enqueue HeadlessIngestRequest, await reply (same channel the IPC uses).
    let (reply_tx, reply_rx) = crossbeam_channel::bounded::<AppResult<i64>>(1);
    let original_filename = fixture
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string();
    let received_at_iso = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let provenance = IngestProvenance::desktop_import(original_filename, received_at_iso);

    println!("\n[3/4] Sending HeadlessIngestRequest to orchestrator ...");
    let t2 = std::time::Instant::now();
    runtime
        .headless_ingest_sender()
        .send(HeadlessIngestRequest {
            samples,
            provenance,
            reply_tx,
        })
        .expect("headless_ingest_sender.send");

    println!(
        "      awaiting orchestrator reply (timeout {}s) ...",
        REPLY_TIMEOUT.as_secs()
    );
    let session_id = match reply_rx.recv_timeout(REPLY_TIMEOUT) {
        Ok(Ok(id)) => id,
        Ok(Err(e)) => {
            eprintln!("\n❌ orchestrator returned error: {e}");
            std::process::exit(3);
        }
        Err(e) => {
            eprintln!("\n❌ reply timeout / channel closed: {e}");
            std::process::exit(3);
        }
    };
    println!(
        "      ✅ ingest complete: session_id={session_id} (round-trip {:?})",
        t2.elapsed()
    );

    // --- Step 5: read back the row + transcripts and pretty-print.
    println!("\n[4/4] Reading back sessions row + transcripts ...");
    let conn = shared.lock().expect("shared db unpoisoned");
    print_session_summary(&conn, session_id);
    drop(conn);

    println!("\nDropping runtime (this tears down hook + dictation thread) ...");
    drop(runtime);
    // The hotkey thread receives WM_QUIT through `_hook`'s Drop and exits
    // before this main returns.

    println!("\n🎉 smoke complete — session {session_id} should be visible in the Dictations page on next launch.");
}

/// Mirror of `scripts/run-mockingbird.ps1`'s env setup so this binary is
/// invokable without a wrapper. Best-effort: missing pieces just produce
/// the same downstream DLL errors the production launcher would.
fn setup_env() {
    let user_profile = env::var("USERPROFILE").expect("USERPROFILE not set");
    let models_dir = env::var("MOCKINGBIRD_MODELS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(&user_profile).join("mockingbird_models"));

    // ORT_DYLIB_PATH — Silero VAD via ort 2.0.0-rc.10.
    if env::var_os("ORT_DYLIB_PATH").is_none() {
        let ort = models_dir.join("onnxruntime.dll");
        if ort.exists() {
            env::set_var("ORT_DYLIB_PATH", &ort);
            println!("  ORT_DYLIB_PATH = {}", ort.display());
        } else {
            eprintln!(
                "  warn: onnxruntime.dll not at {} -- VAD will fail",
                ort.display()
            );
        }
    }

    // CUDA bin on PATH — whisper-rs cuda feature dlopens cudart at startup.
    let cuda_bin = PathBuf::from(r"C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.8\bin");
    if cuda_bin.exists() {
        let cur = env::var_os("PATH").unwrap_or_default();
        let cur_str = cur.to_string_lossy();
        if !cur_str.contains(&*cuda_bin.to_string_lossy()) {
            let new_path = format!("{};{}", cuda_bin.display(), cur_str);
            env::set_var("PATH", new_path);
            println!("  PATH += {}", cuda_bin.display());
        }
    } else {
        eprintln!(
            "  warn: CUDA v12.8 not at {} -- whisper will CPU-fall-back or fail",
            cuda_bin.display()
        );
    }
}

/// Cheap tracing setup so the orchestrator's `tracing::info!` chatter
/// surfaces on stderr while we wait for the reply. No file output, no
/// PII scrubbing — this is a foreground diagnostic, not a long-running
/// process.
fn init_tracing() {
    // Default to INFO; let RUST_LOG override.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info,mockingbird_lib=info")
            }),
        )
        .with_target(true)
        .try_init();
}

fn locate_db() -> PathBuf {
    let appdata = env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(appdata)
        .join("com.dustin.mockingbird")
        .join("mockingbird.db")
}

/// Subset of `sessions` columns we eyeball after an import. Named fields
/// instead of an 8-tuple keep `clippy::type_complexity` quiet and read better.
struct SessionSummaryRow {
    source: String,
    start_mode: String,
    status: String,
    hotkey_pressed: String,
    audio_duration_ms: i64,
    stt_latency_ms: Option<i64>,
    cleanup_latency_ms: Option<i64>,
    injection_status: Option<String>,
}

fn print_session_summary(conn: &Connection, session_id: i64) {
    println!("\n=== sessions row id={session_id} ===");
    let row: Result<SessionSummaryRow, _> = conn.query_row(
        "SELECT source, start_mode, status, hotkey_pressed, audio_duration_ms, \
                stt_latency_ms, cleanup_latency_ms, injection_status \
         FROM sessions WHERE id = ?1",
        [session_id],
        |r| {
            Ok(SessionSummaryRow {
                source: r.get(0)?,
                start_mode: r.get(1)?,
                status: r.get(2)?,
                hotkey_pressed: r.get(3)?,
                audio_duration_ms: r.get(4)?,
                stt_latency_ms: r.get(5)?,
                cleanup_latency_ms: r.get(6)?,
                injection_status: r.get(7)?,
            })
        },
    );
    match row {
        Ok(s) => {
            println!("  source:              {}", s.source);
            println!("  start_mode:          {}", s.start_mode);
            println!("  status:              {}", s.status);
            println!("  hotkey_pressed:      {}", s.hotkey_pressed);
            println!("  audio_duration_ms:   {}", s.audio_duration_ms);
            println!(
                "  stt_latency_ms:      {}",
                s.stt_latency_ms
                    .map(|v| v.to_string())
                    .unwrap_or("NULL".into())
            );
            println!(
                "  cleanup_latency_ms:  {}",
                s.cleanup_latency_ms
                    .map(|v| v.to_string())
                    .unwrap_or("NULL".into())
            );
            println!(
                "  injection_status:    {}",
                s.injection_status.unwrap_or("NULL".into())
            );
        }
        Err(e) => println!("  (query failed: {e})"),
    }

    println!("\n=== transcripts for session {session_id} (ordered by id) ===");
    let mut stmt = conn
        .prepare(
            "SELECT stage, length(text), substr(text, 1, 200), \
                    COALESCE(model_used, 'NULL') \
             FROM transcripts WHERE session_id = ?1 ORDER BY id",
        )
        .expect("prepare transcripts stmt");
    let rows: Vec<(String, i64, String, String)> = stmt
        .query_map([session_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .expect("query")
        .filter_map(|r| r.ok())
        .collect();
    if rows.is_empty() {
        println!("  (no transcript rows)");
    }
    for (stage, len, preview, model) in &rows {
        println!("  [{stage:<7}] len={len:<6} model={model}");
        println!("    preview: {preview:?}");
    }
    println!("  (total stages: {})", rows.len());
    if rows.iter().any(|(s, _, _, _)| s == "final") {
        println!(
            "  ⚠️  WARNING: a 'final' stage row exists — headless ingest should never write one!"
        );
    }

    // Schema spot-check for criterion 4.
    println!("\n=== schema spot-check ===");
    let source_col: rusqlite::Result<String> = conn.query_row(
        "SELECT type FROM pragma_table_info('sessions') WHERE name = 'source'",
        [],
        |r| r.get(0),
    );
    match source_col {
        Ok(t) => println!("  sessions.source column type: {t}"),
        Err(e) => println!("  ⚠️ sessions.source missing: {e}"),
    }
    let idx_sql: rusqlite::Result<String> = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE name = 'idx_sessions_source'",
        [],
        |r| r.get(0),
    );
    match idx_sql {
        Ok(s) => println!("  idx_sessions_source: {s}"),
        Err(e) => println!("  ⚠️ idx_sessions_source missing: {e}"),
    }
}
