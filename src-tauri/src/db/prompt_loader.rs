//! Token substitution for migration 003.
//!
//! Prompt bodies live in `src-tauri/src/cleanup/prompts/*.md` as the
//! single source of truth (ADR 0008). Migration 003 contains tokens
//! like `__PROMPT_NORMAL_BODY__` that are substituted at runtime —
//! keeping prompt edits out of SQL files (which are sealed after
//! `phase-1-complete`).
//!
//! Why include_str! instead of reading at runtime: deterministic
//! bundles, no Tauri resource API churn, no I/O failure modes during
//! database open. The tradeoff is a rebuild on any prompt edit, which
//! is the desired behavior anyway (ADR 0008's append-only versioning
//! means edits go through a `004_prompt_v2.sql` migration, not the
//! in-place Markdown file).

const PROMPT_NORMAL: &str = include_str!("../cleanup/prompts/normal.md");
const PROMPT_NORMAL_V2: &str = include_str!("../cleanup/prompts/normal_v2.md");
const PROMPT_VERBOSE: &str = include_str!("../cleanup/prompts/verbose.md");
const PROMPT_FRAGMENT: &str = include_str!("../cleanup/prompts/fragment.md");
const PROMPT_REWRITE: &str = include_str!("../cleanup/prompts/rewrite.md");
const PROMPT_EXPAND: &str = include_str!("../cleanup/prompts/expand.md");
const PROMPT_SUMMARIZE: &str = include_str!("../cleanup/prompts/summarize.md");

/// Replace `__PROMPT_*_BODY__` tokens with SQL-escaped prompt bodies.
///
/// Adding a new prompt-bearing migration? Add the `include_str!`
/// constant above + the corresponding `.replace(...)` below in the
/// SAME commit as the migration SQL. Order doesn't matter — these
/// are simple non-overlapping replacements.
pub fn substitute_prompt_bodies(sql: &str) -> String {
    // Distinct tokens — `__PROMPT_NORMAL_V2_BODY__` is not a prefix or
    // suffix of `__PROMPT_NORMAL_BODY__`, so the chained replace() is
    // non-overlapping regardless of order. v1 stays addressable for
    // historical session rows; v2 ships in migration 006.
    let substituted = sql
        .replace("__PROMPT_NORMAL_BODY__", &sql_escape(PROMPT_NORMAL))
        .replace("__PROMPT_NORMAL_V2_BODY__", &sql_escape(PROMPT_NORMAL_V2))
        .replace("__PROMPT_VERBOSE_BODY__", &sql_escape(PROMPT_VERBOSE))
        .replace("__PROMPT_FRAGMENT_BODY__", &sql_escape(PROMPT_FRAGMENT))
        .replace("__PROMPT_REWRITE_BODY__", &sql_escape(PROMPT_REWRITE))
        .replace("__PROMPT_EXPAND_BODY__", &sql_escape(PROMPT_EXPAND))
        .replace("__PROMPT_SUMMARIZE_BODY__", &sql_escape(PROMPT_SUMMARIZE));

    // **Leftover-token guard.** If a real `__PROMPT_<NAME>_BODY__`
    // marker survived substitution, fail HARD at boot rather than
    // shipping malformed SQL to SQLite (cryptic "syntax error near
    // 'voice'" — see LESSONS 2026-05-17 phase5-postship-5).
    //
    // Tokens are recognised by shape: `__PROMPT_` + one-or-more chars
    // from `[A-Z0-9_]` + `_BODY__`. The prose convention
    // `__PROMPT_*_BODY__` (literal asterisk) is intentionally NOT
    // matched — it's the safe-comment-reference form documented in
    // migrations 003/005. If you write the EXACT name of a real
    // substitution token in a `--` comment, the prompt body gets
    // injected into the comment line, the body's first `\n`
    // terminates the comment, and havoc ensues. Use `*` for prose.
    if let Some(token) = find_unsubstituted_prompt_token(&substituted) {
        panic!(
            "prompt_loader: unsubstituted token `{token}` survived in migration SQL. \
             Either (a) add the matching include_str! + .replace() in prompt_loader.rs, \
             or (b) if the token appears in a `--` comment, paraphrase to `__PROMPT_*_BODY__` \
             (literal asterisk) so the blanket replacer doesn't match it."
        );
    }
    substituted
}

