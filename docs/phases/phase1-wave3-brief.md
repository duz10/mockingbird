# Phase 1 Wave 3 — Implementation brief

> **Read this BEFORE writing any DB repository module.** It captures
> every design decision PLAN §7 + Wave 2 don't fully pin down, so
> Wave 3 is mechanical implementation, not architectural decision-
> making under context pressure. Written end-of-Wave-2 by
> code-puppy-adeb7b with fresh context on the migrations + runner.
>
> Wave 2's brief produced 15/15 tests first run with zero escalations
> (see `docs/LESSONS.md` "Wave-specific briefs ship 100% test pass
> rates"). Same pattern here. **Treat as binding.**

## Tasks in scope

| bd id    | File                                       | Approx. lines | Owner       |
|----------|--------------------------------------------|---------------|-------------|
| `mb-7oi` | `src-tauri/src/db/transcripts.rs`          | ~200          | code-puppy  |
| `mb-4f8` | `src-tauri/src/db/search.rs`               | ~150          | code-puppy  |
| `mb-9pn` | `src-tauri/src/db/sessions.rs`             | ~300          | code-puppy  |
| `mb-91x` | `src-tauri/src/db/prompts.rs`              | ~120          | code-puppy  |
| `mb-d5z` | `src-tauri/src/db/dictionary.rs`           | ~250          | code-puppy  |
| `mb-z4k` | `src-tauri/src/db/examples.rs`             | ~220          | code-puppy  |
| `mb-344` | `src-tauri/src/db/audit.rs`                | ~300          | code-puppy  |
| (NEW)    | `src-tauri/tests/db_repos.rs`              | ~250          | code-puppy  |

Plus housekeeping in `src-tauri/src/db/mod.rs`:
- Add `pub mod transcripts;` … `pub mod audit;` (7 new public submodules)
- A `#[cfg(test)]` `pub use Database::open_in_memory;` is **not** needed — already `pub`.

**Total budget:** ~1,790 lines across 8 files. Every file well under
the 600-line per-file hard limit.

## Cross-cutting decisions (binding)

These apply across every module — get them right once, ride them through
all 7 files.

### 1. No `Repository` trait abstraction in Wave 3

Planning-agent originally suggested "mockall trait boundary" on some
modules. **Don't add traits yet.** YAGNI: Wave 4 commands haven't been
written, we don't know which call sites benefit from mockable trait
objects, and concrete `&Connection` signatures are simpler. Wave 4 can
introduce traits if/when a specific command needs to mock a repo.

### 2. Every public function takes `&Connection`, not `&Database`

Repos are stateless and depend only on the connection. Taking `&Database`
would couple them to the wrapper type for no semantic gain. Wave 4's
command handlers will extract `&conn` from `tauri::State<Database>`.

### 3. Every public function returns `AppResult<T>`

`AppError` already has the `Sqlite(#[from] rusqlite::Error)` variant
(added in Wave 2). No new variants required for Wave 3 unless a module
surfaces a domain error that doesn't naturally fit (e.g. "raw transcript
already exists" — see §transcripts).

### 4. Timestamps are `String` (ISO 8601), not `chrono::DateTime`

SQLite stores them as TEXT (`DEFAULT CURRENT_TIMESTAMP` → `YYYY-MM-DD HH:MM:SS`).
Repo types carry them as `String` and the cleanup-LLM / UI layers can
parse to `chrono::DateTime<Utc>` where they need datetime ops. Avoids
chrono round-trip ambiguity at every read site. Wave 5 docs will note
the format.

### 5. INTEGER columns are `i64`, REAL are `f64`, IDs are `i64`

SQLite has no `u32`. `i64` everywhere, even where the natural domain
is unsigned. Cargo clippy doesn't flag it.

### 6. NewX / X type pattern

Each table gets two types:
- `NewX` — the insertable shape (no `id`, no `created_at`)
- `X` — the readable shape (all columns)

This keeps the contract honest: callers can't accidentally insert
rows with manufactured IDs.

### 7. Hook `block-raw-transcript-edit` will scan source

The hook (per Wave 5 task) greps for risky patterns. **transcripts.rs
must not contain** an `update_raw`, `upsert_raw`, or any `UPDATE transcripts`
where `stage = 'raw'`. Brief flag-and-forbid these explicitly.

### 8. Mock at the integration-test boundary, not at module boundaries

Each module's `#[cfg(test)] mod tests` opens an in-memory DB via
`Database::open_in_memory()` (a real SQLite connection, real migrations).
This is fast (~5ms per test) and exercises real schema constraints.
**No `mockall` in Wave 3.**

---

## Module 1: `src-tauri/src/db/transcripts.rs` (~200 lines)

### Types

```rust
/// Lifecycle stage of a transcript row. Storage column is TEXT;
/// this enum is the typed view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    Raw,
    Cleaned,
    Final,
}

impl Stage {
    pub fn as_str(self) -> &'static str { /* "raw"|"cleaned"|"final" */ }
    pub fn parse(s: &str) -> AppResult<Self> { /* … */ }
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub id: i64,
    pub session_id: i64,
    pub stage: Stage,
    pub text: String,
    pub model_used: Option<String>,
    pub created_at: String,
}
```

### Public API

```rust
/// Insert the immutable raw transcript for a session. Errors if a raw
/// transcript already exists for that session (UNIQUE(session_id, stage)).
pub fn insert_raw(conn: &Connection, session_id: i64, text: &str) -> AppResult<i64>;

/// Insert the cleaned transcript. `model_used` is required for cleaned
/// (we always know which model produced it).
pub fn insert_cleaned(
    conn: &Connection,
    session_id: i64,
    text: &str,
    model_used: &str,
) -> AppResult<i64>;

/// Insert the final transcript (what was actually injected). `model_used`
/// is optional because Tier-0 (pass-through cleaned) has no second model.
pub fn insert_final(
    conn: &Connection,
    session_id: i64,
    text: &str,
    model_used: Option<&str>,
) -> AppResult<i64>;

/// All transcripts for a session, ordered by stage.
pub fn get_by_session(conn: &Connection, session_id: i64) -> AppResult<Vec<Transcript>>;

/// Lookup a single stage.
pub fn get_stage(
    conn: &Connection,
    session_id: i64,
    stage: Stage,
) -> AppResult<Option<Transcript>>;
```

### Hard prohibitions (hook will scan)

```rust
// ❌ NEVER:
// pub fn update_raw(...) { ... }
// pub fn upsert_raw(...) { ... }
// conn.execute("UPDATE transcripts SET text = ?1 WHERE stage = 'raw' ...
```

The hook scans for `UPDATE transcripts` in non-test code. The FTS5
triggers from migration 001 contain `INSERT INTO transcripts_fts(...
VALUES('delete', ...)` for old.id which is fine (hook allowlists FTS5
trigger contexts).

### Unit tests (in-file, `#[cfg(test)]`)

```rust
#[test] fn insert_raw_returns_id_and_round_trips() { … }
#[test] fn duplicate_raw_for_same_session_errors() { … }
#[test] fn insert_cleaned_requires_model_used() { /* compile-time via signature */ }
#[test] fn get_by_session_returns_stages_in_order() { /* raw, cleaned, final */ }
#[test] fn get_stage_returns_none_for_missing() { … }
#[test] fn stage_parse_accepts_canonical_strings_only() {
    assert!(Stage::parse("raw").is_ok());
    assert!(Stage::parse("RAW").is_err()); // case-sensitive
    assert!(Stage::parse("bogus").is_err());
}
```

Each test starts with a session-fixture helper (defined once in the
test mod):

```rust
fn session_fixture(conn: &Connection) -> i64 {
    // Insert minimal session row, return its id. Uses mode_id=1 which
    // exists from migration 003's seed.
    conn.execute(
        "INSERT INTO sessions (uuid, mode_id, hotkey_pressed, started_at, \
         recording_ended_at, status, audio_duration_ms, prompt_id, \
         dictionary_snapshot_id, example_set_id) \
         VALUES (?1, 1, 'Ctrl+Win', '2026-05-15T00:00:00Z', \
         '2026-05-15T00:00:05Z', 'complete', 5000, NULL, NULL, NULL)",
        [uuid::Uuid::new_v4().to_string()],
    ).unwrap();
    conn.last_insert_rowid()
}
```

⚠️ **Session-fixture NOTE:** the session fixture above passes NULL for
provenance FKs. That's fine **only inside transcripts.rs tests** because
PLAN §7's schema allows the columns to be NULL (and the test isn't
asserting provenance). The application-layer "provenance is total"
rule is enforced by `sessions.rs::insert()` requiring non-Option ids
(see §sessions). The schema and the API layer disagree by design.

