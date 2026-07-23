//! `mac_ram_aware_select_probe` — judge for ADR 0064 (mb-mac-v1.6.3).
//!
//! Proves the RAM-aware cleanup-model selector behaves correctly:
//!
//! MOCKED budgets (cross-platform, no Ollama needed):
//!   - 8 GB  + {7B,3B} installed -> 3B  (downgrade, fits unified mem)
//!   - 16 GB + {7B,3B} installed -> 7B  (parity kept)
//!   - 16 GB + {3B,2B} installed -> 3B  (ideal not installed -> best)
//!   - no budget (Windows path)  -> 7B  (identity / byte-identical)
//!
//! REAL sysctl (macOS only): `hw.memsize` on THIS box feeds the
//! selector against the live Ollama install. On the 8 GB canary the
//! parity 7B must resolve to the 3B. Requires `ollama serve` with
//! qwen2.5:3b pulled; otherwise the real sub-check SKIPs (the mocked
//! cases still gate).
//!
//! Run:
//!   scripts/dev/cargo-mac.sh run --release --example mac_ram_aware_select_probe
//!
//! Exit code: 0 = all gated checks passed; 1 = a check failed.

use mockingbird_lib::cleanup::model_select::{select_model, MemoryBudget};

const PARITY_7B: &str = "qwen2.5:7b-instruct-q4_K_M";
const SMALL_3B: &str = "qwen2.5:3b-instruct-q4_K_M";
const TINY_2B: &str = "gemma2:2b-instruct-q4_K_M";

const GIB: u64 = 1024 * 1024 * 1024;

fn v(tags: &[&str]) -> Vec<String> {
    tags.iter().map(|s| s.to_string()).collect()
}

/// Assert `got == want`; print a PASS/FAIL line. Returns 1 on failure.
fn check(label: &str, got: &str, want: &str) -> i32 {
    if got == want {
        println!("  PASS  {label}: -> {got}");
        0
    } else {
        println!("  FAIL  {label}: got {got}, want {want}");
        1
    }
}

fn main() {
    println!("=== mac_ram_aware_select_probe (ADR 0064 — mb-mac-v1.6.3) ===");
    let mut failures = 0;

    // ── Mocked budgets (run on every platform) ──────────────────────
    println!("\n[mocked budgets]");
    let both = v(&[PARITY_7B, SMALL_3B]);

    failures += check(
        "8GB {7B,3B}",
        &select_model(
            Some(MemoryBudget {
                physical_bytes: 8 * GIB,
            }),
            PARITY_7B,
            &both,
        ),
        SMALL_3B,
    );
    failures += check(
        "16GB {7B,3B}",
        &select_model(
            Some(MemoryBudget {
                physical_bytes: 16 * GIB,
            }),
            PARITY_7B,
            &both,
        ),
        PARITY_7B,
    );
    failures += check(
        "16GB ideal-not-installed {3B,2B}",
        &select_model(
            Some(MemoryBudget {
                physical_bytes: 16 * GIB,
            }),
            PARITY_7B,
            &v(&[SMALL_3B, TINY_2B]),
        ),
        SMALL_3B,
    );
    failures += check(
        "no-budget identity (Windows byte-identical path)",
        &select_model(None, PARITY_7B, &both),
        PARITY_7B,
    );

    // ── Real sysctl + live Ollama (macOS only) ──────────────────────
    println!("\n[real sysctl on this box]");
    #[cfg(target_os = "macos")]
    {
        use mockingbird_lib::cleanup::model_select::detect_memory_budget;
        use mockingbird_lib::cleanup::OllamaProvider;

        let budget = detect_memory_budget();
        match budget {
            Some(b) => println!(
                "  detected hw.memsize = {} bytes (~{} GiB)",
                b.physical_bytes,
                b.physical_bytes / GIB
            ),
            None => println!("  WARN  sysctl hw.memsize detection returned None"),
        }

        let provider = OllamaProvider::new();
        match provider.list_models() {
            Ok(installed) if installed.iter().any(|m| m == SMALL_3B) => {
                let effective = select_model(budget, PARITY_7B, &installed);
                let want = if budget
                    .map(|b| b.physical_bytes >= 16 * GIB)
                    .unwrap_or(false)
                {
                    // A >=16GB Mac running this probe legitimately keeps 7B.
                    PARITY_7B
                } else {
                    SMALL_3B
                };
                failures += check("real sysctl + live Ollama", &effective, want);
            }
            Ok(_) => println!(
                "  SKIP  Ollama reachable but {SMALL_3B} not pulled — \
                 cannot prove the downgrade target"
            ),
            Err(e) => println!("  SKIP  Ollama not reachable ({e}) — start `ollama serve`"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        println!("  N/A   non-macOS: no unified-memory provider (selector is identity here)");
    }

    println!(
        "\n=== {} ===",
        if failures == 0 {
            "ALL CHECKS PASSED"
        } else {
            "FAILURES PRESENT"
        }
    );
    if failures != 0 {
        std::process::exit(1);
    }
}