/// Scan `sql` for any `__PROMPT_<NAME>_BODY__` token where `<NAME>` is
/// one or more uppercase ASCII letters, digits, or underscores. Returns
/// the first such token (including the framing `__...__`), or `None`
/// if only the safe asterisk-prose form (or no tokens at all) remain.
///
/// Intentionally tolerant: we don't try to skip over `--` comments
/// when scanning. The whole point of the guard is that comments are
/// NOT a safe place to write the literal token — a real token sitting
/// in a comment is still a bug.
fn find_unsubstituted_prompt_token(sql: &str) -> Option<String> {
    const PREFIX: &str = "__PROMPT_";
    const SUFFIX: &str = "_BODY__";
    let bytes = sql.as_bytes();
    let mut search_from = 0;
    while let Some(rel) = sql[search_from..].find(PREFIX) {
        let start = search_from + rel;
        // The name runs from `start + PREFIX.len()` up to the next
        // occurrence of `_BODY__`. If none found, give up on this
        // occurrence — it's prose, not a token.
        let name_start = start + PREFIX.len();
        if let Some(suf_rel) = sql[name_start..].find(SUFFIX) {
            let name_end = name_start + suf_rel;
            let name = &bytes[name_start..name_end];
            // Real substitution names are `[A-Z0-9_]+`. The prose
            // `*` form (or anything containing other chars) is
            // skipped — advance one byte past PREFIX and keep looking.
            let looks_like_token = !name.is_empty()
                && name
                    .iter()
                    .all(|&b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_');
            if looks_like_token {
                let token_end = name_end + SUFFIX.len();
                return Some(sql[start..token_end].to_string());
            }
        }
        // Advance past this PREFIX so we don't loop forever.
        search_from = start + PREFIX.len();
    }
    None
}

#[cfg(test)]
mod guard_tests {
    use super::find_unsubstituted_prompt_token;

    #[test]
    fn safe_asterisk_prose_is_ignored() {
        // Both 003 and 005 use this exact form in their `--` comments.
        let sql = "-- the runner substitutes the `__PROMPT_*_BODY__` token\nINSERT INTO x VALUES (1);";
        assert_eq!(find_unsubstituted_prompt_token(sql), None);
    }

    #[test]
    fn real_unsubstituted_token_is_caught() {
        let sql = "INSERT INTO prompts VALUES ('x', 1, '__PROMPT_NORMAL_V2_BODY__');";
        assert_eq!(
            find_unsubstituted_prompt_token(sql).as_deref(),
            Some("__PROMPT_NORMAL_V2_BODY__")
        );
    }

    #[test]
    fn unrelated_text_is_ignored() {
        let sql = "-- no tokens here at all\nCREATE TABLE t (id INTEGER);";
        assert_eq!(find_unsubstituted_prompt_token(sql), None);
    }

    #[test]
    fn token_in_comment_is_still_caught() {
        // The original 006 bug: real token in a `--` comment.
        let sql = "-- see __PROMPT_NORMAL_BODY__ for the body\nSELECT 1;";
        assert!(find_unsubstituted_prompt_token(sql).is_some());
    }

    #[test]
    fn prose_followed_by_real_token_finds_real_one() {
        // Mixed case: prose reference + a forgotten real one.
        let sql = "-- __PROMPT_*_BODY__ is the convention\n-- but __PROMPT_SOMETHING_BODY__ leaked";
        assert_eq!(
            find_unsubstituted_prompt_token(sql).as_deref(),
            Some("__PROMPT_SOMETHING_BODY__")
        );
    }
}

/// Double single-quotes so the body can sit inside a SQL string
/// literal. This is the only escape SQLite needs for `'...' `
/// string literals — no backslash escapes, no NUL handling required
/// because Markdown sources don't contain NULs.
fn sql_escape(s: &str) -> String {
    s.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitution_replaces_all_six_tokens() {
        let template = "a __PROMPT_NORMAL_BODY__ b __PROMPT_VERBOSE_BODY__ c \
                        __PROMPT_FRAGMENT_BODY__ d __PROMPT_REWRITE_BODY__ e \
                        __PROMPT_EXPAND_BODY__ f __PROMPT_SUMMARIZE_BODY__";
        let out = substitute_prompt_bodies(template);
        assert!(
            !out.contains("__PROMPT_"),
            "all six tokens should have been replaced; got: {out}"
        );
    }

    #[test]
    fn sql_escape_doubles_single_quotes() {
        assert_eq!(sql_escape("don't"), "don''t");
        assert_eq!(sql_escape("she said 'hi'"), "she said ''hi''");
        assert_eq!(sql_escape("no quotes here"), "no quotes here");
    }

    #[test]
    fn substitution_handles_apostrophes_in_prompt_bodies() {
        // The actual prompt files have natural English with apostrophes
        // ("don't", "speaker's"). Verify nothing in our substituted
        // output leaves bare single quotes that would break SQL.
        let template = "INSERT INTO prompts VALUES ('__PROMPT_NORMAL_BODY__');";
        let out = substitute_prompt_bodies(template);
        // No bare single-quotes inside the value beyond our wrapping pair.
        // Strip the SQL frame and check the middle has only doubled quotes.
        let inner = out
            .trim_start_matches("INSERT INTO prompts VALUES ('")
            .trim_end_matches("');");
        // Count unescaped single quotes (a single `'` not followed by another `'`).
        let chars: Vec<char> = inner.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\'' {
                assert_eq!(
                    chars.get(i + 1),
                    Some(&'\''),
                    "unescaped single quote at byte index {i} would break SQL"
                );
                i += 2;
            } else {
                i += 1;
            }
        }
    }
}
