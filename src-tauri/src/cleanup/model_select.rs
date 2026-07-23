//! RAM-aware cleanup-model selection (ADR 0064).
//!
//! The `modes` table stores each mode's **parity** cleanup model — the
//! Windows-tuned default (today a 7B-q4 Qwen for normal/casual/formal).
//! On a memory-constrained machine that parity model may not fit: an
//! 8 GB Apple-silicon Mac shares unified memory between Whisper-Metal,
//! the OS, and Ollama, so a cold 7B-q4 cold-loads in ~55 s and blows
//! the cleanup request timeout → the user silently gets raw passthrough.
//!
//! Rather than mutate the modes table (which must stay parity-pure so
//! the Windows build is unaffected), we substitute the **effective**
//! model at runtime resolution. This module owns two concerns, split
//! along the platform seam from drawer 82 / ADR 0064:
//!
//!   1. A **shared, pure** [`select_model`] — given a memory budget, a
//!      mode default, and the installed Ollama models, it returns the
//!      model id the dictation thread should actually load. It is
//!      deterministic and platform-agnostic so it can be unit-tested
//!      everywhere and reused by a future Windows VRAM provider.
//!   2. A **platform-gated** budget provider ([`detect_memory_budget`]).
//!      On macOS it reads physical unified memory via `sysctl -n
//!      hw.memsize` (subprocess, mirroring [`super::vram_probe`]'s
//!      `nvidia-smi` pattern — no libc FFI dep). On every other target
//!      it returns `None`.
//!
//! **Windows-byte-identical guarantee.** When the budget is `None`
//! (every non-macOS target today, or a detection failure), [`select_model`]
//! returns the mode default *unchanged*. The only caller wires the macOS
//! substitution behind `#[cfg(target_os = "macos")]`, so on Windows the
//! cleanup code path is exactly what it was before this module existed.
//! VRAM != RAM; the Windows provider is a separate, later effort.

/// Memory at or above this threshold is treated as able to run the
/// mode's parity (7B-class) model. Below it, selection caps at the
/// small-model tier. Sourced from drawer 82: ">= 16 GB -> parity 7B,
/// < 16 GB -> 3B". Tiers, deliberately NOT a precise resident-footprint
/// formula — resident size != file size, and the unified-memory ceiling
/// (Metal `recommendedMaxWorkingSetSize`) is itself only ~66-75% of
/// physical RAM.
const HIGH_TIER_MIN_BYTES: u64 = 16 * 1024 * 1024 * 1024; // 16 GiB

/// Parameter-count ceiling (in billions) for the small-memory tier.
/// A 3B-q4 (~1.9 GB) coexists with Whisper-Metal on an 8 GB box; a
/// 7B-q4 (~4.7 GB) does not.
const SMALL_TIER_MAX_BILLIONS: f32 = 3.0;

/// A platform-detected memory budget signal.
///
/// Currently just the physical byte count; modelled as a struct so the
/// future Windows VRAM provider can extend it (e.g. a separate
/// `vram_bytes`) without churning [`select_model`]'s signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBudget {
    /// Physical memory in bytes. On macOS this is unified RAM
    /// (`hw.memsize`); shared between CPU, GPU/Metal, and Ollama.
    pub physical_bytes: u64,
}

// ─────────────────────────────────────────────────────────────────────
// Shared, pure selection logic (compiled + tested on every platform).
// ─────────────────────────────────────────────────────────────────────

