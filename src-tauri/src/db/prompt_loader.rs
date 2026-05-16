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
const PROMPT_VERBOSE: &str = include_str!("../cleanup/prompts/verbose.md");
const PROMPT_FRAGMENT: &str = include_str!("../cleanup/prompts/fragment.md");

/// Replace `__PROMPT_*_BODY__` tokens with SQL-escaped prompt bodies.
pub fn substitute_prompt_bodies(sql: &str) -> String {
    sql.replace("__PROMPT_NORMAL_BODY__", &sql_escape(PROMPT_NORMAL))
        .replace("__PROMPT_VERBOSE_BODY__", &sql_escape(PROMPT_VERBOSE))
        .replace("__PROMPT_FRAGMENT_BODY__", &sql_escape(PROMPT_FRAGMENT))
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
    fn substitution_replaces_all_three_tokens() {
        let template =
            "x __PROMPT_NORMAL_BODY__ y __PROMPT_VERBOSE_BODY__ z __PROMPT_FRAGMENT_BODY__";
        let out = substitute_prompt_bodies(template);
        assert!(
            !out.contains("__PROMPT_"),
            "all three tokens should have been replaced; got: {out}"
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