---

## Module 2: `src-tauri/src/db/search.rs` (~150 lines)

### Types

```rust
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub transcript_id: i64,
    pub session_id: i64,
    pub stage: Stage,                  // reuse the enum from transcripts.rs
    pub snippet: String,               // SQLite's snippet() output
    pub bm25_rank: f64,                // SQLite's bm25() — lower is better
}
```

### Public API

```rust
/// Full-text search across all transcripts. Returns hits ordered by
/// bm25 rank (best match first), limited to `limit` rows.
///
/// `query` is sanitized as a single FTS5 *phrase*: quotes doubled,
/// then wrapped in `"…"`. This is the Phase-1 conservative escaping;
/// Phase 6 will add operator-aware parsing (AND/OR/NEAR/prefix).
pub fn search(
    conn: &Connection,
    query: &str,
    limit: usize,
) -> AppResult<Vec<SearchHit>>;

/// Smoke-test entry point used by the Wave-5 fts5-smoke judge and
/// the Phase-1 `fts_smoke_test(...)` Tauri command. Returns hit count;
/// good enough to verify FTS5 is wired without exposing the result shape.
pub fn smoke_test_count(conn: &Connection, query: &str) -> AppResult<usize>;

/// Convert a free-form query into an FTS5-safe phrase. Public so unit
/// tests can assert the escaping.
pub fn sanitize_query(raw: &str) -> String;
```

