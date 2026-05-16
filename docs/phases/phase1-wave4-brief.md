# Phase 1 Wave 4 — Implementation brief

> **Read this BEFORE writing any of: logging, settings, tray, or commands.**
> Wave 4 wires the database from Wave 3 into the running Tauri app:
> typed settings facade, daily-rotated tracing with PII scrubbing, tray
> with placeholder menu, and the first three `#[tauri::command]` handlers
> (`get_setting`, `set_setting`, `fts_smoke_test`). Plus `lib.rs::run()`
> gains real wiring.
>
> Wave 2 produced 15/15 first-run tests; Wave 3 produced 77/77 with 2
> trivial test-only fixes. Same pattern here. **Treat as binding.**

## Tasks in scope

| bd id    | File                                       | Approx. lines | Notes                                    |
|----------|--------------------------------------------|---------------|------------------------------------------|
| `mb-uo1` | `src-tauri/src/logging.rs`                 | ~200          | tracing + appender + PII scrub layer     |
| `mb-7si` | `src-tauri/src/settings/model.rs`          | ~180          | `SettingKey` enum + defaults             |
| `mb-yof` | `src-tauri/src/settings/mod.rs`            | ~180          | typed `Settings` facade over the table   |
| `mb-8og` | `src-tauri/src/tray.rs`                    | ~150          | placeholder menu, icon-state stubs       |
| `mb-nk5` | `src-tauri/src/commands.rs`                | ~180          | 3 `#[tauri::command]` handlers + state shape |
| `mb-mpv` | `src-tauri/src/lib.rs` (edits)             | ~50 net       | `.manage(Database)`, register tray + commands |

**Total budget:** ~940 lines net new. All files under 600.

## Cross-cutting decisions (binding)

### 1. `AppError` variants get a `Tracing` and (optional) `Settings` variant

Wave 4 adds new fallible operations:
- Logging init can fail (file system, env var parse) → add `AppError::Tracing(String)`
- Settings deserialization can fail → reuse existing `AppError::Other` for these (no new variant; the error message context is enough)

### 2. Settings table values are JSON-encoded TEXT

`settings (key TEXT PRIMARY KEY, value TEXT NOT NULL)` from migration 001.
We store JSON in the `value` column so settings of any shape round-trip:
booleans become `true`/`false`, integers become `42`, strings become
`"theme-dark"`. The `Settings` facade is the only thing that should
read/write this table directly.

### 3. The `Database` moves into `app.manage()`

In `lib.rs::run()`'s `.setup()`, after `Database::open()` returns, we
move the Database into Tauri's managed state via `app.manage(database)`.
Commands receive it via `tauri::State<'_, Database>`. This means:
- `Database`'s connection must be `Send + Sync` — rusqlite::Connection
  is `Send`, **not** `Sync`. Wrap in a `Mutex` at the managed-state
  boundary: `app.manage(Mutex::new(database))` and commands take
  `State<Mutex<Database>>`.
- Alternative: use a connection pool (`r2d2_sqlite`). Overkill for
  Phase 1's single-writer workload. Stick with `Mutex<Database>`.

### 4. PII scrubbing is a `Layer`, not an after-the-fact regex

`tracing_subscriber::Layer` composition lets us scrub field values
before they reach the file appender. We implement a minimal layer
that intercepts `tracing::Event`s and rewrites the string fields
through a regex set. Out of band: don't scrub structured fields
that aren't strings (numbers, bools — safe by construction).

Patterns to scrub:
- API keys: `sk-[A-Za-z0-9_-]{20,}` → `sk-<REDACTED>`
- OpenAI-style keys: `sk-proj-[A-Za-z0-9_-]{20,}` → handled by above
- Anthropic keys: `sk-ant-[A-Za-z0-9_-]{20,}` → handled by above
- Emails: `[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}` → `<EMAIL>`
- User-profile paths: literal expansion of `%USERPROFILE%` resolved at
  init time → replaced with `<HOME>`. (Static prefix match; not regex.)

### 5. Commands are async and return `Result<T, String>`

Tauri commands surface errors to the frontend; the cleanest type is
`Result<T, String>` because `AppError`'s `Display` is what the user
sees. We provide a tiny `into_command_err` helper that maps
`AppError → String` (essentially `.to_string()`). Don't try to
preserve typed errors across the IPC boundary — the JS side gets a
string.

