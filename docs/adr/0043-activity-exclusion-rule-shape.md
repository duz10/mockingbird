# ADR 0043 — Activity exclusion rule shape + capture-time enforcement

- **Status:** Accepted
- **Date:** 2026-05-26
- **Phase:** 10 (Wave 5 — Hardening)
- **Author:** code-puppy (Bernard) on behalf of Dustin
- **Supersedes:** none
- **Superseded by:** none

## Context

Phase 10 Wave 5 introduces an exclusion list: per-app / per-window /
system-rule policies that prevent matching events from ever reaching
`activity_events`. Cross-wave invariant #4: **exclusion list honored
at capture, not display.**

The implementation question is the rule shape. Two extremes:

- **Strict glob list** — only literal app names. Simple, fast, brittle.
  Doesn't catch "Chrome with `bank of america - login` in the title".
- **Arbitrary user-script** — a JS/lua expression evaluated per event.
  Maximally flexible, security horrorshow.

We need something in the middle.

## Decision

Three rule kinds, stored in a new `activity_exclusion_rules` table:

| `kind`        | `pattern` semantics                                                                | Example                                  |
|---------------|------------------------------------------------------------------------------------|------------------------------------------|
| `app_glob`    | Case-insensitive glob against `app_name`. Supports `*` and `?` only.               | `1Password*`, `consent.exe`              |
| `title_regex` | Case-insensitive `regex` crate pattern against `window_title`.                     | `(?i)\b(bank\|login\|password)\b`        |
| `system`      | Sentinel string naming a built-in policy. Currently only `password_field_active`.  | `password_field_active`                  |

### Capture-time enforcement contract

Inside `ActivityCaptureRuntime::record_event`, **before** any
`insert_event` call:

1. For `SamplerEvent::AppSwitch { app, title, .. }` or
   `SamplerEvent::ContextSnapshot { app, title, snapshot_json, .. }`,
   the matcher is consulted with `(app, title, password_field_active)`.
2. If any enabled rule matches, the entire event is dropped (no INSERT,
   no row created). A `tracing::debug` line is emitted at
   `target: "activity::exclusion"` so power users can see what was
   filtered without the data ever touching disk.
3. Idle/control events (`idle_start`, `idle_end`, `paused`, `resumed`,
   `layer_error`) are NEVER excluded — they have no content payload and
   are essential session-flow markers.

### `system:password_field_active` rule

Reads `snapshot_json` for a `"password_field_active": true` key (set by
the Wave-2 UIA probe). When matched, the ENTIRE `ContextSnapshot` event
is dropped, NOT just redacted. The kickoff explicitly mandates this:
"stronger than `SecureInputGuard` because it works on any focused
edit across any UIA-exposing app."

### Built-in rules

Ship with the migration. Default-enabled, marked `is_builtin = 1`.
The UI lets users disable them but NOT delete them. User-created
rules (kind+pattern combo not on the built-in seed list) can be
freely added/edited/deleted.

The Wave-5 built-in seed list:

| `kind`        | `pattern`                                            | `note`                                |
|---------------|------------------------------------------------------|---------------------------------------|
| `app_glob`    | `1Password*`                                         | 1Password credentials manager         |
| `app_glob`    | `Bitwarden*`                                         | Bitwarden credentials manager         |
| `app_glob`    | `KeePass*`                                           | KeePass credentials manager           |
| `app_glob`    | `LastPass*`                                          | LastPass credentials manager          |
| `app_glob`    | `consent.exe`                                        | Windows UAC consent dialog            |
| `app_glob`    | `LogonUI.exe`                                        | Windows lock screen / sign-in         |
| `title_regex` | `(?i)\b(bank\|login\|password\|signin\|sign-in)\b`   | Browser sign-in / banking tabs        |
| `system`      | `password_field_active`                              | UIA-detected password input focus     |

### Glob semantics

We are NOT shipping the `glob` crate for this. The `app_glob` matcher
is implemented in `activity/exclusion.rs` as a hand-rolled walk that
honors `*` (zero-or-more chars, case-insensitive) and `?` (exactly one
char). No character classes, no `[abc]`, no escaping. The pattern
language is documented in the Settings UI tooltip ("Use `*` and `?`
only; matching is case-insensitive"). YAGNI.

### Regex compilation

`title_regex` patterns are compiled lazily into a `Vec<regex::Regex>`
on matcher load. Invalid patterns are logged + skipped (don't crash
the runtime); the Settings UI validates patterns on save via a
round-trip IPC (`activity_exclusion_validate`).

## Storage

Migration 015 adds:

```sql
CREATE TABLE activity_exclusion_rules (
  id          TEXT PRIMARY KEY,                 -- ULID-style; built-ins use 'builtin-<slug>'
  kind        TEXT NOT NULL,                    -- 'app_glob' | 'title_regex' | 'system'
  pattern     TEXT NOT NULL,
  enabled     INTEGER NOT NULL DEFAULT 1,
  is_builtin  INTEGER NOT NULL DEFAULT 0,
  note        TEXT,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX idx_activity_exclusion_rules_enabled
  ON activity_exclusion_rules(enabled, kind);
```

## Matcher lifecycle

- Loaded once at `ActivityCaptureRuntime::spawn` from the DB.
- Held in `Arc<RwLock<ExclusionMatcher>>` inside the runtime.
- Reloaded via `runtime.reload_exclusion_rules()` when the IPC layer
  edits a rule. The IPC layer is responsible for the round-trip; the
  matcher doesn't subscribe to DB change events.
- Cheap clones of the matcher are taken for each event via a read
  guard; the read-side is hot, the write-side is cold.

## Alternatives considered

- **glob crate** — pulls in `aho-corasick` transitively + ~10kloc of
  code for what is, in practice, a 30-line walker. Rejected.
- **Single regex per rule** — would unify `app_glob` and `title_regex`
  but blurs the user mental model. The Settings UI presents them as
  two distinct affordances; mirroring that at the data layer is
  clearer.
- **`hosts_file` / browser-extension-style domain blocklists** — out
  of scope; `title_regex` covers browser tabs via title matching.

## Test plan

- Pure-Rust matcher unit tests (throwaway crate):
  - empty matcher → never excludes
  - case-insensitive glob match (`1Password*` matches `1Password 7`)
  - glob `?` matches exactly one char
  - title regex match
  - `system:password_field_active` true → match regardless of app/title
  - disabled rule → ignored
  - first matching rule wins; matcher returns the rule id for logging
- Runtime integration test: rules table seeded with `1Password*`,
  feed a `SamplerEvent::AppSwitch { app: "1Password 7" }`, assert no
  row in `activity_events`.
- Idle/control events are never excluded — explicit test.

## UI surface

Settings → Activity → "Exclusion rules" section:

- Table of rules with columns: `kind`, `pattern`, `note`, `enabled` toggle.
- Built-ins shown with a 🔒 lock icon next to the delete button (toggle
  enabled, can't delete).
- "Add rule" button opens a small inline form (kind picker + pattern
  textarea + note). Save validates via `activity_exclusion_validate`.
- "Test rule" affordance — paste a sample app+title and see which rules
  would match. Defers to v1.1; Wave 5 ships without.
