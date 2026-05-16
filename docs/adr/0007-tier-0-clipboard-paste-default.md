# ADR-0007: Tier-0 clipboard paste as injection default

- **Status:** Accepted
- **Date:** 2026-05-15
- **Deciders:** Dustin, code-puppy-adeb7b

## Context

PLAN §6 describes three text-injection tiers:

- **Tier 0:** save clipboard → set clipboard → send Ctrl+V → restore clipboard
- **Tier 1:** UI Automation `ValuePattern.SetValue`
- **Tier 2:** SendInput keystroke synthesis

Each has tradeoffs. We need a default that works in 95%+ of target apps
without per-app configuration, with the other tiers reserved for
documented exceptions.

## Decision

**Tier 0 (clipboard paste with save/restore)** is the default
injection mechanism. Per-app recipes override only when Tier 0 demonstrably fails
in a verified target.

Tier 0's clipboard save/restore is non-optional — implemented in
`src-tauri/src/injection/paste.rs::with_clipboard_saved` (Phase 3),
and the hook `warn-bare-clipboard-set` flags any code path that
writes the clipboard outside that helper.

## Consequences

- **Positive:** works in browsers, editors, chat apps, terminals
  with paste support, Office, Slack, Discord, VS Code, JetBrains
  IDEs, etc. — the vast majority of dictation targets.
- **Negative:** fails in apps that filter paste (some banking
  password fields, certain terminals). Tier 1 (UIA) and Tier 2
  (SendInput) cover the long tail via explicit recipes.
- **Neutral:** clipboard save/restore adds ~20-50ms latency.
  Acceptable in the 800ms per-paste budget.

## Alternatives considered

- **Tier 1 by default:** UIA is slower and varies in coverage —
  some apps don't implement ValuePattern.
- **Tier 2 by default:** synthetic keystrokes break IME composition
  and trigger keyboard shortcuts mid-paste in some apps.

## Cross-references

- PLAN §6 (full state diagram)
- `.code_puppy/skills/injection-recipes/SKILL.md`
- `scripts/hooks/warn-bare-clipboard-set.py`
- AGENTS.md "Principles" #7