### 6. Tray menu items use string IDs, handler is a single match

Tauri 2's `TrayIconBuilder::on_menu_event` takes a closure that
receives a `MenuEvent` whose `id()` is the string we set on the menu
item. One match-on-id handler, separate match arms per menu action.
Keeps the wiring obvious.

### 7. Logging init returns a `WorkerGuard` that MUST stay alive

`tracing_appender::non_blocking()` returns a `(NonBlocking, WorkerGuard)`
pair. The guard flushes on drop. If we let it drop at the end of `init()`,
the appender's background thread shuts down immediately and logs are
lost. **`init()` returns the guard; `run()` binds it to a local that
lives for the duration of the app.**

---

## Module 1: `src-tauri/src/logging.rs` (~200 lines)

### Public API

```rust
use tracing_appender::non_blocking::WorkerGuard;

/// Initialize the tracing subscriber with file rotation + PII scrubbing.
///
/// Logs land at `<app_data_dir>/logs/mockingbird.log` with daily
/// rotation (7-day retention). Level defaults to INFO; override via
/// `RUST_LOG` env var (standard tracing-subscriber env-filter syntax).
///
/// Returns a `WorkerGuard` that MUST be held by the caller for the
/// duration of the app — dropping it shuts down the background writer
/// and silently loses log lines.
pub fn init(app_data_dir: &std::path::Path) -> AppResult<WorkerGuard>;
```

### Implementation sketch

```rust
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init(app_data_dir: &Path) -> AppResult<WorkerGuard> {
    let logs_dir = app_data_dir.join("logs");
    std::fs::create_dir_all(&logs_dir)?;

    let file_appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("mockingbird")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&logs_dir)
        .map_err(|e| AppError::Tracing(format!("rolling appender: {e}")))?;
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_writer(non_blocking)
        .with_target(true);

    let stdout_layer = fmt::layer()
        .with_ansi(true)
        .with_target(false);

    let scrubber = PiiScrubLayer::new(user_profile_dir());

    tracing_subscriber::registry()
        .with(env_filter)
        .with(scrubber)
        .with(file_layer)
        .with(stdout_layer)
        .try_init()
        .map_err(|e| AppError::Tracing(format!("subscriber init: {e}")))?;

    Ok(guard)
}
```

### PII scrub layer

The naive form: a `tracing_subscriber::Layer<S>` impl that intercepts
events, visits their fields, rewrites string values through the scrub
regex set, and re-emits. **Implementing a generic visit-and-rewrite
layer is complex.** Simpler approach for Phase 1:

**Implement scrubbing inside the `fmt::layer()` field formatter** via a
custom `MakeWriter` that wraps the non-blocking writer and runs each
written line through the regex set before flushing. This catches
everything the formatter would write — message + field values — at
serialization time, after tracing's own formatting.

```rust
struct ScrubbingWriter<W: Write> {
    inner: W,
    scrubbers: Arc<ScrubberSet>,
}

impl<W: Write> Write for ScrubbingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let s = std::str::from_utf8(buf).unwrap_or("<non-utf8>");
        let scrubbed = self.scrubbers.scrub(s);
        let written = self.inner.write(scrubbed.as_bytes())?;
        // Return original length so tracing's accounting stays sane.
        Ok(buf.len().min(written.max(buf.len())))
    }
    fn flush(&mut self) -> io::Result<()> { self.inner.flush() }
}

struct ScrubberSet {
    api_key: Regex,
    email: Regex,
    user_profile_path: String, // literal prefix to redact
}

impl ScrubberSet {
    fn scrub(&self, s: &str) -> String {
        let s = self.api_key.replace_all(s, "sk-<REDACTED>");
        let s = self.email.replace_all(&s, "<EMAIL>");
        if self.user_profile_path.is_empty() {
            s.into_owned()
        } else {
            s.replace(&self.user_profile_path, "<HOME>")
        }
    }
}
```

### Cargo.toml addition (Wave 4 only)

```toml
regex = "1"
```

Add to `[workspace.dependencies]` and `[dependencies]` in
`src-tauri/Cargo.toml`. `tracing_appender` and `tracing_subscriber`
are already pinned.

### Unit tests