### Sanitization rules (Phase 1)

```
"hello"                  →  "\"hello\""
hello world              →  "\"hello world\""
quote " in middle        →  "\"quote \"\" in middle\""
SQL ; injection          →  "\"SQL ; injection\""   ← semicolons fine inside phrase
operator AND OR NEAR     →  "\"operator AND OR NEAR\"" ← treated literal
```

FTS5 phrase-matching is conservative: prevents operator injection AND
SQL injection (the wrapping `"…"` makes the whole input a single phrase
token). Phase 6's brief will add operator-aware parsing.

### Unit tests

```rust
#[test] fn sanitize_wraps_in_quotes_and_doubles_internal_quotes() { … }
#[test] fn sanitize_treats_operators_as_literal_text() { … }
#[test] fn search_returns_empty_for_no_matches() { … }
#[test] fn search_finds_inserted_raw_transcript() { … }
#[test] fn search_respects_limit() { … }
#[test] fn search_ranks_better_matches_higher() { /* bm25 ordering */ }
#[test] fn smoke_test_count_returns_zero_for_no_matches() { … }
```

---

## Module 3: `src-tauri/src/db/sessions.rs` (~300 lines)

This is the biggest module — sessions has 19 columns, three update
paths, and the provenance enforcement.

### Types

```rust
#[derive(Debug, Clone)]
pub struct NewSession {
    pub uuid: String,
    pub mode_id: i64,
    pub hotkey_pressed: String,
    pub started_at: String,            // ISO 8601
    pub recording_ended_at: String,    // ISO 8601
    pub status: SessionStatus,
    pub foreground_app: Option<String>,
    pub foreground_window_title: Option<String>,
    pub audio_duration_ms: i64,
    pub audio_blob_path: Option<String>,

    // Provenance — REQUIRED at application layer (binding rule:
    // PLAN §12 + AGENTS.md "Provenance is total").
    // SQL columns are nullable, the API is not.
    pub prompt_id: i64,
    pub dictionary_snapshot_id: i64,
    pub example_set_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    Recording,
    Processing,
    Complete,
    Error,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub id: i64,
    pub uuid: String,
    pub mode_id: i64,
    pub hotkey_pressed: String,
    pub started_at: String,
    pub recording_ended_at: String,
    pub processing_completed_at: Option<String>,
    pub status: SessionStatus,
    pub error_message: Option<String>,
    pub foreground_app: Option<String>,
    pub foreground_window_title: Option<String>,
    pub audio_duration_ms: i64,
    pub audio_blob_path: Option<String>,
    pub prompt_id: Option<i64>,
    pub dictionary_snapshot_id: Option<i64>,
    pub example_set_id: Option<i64>,
    pub stt_latency_ms: Option<i64>,
    pub cleanup_latency_ms: Option<i64>,
    pub injection_latency_ms: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessingCompletion {
    pub completed_at: String,
    pub status: SessionStatus,
    pub stt_latency_ms: Option<i64>,
    pub cleanup_latency_ms: Option<i64>,
    pub injection_latency_ms: Option<i64>,
}
```

