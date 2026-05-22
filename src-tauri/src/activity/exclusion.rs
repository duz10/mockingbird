//! Activity-capture exclusion matcher.
//!
//! Phase 10 Wave 5. ADR 0043 (rule shape + capture-time enforcement).
//!
//! The matcher is consulted by [`ActivityCaptureRuntime::record_event`]
//! BEFORE the `INSERT` against `activity_events`. If any enabled rule
//! matches the incoming event, the event is dropped — no row is ever
//! written. This is the "honored at capture, not display"
//! invariant from `docs/phases/phase10.md` Cross-wave invariant #4.
//!
//! ## Rule kinds (ADR 0043)
//!
//! - [`RuleKind::AppGlob`] — case-insensitive glob against `app_name`.
//!   Supports `*` (zero-or-more) and `?` (exactly-one). No character
//!   classes. Hand-rolled walker; we don't pull the `glob` crate for
//!   what is, in practice, a 30-line algorithm.
//! - [`RuleKind::TitleRegex`] — `regex` crate pattern against
//!   `window_title`. Patterns that fail to compile are logged + skipped.
//! - [`RuleKind::System`] — sentinel for built-in policies. Currently
//!   only `"password_field_active"` (drop the snapshot when the UIA
//!   probe reported an active password field).
//!
//! ## Hot path
//!
//! `ExclusionMatcher::matches` is called per `SamplerEvent` for the
//! `AppSwitch` and `ContextSnapshot` variants. It walks rules in
//! insertion order and returns on first match. Idle / control events
//! (`idle_start`, `idle_end`, `paused`, `resumed`, `layer_error`)
//! never consult the matcher — they have no content to filter and are
//! essential session-flow markers.

#![allow(missing_docs)]

use std::time::SystemTime;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Rule kinds. Mirrors the `kind` column in `activity_exclusion_rules`
/// (migration 015). Wire format is the lower-snake-case string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleKind {
    /// Case-insensitive glob (`*` and `?` only) against `app_name`.
    AppGlob,
    /// Regex via the `regex` crate against `window_title`.
    TitleRegex,
    /// Built-in system policy. The `pattern` is a sentinel string
    /// naming the policy (currently only `password_field_active`).
    System,
}

impl RuleKind {
    /// Wire string for DB storage + IPC.
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::AppGlob => "app_glob",
            Self::TitleRegex => "title_regex",
            Self::System => "system",
        }
    }

    /// Parse from wire string. Returns `None` for unknown kinds so the
    /// matcher can defensively skip future-DB-rows it doesn't recognize
    /// (forward compat).
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "app_glob" => Some(Self::AppGlob),
            "title_regex" => Some(Self::TitleRegex),
            "system" => Some(Self::System),
            _ => None,
        }
    }
}

/// One stored exclusion rule. Read-only DTO for IPC + matcher input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionRule {
    pub id: String,
    pub kind: RuleKind,
    pub pattern: String,
    pub enabled: bool,
    pub is_builtin: bool,
    pub note: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Compiled matcher ready for the hot path.
///
/// Built via [`ExclusionMatcher::load`] from the DB. Cheap to clone
/// because the inner regexes are wrapped in an `Arc` upstream
/// (`regex::Regex` itself is `Send + Sync + Clone`).
#[derive(Debug, Default, Clone)]
pub struct ExclusionMatcher {
    /// Compiled rules in DB insertion order. First match wins.
    rules: Vec<CompiledRule>,
}

#[derive(Debug, Clone)]
struct CompiledRule {
    id: String,
    kind: RuleKind,
    /// Original pattern string. Used for `RuleKind::System` matching
    /// (where we don't compile anything) + diagnostic logging.
    pattern: String,
    /// Pre-compiled regex for [`RuleKind::TitleRegex`]. `None` for
    /// other kinds.
    regex: Option<regex::Regex>,
}