```rust
#[test] fn scrubber_redacts_api_keys() { … }
#[test] fn scrubber_redacts_emails() { … }
#[test] fn scrubber_redacts_user_profile_paths() { … }
#[test] fn scrubber_passes_innocent_text_unchanged() { … }
#[test] fn init_creates_logs_dir_if_missing() { … }
#[test] fn init_is_idempotent_within_single_process_NOT_TRUE_document_it() {
    // `tracing_subscriber::try_init` errors on second call. init()
    // surfaces that as AppError::Tracing. Document: init() is meant
    // to be called exactly once at startup.
}
```

### Risks

- **`tracing_subscriber::try_init` is once-per-process.** Second call
  returns an error. Tests that call `init()` must use isolated DB temp
  dirs AND avoid double-init within the same test binary. Strategy:
  put init tests in `tests/` (separate test binary per file) OR use a
  `static Once` gate in the test module.
- **`RollingFileAppender::builder()` API varies between
  `tracing-appender` versions.** If 0.2's builder is different, fall
  back to `tracing_appender::rolling::daily(dir, prefix)` and live
  without the max_log_files retention (Phase 7 polish can revisit).

---

## Module 2: `src-tauri/src/settings/model.rs` (~180 lines)

### Public API

```rust
use serde::{Deserialize, Serialize};

/// Every known setting key. Adding a new setting:
///   1. Add a variant here
///   2. Add it to `as_str()`, `default_value()`, `try_parse()`
///   3. Document the key in `docs/SETTINGS.md`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingKey {
    AutostartEnabled,
    LogLevel,
    Theme,
    ReducedMotion,
    SoundFeedback,
    /// Reference to the Windows Credential Manager entry holding the
    /// Claude API key. Phase 4 wires the actual lookup; Phase 1 just
    /// stores the string.
    ClaudeApiKeyRef,
    AudioRetentionDays,
    LearningEnabled,
}

impl SettingKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutostartEnabled    => "autostart_enabled",
            Self::LogLevel            => "log_level",
            Self::Theme               => "theme",
            Self::ReducedMotion       => "reduced_motion",
            Self::SoundFeedback       => "sound_feedback",
            Self::ClaudeApiKeyRef     => "claude_api_key_ref",
            Self::AudioRetentionDays  => "audio_retention_days",
            Self::LearningEnabled     => "learning_enabled",
        }
    }

    pub fn try_parse(s: &str) -> AppResult<Self> {
        match s {
            "autostart_enabled"    => Ok(Self::AutostartEnabled),
            "log_level"            => Ok(Self::LogLevel),
            "theme"                => Ok(Self::Theme),
            "reduced_motion"       => Ok(Self::ReducedMotion),
            "sound_feedback"       => Ok(Self::SoundFeedback),
            "claude_api_key_ref"   => Ok(Self::ClaudeApiKeyRef),
            "audio_retention_days" => Ok(Self::AudioRetentionDays),
            "learning_enabled"     => Ok(Self::LearningEnabled),
            other => Err(AppError::Other(format!("unknown setting key: {other:?}"))),
        }
    }

    /// The default value for a setting that has never been set.
    /// Returned as `serde_json::Value` — typed callers downcast.
    pub fn default_value(self) -> serde_json::Value {
        match self {
            Self::AutostartEnabled   => serde_json::json!(false),
            Self::LogLevel           => serde_json::json!("info"),
            Self::Theme              => serde_json::json!("system"),  // system | light | dark
            Self::ReducedMotion      => serde_json::json!(false),
            Self::SoundFeedback      => serde_json::json!(true),
            Self::ClaudeApiKeyRef    => serde_json::json!(null),
            Self::AudioRetentionDays => serde_json::json!(30),
            Self::LearningEnabled    => serde_json::json!(true),
        }
    }
}
```

### Unit tests

```rust
#[test] fn every_key_round_trips_via_as_str_and_try_parse() { … }
#[test] fn try_parse_rejects_unknown_keys() { … }
#[test] fn every_key_has_a_default_value() { … }
#[test] fn defaults_match_documented_types_in_settings_md() {
    // sanity: theme returns a string, autostart returns a bool, etc.
}
```

---

## Module 3: `src-tauri/src/settings/mod.rs` (~180 lines)

### Public API

