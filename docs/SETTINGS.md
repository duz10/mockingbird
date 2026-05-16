# Mockingbird settings reference

All settings live in the `settings` table:

```sql
CREATE TABLE settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
```

Values are JSON-encoded TEXT, so any shape round-trips: booleans as
`true`/`false`, integers as `42`, strings as `"theme-dark"`,
arrays/objects as JSON literals.

## Access

- **Rust:** `mockingbird_lib::settings::Settings` (typed via
  `get<T>`/`set<T>` over `serde_json` round-trip; raw via
  `get_raw`/`set_raw`).
- **Frontend (Phase 5+):** Tauri commands `get_setting(key)` and
  `set_setting(key, value)`. Both return `Result<..., String>`.
- **Direct SQL:** discouraged. Use the facade — it handles defaults,
  corrupt-value fallback, and UPSERT semantics.

## Key registry

| Key | Type | Default | Owner phase | Notes |
|-----|------|---------|-------------|-------|
| `autostart_enabled` | bool | `false` | Phase 1 | Start at login. Phase 4 wires OS registration. |
| `log_level` | string | `"info"` | Phase 1 | One of `trace`/`debug`/`info`/`warn`/`error`. Picked up at next start via `RUST_LOG` env override or this key. |
| `theme` | string | `"system"` | Phase 5 | `system` \| `light` \| `dark`. |
| `reduced_motion` | bool | `false` | Phase 5 | Disable UI animations. |
| `sound_feedback` | bool | `true` | Phase 5 | Play recording start/stop beep. |
| `claude_api_key_ref` | string \| null | `null` | Phase 4 | Reference (NOT the secret) to the Windows Credential Manager entry holding the Claude API key. |
| `audio_retention_days` | int | `30` | Phase 5 | Days to keep audio blob files. `0` = forever, `-1` = never store. |
| `learning_enabled` | bool | `true` | Phase 8 | Run the learning loop on a schedule. |

## Defaults

Defaults live in `SettingKey::default_value()` and are returned by
`Settings::get_raw` when:
1. The key has never been written, OR
2. The stored value is corrupted (not valid JSON) — in which case
   `Settings::get_raw` logs a `warn` and falls back.

## Adding a new setting

1. Add a variant to `SettingKey` in
   `src-tauri/src/settings/model.rs`.
2. Add it to `as_str`, `try_parse`, `default_value`, `all` (four
   match arms).
3. Add a row to the table above.
4. If it changes existing behavior, drop a note in
   `docs/LESSONS.md`.
5. (Phase 5+) Surface in the Settings UI.

## Corrupt-value behavior

`Settings::get_raw` is permissive: a parse failure → warn log →
return the default. This is intentional: we'd rather the app keep
running with a sane default than crash on a malformed config. The
malformed row stays until the next `set_raw` overwrites it.

Tests pin this contract:
`settings::tests::corrupt_stored_value_falls_back_to_default`.

## Why JSON-in-TEXT, not native types

SQLite's type affinity is loose; we'd be reading/writing strings
anyway. JSON gives a uniform encoder/decoder pair, supports
arbitrarily-shaped values without schema changes, and round-trips
cleanly through Tauri's IPC layer (which speaks JSON natively).

## Cross-references

- `src-tauri/src/settings/model.rs` — typed keys
- `src-tauri/src/settings/mod.rs` — facade
- `src-tauri/src/commands.rs` — Tauri command wrappers
- `.code_puppy/skills/quality/SKILL.md` — quality bar
- `PLAN-mockingbird-v2.md` Section 10 Phase 1 deliverables
