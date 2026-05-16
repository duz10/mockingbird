---
name: injection-recipes
description: Per-app text-injection recipes for Mockingbird, organized as Tier 0 (universal Ctrl+V paste with clipboard save/restore), Tier 1 (UI Automation Value Pattern), Tier 2 (SendInput synthesis). Activate this skill whenever you are touching `src-tauri/src/injection/`, adding a new target app recipe, or debugging why a paste failed in a specific application.
---

# Mockingbird injection recipes

## Three-tier strategy (PLAN Section 6)

| Tier | Mechanism | Use when |
|------|-----------|----------|
| 0    | Save clipboard → set clipboard → send Ctrl+V → wait → restore clipboard | Default. Works in 95% of targets. |
| 1    | UI Automation `ValuePattern.SetValue` | Targets that filter synthetic keystrokes (banking, password fields that *aren't* secure-input). |
| 2    | `SendInput` synthesis char-by-char | Last resort: terminals, RDP, games. |

A recipe is `(app_id, tier, params)`. We start with one global Tier-0
recipe and add overrides over time (Phase 3 and ongoing).

## Mandatory guards

1. **Clipboard save/restore is non-negotiable** (PLAN §12.17).
   The `paste::with_clipboard_saved` helper is the *only* sanctioned
   path. Hook `warn-bare-clipboard-set` flags bare `Set-Clipboard` etc.

2. **Secure-input fields abort injection.** Before any paste, check
   `SecureInputGuard::is_blocked()`. If true: toast and return — never
   inject into password fields.

3. **Foreground-app capture must happen *before* the paste.** Otherwise
   focus moves to the Mockingbird HUD and you paste into yourself.
   That's an embarrassing demo.

4. **Per-recipe timeout.** No paste path may block longer than 800 ms
   end-to-end. If a recipe is slow, gate it behind a flag and fall
   back to Tier 0.

## Adding a new recipe

1. Add an integration row in the `app_recipes` table (migration).
2. Implement the recipe in `injection::recipes::<app_id>::paste`.
3. Add a Playwright/uia-based test (qa-kitten owns this).
4. Document the *why* in `docs/recipes/<app_id>.md`.
5. Add an entry to `docs/LESSONS.md` if the recipe is non-obvious.

## What `injection-author` (the project agent) needs

When `invoke_agent("injection-author", ...)`, hand it:
- Target app's process name + AUMID
- Symptom (e.g. "Ctrl+V pastes literal '^V'")
- Sample input the test should send
- Expected behavior on success

## Cross-references

- PLAN Section 6 — full injection state diagram
- ADR 0006 (Tier-0 default) — write if not present
- `src-tauri/src/injection/paste.rs` — the clipboard guard
