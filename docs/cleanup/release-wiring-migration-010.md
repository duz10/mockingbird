# Release-build wiring for migration 010 (ADR 0024 Wave D)

> Status: ready to ship.
> Owner: implementor (code-puppy / Bernard).
> Last updated: 2026-05-18.

This document traces the full path a prompt edit takes from
`src-tauri/src/cleanup/prompts/casual_v2.md` all the way to a user's
local SQLite database on app launch. It exists so that future prompt
revisions (and ADR 0024-style empirical iterations) can ship without
re-deriving the chain.

## The chain

```text
src-tauri/src/cleanup/prompts/casual_v2.md       (markdown file)
                  │
                  ▼  include_str! at compile time
src-tauri/src/db/prompt_loader.rs                (PROMPT_CASUAL_V2 const)
                  │
                  ▼  substitute_prompt_bodies(MIGRATION_010)
src-tauri/src/db/migrations/010_adr0024_prompt_v2.sql
                  │
                  ▼  conn.execute_batch(&prepared)
%APPDATA%\Mockingbird\mockingbird.db                  (user's SQLite file)
                  │
                  ▼  ModeRepository::resolve(slug)
LlmCleaner::clean(...)                                (production cleanup path)
```

Every arrow is automatic — no hand-step. Compile the release binary,
ship it, on first launch the migration runner sees `schema_version=9`
in the user's DB, applies migration 010, and bumps to `10`. Subsequent
launches no-op (the `if current < 10` guard in
`src-tauri/src/db/migrations.rs` is satisfied).

## What ships in migration 010

1. **Three new `prompts` rows.** `casual@2`, `normal@5`, `formal@2`.
   `id` is auto-assigned by SQLite. ADR 0008 compliance: v1/v4 rows
   are NOT touched — historical sessions in the user's DB continue to
   resolve their original prompt body via their stored `prompt_id`.
2. **Three `modes` row updates.** Each mode's `prompt_id` column now
   points at its new latest row. Other columns (`model_id`,
   `max_tokens`, etc.) are unchanged except for…
3. **Casual `temperature` 0.4 → 0.2.** Defensive: reduces creative
   drift on the 3B model under length pressure, complementing the
   anti-substitution rule added to `casual_v2`. Other mode rows'
   temperatures are unchanged.
4. **`schema_meta.schema_version` → `10`.**

Three writes (prompts INSERT), three writes (modes UPDATE), one write
(schema_meta UPDATE). All inside a `BEGIN TRANSACTION ... COMMIT` so
partial application is impossible.

## How to verify on a real install

After installing a new release build over an existing v0.x install:

```sql
-- From sqlite3 against %APPDATA%\Mockingbird\mockingbird.db:
SELECT key, value FROM schema_meta WHERE key = 'schema_version';
-- expect: schema_version | 10

SELECT m.slug, m.temperature, p.mode_slug, p.version, length(p.body)
  FROM modes m JOIN prompts p ON m.prompt_id = p.id
 WHERE m.slug IN ('casual', 'normal', 'formal')
 ORDER BY m.slug;
-- expect three rows:
--   casual | 0.2 | casual | 2 | ~3000
--   formal | 0.6 | formal | 2 | ~5000
--   normal | 0.4 | normal | 5 | ~2400
-- (sizes approximate; the point is that version is 2/5/2 not 1/4/1)

SELECT COUNT(*) FROM prompts;
-- expect: previous count + 3 (the v1 / v4 rows are still there)
```

The app logs a one-liner at startup confirming the migration ran:

```text
[INFO] db::migrations: applied migration 010 (schema_version 9 → 10)
```

(That log line is emitted by the existing `apply_all` runner — no new
logging required for migration 010 specifically; the guard pattern
inherits the log.)

## Smoketest plan post-deploy

Once the release ships, run three short dictations to confirm the new
prompts are live:

1. **Casual smoketest.** Dictate a long technical sentence — e.g. "the
   cleanup pipeline takes the raw whisper output and runs it through
   the deterministic preprocessor then through the local llm". Expect
   the technical terms preserved verbatim, light casual cleanup, NO
   appearance of `"milk, eggs, and bread"` text (regression-canary
   from iter-0).
2. **Normal smeoketest.** Dictate the Santa list (`"I'm making a list
   of things and checking it twice..."`). Expect a 3-bullet list with
   the lead-in sentence preserved AND the question marks rendered.
3. **Formal smoketest.** Dictate a casual grocery request (`"hey can
   you grab milk on the way home"`) **in formal mode**. Expect a
   polite formal rendering ("Could you please pick up milk on the way
   home?") and **explicitly NOT** a content-policy refusal lecture
   (regression-canary from iter-1).

If any smoketest fails, the rollback is straightforward: ship a
migration 011 that flips the affected mode's `prompt_id` back to the
previous version. Old prompt rows are still in the DB by ADR 0008.

## Why no new code outside the migration

The cleanup pipeline (`LlmCleaner` in `src-tauri/src/cleanup/llm.rs`)
already resolves the prompt body via
`ModeRepository::resolve(mode_slug)` on every cleanup call. That
resolver does `SELECT body FROM prompts WHERE id = modes.prompt_id` —
so once migration 010 has repointed the modes table, every subsequent
cleanup call picks up the new body. **No restart required after the
migration runs.** (The migration itself runs at app launch, which is
implicitly a restart; but the LLM cleaner does not cache prompts.)

This is the same wiring that delivered `normal_v2` (migration 006),
`normal_v3` (migration 007), and the original three-mode set
(migration 008). Migration 010 is the fourth use of this pattern;
nothing new in the mechanism, just new content flowing through it.

## What this doc deliberately does NOT cover

- **UI changes.** The mode-selector dropdown UI is unchanged; the
  three modes (`casual`/`normal`/`formal`) keep the same slugs and
  display names. No frontend work required for migration 010.
- **Latency improvements.** Streaming + LLM-skip for short casual
  (the actual route to Wisprflow-parity latency) are ticketed under
  mb-cjc Wave 3 and are out of scope here. Migration 010 is purely a
  preservation + correctness ship.
- **Eval rig itself.** The `mode_eval` bin is a developer tool, not
  user-facing. It does not ship in the release binary
  (`#[cfg(any(test, ...))]` is not used; the bin compiles only when
  invoked with `cargo build --bin mode_eval` — it is excluded from
  default `tauri build`). The eval harness lives in `src/bin/` and
  is invoked explicitly for prompt iteration; no user is exposed to
  it. (Future revision: if we want to ship a "self-check" UI button
  that runs a tiny mode_eval against bundled fixtures, that becomes
  a separate ADR with its own ship vehicle.)

---

_Cross-references: ADR 0008 (prompt versioning), ADR 0022 (three-mode
pipeline, now Accepted), ADR 0024 (empirical mode tuning), bd mb-35t
(this wave)._
