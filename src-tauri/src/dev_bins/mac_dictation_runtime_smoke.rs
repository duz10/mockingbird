//! `mac_dictation_runtime_smoke` — judge shim for
//! `mac-p3-dictation-runtime-spawn` (mb-mac-v1.4.7c, ADR 0063). Thin
//! wrapper over
//! [`mockingbird_lib::dictation::judges_macos_v1::spawn_teardown_probe`].
//!
//! Proves the `.4.7c` runtime wiring: `DictationRuntime::spawn_with_deps`
//! SPAWNS (real channels + threads + the REAL `MacKeyboardHook`
//! listener) and TEARS DOWN cleanly when dropped — no hang, no panic.
//! Device backends (audio/VAD/STT) are doubled; the listener +
//! thread-wiring + Drop teardown are real (see the judge module for the
//! full real-vs-doubled boundary).
//!
//! The probe runs on a worker thread bounded by a timeout watchdog: a
//! (hypothetical) hung teardown surfaces as an explicit FAIL rather than
//! an indefinite hang.
//!
//! Run (via the Mac wrapper which injects --features metal):
//!   scripts/dev/cargo-mac.sh run --release --example mac_dictation_runtime_smoke
//!
//! Exit codes: 0 = pass · 1 = runtime/assert/hang failure · 2 = wrong platform.

// Built as a real probe only on macOS WITH metal (the
// `dictation::judges_macos_v1` module — and thus the probe — is gated
// `all(macos, metal)`). Every other config gets a stub so
// `cargo build/clippy --all-targets` stays green without metal.
#[cfg(not(all(target_os = "macos", feature = "metal")))]
fn main() {
    eprintln!(
        "mac_dictation_runtime_smoke requires macOS + `--features metal` \
         (use scripts/dev/cargo-mac.sh)"
    );
    std::process::exit(2);
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn main() {
    use std::io::Write;
    use std::sync::mpsc;
    use std::time::Duration;

    use mockingbird_lib::dictation::judges_macos_v1::spawn_teardown_probe;

    // No `ort` session is loaded in this probe (the VAD is doubled), so
    // the onnxruntime teardown-abort can't fire — but we keep the
    // `_exit` convention the other mac judges use for uniformity.
    fn clean_exit(code: i32) -> ! {
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
        // SAFETY: deliberate fast process exit; all our work + I/O is done.
        unsafe { libc::_exit(code) }
    }

    // Hang ceiling: spawn + 150ms settle + a clean teardown is sub-second
    // in practice; 15s is a generous "it's wedged" threshold.
    const TIMEOUT: Duration = Duration::from_secs(15);
    // A clean teardown returns near-instantly; flag anything slow.
    const MAX_TEARDOWN_MS: u64 = 5_000;

    println!("=== mac_dictation_runtime_smoke (mac-p3-dictation-runtime-spawn) ===");

    // Run the probe on a worker thread so the main thread can enforce a
    // timeout — a hung `drop(runtime)` would otherwise block forever.
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("spawn-teardown-probe".into())
        .spawn(move || {
            let _ = tx.send(spawn_teardown_probe());
        })
        .expect("spawn probe worker");

    let report = match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            eprintln!("FAIL: probe error: {e}");
            clean_exit(1);
        }
        Err(_) => {
            eprintln!(
                "FAIL: DictationRuntime spawn/teardown did not complete within {}s \
                 — teardown is hung.",
                TIMEOUT.as_secs()
            );
            clean_exit(1);
        }
    };
    let _ = worker.join();

    println!("spawn_ok:     {}", report.spawn_ok);
    println!("teardown_ms:  {}", report.teardown_ms);
    println!();

    let mut ok = true;
    if !report.spawn_ok {
        eprintln!("FAIL: DictationRuntime::spawn_with_deps did not return Ok");
        ok = false;
    }
    if report.teardown_ms > MAX_TEARDOWN_MS {
        eprintln!(
            "FAIL: teardown took {}ms (> {}ms) — listener/thread teardown is not clean",
            report.teardown_ms, MAX_TEARDOWN_MS
        );
        ok = false;
    }

    if ok {
        println!(
            "PASS: DictationRuntime spawned (real listener + threads) and tore down cleanly \
             in {}ms with no hang or panic.",
            report.teardown_ms
        );
        clean_exit(0);
    }
    clean_exit(1);
}