```rust
pub mod model;
use model::SettingKey;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};

/// Typed read/write over the `settings` table.
pub struct Settings<'a> {
    conn: &'a Connection,
}

impl<'a> Settings<'a> {
    pub fn new(conn: &'a Connection) -> Self { Self { conn } }

    /// Get a typed setting. Falls back to the key's default if the row
    /// is missing OR if deserialization fails (with a tracing warn).
    pub fn get<T: DeserializeOwned>(&self, key: SettingKey) -> AppResult<T> {
        let raw = self.get_raw(key)?;
        serde_json::from_value(raw).map_err(|e| AppError::Other(
            format!("deserialize {}: {e}", key.as_str())
        ))
    }

    /// Get the raw JSON value. Returns the key's default if absent.
    pub fn get_raw(&self, key: SettingKey) -> AppResult<serde_json::Value> {
        let stored: Option<String> = self.conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key.as_str()],
            |r| r.get(0),
        ).optional()?;
        let Some(s) = stored else { return Ok(key.default_value()); };
        serde_json::from_str(&s).or_else(|_| {
            tracing::warn!(key = key.as_str(), "corrupt setting; using default");
            Ok(key.default_value())
        })
    }

    pub fn set<T: Serialize>(&self, key: SettingKey, value: &T) -> AppResult<()> {
        let json = serde_json::to_value(value).map_err(|e| {
            AppError::Other(format!("serialize {}: {e}", key.as_str()))
        })?;
        self.set_raw(key, &json)
    }

    pub fn set_raw(&self, key: SettingKey, value: &serde_json::Value) -> AppResult<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key.as_str(), value.to_string()],
        )?;
        Ok(())
    }

    pub fn reset_to_default(&self, key: SettingKey) -> AppResult<()> {
        self.conn.execute("DELETE FROM settings WHERE key = ?1", params![key.as_str()])?;
        Ok(())
    }
}
```

### Unit tests

```rust
#[test] fn get_returns_default_when_unset() { … }
#[test] fn set_then_get_round_trips_bool() { … }
#[test] fn set_then_get_round_trips_string() { … }
#[test] fn set_then_get_round_trips_int() { … }
#[test] fn set_overwrites_via_upsert() { … }
#[test] fn reset_to_default_removes_row() { … }
#[test] fn get_with_wrong_type_errors_cleanly() {
    // set theme as int 42, get as String → error message includes "theme"
}
#[test] fn corrupt_stored_value_falls_back_to_default() {
    // raw INSERT garbage TEXT into settings → get_raw returns default
}
```

---

## Module 4: `src-tauri/src/tray.rs` (~150 lines)

### Public API

```rust
use tauri::{App, AppHandle, Manager};

/// Build and register the system tray with placeholder menu items.
///
/// Phase 1: Open History (stub log), Pause (stub log), Settings (stub
/// log), Quit (exits the app). Phase 5 hooks real behavior to the
/// stubs.
///
/// Idempotent: safe to call once during `.setup()`.
pub fn register(app: &mut App) -> AppResult<()>;
```

### Implementation sketch (Tauri 2 API)

```rust
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
};

pub fn register(app: &mut App) -> AppResult<()> {
    let open_history = MenuItemBuilder::with_id("open_history", "Open History").build(app)?;
    let pause        = MenuItemBuilder::with_id("pause", "Pause").build(app)?;
    let settings     = MenuItemBuilder::with_id("settings", "Settings…").build(app)?;
    let separator    = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit         = MenuItemBuilder::with_id("quit", "Quit Mockingbird").build(app)?;

    let menu = MenuBuilder::new(app)
        .items(&[&open_history, &pause, &settings, &separator, &quit])
        .build()?;

    let _tray = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .on_menu_event(|app, event| handle_menu_event(app, event.id().as_ref()))
        .build(app)?;
    Ok(())
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "open_history" => tracing::info!("tray: open_history (stub)"),
        "pause"        => tracing::info!("tray: pause (stub)"),
        "settings"     => tracing::info!("tray: settings (stub)"),
        "quit"         => app.exit(0),
        other => tracing::warn!(?other, "tray: unknown menu id"),
    }
}
```

### Tests

Tray construction depends on a real `App` instance — hard to unit-test
without spinning up Tauri. **Skip unit tests for tray.rs.** The
Wave-5 manual smoke (`cargo tauri dev` → click each menu item → see
logs) covers it; a Phase-6 Playwright run via qa-kitten will automate.