/// Resolve the effective cleanup model for the given hardware budget.
///
/// * `budget`        — `None` means "no memory signal" → identity
///   (returns `mode_default` verbatim). This is the Windows-byte-identical
///   path: with no provider, nothing changes.
/// * `mode_default`  — the mode's parity model id from the `modes` table.
/// * `installed`     — Ollama model tags actually pulled locally
///   (`OllamaProvider::list_models`).
///
/// Strategy:
///   1. No budget → keep the parity default (identity).
///   2. Parity-first: if the default fits the budget's tier **and** is
///      installed, keep it (never downgrade capable hardware).
///   3. Otherwise pick the **largest installed** model that fits the
///      tier (best-fit; recommend-a-pull is the caller's job, we never
///      auto-pull multi-GB weights).
///   4. If nothing installed fits, fall back to the parity default and
///      let the runtime-fallback / passthrough safety net handle it.
pub fn select_model(
    budget: Option<MemoryBudget>,
    mode_default: &str,
    installed: &[String],
) -> String {
    let Some(budget) = budget else {
        // No provider / detection failed → identity. Windows lands here.
        return mode_default.to_string();
    };

    let cap = tier_cap_billions(budget.physical_bytes);

    // Parity-first: honour the mode default when the hardware allows it.
    if model_fits(mode_default, cap) && is_installed(mode_default, installed) {
        return mode_default.to_string();
    }

    // Best-fit among installed models under the tier cap.
    best_installed_under_cap(installed, cap).unwrap_or_else(|| mode_default.to_string())
}

/// Resolve the effective cleanup model, honouring an optional user
/// **pin** (per-mode model override, ADR 0066) ahead of the RAM-aware
/// auto-selection.
///
/// * `override_model = Some(id)` — the user explicitly pinned a model in
///   the Modes screen. Return it verbatim, with **no** RAM-aware
///   substitution: an explicit choice beats the heuristic. (`installed`
///   / `budget` are deliberately ignored — we never silently swap a
///   pinned model, even if it looks too big for the box; the runtime
///   passthrough net still protects against a model that won't load.)
/// * `override_model = None` — "Auto": fall through to [`select_model`],
///   which on a `None` budget (every non-macOS target) is the identity.
///   **This is the unchanged, byte-identical default path.**
pub fn select_effective_model(
    budget: Option<MemoryBudget>,
    mode_default: &str,
    installed: &[String],
    override_model: Option<&str>,
) -> String {
    match override_model {
        Some(pinned) => pinned.to_string(),
        None => select_model(budget, mode_default, installed),
    }
}

/// Map a physical-memory budget to a parameter-count ceiling (billions).
/// `>= 16 GiB` → no cap (parity allowed); otherwise the small-model tier.
fn tier_cap_billions(physical_bytes: u64) -> f32 {
    if physical_bytes >= HIGH_TIER_MIN_BYTES {
        f32::INFINITY
    } else {
        SMALL_TIER_MAX_BILLIONS
    }
}

/// True iff `model_id`'s parsed parameter count is within `cap_billions`.
/// An unparseable model id conservatively does NOT fit (so we prefer a
/// known-good installed model over an opaque tag).
fn model_fits(model_id: &str, cap_billions: f32) -> bool {
    param_billions(model_id).is_some_and(|b| b <= cap_billions)
}

/// Exact-match installed check. Ollama tags are canonical, so equality
/// is correct; we deliberately avoid fuzzy matching that could pick a
/// differently-quantised sibling.
fn is_installed(model_id: &str, installed: &[String]) -> bool {
    installed.iter().any(|m| m == model_id)
}

/// Largest installed model whose parameter count fits under the cap.
/// Returns `None` when no installed model parses + fits.
fn best_installed_under_cap(installed: &[String], cap_billions: f32) -> Option<String> {
    installed
        .iter()
        .filter_map(|m| param_billions(m).map(|b| (b, m)))
        .filter(|(b, _)| *b <= cap_billions)
        .max_by(|(a, _), (b, _)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(_, m)| m.clone())
}

/// Extract the parameter count (in billions) encoded in an Ollama model
/// tag: `qwen2.5:7b-instruct-q4_K_M` → `7.0`, `gemma2:2b-...` → `2.0`,
/// `qwen2.5:1.5b-...` → `1.5`. Returns `None` when no `<number>b` token
/// is present (e.g. `nomic-embed-text` — the `b` in "embed" is not
/// preceded by a digit, so it is correctly ignored).
pub fn param_billions(model_id: &str) -> Option<f32> {
    let bytes = model_id.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        if c != b'b' && c != b'B' {
            continue;
        }
        // Walk back over the contiguous [0-9.] run immediately before `b`.
        let mut start = i;
        while start > 0 {
            let prev = bytes[start - 1];
            if prev.is_ascii_digit() || prev == b'.' {
                start -= 1;
            } else {
                break;
            }
        }
        if start == i {
            continue; // no numeric run before this `b`
        }
        let token = &model_id[start..i];
        if token.bytes().any(|b| b.is_ascii_digit()) {
            if let Ok(v) = token.parse::<f32>() {
                return Some(v);
            }
        }
    }
    None
}

