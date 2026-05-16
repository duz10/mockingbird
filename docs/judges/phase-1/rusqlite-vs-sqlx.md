# Judge: rusqlite-vs-sqlx (Phase 1)

**Target:** `src-tauri/**/*.rs`, `Cargo.toml`, `src-tauri/Cargo.toml`, `Cargo.lock`

**Question:** Is `rusqlite` the SQLite driver throughout, with `sqlx` and `tauri-plugin-sql` absent from all dependency lists and source imports?

**Rationale:** ADR 0004 chose `rusqlite` over `sqlx` for synchronous semantics, smaller dependency tree, explicit transaction control, and avoiding async runtime contamination of the data layer. Drift back to `sqlx` would require rewriting the migration runner, every repository module, and the audit subsystem.

**Pass criteria:**

1. `grep -E "(^|[^a-zA-Z])sqlx([^a-zA-Z]|$)" src-tauri/ Cargo.toml src-tauri/Cargo.toml` returns **zero** non-comment matches.
2. `grep "tauri-plugin-sql" src-tauri/ Cargo.toml src-tauri/Cargo.toml` returns **zero** non-comment matches.
3. `Cargo.lock` does not contain `sqlx` as a top-level dependency.
4. All DB access in `src-tauri/src/db/*.rs` uses `rusqlite::Connection`.

**On failure:**

- Note in `docs/LESSONS.md` with the offending change's commit SHA.
- File a `bd` task to roll back.
- Block release until reverted; this is a foundational architectural choice.

**Last run:** _Wave 5 (judge cards minted; Wiggum to execute when wired up)._
