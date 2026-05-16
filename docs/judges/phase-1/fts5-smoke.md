# Judge: fts5-smoke (Phase 1)

**Target:** `src-tauri/src/db/search.rs`, `src-tauri/src/db/migrations/001_initial.sql`

**Question:** After inserting a raw transcript into a fresh in-memory database, does `search::smoke_test_count` return ≥ 1 for a word that appears in the transcript?

**Rationale:** FTS5 wiring (virtual table + triggers in migration 001) is opaque; a silent regression — a dropped trigger, a misconfigured tokenizer, an FTS5 module failing to load — could leave search permanently broken without compile-time signal. The smoke test gives us a binary signal that FTS5 is actually indexing what gets inserted.

**Pass criteria:**

```bash
cargo test --workspace -- \
  db::search::tests::search_finds_inserted_raw_transcript \
  db::search::tests::smoke_test_count_returns_positive_after_insert
```

Both tests pass.

**Additional sanity check:**

```bash
cargo test --workspace -- db_repos::search_after_full_flow_finds_hits_in_all_stages
```

Integration test confirms FTS5 indexes all three transcript stages (raw, cleaned, final).

**On failure:**

- **Block the `phase-1-complete` tag.** FTS5 is a Phase 1 deliverable; without it, search doesn't work and the History viewer (Phase 6) can't ship.
- Check `001_initial.sql` for the FTS5 virtual-table declaration and the two AFTER-INSERT/DELETE triggers.
- Verify `rusqlite` was built with the `bundled` feature (which includes FTS5).

**Last run:** _Wave 5 — passes locally (`cargo test --workspace` green, 101/101 tests including the two smoke targets above)._