### Public API

```rust
/// Insert a new session. Provenance FKs are required at the type level
/// (they are nullable in SQL but mandatory at the application layer).
pub fn insert(conn: &Connection, new: &NewSession) -> AppResult<i64>;

pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<Session>>;
pub fn get_by_uuid(conn: &Connection, uuid: &str) -> AppResult<Option<Session>>;

/// Most recent sessions, ordered by started_at DESC, limited to `limit`.
pub fn list_recent(conn: &Connection, limit: usize) -> AppResult<Vec<Session>>;

/// Set processing completion fields. Use for happy-path transitions:
/// recording → processing → complete.
pub fn update_processing_complete(
    conn: &Connection,
    id: i64,
    completion: &ProcessingCompletion,
) -> AppResult<()>;

/// Set error state. Use for any failure path.
pub fn update_status_error(
    conn: &Connection,
    id: i64,
    error_message: &str,
) -> AppResult<()>;
```

### Unit tests

```rust
#[test] fn insert_and_round_trip() { … }
#[test] fn duplicate_uuid_errors() { /* UNIQUE constraint */ }
#[test] fn get_by_uuid_returns_none_for_missing() { … }
#[test] fn list_recent_orders_by_started_at_desc() { … }
#[test] fn update_processing_complete_sets_latencies() { … }
#[test] fn update_status_error_sets_status_and_message() { … }
#[test] fn session_status_round_trips_via_serde() { … }
```