If we MUST have a test, exercise `handle_menu_event` as a free
function with the recognized ids and assert no panic. That's the
extent of what's testable without Tauri's runtime.

### Risk: Tauri 2 tray API churn

If the `MenuItemBuilder::with_id` / `TrayIconBuilder::with_id` shapes
have changed in 2.11+, fall back to `MenuItemBuilder::new(...).id(...)`
patterns. Cargo docs (`cargo doc --open`) will tell you what shipped.

---

## Module 5: `src-tauri/src/commands.rs` (~180 lines)

### State shape (lives in mod.rs or commands.rs — your call; commands.rs is cleaner)

```rust
use std::sync::Mutex;
use crate::db::Database;

/// Wrapper around `Database` for Tauri's managed state. We need
/// `Sync` because `tauri::State<T>` requires it; `rusqlite::Connection`
/// is `Send` but not `Sync`. A Mutex bridges the gap.
pub struct AppState {
    pub db: Mutex<Database>,
}

impl AppState {
    pub fn new(db: Database) -> Self {
        Self { db: Mutex::new(db) }
    }
}
```

### Commands

```rust
use tauri::State;
use crate::settings::{model::SettingKey, Settings};

fn into_command_err(e: AppError) -> String { e.to_string() }

#[tauri::command]
pub async fn get_setting(
    state: State<'_, AppState>,
    key: String,
) -> Result<serde_json::Value, String> {
    let key = SettingKey::try_parse(&key).map_err(into_command_err)?;
    let guard = state.db.lock().map_err(|e| format!("db lock: {e}"))?;
    let settings = Settings::new(&guard.conn);
    settings.get_raw(key).map_err(into_command_err)
}

#[tauri::command]
pub async fn set_setting(
    state: State<'_, AppState>,
    key: String,
    value: serde_json::Value,
) -> Result<(), String> {
    let key = SettingKey::try_parse(&key).map_err(into_command_err)?;
    let guard = state.db.lock().map_err(|e| format!("db lock: {e}"))?;
    let settings = Settings::new(&guard.conn);
    settings.set_raw(key, &value).map_err(into_command_err)
}

#[tauri::command]
pub async fn fts_smoke_test(
    state: State<'_, AppState>,
    query: String,
) -> Result<usize, String> {
    let guard = state.db.lock().map_err(|e| format!("db lock: {e}"))?;
    crate::db::search::smoke_test_count(&guard.conn, &query).map_err(into_command_err)
}
```

### Why I'm wrapping logic in `#[tauri::command]` directly (not via free functions)

Tauri commands are async fns that can be hard to test directly. **For
Wave 4, the commands are 3-line wrappers around already-tested repo
functions.** Free-function extraction would add files for no test
benefit (the repos are tested; the unwrapping/locking is mechanical).
Wave 5 manual smoke + a Phase-6 Playwright pass covers the
end-to-end.

### Unit tests

Per the rationale above, only test things that don't need Tauri's
runtime:

```rust
#[test] fn into_command_err_renders_displayable() {
    let s = into_command_err(AppError::Other("boom".into()));
    assert!(s.contains("boom"));
}

#[test] fn app_state_wraps_database() {
    let db = Database::open_in_memory().unwrap();
    let _state = AppState::new(db); // smoke compile-and-construct
}
```

Real command behavior is covered by:
- The underlying repo tests (already green from Wave 3)
- The settings facade tests (Wave 4)
- Wave 5 manual `cargo tauri dev` smoke

---

## Module 6: `src-tauri/src/lib.rs` (edit, ~50 net new lines)

### New shape

