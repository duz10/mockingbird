//! Apply classifier verdicts to the DB:
//!
//! - `new_vocab` — upsert into `dictionary` (skip if term already
//!   present with any source; we don't overwrite user-curated entries).
//! - `style_change` — insert `(before → after)` into `style_examples`
//!   as `source = 'learned'`.
//! - `mistranscription` — no-op.
//! - `noise` — no-op.
//!
//! Then prunes each mode's enabled style examples down to a target
//! count by disabling the lowest-ranked tail (`enabled = 0` rather
//! than DELETE — preserves audit history per ADR 0010).

use rusqlite::Connection;

use crate::db::dictionary::{self, NewDictionaryEntry};
use crate::db::examples::{self, NewStyleExample};
use crate::error::AppResult;
use crate::learning::classifier::Classification;
use crate::learning::corrections::Correction;

/// Per-mode cap on enabled style examples. Lowest-ranked tail beyond
/// this is disabled (not deleted — audit-friendly). PLAN §10 says
/// "~50"; we make it configurable so the eval-driven future Phase
/// can tune.
pub const DEFAULT_EXAMPLES_PER_MODE: usize = 50;

/// Promotion outcome — counts so the runner can fill in `Completion`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PromotionStats {
    /// New rows in `dictionary` (`new_vocab` classifications).
    pub dictionary_terms_added: i64,
    /// New rows in `style_examples` (`style_change` classifications).
    pub examples_added: i64,
    /// Existing rows in `style_examples` newly disabled by pruning.
    pub examples_removed: i64,
}

/// Promote one classified correction.
///
/// Mode slug is needed for style-example inserts. Looked up by the
/// caller (the runner) via the correction's session_id.
pub fn promote_one(
    conn: &Connection,
    correction: &Correction,
    classification: Classification,
    mode_slug: &str,
) -> AppResult<PromotionStats> {
    let mut stats = PromotionStats::default();
    match classification {
        Classification::NewVocab => {
            // Upsert: if the term exists with any source, leave it.
            // First insert wins; the learning loop doesn't overwrite
            // user-curated entries.
            let term = correction.after_text.trim();
            if term.is_empty() {
                return Ok(stats);
            }
            let existing = dictionary::find_by_term(conn, term, None)?;
            if existing.is_none() {
                dictionary::insert(
                    conn,
                    &NewDictionaryEntry {
                        term: term.to_string(),
                        canonical: Some(term.to_string()),
                        source: "learned".into(),
                        confidence: Some(0.7),
                        app_context: None,
                    },
                )?;
                stats.dictionary_terms_added = 1;
            }
        }
        Classification::StyleChange => {
            examples::insert(
                conn,
                &NewStyleExample {
                    mode_slug: mode_slug.to_string(),
                    session_id: Some(correction.session_id),
                    raw_input: correction.before_text.clone(),
                    ideal_output: correction.after_text.clone(),
                    app_context: None,
                    source: "learned".into(),
                    rank: 0.6, // user-marked = 1.0; learned starts a touch lower.
                    enabled: true,
                },
            )?;
            stats.examples_added = 1;
        }
        Classification::Mistranscription | Classification::Noise => {
            // No-op.
        }
    }
    Ok(stats)
}

