//! Wave 4.9 QA verification probe.
//!
//! Reads `%APPDATA%\com.dustin.mockingbird\mockingbird.db` via the
//! same `rusqlite` we ship with and prints the four Bug-A / Bug-B /
//! ADR-0020 status reports a human needs to eyeball after running
//! through the Wave-4 QA matrix.
//!
//! Bug C (clipboard restore) cannot be SQL-verified — it needs a
//! human + a real paste target. The script prints the manual
//! recipe at the end.
//!
//! Usage:
//!   cargo run --example verify_wave49
//!
//! Exits 0 always — this is a probe, not a gate.

use std::path::PathBuf;

use rusqlite::Connection;

fn main() {
    let db = locate_db();
    if !db.exists() {
        eprintln!("DB not found at {}", db.display());
        eprintln!("Has mockingbird ever started? Run scripts\\run-mockingbird.ps1 first.");
        std::process::exit(1);
    }

    let conn = match Connection::open(&db) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to open {}: {e}", db.display());
            std::process::exit(2);
        }
    };

    println!("🐶 Wave 4.9 verification — DB at {}", db.display());

    section("BUG A — Last 5 sessions: transcript counts + stages");
    println!("Expect: 3 stages (raw,cleaned,final) for injection_status='ok'");
    println!("        2 stages (raw,cleaned)       for aborted_* statuses\n");
    run_table(
        &conn,
        "\
        SELECT s.id, s.injection_status, COUNT(t.id) AS transcript_rows, \
               GROUP_CONCAT(t.stage) AS stages \
        FROM sessions s \
        LEFT JOIN transcripts t ON t.session_id = s.id \
        GROUP BY s.id \
        ORDER BY s.id DESC \
        LIMIT 5",
        &["id", "injection_status", "rows", "stages"],
    );

    section("BUG A — Last 6 transcript rows");
    println!("Expect: raw rows have model_used=NULL; cleaned+final have 'passthrough'\n");
    run_table(
        &conn,
        "SELECT id, session_id, stage, substr(text, 1, 40), \
                COALESCE(model_used, 'NULL'), \
                substr(created_at, 12, 8) \
         FROM transcripts ORDER BY id DESC LIMIT 6",
        &[
            "id",
            "session_id",
            "stage",
            "text(40)",
            "model_used",
            "time",
        ],
    );

    section("BUG B — foreground_app distribution");
    println!("Expect: process names like 'notepad.exe', 'Code.exe'. NO empty strings.\n");
    run_table(
        &conn,
        "SELECT COALESCE(NULLIF(foreground_app,''), '<EMPTY>') AS app, \
                COUNT(*) AS sessions, MAX(id) AS last_id \
         FROM sessions GROUP BY app ORDER BY sessions DESC",
        &["foreground_app", "sessions", "last_id"],
    );

    section("OVERVIEW — Last 5 sessions");
    run_table(
        &conn,
        "SELECT id, \
                COALESCE(NULLIF(foreground_app,''), '<EMPTY>'), \
                injection_status, \
                substr(recording_ended_at, 12, 8) \
         FROM sessions ORDER BY id DESC LIMIT 5",
        &["id", "foreground_app", "injection_status", "ended_utc"],
    );

    section("ADR 0020 — injection_status distribution");
    println!("Expect: new sessions never produce 'aborted_focus_changed'");
    println!("        (legacy pre-4.9 rows with that value are fine)\n");
    run_table(
        &conn,
        "SELECT COALESCE(injection_status, 'NULL') AS status, COUNT(*) \
         FROM sessions GROUP BY injection_status ORDER BY 2 DESC",
        &["injection_status", "count"],
    );

    println!("\n--- HUMAN CHECKS (no SQL can verify these) ---\n");

    println!("BUG C — Clipboard restore:");
    println!("  1. Copy `OLD-CLIP-MARKER` to your clipboard (highlight + Ctrl+C).");
    println!("  2. Focus a Notepad window.");
    println!("  3. Hold RightAlt, say 'hello world test', release.");
    println!("  4. Notepad shows the transcribed text.");
    println!("  5. Click elsewhere (NOT Notepad), Ctrl+V.");
    println!("  6. PASS: you see 'OLD-CLIP-MARKER'.");
    println!("     FAIL: you see the transcribed text → restore is broken.\n");

    println!("PERMISSIVE FOCUS — ADR 0020:");
    println!("  1. Focus Notepad.");
    println!("  2. Hold RightAlt; while still holding, Alt+Tab to Chrome address bar.");
    println!("  3. Say something, release RightAlt.");
    println!("  4. PASS: text appears in Chrome. Re-run this probe — last");
    println!("     session has foreground_app='chrome.exe', injection_status='ok'.");
    println!("     FAIL: nothing in Chrome AND last session is 'aborted_focus_changed'.\n");

    println!(
        "Logs: {}\\com.dustin.mockingbird\\logs\\",
        std::env::var("APPDATA").unwrap_or_default()
    );
    println!("  - look for: 'focus changed during dictation; proceeding into key-up app'");
    println!("  - look for: 'clipboard sequence diverged' (should be RARE, not every paste)");
}

fn locate_db() -> PathBuf {
    let appdata = std::env::var("APPDATA").unwrap_or_else(|_| String::from("."));
    PathBuf::from(appdata)
        .join("com.dustin.mockingbird")
        .join("mockingbird.db")
}

fn section(title: &str) {
    println!("\n=== {title} ===\n");
}

/// Run a query whose row shape matches `headers.len()` text columns
/// and pretty-print as a fixed-width table. Intentionally naive —
/// this is a diagnostic, not a reporting tool.
fn run_table(conn: &Connection, sql: &str, headers: &[&str]) {
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(e) => {
            println!("(query error: {e})");
            return;
        }
    };
    let col_count = headers.len();
    let rows_iter = stmt.query_map([], |row| {
        let mut v = Vec::with_capacity(col_count);
        for i in 0..col_count {
            // Pull as String — SQLite affinity handles int/text/null uniformly.
            let cell: rusqlite::types::Value = row.get(i)?;
            v.push(value_to_string(&cell));
        }
        Ok(v)
    });
    let rows: Vec<Vec<String>> = match rows_iter {
        Ok(it) => it.filter_map(|r| r.ok()).collect(),
        Err(e) => {
            println!("(query error: {e})");
            return;
        }
    };
    if rows.is_empty() {
        println!("(no rows)");
        return;
    }
    // Column widths.
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for r in &rows {
        for (i, cell) in r.iter().enumerate() {
            if i < widths.len() && cell.len() > widths[i] {
                widths[i] = cell.len();
            }
        }
    }
    // Print headers.
    let mut head = String::new();
    for (i, h) in headers.iter().enumerate() {
        head.push_str(&format!("{:<w$}  ", h, w = widths[i]));
    }
    println!("{}", head.trim_end());
    println!("{}", "-".repeat(head.trim_end().len()));
    // Rows.
    for r in &rows {
        let mut line = String::new();
        for (i, cell) in r.iter().enumerate() {
            line.push_str(&format!("{:<w$}  ", cell, w = widths[i]));
        }
        println!("{}", line.trim_end());
    }
}

fn value_to_string(v: &rusqlite::types::Value) -> String {
    use rusqlite::types::Value;
    match v {
        Value::Null => String::from("NULL"),
        Value::Integer(i) => i.to_string(),
        Value::Real(f) => format!("{f}"),
        Value::Text(s) => s.clone(),
        Value::Blob(_) => String::from("<blob>"),
    }
}