/// Result of a matcher check. Returned to the runtime so the
/// drop-decision can be logged with rule provenance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExclusionHit<'a> {
    pub rule_id: &'a str,
    pub kind: RuleKind,
}

impl ExclusionMatcher {
    /// Empty matcher — never excludes anything. Useful for tests and
    /// as a transient state when [`load`] hasn't run yet.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Load + compile all enabled rules from `activity_exclusion_rules`.
    /// Disabled rules are skipped at load time so the hot path has
    /// nothing to filter. Invalid regexes log a warning and are
    /// skipped (the runtime stays online).
    pub fn load(conn: &Connection) -> AppResult<Self> {
        let mut stmt = conn.prepare(
            "SELECT id, kind, pattern, is_builtin, note, created_at, updated_at \
             FROM activity_exclusion_rules \
             WHERE enabled = 1 \
             ORDER BY created_at ASC, id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut rules: Vec<CompiledRule> = Vec::new();
        for r in rows {
            let (id, kind_s, pattern) = r?;
            let Some(kind) = RuleKind::from_db_str(&kind_s) else {
                tracing::warn!(
                    target: "activity::exclusion",
                    %id, kind = %kind_s,
                    "unknown rule kind; skipping"
                );
                continue;
            };
            let regex = if matches!(kind, RuleKind::TitleRegex) {
                match regex::Regex::new(&pattern) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        tracing::warn!(
                            target: "activity::exclusion",
                            %id, pattern = %pattern, error = %e,
                            "title_regex failed to compile; skipping rule"
                        );
                        continue;
                    }
                }
            } else {
                None
            };
            rules.push(CompiledRule {
                id,
                kind,
                pattern,
                regex,
            });
        }
        tracing::debug!(
            target: "activity::exclusion",
            count = rules.len(),
            "exclusion matcher loaded"
        );
        Ok(Self { rules })
    }

    /// Number of compiled (enabled) rules. Useful for tests + log lines.
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Convenience.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Decide whether to drop the event with this `(app, title,
    /// password_field_active)` triple.
    ///
    /// `password_field_active` is the parsed-out `snapshot_json` flag
    /// (Wave 2 UIA probe). Pass `false` for `AppSwitch` events that
    /// don't have a snapshot.
    pub fn matches<'a>(
        &'a self,
        app: Option<&str>,
        title: Option<&str>,
        password_field_active: bool,
    ) -> Option<ExclusionHit<'a>> {
        for r in &self.rules {
            let hit = match r.kind {
                RuleKind::AppGlob => app
                    .map(|a| glob_match_case_insensitive(&r.pattern, a))
                    .unwrap_or(false),
                RuleKind::TitleRegex => match (&r.regex, title) {
                    (Some(rx), Some(t)) => rx.is_match(t),
                    _ => false,
                },
                RuleKind::System => match r.pattern.as_str() {
                    "password_field_active" => password_field_active,
                    other => {
                        tracing::trace!(
                            target: "activity::exclusion",
                            sentinel = %other,
                            "unknown system sentinel; not matching"
                        );
                        false
                    }
                },
            };
            if hit {
                return Some(ExclusionHit {
                    rule_id: &r.id,
                    kind: r.kind,
                });
            }
        }
        None
    }
}

/// Case-insensitive glob match supporting `*` (zero-or-more) and `?`
/// (exactly-one). Pure ASCII-aware lowercase via `to_ascii_lowercase`;
/// unicode-folding is out of scope (process names + Windows app names
/// are effectively ASCII for the apps we care about — `1Password*`,
/// `consent.exe`, etc).
///
/// The implementation is a classic two-pointer walk with back-tracking
/// at `*`. Algorithm is O(n*m) worst-case but in practice O(n+m) for
/// the patterns we ship (one trailing `*`).
pub fn glob_match_case_insensitive(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().map(|c| c.to_ascii_lowercase()).collect();
    let t: Vec<char> = text.chars().map(|c| c.to_ascii_lowercase()).collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    // Star backtrack positions.
    let mut star: Option<usize> = None;
    let mut match_ti: usize = 0;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            match_ti = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            match_ti += 1;
            ti = match_ti;
        } else {
            return false;
        }
    }
    // Consume trailing `*`s.
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