/// Prune a mode's enabled style examples down to `cap`. Disables
/// the lowest-ranked tail (rank ASC, created_at ASC tiebreak).
/// Returns the number of rows disabled.
pub fn prune_mode_examples(conn: &Connection, mode_slug: &str, cap: usize) -> AppResult<i64> {
    let enabled_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM style_examples WHERE mode_slug = ?1 AND enabled = 1",
        [mode_slug],
        |r| r.get(0),
    )?;
    if enabled_count <= cap as i64 {
        return Ok(0);
    }
    let to_remove = enabled_count - cap as i64;
    // Disable the bottom `to_remove` rows ordered by lowest rank, oldest first.
    let updated = conn.execute(
        "UPDATE style_examples SET enabled = 0 \
         WHERE id IN ( \
            SELECT id FROM style_examples \
            WHERE mode_slug = ?1 AND enabled = 1 \
            ORDER BY rank ASC, created_at ASC \
            LIMIT ?2 \
         )",
        rusqlite::params![mode_slug, to_remove],
    )?;
    Ok(updated as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::learning::corrections::NewCorrection;

    fn fresh() -> Connection {
        Database::open_in_memory().unwrap().conn
    }

    fn correction(session_id: i64, b: &str, a: &str) -> Correction {
        Correction {
            id: 0,
            session_id,
            before_text: b.into(),
            after_text: a.into(),
            detection_method: "manual".into(),
            classification: None,
            classified_at: None,
            created_at: "now".into(),
        }
    }

    #[test]
    fn promote_new_vocab_inserts_dictionary_entry() {
        let conn = fresh();
        let c = correction(1, "kubectl", "kubeCTL");
        let stats = promote_one(&conn, &c, Classification::NewVocab, "normal").unwrap();
        assert_eq!(stats.dictionary_terms_added, 1);
        let entry = dictionary::find_by_term(&conn, "kubeCTL", None)
            .unwrap()
            .unwrap();
        assert_eq!(entry.source, "learned");
    }

    #[test]
    fn promote_new_vocab_skips_if_term_exists() {
        let conn = fresh();
        dictionary::insert(
            &conn,
            &NewDictionaryEntry {
                term: "Mockingbird".into(),
                canonical: Some("Mockingbird".into()),
                source: "user".into(),
                confidence: Some(1.0),
                app_context: None,
            },
        )
        .unwrap();
        let c = correction(1, "mockingbird", "Mockingbird");
        let stats = promote_one(&conn, &c, Classification::NewVocab, "normal").unwrap();
        assert_eq!(stats.dictionary_terms_added, 0);
        // Existing source preserved.
        let entry = dictionary::find_by_term(&conn, "Mockingbird", None)
            .unwrap()
            .unwrap();
        assert_eq!(entry.source, "user");
    }

    #[test]
    fn promote_style_change_inserts_style_example() {
        let conn = fresh();
        let c = correction(1, "hi", "Hello there.");
        let stats = promote_one(&conn, &c, Classification::StyleChange, "normal").unwrap();
        assert_eq!(stats.examples_added, 1);
        let cands = crate::cleanup::few_shot::select_candidates(&conn, "normal", None).unwrap();
        assert_eq!(cands.len(), 1);
        assert_eq!(cands[0].source, "learned");
        assert_eq!(cands[0].ideal_output, "Hello there.");
    }

    #[test]
    fn promote_mistranscription_is_noop() {
        let conn = fresh();
        let c = correction(1, "kubectl", "kubectk");
        let stats = promote_one(&conn, &c, Classification::Mistranscription, "normal").unwrap();
        assert_eq!(stats, PromotionStats::default());
        assert!(dictionary::find_by_term(&conn, "kubectk", None)
            .unwrap()
            .is_none());
    }

    #[test]
    fn promote_noise_is_noop() {
        let conn = fresh();
        let c = correction(1, "x", "y");
        let stats = promote_one(&conn, &c, Classification::Noise, "normal").unwrap();
        assert_eq!(stats, PromotionStats::default());
    }

    #[test]
    fn prune_returns_zero_when_under_cap() {
        let conn = fresh();
        for i in 0..3 {
            examples::insert(
                &conn,
                &NewStyleExample {
                    mode_slug: "normal".into(),
                    session_id: None,
                    raw_input: format!("r{i}"),
                    ideal_output: format!("i{i}"),
                    app_context: None,
                    source: "learned".into(),
                    rank: 0.5,
                    enabled: true,
                },
            )
            .unwrap();
        }
        let removed = prune_mode_examples(&conn, "normal", 10).unwrap();
        assert_eq!(removed, 0);
    }

    #[test]
    fn prune_disables_lowest_rank_tail() {
        let conn = fresh();
        // 5 rows with ranks 0.1 .. 0.5; cap = 3 → bottom 2 disabled.
        for i in 0..5 {
            examples::insert(
                &conn,
                &NewStyleExample {
                    mode_slug: "normal".into(),
                    session_id: None,
                    raw_input: format!("r{i}"),
                    ideal_output: format!("i{i}"),
                    app_context: None,
                    source: "learned".into(),
                    rank: 0.1 + (i as f64) * 0.1,
                    enabled: true,
                },
            )
            .unwrap();
        }
        let removed = prune_mode_examples(&conn, "normal", 3).unwrap();
        assert_eq!(removed, 2);
        let enabled = crate::cleanup::few_shot::select_candidates(&conn, "normal", None).unwrap();
        assert_eq!(enabled.len(), 3);
        // The top 3 (ranks 0.3, 0.4, 0.5) should be the survivors.
        for e in &enabled {
            assert!(e.rank >= 0.3, "kept low-rank example: rank={}", e.rank);
        }
    }

    #[test]
    fn prune_is_idempotent() {
        let conn = fresh();
        for i in 0..5 {
            examples::insert(
                &conn,
                &NewStyleExample {
                    mode_slug: "normal".into(),
                    session_id: None,
                    raw_input: format!("r{i}"),
                    ideal_output: format!("i{i}"),
                    app_context: None,
                    source: "learned".into(),
                    rank: 0.1 + (i as f64) * 0.1,
                    enabled: true,
                },
            )
            .unwrap();
        }
        let r1 = prune_mode_examples(&conn, "normal", 3).unwrap();
        let r2 = prune_mode_examples(&conn, "normal", 3).unwrap();
        assert_eq!(r1, 2);
        assert_eq!(r2, 0);
    }

    // Suppress dead-code on NewCorrection import — used in other
    // tests' setup. (Lint-clean.)
    #[allow(dead_code)]
    fn _unused() -> NewCorrection {
        NewCorrection {
            session_id: 0,
            before_text: String::new(),
            after_text: String::new(),
            detection_method: String::new(),
        }
    }
}