/// mb-mac-v1.6.4 — RAM-aware **runtime** fallback chain (Layer 2).
///
/// Layer 1 ([`select_model`]) *predictively* picks a model that should
/// fit the memory budget. This is the defence-in-depth safety net for
/// the borderline case where the chosen model still fails to LOAD /
/// times out at runtime (e.g. a 12 GB box where the tier guessed 7B but
/// it won't coexist with Whisper-Metal + the OS). It returns the
/// installed models strictly SMALLER than `current` (by parsed param
/// count), largest-first, so the runtime can step down one tier at a
/// time until one loads.
///
/// Models that can't be parsed, that equal `current`, or that are `>=`
/// `current` are excluded. If `current` itself can't be parsed we
/// return an empty chain (we can't order a step-down safely). Pure +
/// deterministic → unit-testable on every platform. The *wiring* is
/// macOS-gated: the caller only builds this chain behind
/// `#[cfg(target_os = "macos")]`, so on Windows the chain is always
/// empty and the runtime step-down never fires (byte-identical).
pub fn fallback_chain(current: &str, installed: &[String]) -> Vec<String> {
    let Some(current_b) = param_billions(current) else {
        return Vec::new();
    };
    let mut smaller: Vec<(f32, String)> = installed
        .iter()
        .filter(|m| m.as_str() != current)
        .filter_map(|m| param_billions(m).map(|b| (b, m.clone())))
        .filter(|(b, _)| *b < current_b)
        .collect();
    // Largest-first: step down one tier at a time (least quality loss).
    smaller.sort_by(|(a, _), (b, _)| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    smaller.into_iter().map(|(_, m)| m).collect()
}

// ─────────────────────────────────────────────────────────────────────
// Platform-gated budget provider.
// ─────────────────────────────────────────────────────────────────────

/// macOS: physical unified memory via `sysctl -n hw.memsize`.
///
/// Subprocess (not libc FFI) to mirror [`super::vram_probe`] and avoid a
/// new dependency for a once-per-boot read. Any failure — `sysctl`
/// missing, non-zero exit, unparseable output — collapses to `None`,
/// which [`select_model`] treats as "keep the parity default".
#[cfg(target_os = "macos")]
pub fn detect_memory_budget() -> Option<MemoryBudget> {
    let output = std::process::Command::new("sysctl")
        .arg("-n")
        .arg("hw.memsize")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = std::str::from_utf8(&output.stdout).ok()?;
    parse_memsize(stdout).map(|physical_bytes| MemoryBudget { physical_bytes })
}

/// Non-macOS targets: no unified-memory signal here. Returning `None`
/// makes [`select_model`] a pure identity → the Windows cleanup path is
/// byte-for-byte unchanged. The Windows VRAM provider (VRAM != RAM) is a
/// separate, later effort per drawer 82.
#[cfg(not(target_os = "macos"))]
pub fn detect_memory_budget() -> Option<MemoryBudget> {
    None
}

/// Parse the integer byte count printed by `sysctl -n hw.memsize`.
///
/// `pub` so it is unit-testable on every platform (and counted as
/// reachable library API, which also sidesteps a dead-code lint on
/// non-macOS builds where [`detect_memory_budget`] doesn't call it).
pub fn parse_memsize(stdout: &str) -> Option<u64> {
    stdout.trim().parse::<u64>().ok()
}

/// macOS glue: resolve the effective cleanup model for *this* machine.
///
/// Reads the installed Ollama models, detects the unified-memory budget,
/// and runs [`select_model`]. Logs whether it substituted or kept the
/// parity default. Lives here (not in `runtime_cleaner`) so the single
/// call site there is one cfg-gated line, keeping the non-macOS compile
/// of that function identical to its pre-ADR-0064 form.
///
/// This is also the single, documented seam where a future **manual
/// override** (drawer 82 layer 3 — pin a model in Settings, bypassing
/// auto-select) plugs in: check the override first and short-circuit
/// before calling [`select_model`]. No such setting exists yet
/// (fast-follow bead), so today it always auto-selects.
#[cfg(target_os = "macos")]
pub fn resolve_effective_model(
    provider: &crate::cleanup::OllamaProvider,
    mode_default: String,
    override_model: Option<String>,
) -> String {
    // Auto path (`override_model == None`) is exactly the pre-ADR-0066
    // behaviour. A pin short-circuits the RAM-aware heuristic.
    if let Some(pinned) = override_model {
        tracing::info!(
            parity_model = %mode_default,
            pinned_model = %pinned,
            "per-mode model override (ADR 0066): using user-pinned model, no RAM-aware substitution"
        );
        return pinned;
    }
    let installed = provider.list_models().unwrap_or_default();
    let budget = detect_memory_budget();
    let effective = select_model(budget, &mode_default, &installed);
    let budget_bytes = budget.map_or(0, |b| b.physical_bytes);
    if effective != mode_default {
        tracing::info!(
            parity_model = %mode_default,
            effective_model = %effective,
            budget_bytes,
            "RAM-aware model substitution (macOS unified-memory tier)"
        );
    } else {
        tracing::info!(
            model = %mode_default,
            budget_bytes,
            "RAM-aware selection kept the mode's parity model"
        );
    }
    effective
}

/// macOS glue: build the runtime step-down chain for the effective
/// model on THIS machine, reusing the installed-models list.
///
/// Lists the installed Ollama models and orders those strictly smaller
/// than `effective_model` largest-first via [`fallback_chain`]. Lives
/// behind `#[cfg(target_os = "macos")]` so the chain is only ever
/// populated on macOS — on Windows the runtime step-down stays inert
/// (byte-identical). An unreachable Ollama / empty list yields an
/// empty chain (the passthrough net still protects the user).
#[cfg(target_os = "macos")]
pub fn runtime_fallback_chain(
    provider: &crate::cleanup::OllamaProvider,
    effective_model: &str,
) -> Vec<String> {
    let installed = provider.list_models().unwrap_or_default();
    fallback_chain(effective_model, &installed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(tags: &[&str]) -> Vec<String> {
        tags.iter().map(|s| s.to_string()).collect()
    }

    // ── param_billions ────────────────────────────────────────────────

    #[test]
    fn param_billions_parses_common_tags() {
        assert_eq!(param_billions("qwen2.5:7b-instruct-q4_K_M"), Some(7.0));
        assert_eq!(param_billions("qwen2.5:3b-instruct-q4_K_M"), Some(3.0));
        assert_eq!(param_billions("gemma2:2b-instruct-q4_K_M"), Some(2.0));
        assert_eq!(param_billions("qwen2.5:1.5b-instruct-q4_K_M"), Some(1.5));
        assert_eq!(param_billions("qwen2.5:0.5b"), Some(0.5));
    }

    #[test]
    fn param_billions_ignores_non_param_b() {
        // The `b` in "embed" is not preceded by a digit → ignored.
        assert_eq!(param_billions("nomic-embed-text"), None);
        assert_eq!(param_billions("llama3-no-size"), None);
    }

    #[test]
    fn param_billions_is_case_insensitive_on_b() {
        assert_eq!(param_billions("Foo:7B-instruct"), Some(7.0));
    }

    // ── tier mapping ──────────────────────────────────────────────────

    #[test]
    fn tier_cap_splits_at_16_gib() {
        let eight = 8u64 * 1024 * 1024 * 1024;
        let sixteen = 16u64 * 1024 * 1024 * 1024;
        assert_eq!(tier_cap_billions(eight), SMALL_TIER_MAX_BILLIONS);
        assert!(tier_cap_billions(sixteen).is_infinite());
        assert!(tier_cap_billions(sixteen - 1) == SMALL_TIER_MAX_BILLIONS);
    }

    // ── select_model ──────────────────────────────────────────────────

    #[test]
    fn no_budget_is_identity_windows_byte_identical() {
        // The load-bearing Windows guarantee: with no provider the
        // selector returns the parity default verbatim.
        let inst = installed(&["qwen2.5:3b-instruct-q4_K_M"]);
        assert_eq!(
            select_model(None, "qwen2.5:7b-instruct-q4_K_M", &inst),
            "qwen2.5:7b-instruct-q4_K_M"
        );
    }

    #[test]
    fn eight_gb_downgrades_7b_to_installed_3b() {
        let budget = Some(MemoryBudget {
            physical_bytes: 8 * 1024 * 1024 * 1024,
        });
        let inst = installed(&["qwen2.5:7b-instruct-q4_K_M", "qwen2.5:3b-instruct-q4_K_M"]);
        assert_eq!(
            select_model(budget, "qwen2.5:7b-instruct-q4_K_M", &inst),
            "qwen2.5:3b-instruct-q4_K_M"
        );
    }

    #[test]
    fn sixteen_gb_keeps_parity_7b() {
        let budget = Some(MemoryBudget {
            physical_bytes: 16 * 1024 * 1024 * 1024,
        });
        let inst = installed(&["qwen2.5:7b-instruct-q4_K_M", "qwen2.5:3b-instruct-q4_K_M"]);
        assert_eq!(
            select_model(budget, "qwen2.5:7b-instruct-q4_K_M", &inst),
            "qwen2.5:7b-instruct-q4_K_M"
        );
    }

    #[test]
    fn ideal_not_installed_falls_back_to_best_installed() {
        // 16 GB (parity allowed) but the 7B isn't pulled → best installed.
        let budget = Some(MemoryBudget {
            physical_bytes: 16 * 1024 * 1024 * 1024,
        });
        let inst = installed(&["qwen2.5:3b-instruct-q4_K_M", "gemma2:2b-instruct-q4_K_M"]);
        assert_eq!(
            select_model(budget, "qwen2.5:7b-instruct-q4_K_M", &inst),
            "qwen2.5:3b-instruct-q4_K_M"
        );
    }

    #[test]
    fn nothing_fits_falls_back_to_parity_default() {
        // 8 GB box, only the (too-big) 7B installed → no fit under cap →
        // keep the parity default; the runtime/passthrough net handles it.
        let budget = Some(MemoryBudget {
            physical_bytes: 8 * 1024 * 1024 * 1024,
        });
        let inst = installed(&["qwen2.5:7b-instruct-q4_K_M"]);
        assert_eq!(
            select_model(budget, "qwen2.5:7b-instruct-q4_K_M", &inst),
            "qwen2.5:7b-instruct-q4_K_M"
        );
    }

    #[test]
    fn empty_installed_keeps_default() {
        let budget = Some(MemoryBudget {
            physical_bytes: 8 * 1024 * 1024 * 1024,
        });
        assert_eq!(
            select_model(budget, "qwen2.5:7b-instruct-q4_K_M", &[]),
            "qwen2.5:7b-instruct-q4_K_M"
        );
    }

    // ── select_effective_model (ADR 0066 override layer) ──────────────

    #[test]
    fn override_pin_beats_ram_aware_substitution() {
        // 8 GB box that would normally downgrade 7B → 3B, but the user
        // pinned the 7B explicitly → honour the pin, no substitution.
        let budget = Some(MemoryBudget {
            physical_bytes: 8 * 1024 * 1024 * 1024,
        });
        let inst = installed(&["qwen2.5:7b-instruct-q4_K_M", "qwen2.5:3b-instruct-q4_K_M"]);
        assert_eq!(
            select_effective_model(
                budget,
                "qwen2.5:7b-instruct-q4_K_M",
                &inst,
                Some("qwen2.5:7b-instruct-q4_K_M"),
            ),
            "qwen2.5:7b-instruct-q4_K_M"
        );
    }

    #[test]
    fn no_override_is_ram_aware_auto() {
        // "Auto" (None) on an 8 GB box behaves exactly like select_model.
        let budget = Some(MemoryBudget {
            physical_bytes: 8 * 1024 * 1024 * 1024,
        });
        let inst = installed(&["qwen2.5:7b-instruct-q4_K_M", "qwen2.5:3b-instruct-q4_K_M"]);
        assert_eq!(
            select_effective_model(budget, "qwen2.5:7b-instruct-q4_K_M", &inst, None),
            select_model(budget, "qwen2.5:7b-instruct-q4_K_M", &inst),
        );
    }

    #[test]
    fn no_override_no_budget_is_windows_byte_identical() {
        // The load-bearing guarantee: Auto + no budget = parity default.
        let inst = installed(&["qwen2.5:3b-instruct-q4_K_M"]);
        assert_eq!(
            select_effective_model(None, "qwen2.5:7b-instruct-q4_K_M", &inst, None),
            "qwen2.5:7b-instruct-q4_K_M"
        );
    }

    // ── parse_memsize ─────────────────────────────────────────────────

    // ── fallback_chain (mb-mac-v1.6.4 runtime step-down) ──────────────

    #[test]
    fn fallback_chain_orders_smaller_models_largest_first() {
        let inst = installed(&[
            "qwen2.5:7b-instruct-q4_K_M",
            "qwen2.5:3b-instruct-q4_K_M",
            "gemma2:2b-instruct-q4_K_M",
            "qwen2.5:0.5b",
        ]);
        // Stepping down from 7B: 3B, then 2B, then 0.5B (never the 7B).
        assert_eq!(
            fallback_chain("qwen2.5:7b-instruct-q4_K_M", &inst),
            vec![
                "qwen2.5:3b-instruct-q4_K_M".to_string(),
                "gemma2:2b-instruct-q4_K_M".to_string(),
                "qwen2.5:0.5b".to_string(),
            ]
        );
    }

    #[test]
    fn fallback_chain_excludes_equal_and_larger_and_self() {
        let inst = installed(&[
            "qwen2.5:7b-instruct-q4_K_M",
            "other:3b-instruct-q4_K_M",
            "qwen2.5:3b-instruct-q4_K_M", // same size as current, different tag
        ]);
        // From the 3B: only strictly-smaller models qualify. The 7B is
        // larger; the sibling 3B is equal → both excluded. Nothing left.
        assert_eq!(
            fallback_chain("qwen2.5:3b-instruct-q4_K_M", &inst),
            Vec::<String>::new()
        );
    }

    #[test]
    fn fallback_chain_empty_when_current_unparseable() {
        let inst = installed(&["qwen2.5:3b-instruct-q4_K_M"]);
        assert_eq!(
            fallback_chain("nomic-embed-text", &inst),
            Vec::<String>::new()
        );
    }

    #[test]
    fn fallback_chain_skips_unparseable_installed() {
        let inst = installed(&["nomic-embed-text", "gemma2:2b-instruct-q4_K_M"]);
        assert_eq!(
            fallback_chain("qwen2.5:7b-instruct-q4_K_M", &inst),
            vec!["gemma2:2b-instruct-q4_K_M".to_string()]
        );
    }

    #[test]
    fn parse_memsize_reads_8gib() {
        // The literal `sysctl -n hw.memsize` value on the canary box.
        assert_eq!(parse_memsize("8589934592\n"), Some(8_589_934_592));
    }

    #[test]
    fn parse_memsize_trims_and_rejects_garbage() {
        assert_eq!(parse_memsize("  17179869184  \r\n"), Some(17_179_869_184));
        assert_eq!(parse_memsize("not-a-number"), None);
        assert_eq!(parse_memsize(""), None);
    }
}