⚠️ **Mode-id and provenance-id existence is NOT enforced by the schema's
foreign keys for the provenance ids** (they're nullable). Application-
layer code that calls `insert(&new)` must construct `prompt_id`,
`dictionary_snapshot_id`, and `example_set_id` via the dedicated repos
(prompts.rs, dictionary.rs, examples.rs) BEFORE calling `sessions::insert`.
This is the Phase-1 "provenance is total" enforcement boundary.

`mode_id` IS a real FK in the schema (`REFERENCES modes(id)`); FK
check failures on bad mode ids surface as a `rusqlite::Error`.

---

## Module 4: `src-tauri/src/db/prompts.rs` (~120 lines)

Read-only. The application doesn't insert prompts at runtime — they're
seeded by migration 003 and any new versions ship via future migrations
(per ADR 0008 prompt versioning).

### Types

```rust
#[derive(Debug, Clone)]
pub struct Prompt {
    pub id: i64,
    pub mode_slug: String,
    pub version: i64,
    pub body: String,
    pub created_at: String,
}
```

### Public API

```rust
pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<Prompt>>;
pub fn get_by_mode_and_version(
    conn: &Connection,
    mode_slug: &str,
    version: i64,
) -> AppResult<Option<Prompt>>;

/// Latest version for a mode (max(version) WHERE mode_slug = ?). Used
/// at session start to pin `sessions.prompt_id` to the current prompt.
pub fn get_latest_for_mode(
    conn: &Connection,
    mode_slug: &str,
) -> AppResult<Option<Prompt>>;

/// All versions for a mode, sorted by version DESC.
pub fn list_for_mode(conn: &Connection, mode_slug: &str) -> AppResult<Vec<Prompt>>;
```

### Unit tests

```rust
#[test] fn seeded_prompts_are_discoverable_after_fresh_migrate() { … }
#[test] fn get_latest_for_mode_returns_highest_version() { … }
#[test] fn get_by_mode_and_version_returns_none_for_missing() { … }
#[test] fn list_for_mode_orders_by_version_desc() { … }
```

**Out of scope:** writing prompts at runtime, deleting prompts, editing
prompts. ADR 0008 (prompt versioning) says prompts are append-only via
migrations.

---

## Module 5: `src-tauri/src/db/dictionary.rs` (~250 lines)

### Types

```rust
#[derive(Debug, Clone)]
pub struct NewDictionaryEntry {
    pub term: String,
    pub canonical: Option<String>,
    pub source: String,                 // "user" | "learning" | "import"
    pub confidence: Option<f64>,        // None → DB default (1.0)
    pub app_context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DictionaryEntry {
    pub id: i64,
    pub term: String,
    pub canonical: Option<String>,
    pub source: String,
    pub confidence: f64,
    pub app_context: Option<String>,
    pub use_count: i64,
    pub last_used_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct DictionaryEntryUpdate {
    pub canonical: Option<Option<String>>,    // outer = "should update?", inner = "set to what"
    pub confidence: Option<f64>,
    pub app_context: Option<Option<String>>,
}
```

The `Option<Option<…>>` pattern in `DictionaryEntryUpdate` is the
honest way to model "this field is optional in the update, AND the
value it sets to is itself optional in the row." Phase 6 UI will
likely simplify this when there's a concrete edit form.

### Public API

```rust
pub fn insert(conn: &Connection, new: &NewDictionaryEntry) -> AppResult<i64>;
pub fn update(
    conn: &Connection,
    id: i64,
    changes: &DictionaryEntryUpdate,
) -> AppResult<()>;
pub fn delete(conn: &Connection, id: i64) -> AppResult<()>;

pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<DictionaryEntry>>;
pub fn find_by_term(
    conn: &Connection,
    term: &str,
    app_context: Option<&str>,
) -> AppResult<Option<DictionaryEntry>>;
pub fn list_all(conn: &Connection) -> AppResult<Vec<DictionaryEntry>>;

/// Increment use_count and set last_used_at = CURRENT_TIMESTAMP.
pub fn bump_usage(conn: &Connection, id: i64) -> AppResult<()>;

/// Snapshot the current dictionary state. Returns the new snapshot's id
/// (suitable for `sessions.dictionary_snapshot_id`).
/// Implementation: insert a row in dictionary_snapshots with `term_ids`
/// = JSON array of every enabled dictionary entry's id.
pub fn create_snapshot(conn: &Connection) -> AppResult<i64>;
```

### Unit tests

```rust
#[test] fn insert_and_round_trip() { … }
#[test] fn unique_term_app_context_is_enforced() { /* duplicate insert errors */ }
#[test] fn update_canonical_round_trips() { … }
#[test] fn delete_removes_row_and_fires_audit() { … }
#[test] fn find_by_term_with_and_without_app_context() { … }
#[test] fn bump_usage_increments_count_and_sets_timestamp() { … }
#[test] fn create_snapshot_captures_current_ids_as_json_array() {
    // After inserting 3 entries, snapshot's term_ids should parse as a JSON
    // array of 3 i64s matching the inserted ids.
}
```

⚠️ **Snapshot semantics:** PLAN §7 says `dictionary_snapshots.term_ids
TEXT NOT NULL` — a JSON-array column. The brief decision: include ALL
current entries in the snapshot (no `enabled` filter — the dictionary
table has no `enabled` column; see Wave-2 brief PLAN bug). If we add
soft-delete later, the snapshot helper updates.

---

## Module 6: `src-tauri/src/db/examples.rs` (~220 lines)

### Types

```rust
#[derive(Debug, Clone)]
pub struct NewStyleExample {
    pub mode_slug: String,
    pub session_id: Option<i64>,        // nullable: imported examples have no session
    pub raw_input: String,
    pub ideal_output: String,
    pub app_context: Option<String>,
    pub source: String,                 // "manual" | "learning" | "import"
    pub rank: f64,                      // 0.0 baseline; ranking is Phase 8
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct StyleExample {
    pub id: i64,
    pub mode_slug: String,
    pub session_id: Option<i64>,
    pub raw_input: String,
    pub ideal_output: String,
    pub app_context: Option<String>,
    pub source: String,
    pub rank: f64,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct ExampleSet {
    pub id: i64,
    pub mode_slug: String,
    pub example_ids: Vec<i64>,          // parsed from JSON column
    pub created_at: String,
}
```

### Public API

```rust
pub fn insert(conn: &Connection, new: &NewStyleExample) -> AppResult<i64>;
pub fn delete(conn: &Connection, id: i64) -> AppResult<()>;
pub fn set_enabled(conn: &Connection, id: i64, enabled: bool) -> AppResult<()>;

pub fn get_by_id(conn: &Connection, id: i64) -> AppResult<Option<StyleExample>>;
pub fn list_for_mode(
    conn: &Connection,
    mode_slug: &str,
    enabled_only: bool,
) -> AppResult<Vec<StyleExample>>;

/// Create an immutable example_set. Stored as a JSON array of ids.
pub fn create_example_set(
    conn: &Connection,
    mode_slug: &str,
    example_ids: &[i64],
) -> AppResult<i64>;

pub fn get_example_set(conn: &Connection, id: i64) -> AppResult<Option<ExampleSet>>;
```

**Out of scope for Wave 3 (Phase 8 territory):**
- Ranking algorithms / `rank` value computation
- Automatic example selection from corrections
- Per-session example-set materialization (just store the ids)

### Unit tests

```rust
#[test] fn insert_and_round_trip() { … }
#[test] fn list_for_mode_respects_enabled_only_flag() { … }
#[test] fn create_example_set_stores_ids_as_json() {
    let ids = vec![1, 2, 3];
    let set_id = create_example_set(conn, "normal", &ids).unwrap();
    let set = get_example_set(conn, set_id).unwrap().unwrap();
    assert_eq!(set.example_ids, vec![1, 2, 3]);
}
#[test] fn set_enabled_toggles_value() { … }
#[test] fn delete_removes_row_and_fires_audit() { … }
```

---

## Module 7: `src-tauri/src/db/audit.rs` (~300 lines)

The trickiest module. Walks `_history_*` tables to read or restore prior
state. Phase 8's learning-loop rollback path is the primary consumer
beyond Phase 1.

### Types

```rust
/// Which audited table you're querying / mutating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditedTable {
    Modes,
    Prompts,
    Dictionary,
    StyleExamples,
}

impl AuditedTable {
    pub fn table_name(self) -> &'static str {
        match self {
            Self::Modes => "modes",
            Self::Prompts => "prompts",
            Self::Dictionary => "dictionary",
            Self::StyleExamples => "style_examples",
        }
    }
    pub fn history_table(self) -> &'static str {
        match self {
            Self::Modes => "_history_modes",
            Self::Prompts => "_history_prompts",
            Self::Dictionary => "_history_dictionary",
            Self::StyleExamples => "_history_style_examples",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Operation {
    Insert,
    Update,
    Delete,
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub id: i64,
    pub row_id: i64,
    pub operation: Operation,
    pub patch: serde_json::Value,       // parsed from TEXT column
    pub at: String,
}
```

### Public API

```rust
/// All history entries for a specific row, ordered oldest-first.
pub fn list_history(
    conn: &Connection,
    table: AuditedTable,
    row_id: i64,
) -> AppResult<Vec<HistoryEntry>>;

/// All history entries for a table, ordered oldest-first. For
/// table-level audit UI in Phase 6.
pub fn list_history_for_table(
    conn: &Connection,
    table: AuditedTable,
) -> AppResult<Vec<HistoryEntry>>;

/// Compute the row's state at `at_or_before_ts` by walking history
/// backward from that timestamp.
///
/// Returns:
///   - `Some(state)` if the row existed at that time (INSERT or UPDATE
///     was the last op before the cutoff). State is a JSON object of
///     mutable columns (matches the trigger projection from migration
///     002).
///   - `None` if the row didn't exist (no INSERT before the cutoff,
///     or the last pre-cutoff op was DELETE).
pub fn state_at(
    conn: &Connection,
    table: AuditedTable,
    row_id: i64,
    at_or_before_ts: &str,
) -> AppResult<Option<serde_json::Value>>;

/// Restore a single row to its state at `at_or_before_ts`. Internally:
///   1. Calls `state_at`.
///   2. If Some(target): UPDATE the live row to match (or INSERT if
///      currently absent).
///   3. If None: DELETE the live row.
///
/// **The rollback itself fires audit triggers**, leaving a trail.
/// This is intentional: rollback events are first-class history.
pub fn rollback_row_to_timestamp(
    conn: &Connection,
    table: AuditedTable,
    row_id: i64,
    at_or_before_ts: &str,
) -> AppResult<()>;

/// Rollback every row that has any history before `at_or_before_ts`
/// to its state at that timestamp. Used by Phase 8's learning_runs
/// rollback path. Walks distinct row_ids from the history table and
/// calls `rollback_row_to_timestamp` on each.
pub fn rollback_table_to_timestamp(
    conn: &Connection,
    table: AuditedTable,
    at_or_before_ts: &str,
) -> AppResult<()>;
```

### Implementation notes

- The "compute state at T" algorithm is: find the **last** entry where
  `at <= at_or_before_ts` for the given `row_id`. If it's an INSERT or
  UPDATE, the row's state is the `patch.after` field (for UPDATE) or
  the entire `patch` object (for INSERT). If it's a DELETE, the row
  didn't exist at that time → return `None`.
- **Restoring requires column knowledge.** `rollback_row_to_timestamp`
  builds an UPDATE statement listing every mutable column for the
  given table. Hardcode column lists per-table — they match the audit
  trigger projections from migration 002. Single source of truth: keep
  them in a `mod columns { … }` helper.
- **SQL injection is contained:** `AuditedTable` enum gates all table
  names. User input never reaches SQL identifiers. Column lists are
  `&'static str` constants.
- The rollback's UPDATE fires the appropriate `_audit_update` trigger,
  so the history table gains a new entry showing rollback as a normal
  edit. That's a feature: replays are auditable.

### Column projections (single source of truth)

```rust
mod columns {
    pub const MODES_MUTABLE: &[&str] = &[
        "slug", "display_name", "hotkey", "provider", "model_id",
        "prompt_id", "temperature", "max_tokens", "enabled",
    ];
    pub const PROMPTS_MUTABLE: &[&str] = &["mode_slug", "version", "body"];
    pub const DICTIONARY_MUTABLE: &[&str] = &[
        "term", "canonical", "source", "confidence", "app_context",
    ];
    pub const STYLE_EXAMPLES_MUTABLE: &[&str] = &[
        "mode_slug", "session_id", "raw_input", "ideal_output",
        "app_context", "source", "rank", "enabled",
    ];
}
```

⚠️ **Must match migration 002's projections.** Wave-2 brief had the
extrapolated audit-trigger SQL — these lists are the rust-side mirror.
If migration 002 ever changes (new migration 005+ adds a column to a
mutable list), update this module in the same PR.

### Unit tests

```rust
#[test] fn list_history_returns_entries_oldest_first() { … }

#[test] fn state_at_returns_insert_payload_immediately_after_insert() {
    // Insert dictionary row → call state_at(t_now) → expect Some({term, canonical, ...})
}

#[test] fn state_at_returns_before_after_update() {
    // Insert, sleep, update, capture t_mid (after insert, before update)
    // state_at(t_mid) should return the INSERT projection (not the post-update one).
}

#[test] fn state_at_returns_none_after_delete() {
    // Insert, delete → state_at(t_after_delete) → None.
}

#[test] fn rollback_row_to_timestamp_restores_prior_state() {
    // The headline test. insert(canonical='Foo') @ t0, sleep, update(canonical='FOO') @ t1, sleep, rollback to t0+ε.
    // Expect: row's canonical reads back as 'Foo'. _history_dictionary now has 4 entries
    // (insert, update, then a SECOND update from the rollback itself).
}

#[test] fn rollback_row_to_missing_state_deletes_the_live_row() {
    // state_at returned None → live row gets DELETEd.
}

#[test] fn rollback_table_to_timestamp_walks_every_row() {
    // Insert rows A and B, update both, rollback table to before the updates,
    // expect both rows restored.
}
```

---

## Cross-crate integration tests — `src-tauri/tests/db_repos.rs` (~250 lines)

Wave 2 had `db_migrations.rs`. Wave 3 adds **`db_repos.rs`** for
end-to-end scenarios that touch multiple modules.

```rust
#[test]
fn full_dictation_flow_end_to_end() {
    // Real scenario:
    //   1. dictionary::insert + create_snapshot → snapshot id
    //   2. examples::create_example_set([1,2,3]) → set id
    //   3. prompts::get_latest_for_mode("normal") → prompt id
    //   4. sessions::insert(NewSession{prompt_id, dictionary_snapshot_id, example_set_id, ...})
    //   5. transcripts::insert_raw(session_id, "hello world")
    //   6. transcripts::insert_cleaned(session_id, "Hello, world.", "qwen2.5:3b")
    //   7. transcripts::insert_final(session_id, "Hello, world.", None)
    //   8. sessions::update_processing_complete(...)
    //   9. search::search("hello") → finds the inserted transcripts
    //  10. Assert: session row reads back with all provenance ids set,
    //      all three transcript stages present, FTS5 hit count > 0.
}

#[test]
fn audit_rollback_round_trip() {
    // Dictionary entry insert → update → rollback to pre-update timestamp.
    // Asserts canonical value matches initial, history table has 3 entries.
}

#[test]
fn raw_transcript_immutability_at_api_layer() {
    // Verify there's no public function that mutates a raw transcript.
    // This is a meta-test: it compiles + runs only because no such
    // function exists. If someone adds update_raw, this test stays
    // green but the hook block-raw-transcript-edit catches it.
    // Document as a "vigilance reminder" in a comment.
}

#[test]
fn search_after_full_flow_finds_hits() { /* end-to-end smoke */ }

#[test]
fn session_insert_with_nonexistent_mode_id_errors_via_fk() {
    // mode_id is a real FK → FK violation surfaces as a Sqlite error.
}

#[test]
fn create_snapshot_id_round_trips_through_session() {
    // create_snapshot → use the id in sessions::insert → read back
    // session → dictionary_snapshot_id matches what we stored.
}
```

---

## Wiring updates (`src-tauri/src/db/mod.rs`)

Add 7 new lines to the existing `db/mod.rs`:

```rust
// Wave 3 — repository modules.
pub mod audit;
pub mod dictionary;
pub mod examples;
pub mod prompts;
pub mod search;
pub mod sessions;
pub mod transcripts;
```

Keep `migrations` and `prompt_loader` as `pub(crate)` (Wave 2 setup
unchanged). No changes to `Database::open` or `apply_migrations`.

---

## Wave 3 exit checklist

- [ ] `cargo check --workspace` green
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo test --workspace` green (Wave-2's 15 + Wave-3 unit tests
      across 7 modules + ~6 integration tests in `db_repos.rs`,
      target ~60-80 tests total)
- [ ] `cargo fmt --check` clean
- [ ] `bd close mb-7oi mb-4f8 mb-9pn mb-91x mb-d5z mb-z4k mb-344`
- [ ] STATUS.md updated to "Waves 1+2+3 ✅; Waves 4-5 queued"
- [ ] LESSONS.md: anything non-obvious encountered
- [ ] Commit: `feat(phase-1-wave-3): db repository modules`
- [ ] **DO NOT TAG phase-1-complete YET.** Tag lands at Wave 5.
- [ ] At end-of-iteration: write `docs/phases/phase1-wave4-brief.md`
      while context is loaded (the brief pattern that's now in
      LESSONS as proven 100% test-pass-first-run).

## Known risks

1. **`Option<Option<T>>` in updates may surprise clippy.** If clippy
   complains, alternatives are `enum FieldUpdate<T> { Unchanged, Set(T), Clear }`
   or a struct of typed `Option<FieldChange<…>>`. For Phase 1 stick with
   `Option<Option<T>>`; refactor in Phase 6 if the UI surface demands it.
2. **FTS5 `MATCH` returns rows in unspecified order WITHOUT an `ORDER BY bm25(...)`**.
   The `search` function must include `ORDER BY bm25(transcripts_fts)`
   in its query for deterministic ranking. The unit test
   `search_ranks_better_matches_higher` will catch a missing ORDER BY.
3. **`json_object()` from migration 002 produces SQLite TEXT, not native JSON.**
   When reading `patch` columns in `audit.rs`, use `serde_json::from_str`
   on the TEXT value. Don't try `conn.query_row(... row.get::<_, serde_json::Value>())`
   — rusqlite has no native JSON FromSql.
4. **Audit triggers fire on the rollback's UPDATE.** That's intentional
   (audit trail of replays) but tests asserting "after rollback, history
   has N entries" must account for it. Use exact counts in assertions.
5. **`create_snapshot` returning an empty dictionary** — if there are no
   dictionary entries, term_ids should be `"[]"`, not `"null"` or NULL.
   Test for the empty case.
6. **5-attempt rule applies.** If audit's `rollback_row_to_timestamp`
   resists the test you'd expect to be the headline win, stop after 5
   attempts and escalate via STATUS.md. The state-at algorithm is fiddly
   and getting it right matters more than getting it fast.

## Out of scope for Wave 3 (Waves 4-5 territory)

- Tauri command handlers using these repos (Wave 4)
- Settings facade (Wave 4)
- Logging module (Wave 4)
- Tray (Wave 4)
- Mockall traits over repos (Wave 4, only if needed)
- Phase 6's history-viewer queries (own brief; uses these repos)
- Phase 8's learning-loop ranking (own brief; uses `audit.rs` + `examples.rs`)
- Tag `phase-1-complete` (Wave 5)