// ---------------------------------------------------------------------------
// Repo layer — list / upsert / delete / toggle
// ---------------------------------------------------------------------------

/// List ALL rules (enabled + disabled, built-in + user). Sort order:
/// built-ins first by `id`, then user rules by `created_at`.
pub fn list_all(conn: &Connection) -> AppResult<Vec<ExclusionRule>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, pattern, enabled, is_builtin, note, created_at, updated_at \
         FROM activity_exclusion_rules \
         ORDER BY is_builtin DESC, created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map([], row_to_rule)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Upsert a user-created rule. `id` empty → INSERT new; otherwise
/// UPDATE in place. `is_builtin` is forced to `0` — built-ins can
/// only be created by the migration.
pub fn upsert_user_rule(
    conn: &Connection,
    id: Option<&str>,
    kind: RuleKind,
    pattern: &str,
    enabled: bool,
    note: Option<&str>,
) -> AppResult<String> {
    let now = now_ms();
    if let Some(id) = id {
        // Disallow updating built-in rule shape; only their `enabled`
        // flag flips via `set_enabled`. For built-ins, fall through to
        // an "update enabled only" path — but on this surface (which
        // is for user-created rules), we hard-reject.
        let is_builtin: i64 = conn
            .query_row(
                "SELECT is_builtin FROM activity_exclusion_rules WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        if is_builtin != 0 {
            return Err(AppError::Other(
                "cannot modify built-in exclusion rule shape; use set_enabled instead".into(),
            ));
        }
        conn.execute(
            "UPDATE activity_exclusion_rules \
             SET kind = ?1, pattern = ?2, enabled = ?3, note = ?4, updated_at = ?5 \
             WHERE id = ?6 AND is_builtin = 0",
            params![
                kind.as_db_str(),
                pattern,
                if enabled { 1 } else { 0 },
                note,
                now,
                id
            ],
        )?;
        Ok(id.to_string())
    } else {
        let new_id = format!("user-{}", now);
        conn.execute(
            "INSERT INTO activity_exclusion_rules \
             (id, kind, pattern, enabled, is_builtin, note, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?6)",
            params![
                new_id,
                kind.as_db_str(),
                pattern,
                if enabled { 1 } else { 0 },
                note,
                now
            ],
        )?;
        Ok(new_id)
    }
}

/// Flip the `enabled` flag of any rule (built-in or user).
pub fn set_enabled(conn: &Connection, id: &str, enabled: bool) -> AppResult<()> {
    let n = conn.execute(
        "UPDATE activity_exclusion_rules SET enabled = ?1, updated_at = ?2 WHERE id = ?3",
        params![if enabled { 1 } else { 0 }, now_ms(), id],
    )?;
    if n == 0 {
        return Err(AppError::Other(format!("no exclusion rule with id {id}")));
    }
    Ok(())
}

/// Delete a user-created rule. Built-ins are rejected.
pub fn delete_user_rule(conn: &Connection, id: &str) -> AppResult<()> {
    let n = conn.execute(
        "DELETE FROM activity_exclusion_rules WHERE id = ?1 AND is_builtin = 0",
        params![id],
    )?;
    if n == 0 {
        return Err(AppError::Other(format!(
            "exclusion rule {id} not found or is a built-in (built-ins can be disabled but not deleted)"
        )));
    }
    Ok(())
}

