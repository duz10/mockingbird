//! VRAM probe — discover the GPU's total memory so the cleanup
//! pipeline knows whether the Q5_K_M Ollama models will fit
//! alongside Whisper-large-v3-turbo (~2 GB resident).
//!
//! ADR 0047 §Wave 2.4 sets a 6 GB hard floor for opting in to Q5
//! models: below that, Whisper + Q5 Qwen-7B (~5.5 GB) blows VRAM on
//! a 6 GB card with no headroom for the OS / compositor / driver
//! overhead. The probe's only consumer is the opt-in decision -- it
//! doesn't gate any runtime path; if the probe fails we treat the
//! result as "can't confirm 6 GB" and stay on Q4 (the safer answer).
//!
//! ## Implementation: `nvidia-smi` subprocess
//!
//! `nvidia-smi` ships with every CUDA install (which is the only env
//! Mockingbird runs the GPU-Whisper path in anyway) and lives at the
//! canonical `C:\Windows\System32\nvidia-smi.exe` on Windows. One-line
//! query: `nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits`.
//!
//! Alternatives considered + rejected:
//!
//! * **`nvml-wrapper` crate**: more elegant Rust API but adds a heavy
//!   dep (links to libnvidia-ml) for a one-time first-run check.
//!   Doesn't earn its weight at the current scale -- if we grow
//!   per-frame VRAM monitoring later we can revisit.
//! * **Querying via wgpu / vulkano**: requires a graphics context;
//!   massive overkill for "what's `nvidia-smi`'s memory.total field".
//!
//! ## Failure modes (all collapse to `None`)
//!
//! * `nvidia-smi` not on PATH (e.g. AMD card, integrated GPU,
//!   Mac dev box) -- spawn fails, returns None.
//! * `nvidia-smi` runs but exits non-zero (driver mismatch, etc.).
//! * Output parse fails (multi-GPU comma split, non-numeric, ...).
//!
//! The probe's caller treats `None` as "stay on Q4" -- the same
//! posture as "GPU below threshold". Mocking-bird never asks the
//! user a question the probe can't help them answer.

use std::process::Command;

/// VRAM threshold below which the Q5_K_M opt-in stays off-by-default
/// (in MiB). 6 GB == 6144 MiB. Sourced from ADR 0047 §Wave 2.4: the
/// "if total VRAM < 6 GB, stay on Q4" hard floor.
///
/// Exposed `pub const` so callers (Settings UI deferred to mb-h0nn,
/// first-run wizard if/when it lands) reference the same number the
/// ADR cites rather than duplicating the magic value.
pub const Q5_VRAM_FLOOR_MIB: u64 = 6144;

/// Probe the system's total VRAM (in MiB) via `nvidia-smi`.
///
/// Returns `None` if the probe fails for any reason -- missing
/// executable, non-zero exit, unparseable output. Callers treat
/// `None` as "can't confirm enough VRAM" and stay on the
/// conservative default.
///
/// Multi-GPU note: `nvidia-smi --query-gpu=memory.total` outputs one
/// line per GPU. We use the FIRST line because Mockingbird's
/// CUDA-backed pipelines (whisper-rs, llama.cpp via Ollama) bind to
/// CUDA device 0 by default. Reporting the max across all cards
/// would lie about the VRAM actually available to our pipeline.
pub fn probe_vram_mib() -> Option<u64> {
    let output = Command::new("nvidia-smi")
        .arg("--query-gpu=memory.total")
        .arg("--format=csv,noheader,nounits")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = std::str::from_utf8(&output.stdout).ok()?;
    parse_memory_total(stdout)
}

/// Returns true iff the probed VRAM clears the Q5 floor.
/// `None` from the probe collapses to `false` (conservative default).
///
/// Split out so callers can branch on "definitely enough" without
/// re-introducing the magic threshold at every site.
pub fn meets_q5_floor() -> bool {
    matches!(probe_vram_mib(), Some(mib) if mib >= Q5_VRAM_FLOOR_MIB)
}

/// Parse the first numeric value from `nvidia-smi --query-gpu=memory.total
/// --format=csv,noheader,nounits` output (MiB). Returns `None` if no
/// line parses as a u64.
///
/// Split out for unit testability without spawning a subprocess.
fn parse_memory_total(stdout: &str) -> Option<u64> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .find_map(|line| line.parse::<u64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_gpu_typical_output() {
        // What `nvidia-smi --query-gpu=memory.total --format=csv,
        // noheader,nounits` returns on the dev box (RTX 2060).
        assert_eq!(parse_memory_total("6144\n"), Some(6144));
    }

    #[test]
    fn parse_first_of_multiple_gpus() {
        // Two-GPU dev box would output two lines.
        // We bind to CUDA device 0 -- report its memory, not the max.
        assert_eq!(parse_memory_total("8192\n24576\n"), Some(8192));
    }

    #[test]
    fn parse_trims_trailing_whitespace() {
        assert_eq!(parse_memory_total("  6144  \r\n"), Some(6144));
    }

    #[test]
    fn parse_returns_none_on_empty_output() {
        assert_eq!(parse_memory_total(""), None);
        assert_eq!(parse_memory_total("\n\n"), None);
    }

    #[test]
    fn parse_returns_none_on_garbage() {
        // Driver error / missing GPU produces text rather than a number.
        assert_eq!(
            parse_memory_total("No devices were found\n"),
            None,
            "non-numeric output must collapse to None"
        );
    }

    #[test]
    fn parse_skips_blank_leading_lines() {
        // Defensive: some `nvidia-smi` builds prepend warnings.
        assert_eq!(parse_memory_total("\n\n6144\n"), Some(6144));
    }

    #[test]
    fn q5_floor_is_6_gib_per_adr_0047() {
        // The ADR cites 6 GB as the hard floor; double-check the
        // const so a future copy-edit doesn't silently shift it.
        assert_eq!(Q5_VRAM_FLOOR_MIB, 6 * 1024);
    }
}