```rust
pub mod commands;
pub mod db;
pub mod error;
pub mod logging;
pub mod settings;
pub mod tray;

use commands::AppState;
use tauri::Manager;

pub fn run() {
    // The WorkerGuard must outlive the Tauri runtime to keep the log
    // appender alive. We bind it to a name that lives for the full
    // duration of run().
    let _logging_guard: Option<tracing_appender::non_blocking::WorkerGuard>;

    let context = tauri::generate_context!();

    tauri::Builder::default()
        .setup(|app| -> Result<(), Box<dyn std::error::Error>> {
            // 1. Resolve %APPDATA%/Mockingbird/
            let app_data = app.path().app_data_dir().map_err(box_err)?;
            std::fs::create_dir_all(&app_data)?;

            // 2. Initialize logging FIRST so DB-open errors get captured.
            //    Note: the guard would normally be returned out, but `.setup`
            //    callback eats it. Workaround: leak it for the program's
            //    lifetime (acceptable for a singleton at startup).
            let guard = logging::init(&app_data).map_err(box_err)?;
            std::mem::forget(guard);

            tracing::info!(?app_data, "Mockingbird starting (Phase 1 Wave 4)");

            // 3. Open the DB and apply migrations.
            let db_path = app_data.join("mockingbird.db");
            let database = db::Database::open(&db_path).map_err(box_err)?;
            tracing::info!(?db_path, "database ready");

            // 4. Move DB into managed state.
            app.manage(AppState::new(database));

            // 5. Register the system tray.
            tray::register(app).map_err(box_err)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_setting,
            commands::set_setting,
            commands::fts_smoke_test,
        ])
        .run(context)
        .expect("error while running Tauri application");
}

fn box_err<E: Into<Box<dyn std::error::Error>>>(e: E) -> Box<dyn std::error::Error> {
    e.into()
}
```

⚠️ **`std::mem::forget(guard)` is a deliberate one-shot leak.** The
guard owns a thread handle and a flush; leaking it for the entire
program lifetime keeps the log writer alive. The thread is reclaimed
on process exit. **This pattern is FINE for a singleton init at app
startup, NOT FINE in any reusable code path.** Phase 5 may refactor
when the recording-window lifecycle gets more complex.

---

## Wave 4 exit checklist

- [ ] `cargo check --workspace` green
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean
- [ ] `cargo test --workspace` green (Wave-3's 77 + Wave-4 unit tests,
      target ~100-110 tests total)
- [ ] `cargo fmt --check` clean
- [ ] Manual smoke (`cargo tauri build --debug --no-bundle`) compiles
      a real binary (no actual launch needed unless time permits)
- [ ] `bd close mb-uo1 mb-7si mb-yof mb-8og mb-nk5 mb-mpv`
- [ ] STATUS.md updated to "Waves 1+2+3+4 ✅; Wave 5 queued"
- [ ] LESSONS.md: anything non-obvious
- [ ] Commit: `feat(phase-1-wave-4): logging + settings + tray + commands + app wire`
- [ ] At end-of-iteration: write `docs/phases/phase1-wave5-brief.md`
      while context is loaded
- [ ] **DO NOT TAG phase-1-complete YET.** That's Wave 5 after judges.

## Known risks

1. **Tauri 2 tray/menu API churn** — `MenuItemBuilder::with_id` vs `::new`,
   `TrayIconBuilder::with_id` vs `new`. If a method doesn't exist, check
   `cargo doc --open --package tauri` and fall back to whatever shipped.
2. **`tracing_subscriber::try_init` is once-per-process.** Test isolation
   matters. Put logging-init tests in `tests/` (separate test binary) OR
   use a `Once` gate.
3. **`std::mem::forget` on the worker guard is a deliberate leak.** This
   is a known Rust pattern for singleton startup state. Document the
   "why" in lib.rs.
4. **`Mutex<Database>` deadlock risk if a command holds the lock and
   calls another command synchronously.** Phase 1 commands are flat
   (no command calls another) — fine. Phase 6+ should adopt
   parking_lot::Mutex or rework if recursion appears.
5. **`tracing_appender::rolling::RollingFileAppender::builder()`** API
   may not exist on older 0.2 versions. Fallback:
   `tracing_appender::rolling::daily(dir, prefix)` returns a usable
   appender without the retention controls.
6. **PII scrubbing via `MakeWriter` runs on already-formatted bytes.**
   This catches log lines but misses non-utf8 binary blobs (none should
   exist in tracing output anyway).

## Out of scope for Wave 4 (Wave 5 + later)

- Re-enabling `#![warn(missing_docs)]` with proper docs (Wave 5)
- Tray icon state-swapping based on recording state (Phase 5)
- `cargo tauri dev` end-to-end smoke + Playwright (Wave 5 manual /
  Phase 5)
- Settings UI (Phase 5 introduces React; the typed facade ships now)
- Real Claude API key lookup against Credential Manager (Phase 4)
- Real audio retention enforcement (Phase 5)
- Tag `phase-1-complete` (Wave 5)
