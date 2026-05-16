//! Mockingbird — local-first voice dictation for Windows.
//!
//! Binary entry point is in `main.rs`; this library crate is what gets
//! linked into the Tauri shell. See `PLAN-mockingbird-v2.md` for the
//! full design and `docs/phases/phase1.md` for the current phase plan.

#![warn(missing_docs)]

pub mod error;

/// Build and run the Tauri application.
///
/// Phase 1 wires up:
///   - tracing subscriber (Phase-1 module: `logging`, to be added)
///   - SQLite migrations 001-003 (Phase-1 module: `db`, to be added)
///   - typed settings facade (Phase-1 module: `settings`, to be added)
///   - tray with placeholder menu (Phase-1 module: `tray`, to be added)
///   - typed Tauri commands (Phase-1 module: `commands`, to be added)
///
/// Phase 1 Wave 1 ships only this skeleton — subsequent waves fill in
/// the modules. Calling `run()` today produces a Tauri app that opens
/// to a hidden main window and shows the tray icon. No DB, no commands
/// registered until Wave 2/3/4 land.
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            // Logging + DB + tray + commands wired here in later Phase-1 waves.
            tracing::info!("Mockingbird starting (Phase 1 Wave 1 skeleton)");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Tauri application");
}