/// Validate a candidate `(kind, pattern)` pair without persisting.
/// Returns `Ok(())` if the pattern is syntactically valid for its
/// kind. Used by the Settings UI to pre-flight saves.
pub fn validate(kind: RuleKind, pattern: &str) -> AppResult<()> {
    if pattern.trim().is_empty() {
        return Err(AppError::Other("pattern cannot be empty".into()));
    }
    match kind {
        RuleKind::TitleRegex => regex::Regex::new(pattern)
            .map(|_| ())
            .map_err(|e| AppError::Other(format!("invalid regex: {e}"))),
        RuleKind::AppGlob => Ok(()), // glob is forgiving; no syntax errors possible
        RuleKind::System => {
            if pattern == "password_field_active" {
                Ok(())
            } else {
                Err(AppError::Other(format!(
                    "unknown system sentinel: {pattern:?}"
                )))
            }
        }
    }
}

fn row_to_rule(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExclusionRule> {
    let kind_s: String = row.get(1)?;
    let kind = RuleKind::from_db_str(&kind_s).unwrap_or(RuleKind::AppGlob);
    Ok(ExclusionRule {
        id: row.get(0)?,
        kind,
        pattern: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        is_builtin: row.get::<_, i64>(4)? != 0,
        note: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_one(kind: RuleKind, pattern: &str) -> CompiledRule {
        let regex = if matches!(kind, RuleKind::TitleRegex) {
            Some(regex::Regex::new(pattern).expect("valid regex"))
        } else {
            None
        };
        CompiledRule {
            id: "test".into(),
            kind,
            pattern: pattern.into(),
            regex,
        }
    }

    fn matcher_from(rules: Vec<CompiledRule>) -> ExclusionMatcher {
        ExclusionMatcher { rules }
    }

    #[test]
    fn empty_matcher_never_excludes() {
        let m = ExclusionMatcher::empty();
        assert!(m
            .matches(Some("1Password.exe"), Some("Vault — 1Password"), false)
            .is_none());
        assert!(m.matches(None, None, true).is_none());
    }

    #[test]
    fn glob_matches_are_case_insensitive() {
        assert!(glob_match_case_insensitive("1Password*", "1Password 7"));
        assert!(glob_match_case_insensitive("1Password*", "1password 7"));
        assert!(glob_match_case_insensitive("1PASSWORD*", "1Password 7"));
        assert!(!glob_match_case_insensitive("1Password*", "Bitwarden"));
    }

    #[test]
    fn glob_question_mark_matches_exactly_one_char() {
        assert!(glob_match_case_insensitive("a?c", "abc"));
        assert!(glob_match_case_insensitive("a?c", "axc"));
        assert!(!glob_match_case_insensitive("a?c", "ac"));
        assert!(!glob_match_case_insensitive("a?c", "abbc"));
    }

    #[test]
    fn glob_star_handles_internal_and_trailing() {
        assert!(glob_match_case_insensitive(
            "*Password*",
            "My 1Password Vault"
        ));
        assert!(glob_match_case_insensitive("*", "anything goes"));
        assert!(glob_match_case_insensitive("", ""));
        assert!(!glob_match_case_insensitive("", "x"));
    }

    #[test]
    fn app_glob_matches_app_name() {
        let m = matcher_from(vec![compile_one(RuleKind::AppGlob, "1Password*")]);
        assert!(m
            .matches(Some("1Password 7"), Some("Anything"), false)
            .is_some());
        assert!(m.matches(Some("Bitwarden"), Some("Vault"), false).is_none());
    }

    #[test]
    fn title_regex_matches_window_title() {
        let m = matcher_from(vec![compile_one(
            RuleKind::TitleRegex,
            r"(?i)\b(bank|login|password)\b",
        )]);
        assert!(m
            .matches(Some("chrome.exe"), Some("Bank of America - Login"), false)
            .is_some());
        assert!(m
            .matches(Some("chrome.exe"), Some("HackerNews"), false)
            .is_none());
    }

    #[test]
    fn system_password_active_drops_regardless_of_app() {
        let m = matcher_from(vec![compile_one(RuleKind::System, "password_field_active")]);
        // Even if app+title look innocent, the password-field bit drops it.
        assert!(m
            .matches(Some("Notepad.exe"), Some("Untitled"), true)
            .is_some());
        // When false, we don't drop.
        assert!(m
            .matches(Some("Notepad.exe"), Some("Untitled"), false)
            .is_none());
    }

    #[test]
    fn unknown_system_sentinel_is_not_a_match() {
        let m = matcher_from(vec![compile_one(RuleKind::System, "made_up_sentinel")]);
        assert!(m
            .matches(Some("Notepad.exe"), Some("Untitled"), true)
            .is_none());
    }

    #[test]
    fn first_matching_rule_wins_and_returns_its_id() {
        let mut rules = vec![compile_one(RuleKind::AppGlob, "1Password*")];
        rules[0].id = "rule-a".into();
        let mut second = compile_one(RuleKind::AppGlob, "*Password*");
        second.id = "rule-b".into();
        rules.push(second);
        let m = matcher_from(rules);
        let hit = m
            .matches(Some("1Password 7"), Some(""), false)
            .expect("must match");
        assert_eq!(hit.rule_id, "rule-a");
    }

    #[test]
    fn idle_event_shaped_args_never_match() {
        // Idle/control events come in with no app + no title + no
        // password bit. The runtime is responsible for not consulting
        // the matcher for those, but defense-in-depth: even if it did,
        // none of our rule kinds would fire.
        let m = matcher_from(vec![
            compile_one(RuleKind::AppGlob, "1Password*"),
            compile_one(RuleKind::TitleRegex, r"(?i)bank"),
        ]);
        assert!(m.matches(None, None, false).is_none());
    }

    #[test]
    fn validate_rejects_empty_pattern() {
        assert!(validate(RuleKind::AppGlob, "").is_err());
        assert!(validate(RuleKind::AppGlob, "   ").is_err());
    }

    #[test]
    fn validate_rejects_invalid_regex() {
        assert!(validate(RuleKind::TitleRegex, "(unclosed").is_err());
        assert!(validate(RuleKind::TitleRegex, r"(?i)\b(ok)\b").is_ok());
    }

    #[test]
    fn validate_rejects_unknown_system_sentinel() {
        assert!(validate(RuleKind::System, "password_field_active").is_ok());
        assert!(validate(RuleKind::System, "future_sentinel").is_err());
    }

    #[test]
    fn rule_kind_round_trips_via_db_str() {
        for k in [RuleKind::AppGlob, RuleKind::TitleRegex, RuleKind::System] {
            assert_eq!(RuleKind::from_db_str(k.as_db_str()), Some(k));
        }
        assert_eq!(RuleKind::from_db_str("garbage"), None);
    }

    // ----------------------------------------------------------
    // Phase 10 Wave 6.B — exclusion-is-total judge fixture (C2).
    // ADR 0043 (built-in rules seeded by migration 015).
    // ----------------------------------------------------------

    #[test]
    fn builtin_rules_load_via_load() {
        use crate::db::Database;
        let db = Database::open_in_memory().expect("open in-memory db");
        let m = ExclusionMatcher::load(&db.conn).expect("load matcher");
        // Migration 015 seeds exactly 8 built-in rules — all enabled
        // by default — so the matcher's len() equals the seed count.
        assert_eq!(
            m.len(),
            8,
            "built-in exclusion rules from migration 015 should load via ExclusionMatcher::load"
        );
        // Spot-check: 1Password app_glob should fire against a synthetic
        // event, proving the rules made it into the hot path.
        assert!(m
            .matches(Some("1Password 7"), Some("Vault"), false)
            .is_some());
        // And the password-field-active system rule should fire
        // regardless of app + title.
        assert!(m
            .matches(Some("Notepad.exe"), Some("Untitled"), true)
            .is_some());
    }
}
